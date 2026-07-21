use anyhow::{bail, Context, Result};
use secp256k1::PublicKey;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::{
    address_from_script_pubkey, public_key_hash, sign_hash, target_from_compact, verify_hash,
    Address, Amount, Block, ChainState, CoinRecord, Hash256, OutPoint, ScriptBuf, Settings,
    Transaction, TxIn, TxOut, WalletKey, ATOMS_PER_QUB, TX_VERSION_POOL_SHARE,
};

pub const POOL_CREATE_PREFIX: &[u8] = b"POOLCREATE1|";
pub const POOL_TOPUP_PREFIX: &[u8] = b"POOLTOPUP1|";
pub const POOL_COMMISSION_PREFIX: &[u8] = b"POOLCOMMISSION1|";
pub const POOL_RENAME_PREFIX: &[u8] = b"POOLRENAME1|";
pub const POOL_SHARE_PREFIX: &[u8] = b"POOLSHARE1|";
pub const POOL_BLOCK_PREFIX: &[u8] = b"POOLBLOCK1|";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolsSettings {
    pub enabled: bool,
    pub activation_height: u32,
    pub protocol_name: String,
    pub protocol_address: String,
    pub max_name_chars: u8,
    pub max_name_bytes: usize,
    pub marker_output_atoms: u64,
    pub base_create_atoms: u64,
    pub base_capacity_slots: u32,
    pub capacity_step_atoms: u64,
    pub capacity_step_slots: u32,
    pub max_capacity_slots: u32,
    pub max_active_pools: u32,
    pub max_commission_bps: u16,
    pub share_window_blocks: u32,
    pub share_target_bits: String,
    pub max_share_txs_per_block: usize,
    pub share_stale_blocks: u32,
}

pub fn default_pools_settings() -> PoolsSettings {
    PoolsSettings {
        enabled: false,
        activation_height: u32::MAX,
        protocol_name: "pools.qub".to_string(),
        protocol_address: String::new(),
        max_name_chars: 32,
        max_name_bytes: 128,
        marker_output_atoms: 1,
        base_create_atoms: 25 * ATOMS_PER_QUB,
        base_capacity_slots: 8,
        capacity_step_atoms: 10 * ATOMS_PER_QUB,
        capacity_step_slots: 8,
        max_capacity_slots: 128,
        max_active_pools: 1024,
        max_commission_bps: 2000,
        share_window_blocks: 360,
        share_target_bits: "0x1e00ffff".to_string(),
        max_share_txs_per_block: 128,
        share_stale_blocks: 6,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolRecord {
    pub pool_id: Hash256,
    pub name: String,
    pub manager_address: String,
    pub commission_bps: u16,
    pub capacity_slots: u32,
    pub created_height: u32,
    pub create_txid: Hash256,
    pub total_paid_atoms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolCreate {
    pub name: String,
    pub manager_address: String,
    pub commission_bps: u16,
    pub capacity_slots: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolTopUp {
    pub pool_id: Hash256,
    pub manager_address: String,
    pub extra_capacity_slots: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolSetCommission {
    pub pool_id: Hash256,
    pub manager_address: String,
    pub new_commission_bps: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolRename {
    pub pool_id: Hash256,
    pub manager_address: String,
    pub new_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolShare {
    pub pool_id: Hash256,
    pub miner_address: String,
    pub parent_height: u32,
    pub parent_hash: Hash256,
    pub nonce: u64,
    pub public_key_hex: String,
    pub signature_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolPayoutEntry {
    pub address: String,
    pub score: u128,
    pub amount: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolPayoutPlan {
    pub pool_id: Hash256,
    pub manager_address: String,
    pub commission_bps: u16,
    pub total_score: u128,
    pub commission_amount: u128,
    pub miner_amount: u128,
    pub entries: Vec<PoolPayoutEntry>,
}

#[derive(Debug, Clone)]
struct PoolWindowState {
    active_pool_by_miner: HashMap<String, Hash256>,
    active_miners_by_pool: HashMap<Hash256, HashSet<String>>,
    seen_share_keys: HashSet<String>,
}

impl PoolWindowState {
    fn from_blocks(settings: &Settings, blocks: &[Block], spend_height: u32) -> Self {
        let mut state = Self {
            active_pool_by_miner: HashMap::new(),
            active_miners_by_pool: HashMap::new(),
            seen_share_keys: HashSet::new(),
        };
        let window_start = spend_height.saturating_sub(settings.pools.share_window_blocks);
        for (height, block) in blocks.iter().enumerate().skip(1) {
            let height = height as u32;
            for tx in block.transactions.iter().skip(1) {
                if let Some(share) = parse_pool_share_tx(tx) {
                    state.seen_share_keys.insert(pool_share_key(&share));
                    if height >= window_start && height < spend_height {
                        state.add_active_unchecked(&share);
                    }
                }
            }
        }
        state
    }

    fn add_active_unchecked(&mut self, share: &PoolShare) {
        self.active_pool_by_miner
            .entry(share.miner_address.clone())
            .or_insert(share.pool_id);
        self.active_miners_by_pool
            .entry(share.pool_id)
            .or_default()
            .insert(share.miner_address.clone());
    }

    fn validate_and_add_share(&mut self, share: &PoolShare, pool: &PoolRecord) -> Result<()> {
        let key = pool_share_key(share);
        if !self.seen_share_keys.insert(key) {
            bail!("duplicate pool share");
        }
        if let Some(existing) = self.active_pool_by_miner.get(&share.miner_address) {
            if *existing != share.pool_id {
                bail!("miner address is already active in another pool during the share window");
            }
        }
        let miners = self.active_miners_by_pool.entry(share.pool_id).or_default();
        if !miners.contains(&share.miner_address) && miners.len() >= pool.capacity_slots as usize {
            bail!(
                "pool is full: active miners {}/{}",
                miners.len(),
                pool.capacity_slots
            );
        }
        miners.insert(share.miner_address.clone());
        self.active_pool_by_miner
            .insert(share.miner_address.clone(), share.pool_id);
        Ok(())
    }
}

pub fn validate_pools_settings(settings: &Settings) -> Result<()> {
    if !(settings.pools.enabled && settings.features.pooled_mining_enabled) {
        return Ok(());
    }
    if settings.pools.activation_height == 0 {
        bail!("pools activation_height must be non-zero");
    }
    if settings.pools.protocol_name.trim().is_empty() {
        bail!("pools protocol_name must be set");
    }
    if settings.pools.protocol_address.trim().is_empty() {
        bail!("pools protocol_address must be set when pools are enabled");
    }
    Address::parse_with_prefix(
        &settings.pools.protocol_address,
        &settings.network.address_prefix,
    )
    .context("invalid pools protocol_address")?;
    crate::normalize_qns_name(&settings.pools.protocol_name, settings.qns.max_label_chars)
        .context("invalid pools protocol_name")?;
    if settings.pools.max_name_chars == 0 || settings.pools.max_name_chars > 64 {
        bail!("pools max_name_chars must be 1..64");
    }
    if settings.pools.max_name_bytes == 0 || settings.pools.max_name_bytes > 256 {
        bail!("pools max_name_bytes must be 1..256");
    }
    if settings.pools.marker_output_atoms == 0 {
        bail!("pools marker_output_atoms must be non-zero");
    }
    if settings.pools.base_create_atoms == 0 {
        bail!("pools base_create_atoms must be non-zero");
    }
    if settings.pools.base_capacity_slots == 0 {
        bail!("pools base_capacity_slots must be non-zero");
    }
    if settings.pools.capacity_step_slots == 0 {
        bail!("pools capacity_step_slots must be non-zero");
    }
    if settings.pools.capacity_step_atoms == 0 {
        bail!("pools capacity_step_atoms must be non-zero");
    }
    if settings.pools.max_capacity_slots < settings.pools.base_capacity_slots {
        bail!("pools max_capacity_slots must be >= base_capacity_slots");
    }
    if settings.pools.max_active_pools == 0 {
        bail!("pools max_active_pools must be non-zero");
    }
    if settings.pools.max_commission_bps > 10_000 {
        bail!("pools max_commission_bps cannot exceed 10000");
    }
    if settings.pools.share_window_blocks == 0 {
        bail!("pools share_window_blocks must be non-zero");
    }
    if settings.pools.share_stale_blocks == 0 {
        bail!("pools share_stale_blocks must be non-zero");
    }
    if settings.pools.max_share_txs_per_block == 0 {
        bail!("pools max_share_txs_per_block must be non-zero");
    }
    parse_share_bits(settings)?;
    Ok(())
}

pub fn pools_active(settings: &Settings, height: u32) -> bool {
    settings.features.pooled_mining_enabled
        && settings.pools.enabled
        && height >= settings.pools.activation_height
}

fn parse_share_bits(settings: &Settings) -> Result<u32> {
    let s = settings.pools.share_target_bits.trim();
    if let Some(hex) = s.strip_prefix("0x") {
        Ok(u32::from_str_radix(hex, 16)?)
    } else {
        Ok(s.parse()?)
    }
}

pub fn normalize_pool_name(name: &str, max_chars: u8, max_bytes: usize) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("pool name is empty");
    }
    if trimmed.as_bytes().len() > max_bytes {
        bail!("pool name too large; max {max_bytes} bytes");
    }
    if trimmed.chars().count() > max_chars as usize {
        bail!("pool name too long; max {max_chars} displayed chars");
    }
    for ch in trimmed.chars() {
        if ch.is_control() {
            bail!("pool name cannot contain control characters");
        }
        match ch as u32 {
            0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0xFEFF => {
                bail!("pool name cannot contain zero-width or bidi override characters")
            }
            _ => {}
        }
    }
    Ok(trimmed.to_string())
}

pub fn capacity_slots_valid(settings: &Settings, slots: u32) -> bool {
    if slots < settings.pools.base_capacity_slots || slots > settings.pools.max_capacity_slots {
        return false;
    }
    let extra = slots - settings.pools.base_capacity_slots;
    extra % settings.pools.capacity_step_slots == 0
}

pub fn extra_capacity_slots_valid(settings: &Settings, slots: u32) -> bool {
    slots > 0 && slots % settings.pools.capacity_step_slots == 0
}

pub fn pool_create_price_atoms(settings: &Settings, capacity_slots: u32) -> Result<u64> {
    if !capacity_slots_valid(settings, capacity_slots) {
        bail!("invalid initial pool capacity slots");
    }
    let steps =
        (capacity_slots - settings.pools.base_capacity_slots) / settings.pools.capacity_step_slots;
    settings
        .pools
        .base_create_atoms
        .checked_add(
            (steps as u64)
                .checked_mul(settings.pools.capacity_step_atoms)
                .context("pool create price overflow")?,
        )
        .context("pool create price overflow")
}

pub fn pool_topup_price_atoms(settings: &Settings, extra_capacity_slots: u32) -> Result<u64> {
    if !extra_capacity_slots_valid(settings, extra_capacity_slots) {
        bail!("invalid pool top-up capacity slots");
    }
    let steps = extra_capacity_slots / settings.pools.capacity_step_slots;
    (steps as u64)
        .checked_mul(settings.pools.capacity_step_atoms)
        .context("pool top-up price overflow")
}

pub fn pool_protocol_share_atoms(price_atoms: u64) -> u64 {
    price_atoms / 2
}
pub fn pool_miner_share_atoms(price_atoms: u64) -> u64 {
    price_atoms - (price_atoms / 2)
}

pub fn pool_create_marker_script(
    name: &str,
    manager: &Address,
    commission_bps: u16,
    capacity_slots: u32,
    settings: &Settings,
) -> Result<ScriptBuf> {
    let name = normalize_pool_name(
        name,
        settings.pools.max_name_chars,
        settings.pools.max_name_bytes,
    )?;
    if commission_bps > settings.pools.max_commission_bps {
        bail!("commission exceeds max_commission_bps");
    }
    if !capacity_slots_valid(settings, capacity_slots) {
        bail!("invalid initial pool capacity");
    }
    Ok(ScriptBuf(
        format!(
            "POOLCREATE1|{}|{}|{}|{}",
            hex::encode(name.as_bytes()),
            manager,
            commission_bps,
            capacity_slots
        )
        .into_bytes(),
    ))
}

pub fn pool_topup_marker_script(
    pool_id: Hash256,
    manager: &Address,
    extra_capacity_slots: u32,
    settings: &Settings,
) -> Result<ScriptBuf> {
    if !extra_capacity_slots_valid(settings, extra_capacity_slots) {
        bail!("invalid top-up capacity");
    }
    Ok(ScriptBuf(
        format!(
            "POOLTOPUP1|{}|{}|{}",
            pool_id, manager, extra_capacity_slots
        )
        .into_bytes(),
    ))
}

pub fn pool_commission_marker_script(
    pool_id: Hash256,
    manager: &Address,
    new_commission_bps: u16,
    settings: &Settings,
) -> Result<ScriptBuf> {
    if new_commission_bps > settings.pools.max_commission_bps {
        bail!("commission exceeds max_commission_bps");
    }
    Ok(ScriptBuf(
        format!(
            "POOLCOMMISSION1|{}|{}|{}",
            pool_id, manager, new_commission_bps
        )
        .into_bytes(),
    ))
}

pub fn pool_rename_marker_script(
    pool_id: Hash256,
    manager: &Address,
    new_name: &str,
    settings: &Settings,
) -> Result<ScriptBuf> {
    let new_name = normalize_pool_name(
        new_name,
        settings.pools.max_name_chars,
        settings.pools.max_name_bytes,
    )?;
    Ok(ScriptBuf(
        format!(
            "POOLRENAME1|{}|{}|{}",
            pool_id,
            manager,
            hex::encode(new_name.as_bytes())
        )
        .into_bytes(),
    ))
}

pub fn parse_pool_create_marker_script(
    script: &ScriptBuf,
    settings: &Settings,
) -> Option<PoolCreate> {
    let b = script.as_bytes();
    if !b.starts_with(POOL_CREATE_PREFIX) {
        return None;
    }
    let s = std::str::from_utf8(b).ok()?;
    let mut parts = s.split('|');
    if parts.next()? != "POOLCREATE1" {
        return None;
    }
    let name_bytes = hex::decode(parts.next()?).ok()?;
    let name = std::str::from_utf8(&name_bytes).ok()?;
    let name = normalize_pool_name(
        name,
        settings.pools.max_name_chars,
        settings.pools.max_name_bytes,
    )
    .ok()?;
    let manager_address = parts.next()?.trim().to_string();
    Address::parse_with_prefix(&manager_address, &settings.network.address_prefix).ok()?;
    let commission_bps = parts.next()?.parse::<u16>().ok()?;
    let capacity_slots = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(PoolCreate {
        name,
        manager_address,
        commission_bps,
        capacity_slots,
    })
}

pub fn parse_pool_topup_marker_script(
    script: &ScriptBuf,
    settings: &Settings,
) -> Option<PoolTopUp> {
    let b = script.as_bytes();
    if !b.starts_with(POOL_TOPUP_PREFIX) {
        return None;
    }
    let s = std::str::from_utf8(b).ok()?;
    let mut parts = s.split('|');
    if parts.next()? != "POOLTOPUP1" {
        return None;
    }
    let pool_id = Hash256::from_hex(parts.next()?).ok()?;
    let manager_address = parts.next()?.trim().to_string();
    Address::parse_with_prefix(&manager_address, &settings.network.address_prefix).ok()?;
    let extra_capacity_slots = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(PoolTopUp {
        pool_id,
        manager_address,
        extra_capacity_slots,
    })
}

pub fn parse_pool_commission_marker_script(
    script: &ScriptBuf,
    settings: &Settings,
) -> Option<PoolSetCommission> {
    let b = script.as_bytes();
    if !b.starts_with(POOL_COMMISSION_PREFIX) {
        return None;
    }
    let s = std::str::from_utf8(b).ok()?;
    let mut parts = s.split('|');
    if parts.next()? != "POOLCOMMISSION1" {
        return None;
    }
    let pool_id = Hash256::from_hex(parts.next()?).ok()?;
    let manager_address = parts.next()?.trim().to_string();
    Address::parse_with_prefix(&manager_address, &settings.network.address_prefix).ok()?;
    let new_commission_bps = parts.next()?.parse::<u16>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(PoolSetCommission {
        pool_id,
        manager_address,
        new_commission_bps,
    })
}

pub fn parse_pool_rename_marker_script(
    script: &ScriptBuf,
    settings: &Settings,
) -> Option<PoolRename> {
    let b = script.as_bytes();
    if !b.starts_with(POOL_RENAME_PREFIX) {
        return None;
    }
    let s = std::str::from_utf8(b).ok()?;
    let mut parts = s.split('|');
    if parts.next()? != "POOLRENAME1" {
        return None;
    }
    let pool_id = Hash256::from_hex(parts.next()?).ok()?;
    let manager_address = parts.next()?.trim().to_string();
    Address::parse_with_prefix(&manager_address, &settings.network.address_prefix).ok()?;
    let name_bytes = hex::decode(parts.next()?).ok()?;
    let name = std::str::from_utf8(&name_bytes).ok()?;
    let new_name = normalize_pool_name(
        name,
        settings.pools.max_name_chars,
        settings.pools.max_name_bytes,
    )
    .ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(PoolRename {
        pool_id,
        manager_address,
        new_name,
    })
}

pub fn pool_creates_in_tx(tx: &Transaction, settings: &Settings) -> Vec<(usize, PoolCreate, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_pool_create_marker_script(&out.script_pubkey, settings)
                .map(|m| (idx, m, out.value.atoms()))
        })
        .collect()
}
pub fn pool_topups_in_tx(tx: &Transaction, settings: &Settings) -> Vec<(usize, PoolTopUp, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_pool_topup_marker_script(&out.script_pubkey, settings)
                .map(|m| (idx, m, out.value.atoms()))
        })
        .collect()
}
pub fn pool_commissions_in_tx(
    tx: &Transaction,
    settings: &Settings,
) -> Vec<(usize, PoolSetCommission, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_pool_commission_marker_script(&out.script_pubkey, settings)
                .map(|m| (idx, m, out.value.atoms()))
        })
        .collect()
}

pub fn pool_renames_in_tx(tx: &Transaction, settings: &Settings) -> Vec<(usize, PoolRename, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_pool_rename_marker_script(&out.script_pubkey, settings)
                .map(|m| (idx, m, out.value.atoms()))
        })
        .collect()
}

pub fn pools_registry_from_blocks(
    settings: &Settings,
    blocks: &[Block],
) -> Result<HashMap<Hash256, PoolRecord>> {
    let mut registry = HashMap::new();
    if !settings.pools.enabled || !settings.features.pooled_mining_enabled {
        return Ok(registry);
    }
    for (height, block) in blocks.iter().enumerate().skip(1) {
        let height = height as u32;
        if !pools_active(settings, height) {
            continue;
        }
        for tx in block.transactions.iter().skip(1) {
            for (_, create, _) in pool_creates_in_tx(tx, settings) {
                let pool_id = tx.txid();
                let price = pool_create_price_atoms(settings, create.capacity_slots)?;
                registry.insert(
                    pool_id,
                    PoolRecord {
                        pool_id,
                        name: create.name,
                        manager_address: create.manager_address,
                        commission_bps: create.commission_bps,
                        capacity_slots: create.capacity_slots,
                        created_height: height,
                        create_txid: pool_id,
                        total_paid_atoms: price,
                    },
                );
            }
            for (_, topup, _) in pool_topups_in_tx(tx, settings) {
                if let Some(pool) = registry.get_mut(&topup.pool_id) {
                    let price = pool_topup_price_atoms(settings, topup.extra_capacity_slots)?;
                    pool.capacity_slots = pool
                        .capacity_slots
                        .saturating_add(topup.extra_capacity_slots)
                        .min(settings.pools.max_capacity_slots);
                    pool.total_paid_atoms = pool.total_paid_atoms.saturating_add(price);
                }
            }
            for (_, commission, _) in pool_commissions_in_tx(tx, settings) {
                if let Some(pool) = registry.get_mut(&commission.pool_id) {
                    if commission.new_commission_bps <= pool.commission_bps {
                        pool.commission_bps = commission.new_commission_bps;
                    }
                }
            }
            for (_, rename, _) in pool_renames_in_tx(tx, settings) {
                if let Some(pool) = registry.get_mut(&rename.pool_id) {
                    if rename.manager_address == pool.manager_address {
                        pool.name = rename.new_name;
                    }
                }
            }
        }
    }
    Ok(registry)
}

fn pool_action_count(tx: &Transaction, settings: &Settings) -> usize {
    pool_creates_in_tx(tx, settings).len()
        + pool_topups_in_tx(tx, settings).len()
        + pool_commissions_in_tx(tx, settings).len()
        + pool_renames_in_tx(tx, settings).len()
}

fn protocol_paid_atoms(tx: &Transaction, settings: &Settings) -> Result<u64> {
    let protocol = Address::parse_with_prefix(
        &settings.pools.protocol_address,
        &settings.network.address_prefix,
    )?;
    let script = protocol.script_pubkey().0;
    Ok(tx
        .outputs
        .iter()
        .filter(|out| out.script_pubkey.0 == script)
        .map(|out| out.value.atoms())
        .sum())
}

fn input_authorizes_address_from_utxos(
    tx: &Transaction,
    utxos: &HashMap<OutPoint, CoinRecord>,
    address: &Address,
) -> bool {
    let script = address.script_pubkey().0;
    tx.inputs.iter().any(|input| {
        utxos
            .get(&input.previous_output)
            .map(|coin| coin.tx_out.script_pubkey.0 == script)
            .unwrap_or(false)
    })
}

fn validate_pool_action_tx(
    tx: &Transaction,
    fee_atoms: u64,
    height: u32,
    settings: &Settings,
    registry: &mut HashMap<Hash256, PoolRecord>,
    utxos: &HashMap<OutPoint, CoinRecord>,
) -> Result<()> {
    let creates = pool_creates_in_tx(tx, settings);
    let topups = pool_topups_in_tx(tx, settings);
    let commissions = pool_commissions_in_tx(tx, settings);
    let renames = pool_renames_in_tx(tx, settings);
    let actions = creates.len() + topups.len() + commissions.len() + renames.len();
    if actions == 0 {
        return Ok(());
    }
    if !pools_active(settings, height) {
        bail!(
            "pool transaction before activation height {}",
            settings.pools.activation_height
        );
    }
    if tx.is_coinbase() {
        bail!("coinbase cannot contain pool protocol actions");
    }
    if actions != 1 {
        bail!("a transaction may contain exactly one pool protocol action");
    }
    if !crate::qns_registrations_in_tx(tx, settings).is_empty()
        || !crate::jin_transfers_in_tx(tx, settings).is_empty()
        || !crate::jin_conversions_in_tx(tx, settings).is_empty()
    {
        bail!("pool protocol actions cannot be mixed with QNS/JIN protocol markers in the same transaction");
    }
    if let Some((_idx, create, marker_atoms)) = creates.into_iter().next() {
        if marker_atoms != settings.pools.marker_output_atoms {
            bail!(
                "pool create marker output must be exactly {} atom(s)",
                settings.pools.marker_output_atoms
            );
        }
        if registry.len() >= settings.pools.max_active_pools as usize {
            bail!("max active pools reached");
        }
        if create.commission_bps > settings.pools.max_commission_bps {
            bail!("pool commission exceeds max");
        }
        if !capacity_slots_valid(settings, create.capacity_slots) {
            bail!("invalid pool capacity");
        }
        let price = pool_create_price_atoms(settings, create.capacity_slots)?;
        let protocol_required = pool_protocol_share_atoms(price);
        let paid = protocol_paid_atoms(tx, settings)?;
        if paid < protocol_required {
            bail!(
                "pool create underpayment: need {} QUB to protocol, paid {} QUB",
                Amount::from_atoms(protocol_required)?,
                Amount::from_atoms(paid)?
            );
        }
        let miner_required = pool_miner_share_atoms(price);
        if fee_atoms < miner_required {
            bail!(
                "pool create miner split underpayment: required {} atoms as block fee, got {}",
                miner_required,
                fee_atoms
            );
        }
        let pool_id = tx.txid();
        registry.insert(
            pool_id,
            PoolRecord {
                pool_id,
                name: create.name,
                manager_address: create.manager_address,
                commission_bps: create.commission_bps,
                capacity_slots: create.capacity_slots,
                created_height: height,
                create_txid: pool_id,
                total_paid_atoms: price,
            },
        );
        return Ok(());
    }
    if let Some((_idx, topup, marker_atoms)) = topups.into_iter().next() {
        if marker_atoms != settings.pools.marker_output_atoms {
            bail!(
                "pool top-up marker output must be exactly {} atom(s)",
                settings.pools.marker_output_atoms
            );
        }
        let pool = registry
            .get(&topup.pool_id)
            .cloned()
            .context("unknown pool_id for top-up")?;
        if topup.manager_address != pool.manager_address {
            bail!("pool top-up manager mismatch");
        }
        let manager =
            Address::parse_with_prefix(&topup.manager_address, &settings.network.address_prefix)?;
        if !input_authorizes_address_from_utxos(tx, utxos, &manager) {
            bail!("pool top-up is not authorized by a manager-owned input");
        }
        if !extra_capacity_slots_valid(settings, topup.extra_capacity_slots) {
            bail!("invalid pool top-up capacity");
        }
        let new_capacity = pool
            .capacity_slots
            .checked_add(topup.extra_capacity_slots)
            .context("pool capacity overflow")?;
        if new_capacity > settings.pools.max_capacity_slots {
            bail!("pool capacity exceeds max_capacity_slots");
        }
        let price = pool_topup_price_atoms(settings, topup.extra_capacity_slots)?;
        let protocol_required = pool_protocol_share_atoms(price);
        let paid = protocol_paid_atoms(tx, settings)?;
        if paid < protocol_required {
            bail!(
                "pool top-up underpayment: need {} QUB to protocol, paid {} QUB",
                Amount::from_atoms(protocol_required)?,
                Amount::from_atoms(paid)?
            );
        }
        let miner_required = pool_miner_share_atoms(price);
        if fee_atoms < miner_required {
            bail!(
                "pool top-up miner split underpayment: required {} atoms as block fee, got {}",
                miner_required,
                fee_atoms
            );
        }
        let pool = registry
            .get_mut(&topup.pool_id)
            .expect("checked pool exists");
        pool.capacity_slots = new_capacity;
        pool.total_paid_atoms = pool
            .total_paid_atoms
            .checked_add(price)
            .context("pool paid overflow")?;
        return Ok(());
    }
    if let Some((_idx, commission, marker_atoms)) = commissions.into_iter().next() {
        if marker_atoms != settings.pools.marker_output_atoms {
            bail!(
                "pool commission marker output must be exactly {} atom(s)",
                settings.pools.marker_output_atoms
            );
        }
        let pool = registry
            .get(&commission.pool_id)
            .cloned()
            .context("unknown pool_id for commission update")?;
        if commission.manager_address != pool.manager_address {
            bail!("pool commission manager mismatch");
        }
        let manager = Address::parse_with_prefix(
            &commission.manager_address,
            &settings.network.address_prefix,
        )?;
        if !input_authorizes_address_from_utxos(tx, utxos, &manager) {
            bail!("pool commission update is not authorized by a manager-owned input");
        }
        if commission.new_commission_bps > pool.commission_bps {
            bail!("pool commission can only decrease");
        }
        if commission.new_commission_bps > settings.pools.max_commission_bps {
            bail!("pool commission exceeds max");
        }
        registry
            .get_mut(&commission.pool_id)
            .expect("checked pool exists")
            .commission_bps = commission.new_commission_bps;
        return Ok(());
    }
    if let Some((_idx, rename, marker_atoms)) = renames.into_iter().next() {
        if marker_atoms != settings.pools.marker_output_atoms {
            bail!(
                "pool rename marker output must be exactly {} atom(s)",
                settings.pools.marker_output_atoms
            );
        }
        let pool = registry
            .get(&rename.pool_id)
            .cloned()
            .context("unknown pool_id for rename")?;
        if rename.manager_address != pool.manager_address {
            bail!("pool rename manager mismatch");
        }
        let manager =
            Address::parse_with_prefix(&rename.manager_address, &settings.network.address_prefix)?;
        if !input_authorizes_address_from_utxos(tx, utxos, &manager) {
            bail!("pool rename is not authorized by a manager-owned input");
        }
        let new_name = normalize_pool_name(
            &rename.new_name,
            settings.pools.max_name_chars,
            settings.pools.max_name_bytes,
        )?;
        registry
            .get_mut(&rename.pool_id)
            .expect("checked pool exists")
            .name = new_name;
        return Ok(());
    }
    Ok(())
}

pub fn is_pool_share_transaction(tx: &Transaction) -> bool {
    tx.version == TX_VERSION_POOL_SHARE
        && tx.inputs.len() == 1
        && tx.inputs[0].previous_output == OutPoint::null()
        && tx.outputs.is_empty()
}

pub fn parse_pool_share_tx(tx: &Transaction) -> Option<PoolShare> {
    if !is_pool_share_transaction(tx) {
        return None;
    }
    let b = tx.inputs.first()?.signature_script.as_bytes();
    if !b.starts_with(POOL_SHARE_PREFIX) {
        return None;
    }
    let s = std::str::from_utf8(b).ok()?;
    let mut parts = s.split('|');
    if parts.next()? != "POOLSHARE1" {
        return None;
    }
    let pool_id = Hash256::from_hex(parts.next()?).ok()?;
    let miner_address = parts.next()?.trim().to_string();
    let parent_height = parts.next()?.parse::<u32>().ok()?;
    let parent_hash = Hash256::from_hex(parts.next()?).ok()?;
    let nonce = parts.next()?.parse::<u64>().ok()?;
    let public_key_hex = parts.next()?.trim().to_string();
    let signature_hex = parts.next()?.trim().to_string();
    if parts.next().is_some() {
        return None;
    }
    Some(PoolShare {
        pool_id,
        miner_address,
        parent_height,
        parent_hash,
        nonce,
        public_key_hex,
        signature_hex,
    })
}

pub fn pool_share_signing_hash(
    pool_id: Hash256,
    miner_address: &str,
    parent_height: u32,
    parent_hash: Hash256,
    nonce: u64,
) -> Hash256 {
    Hash256::double_sha256(
        format!(
            "POOLSHARE-SIG-v1|{}|{}|{}|{}|{}",
            pool_id, miner_address, parent_height, parent_hash, nonce
        )
        .as_bytes(),
    )
}

pub fn pool_share_work_hash(
    pool_id: Hash256,
    miner_address: &str,
    parent_height: u32,
    parent_hash: Hash256,
    nonce: u64,
) -> Hash256 {
    Hash256::double_sha256(
        format!(
            "POOLSHARE-WORK-v1|{}|{}|{}|{}|{}",
            pool_id, miner_address, parent_height, parent_hash, nonce
        )
        .as_bytes(),
    )
}

pub fn pool_share_key(share: &PoolShare) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        share.pool_id, share.miner_address, share.parent_height, share.parent_hash, share.nonce
    )
}

pub fn hash_meets_compact_target(hash: Hash256, bits: u32) -> Result<bool> {
    let target = target_from_compact(bits)?;
    let mut hash_be = hash.0;
    hash_be.reverse();
    Ok(hash_be.as_slice() <= target.as_slice())
}

pub fn pool_share_meets_target(
    settings: &Settings,
    pool_id: Hash256,
    miner_address: &str,
    parent_height: u32,
    parent_hash: Hash256,
    nonce: u64,
) -> Result<bool> {
    let bits = parse_share_bits(settings)?;
    let hash = pool_share_work_hash(pool_id, miner_address, parent_height, parent_hash, nonce);
    hash_meets_compact_target(hash, bits)
}

pub fn create_pool_share_transaction(
    settings: &Settings,
    pool_id: Hash256,
    miner_key: &WalletKey,
    parent_height: u32,
    parent_hash: Hash256,
    nonce: u64,
) -> Result<Transaction> {
    let miner = Address::parse_with_prefix(&miner_key.address, &settings.network.address_prefix)?;
    let public = PublicKey::from_slice(&hex::decode(&miner_key.public_key_hex)?)?;
    if public_key_hash(&public.serialize()) != miner.payload {
        bail!("pool share wallet key does not match miner address");
    }
    let secret = crate::secret_key_from_hex(&miner_key.secret_key_hex)?;
    let sig_hash = pool_share_signing_hash(
        pool_id,
        &miner.to_string(),
        parent_height,
        parent_hash,
        nonce,
    );
    let sig = sign_hash(&secret, sig_hash)?;
    let script = ScriptBuf(
        format!(
            "POOLSHARE1|{}|{}|{}|{}|{}|{}|{}",
            pool_id,
            miner,
            parent_height,
            parent_hash,
            nonce,
            hex::encode(public.serialize()),
            hex::encode(sig)
        )
        .into_bytes(),
    );
    Ok(Transaction {
        version: TX_VERSION_POOL_SHARE,
        inputs: vec![TxIn {
            previous_output: OutPoint::null(),
            signature_script: script,
            sequence: u32::MAX,
        }],
        outputs: Vec::new(),
        locktime: 0,
    })
}

fn validate_share_tx_against_context(
    tx: &Transaction,
    settings: &Settings,
    blocks: &[Block],
    spend_height: u32,
    registry: &HashMap<Hash256, PoolRecord>,
    state: &mut PoolWindowState,
) -> Result<()> {
    if !pools_active(settings, spend_height) {
        bail!(
            "pool share before activation height {}",
            settings.pools.activation_height
        );
    }
    let share = parse_pool_share_tx(tx).context("invalid pool share transaction")?;
    Address::parse_with_prefix(&share.miner_address, &settings.network.address_prefix)?;
    let pool = registry
        .get(&share.pool_id)
        .context("unknown pool_id for share")?;
    if share.parent_height < pool.created_height {
        bail!("pool share parent predates pool creation");
    }
    if share.parent_height as usize >= blocks.len() {
        bail!("pool share references unknown parent height");
    }
    if share.parent_hash != blocks[share.parent_height as usize].block_hash() {
        bail!("pool share parent hash mismatch");
    }
    if share.parent_height >= spend_height {
        bail!("pool share parent must be before spend height");
    }
    let age = spend_height.saturating_sub(share.parent_height);
    if age == 0 || age > settings.pools.share_stale_blocks {
        bail!("stale pool share");
    }
    if !pool_share_meets_target(
        settings,
        share.pool_id,
        &share.miner_address,
        share.parent_height,
        share.parent_hash,
        share.nonce,
    )? {
        bail!("pool share does not meet target");
    }
    let pk = hex::decode(&share.public_key_hex)?;
    let sig = hex::decode(&share.signature_hex)?;
    let public = PublicKey::from_slice(&pk)?;
    let addr = Address::parse_with_prefix(&share.miner_address, &settings.network.address_prefix)?;
    if public_key_hash(&public.serialize()) != addr.payload {
        bail!("pool share public key does not match miner address");
    }
    let sig_hash = pool_share_signing_hash(
        share.pool_id,
        &share.miner_address,
        share.parent_height,
        share.parent_hash,
        share.nonce,
    );
    if !verify_hash(&pk, &sig, sig_hash) {
        bail!("pool share signature verification failed");
    }
    state.validate_and_add_share(&share, pool)?;
    Ok(())
}

pub fn validate_pools_transaction_against_chain(
    tx: &Transaction,
    chain: &ChainState,
    spend_height: u32,
    settings: &Settings,
) -> Result<()> {
    let has_pool_action = pool_action_count(tx, settings) > 0;
    let is_share = is_pool_share_transaction(tx);
    if !has_pool_action && !is_share {
        return Ok(());
    }
    if !settings.features.pooled_mining_enabled || !settings.pools.enabled {
        bail!("pooled mining is disabled on this network");
    }
    let mut registry = pools_registry_from_blocks(settings, &chain.blocks)?;
    let mut state = PoolWindowState::from_blocks(settings, &chain.blocks, spend_height);
    for mem in chain.mempool.iter() {
        if mem.txid() == tx.txid() {
            continue;
        }
        if let Some(share) = parse_pool_share_tx(mem) {
            let age = spend_height.saturating_sub(share.parent_height);
            let parent_known = (share.parent_height as usize) < chain.blocks.len();
            if parent_known && age > 0 && age <= settings.pools.share_stale_blocks {
                state.seen_share_keys.insert(pool_share_key(&share));
                state.add_active_unchecked(&share);
            }
        }
    }
    if is_share {
        return validate_share_tx_against_context(
            tx,
            settings,
            &chain.blocks,
            spend_height,
            &registry,
            &mut state,
        );
    }
    let fee = crate::validate_tx_contextual(tx, &chain.utxos, spend_height, settings, true)?;
    validate_pool_action_tx(tx, fee, spend_height, settings, &mut registry, &chain.utxos)
}

pub fn validate_pools_block(
    block: &Block,
    prior_blocks: &[Block],
    base_utxos: &HashMap<OutPoint, CoinRecord>,
    height: u32,
    settings: &Settings,
) -> Result<()> {
    let block_has_pool_marker = parse_pool_block_marker(block).is_some();
    let block_has_pool_txs = block
        .transactions
        .iter()
        .skip(1)
        .any(|tx| is_pool_share_transaction(tx) || pool_action_count(tx, settings) > 0);
    if !block_has_pool_marker && !block_has_pool_txs {
        return Ok(());
    }
    if !settings.features.pooled_mining_enabled || !settings.pools.enabled {
        bail!("block contains pool data but pooled mining is disabled");
    }
    if !pools_active(settings, height) {
        bail!(
            "pool protocol data before activation height {}",
            settings.pools.activation_height
        );
    }

    let mut registry = pools_registry_from_blocks(settings, prior_blocks)?;
    let mut state = PoolWindowState::from_blocks(settings, prior_blocks, height);
    let mut scratch = base_utxos.clone();
    let mut fees = 0u128;
    let mut share_txs = 0usize;

    for tx in block.transactions.iter().skip(1) {
        let fee = crate::validate_tx_contextual(tx, &scratch, height, settings, true)?;
        if is_pool_share_transaction(tx) {
            share_txs = share_txs.saturating_add(1);
            if share_txs > settings.pools.max_share_txs_per_block {
                bail!("too many pool share txs in block");
            }
            validate_share_tx_against_context(
                tx,
                settings,
                prior_blocks,
                height,
                &registry,
                &mut state,
            )?;
        } else {
            validate_pool_action_tx(tx, fee, height, settings, &mut registry, &scratch)?;
            crate::connect_tx_utxos(tx, &mut scratch, height, false)?;
        }
        fees = fees.checked_add(fee as u128).context("fee overflow")?;
    }

    if let Some(pool_id) = parse_pool_block_marker(block) {
        let total_reward = crate::block_subsidy(height as u64, settings) as u128 + fees;
        let expected =
            expected_pool_coinbase_outputs(settings, prior_blocks, pool_id, total_reward)?;
        let actual = &block
            .transactions
            .first()
            .context("missing coinbase")?
            .outputs;
        if actual != &expected {
            bail!("pool coinbase payout mismatch");
        }
    }
    Ok(())
}

pub fn pool_share_scores_from_blocks(
    settings: &Settings,
    blocks: &[Block],
    spend_height: u32,
    pool_id: Hash256,
) -> HashMap<String, u128> {
    let mut scores = HashMap::new();
    let window_start = spend_height.saturating_sub(settings.pools.share_window_blocks);
    let start_idx = window_start.max(1) as usize;
    let end_idx = (spend_height as usize).min(blocks.len());
    for (height, block) in blocks.iter().enumerate().take(end_idx).skip(start_idx) {
        let height = height as u32;
        if height >= spend_height {
            continue;
        }
        for tx in block.transactions.iter().skip(1) {
            if let Some(share) = parse_pool_share_tx(tx) {
                if share.pool_id == pool_id {
                    *scores.entry(share.miner_address).or_insert(0) += 1;
                }
            }
        }
    }
    scores
}

pub fn pool_active_miners_from_blocks(
    settings: &Settings,
    blocks: &[Block],
    spend_height: u32,
    pool_id: Hash256,
) -> HashSet<String> {
    pool_share_scores_from_blocks(settings, blocks, spend_height, pool_id)
        .into_keys()
        .collect()
}

fn split_amount(
    total: u128,
    manager: &str,
    commission_bps: u16,
    mut scores: Vec<(String, u128)>,
) -> Result<(u128, u128, Vec<PoolPayoutEntry>)> {
    scores.retain(|(_, score)| *score > 0);
    scores.sort_by(|a, b| a.0.cmp(&b.0));
    let total_score: u128 = scores.iter().map(|(_, s)| *s).sum();
    if total_score == 0 {
        bail!("pool has no shares in PPLNS window");
    }
    let commission = total
        .checked_mul(commission_bps as u128)
        .context("commission overflow")?
        / 10_000u128;
    let miner_total = total
        .checked_sub(commission)
        .context("commission exceeds total")?;
    let mut entries = Vec::new();
    let mut floors_sum = 0u128;
    let mut remainders = Vec::<(String, u128)>::new();
    for (addr, score) in scores {
        let product = miner_total.checked_mul(score).context("payout overflow")?;
        let amount = product / total_score;
        let rem = product % total_score;
        floors_sum = floors_sum.checked_add(amount).context("payout overflow")?;
        entries.push(PoolPayoutEntry {
            address: addr.clone(),
            score,
            amount,
        });
        remainders.push((addr, rem));
    }
    let mut leftover = miner_total
        .checked_sub(floors_sum)
        .context("payout remainder underflow")?;
    remainders.sort_by(|a, b| match b.1.cmp(&a.1) {
        Ordering::Equal => a.0.cmp(&b.0),
        other => other,
    });
    for (addr, _) in remainders {
        if leftover == 0 {
            break;
        }
        if let Some(entry) = entries.iter_mut().find(|e| e.address == addr) {
            entry.amount = entry.amount.checked_add(1).context("payout overflow")?;
            leftover -= 1;
        }
    }
    entries.sort_by(|a, b| a.address.cmp(&b.address));
    // Keep the manager commission separate from miner entries even if the manager also mined shares.
    let _ = manager;
    Ok((commission, miner_total, entries))
}

pub fn pool_payout_plan(
    settings: &Settings,
    prior_blocks: &[Block],
    pool_id: Hash256,
    total_amount: u128,
) -> Result<PoolPayoutPlan> {
    let height = prior_blocks.len() as u32;
    let registry = pools_registry_from_blocks(settings, prior_blocks)?;
    let pool = registry
        .get(&pool_id)
        .context("unknown pool_id for payout")?;
    let scores = pool_share_scores_from_blocks(settings, prior_blocks, height, pool_id)
        .into_iter()
        .collect::<Vec<_>>();
    let total_score: u128 = scores.iter().map(|(_, s)| *s).sum();
    let (commission_amount, miner_amount, entries) = split_amount(
        total_amount,
        &pool.manager_address,
        pool.commission_bps,
        scores,
    )?;
    Ok(PoolPayoutPlan {
        pool_id,
        manager_address: pool.manager_address.clone(),
        commission_bps: pool.commission_bps,
        total_score,
        commission_amount,
        miner_amount,
        entries,
    })
}

pub fn expected_pool_coinbase_outputs(
    settings: &Settings,
    prior_blocks: &[Block],
    pool_id: Hash256,
    total_reward_atoms: u128,
) -> Result<Vec<TxOut>> {
    if total_reward_atoms > u64::MAX as u128 {
        bail!("pool reward overflow");
    }
    let plan = pool_payout_plan(settings, prior_blocks, pool_id, total_reward_atoms)?;
    let mut outputs = Vec::new();
    if plan.commission_amount > 0 {
        let manager =
            Address::parse_with_prefix(&plan.manager_address, &settings.network.address_prefix)?;
        outputs.push(TxOut {
            value: Amount::from_atoms(plan.commission_amount as u64)?,
            script_pubkey: manager.script_pubkey(),
        });
    }
    for entry in plan.entries {
        if entry.amount == 0 {
            continue;
        }
        let addr = Address::parse_with_prefix(&entry.address, &settings.network.address_prefix)?;
        outputs.push(TxOut {
            value: Amount::from_atoms(entry.amount as u64)?,
            script_pubkey: addr.script_pubkey(),
        });
    }
    if outputs.is_empty() {
        bail!("pool payout produced no outputs");
    }
    Ok(outputs)
}

pub fn pool_block_marker_script(height: u32, extra_nonce: u64, pool_id: Hash256) -> ScriptBuf {
    let mut sig = Vec::new();
    sig.extend_from_slice(&height.to_le_bytes());
    sig.extend_from_slice(&extra_nonce.to_le_bytes());
    sig.extend_from_slice(b"/QUB Core v1.4.8/");
    sig.extend_from_slice(POOL_BLOCK_PREFIX);
    sig.extend_from_slice(pool_id.to_string().as_bytes());
    ScriptBuf(sig)
}

pub fn parse_pool_block_marker(block: &Block) -> Option<Hash256> {
    let coinbase = block.transactions.first()?;
    let script = coinbase.inputs.first()?.signature_script.as_bytes();
    let pos = script
        .windows(POOL_BLOCK_PREFIX.len())
        .position(|w| w == POOL_BLOCK_PREFIX)?;
    let rest = &script[pos + POOL_BLOCK_PREFIX.len()..];
    let s = std::str::from_utf8(rest).ok()?.trim();
    let id_hex = s
        .split(|c: char| !c.is_ascii_hexdigit())
        .next()
        .unwrap_or(s);
    Hash256::from_hex(id_hex).ok()
}

pub fn jin_fee_receivers_for_block(
    settings: &Settings,
    prior_blocks: &[Block],
    block: &Block,
    total_fee_units: u128,
) -> Result<Vec<(String, u128)>> {
    if total_fee_units == 0 {
        return Ok(Vec::new());
    }
    if let Some(pool_id) = parse_pool_block_marker(block) {
        let plan = pool_payout_plan(settings, prior_blocks, pool_id, total_fee_units)?;
        let mut out = Vec::new();
        if plan.commission_amount > 0 {
            out.push((plan.manager_address.clone(), plan.commission_amount));
        }
        for entry in plan.entries {
            if entry.amount > 0 {
                out.push((entry.address, entry.amount));
            }
        }
        return Ok(merge_amounts(out));
    }
    if let Some(addr) = block
        .transactions
        .first()
        .and_then(|tx| tx.outputs.first())
        .and_then(|out| {
            address_from_script_pubkey(&settings.network.address_prefix, &out.script_pubkey)
        })
        .map(|a| a.to_string())
    {
        return Ok(vec![(addr, total_fee_units)]);
    }
    Ok(Vec::new())
}

fn merge_amounts(rows: Vec<(String, u128)>) -> Vec<(String, u128)> {
    let mut map = HashMap::<String, u128>::new();
    for (addr, amount) in rows {
        *map.entry(addr).or_insert(0) += amount;
    }
    let mut out = map.into_iter().collect::<Vec<_>>();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub fn pool_create_summary_json(
    settings: &Settings,
    chain: &ChainState,
    pool: &PoolRecord,
) -> serde_json::Value {
    let active =
        pool_active_miners_from_blocks(settings, &chain.blocks, chain.height() + 1, pool.pool_id);
    let scores =
        pool_share_scores_from_blocks(settings, &chain.blocks, chain.height() + 1, pool.pool_id);
    let recent_shares: u128 = scores.values().copied().sum();
    serde_json::json!({
        "pool_id": pool.pool_id.to_string(),
        "name": pool.name,
        "manager_address": pool.manager_address,
        "commission_bps": pool.commission_bps,
        "commission_percent": format!("{:.2}", pool.commission_bps as f64 / 100.0),
        "capacity_slots": pool.capacity_slots,
        "active_miners": active.len(),
        "open_slots": pool.capacity_slots.saturating_sub(active.len() as u32),
        "recent_shares": recent_shares,
        "created_height": pool.created_height,
        "create_txid": pool.create_txid.to_string(),
        "total_paid_atoms": pool.total_paid_atoms,
        "total_paid_qub": Amount::from_atoms(pool.total_paid_atoms).map(|a| a.to_string()).unwrap_or_else(|_| pool.total_paid_atoms.to_string()),
    })
}

pub fn pool_share_target_as_hex(settings: &Settings) -> Result<String> {
    Ok(format!("0x{:08x}", parse_share_bits(settings)?))
}
