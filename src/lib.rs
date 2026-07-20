use anyhow::{bail, Context, Result};
use num_bigint::BigUint;
use num_traits::{One, Zero};
use rand::rngs::OsRng;
use ripemd::Ripemd160;
use secp256k1::{ecdsa::Signature, Message, PublicKey, Secp256k1, SecretKey};
use serde::de::{DeserializeSeed, Error as DeError, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod p2p;
pub mod pools;
pub mod rpc;
pub use pools::*;

pub const ATOMS_PER_QUB: u64 = 100_000_000;
pub const TX_VERSION_POOL_SHARE: u32 = 0x5155_4253; // QUBS: zero-fee PoW-gated pool-share tx
pub const MAX_SEND_ENTRIES_PER_TX: usize = 256;
pub const BLAST_SCRIPT_PREFIX: &[u8] = b"BLAST1|";
pub const MAX_MONEY_QUB: u64 = 21_000_000;
pub const MAX_MONEY_ATOMS: u64 = MAX_MONEY_QUB * ATOMS_PER_QUB;
pub const JIN_DECIMALS: u8 = 18;
pub const JIN_UNITS_PER_COIN: u128 = 1_000_000_000_000_000_000u128;
pub const JIN_TOTAL_COINS: u128 = 105_000_000u128;
pub const JIN_TOTAL_SUPPLY_UNITS: u128 = JIN_TOTAL_COINS * JIN_UNITS_PER_COIN;
const P2PKH_PREFIX: [u8; 3] = [0x76, 0xa9, 0x14];
const P2PKH_SUFFIX: [u8; 2] = [0x88, 0xac];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Amount(pub u64);

impl Amount {
    pub fn from_atoms(value: u64) -> Result<Self> {
        if value > MAX_MONEY_ATOMS {
            bail!("amount exceeds MAX_MONEY");
        }
        Ok(Self(value))
    }
    pub fn atoms(self) -> u64 {
        self.0
    }
    pub fn checked_add(self, rhs: Self) -> Result<Self> {
        Self::from_atoms(self.0.checked_add(rhs.0).context("amount overflow")?)
    }
    pub fn checked_sub(self, rhs: Self) -> Result<Self> {
        Self::from_atoms(self.0.checked_sub(rhs.0).context("amount underflow")?)
    }
    pub fn to_qub_string(self) -> String {
        let whole = self.0 / ATOMS_PER_QUB;
        let frac = self.0 % ATOMS_PER_QUB;
        if frac == 0 {
            return whole.to_string();
        }
        let mut s = format!("{frac:08}");
        while s.ends_with('0') {
            s.pop();
        }
        format!("{whole}.{s}")
    }
}
impl Display for Amount {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_qub_string())
    }
}
impl FromStr for Amount {
    type Err = anyhow::Error;
    fn from_str(input: &str) -> Result<Self> {
        let input = input.trim();
        if input.is_empty() || input.starts_with('-') {
            bail!("invalid amount");
        }
        let mut parts = input.split('.');
        let whole = parts.next().unwrap_or("0").parse::<u64>()?;
        let frac = parts.next();
        if parts.next().is_some() {
            bail!("invalid amount");
        }
        let mut atoms = whole
            .checked_mul(ATOMS_PER_QUB)
            .context("amount overflow")?;
        if let Some(frac) = frac {
            if frac.len() > 8 || !frac.chars().all(|c| c.is_ascii_digit()) {
                bail!("invalid fractional amount");
            }
            let mut padded = frac.to_string();
            while padded.len() < 8 {
                padded.push('0');
            }
            atoms = atoms
                .checked_add(padded.parse::<u64>()?)
                .context("amount overflow")?;
        }
        Self::from_atoms(atoms)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash256(pub [u8; 32]);
impl Hash256 {
    pub fn zero() -> Self {
        Self([0u8; 32])
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn double_sha256(data: &[u8]) -> Self {
        let first = Sha256::digest(data);
        let second = Sha256::digest(first);
        let mut out = [0u8; 32];
        out.copy_from_slice(&second);
        Self(out)
    }
    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s.trim())?;
        if bytes.len() != 32 {
            bail!("expected 32-byte hash");
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        Ok(Self(out))
    }
}
impl Display for Hash256 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}
impl FromStr for Hash256 {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Self::from_hex(s)
    }
}
impl Serialize for Hash256 {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self.0))
    }
}
impl<'de> Deserialize<'de> for Hash256 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(DeError::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OutPoint {
    pub txid: Hash256,
    pub vout: u32,
}
impl OutPoint {
    pub fn null() -> Self {
        Self {
            txid: Hash256::zero(),
            vout: u32::MAX,
        }
    }
    pub fn key(&self) -> String {
        format!("{}:{}", self.txid, self.vout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptBuf(pub Vec<u8>);
impl ScriptBuf {
    pub fn empty() -> Self {
        Self(Vec::new())
    }
    pub fn from_bytes(v: Vec<u8>) -> Self {
        Self(v)
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxIn {
    pub previous_output: OutPoint,
    pub signature_script: ScriptBuf,
    pub sequence: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxOut {
    pub value: Amount,
    pub script_pubkey: ScriptBuf,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub version: u32,
    pub inputs: Vec<TxIn>,
    pub outputs: Vec<TxOut>,
    pub locktime: u32,
}

fn write_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_varint(out: &mut Vec<u8>, v: u64) {
    match v {
        0..=0xfc => out.push(v as u8),
        0xfd..=0xffff => {
            out.push(0xfd);
            out.extend_from_slice(&(v as u16).to_le_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(0xfe);
            out.extend_from_slice(&(v as u32).to_le_bytes());
        }
        _ => {
            out.push(0xff);
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
}
fn write_varbytes(out: &mut Vec<u8>, b: &[u8]) {
    write_varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

impl Transaction {
    pub fn is_coinbase(&self) -> bool {
        self.version != TX_VERSION_POOL_SHARE
            && self.inputs.len() == 1
            && self.inputs[0].previous_output == OutPoint::null()
    }
    pub fn txid(&self) -> Hash256 {
        Hash256::double_sha256(&self.serialize(None))
    }
    pub fn serialize_base(&self) -> Vec<u8> {
        self.serialize(None)
    }
    fn serialize(&self, override_script: Option<(usize, &ScriptBuf)>) -> Vec<u8> {
        let mut out = Vec::new();
        write_u32(&mut out, self.version);
        write_varint(&mut out, self.inputs.len() as u64);
        for (i, input) in self.inputs.iter().enumerate() {
            out.extend_from_slice(input.previous_output.txid.as_bytes());
            write_u32(&mut out, input.previous_output.vout);
            match override_script {
                Some((idx, script)) if idx == i => write_varbytes(&mut out, script.as_bytes()),
                Some(_) => write_varbytes(&mut out, &[]),
                None => write_varbytes(&mut out, input.signature_script.as_bytes()),
            }
            write_u32(&mut out, input.sequence);
        }
        write_varint(&mut out, self.outputs.len() as u64);
        for output in &self.outputs {
            write_u64(&mut out, output.value.atoms());
            write_varbytes(&mut out, output.script_pubkey.as_bytes());
        }
        write_u32(&mut out, self.locktime);
        out
    }
    pub fn sighash_all(&self, input_index: usize, prev_script: &ScriptBuf) -> Result<Hash256> {
        if input_index >= self.inputs.len() {
            bail!("sighash input index out of range");
        }
        let mut raw = self.serialize(Some((input_index, prev_script)));
        write_u32(&mut raw, 1);
        Ok(Hash256::double_sha256(&raw))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockHeader {
    pub version: u32,
    pub prev_block_hash: Hash256,
    pub merkle_root: Hash256,
    pub time: u32,
    pub bits: u32,
    pub nonce: u32,
}
impl BlockHeader {
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(80);
        write_u32(&mut out, self.version);
        out.extend_from_slice(self.prev_block_hash.as_bytes());
        out.extend_from_slice(self.merkle_root.as_bytes());
        write_u32(&mut out, self.time);
        write_u32(&mut out, self.bits);
        write_u32(&mut out, self.nonce);
        out
    }
    pub fn hash(&self) -> Hash256 {
        Hash256::double_sha256(&self.serialize())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}
impl Block {
    pub fn block_hash(&self) -> Hash256 {
        self.header.hash()
    }
    pub fn compute_merkle_root(&self) -> Hash256 {
        merkle_root(
            &self
                .transactions
                .iter()
                .map(|tx| tx.txid())
                .collect::<Vec<_>>(),
        )
    }
}
pub fn merkle_root(leaves: &[Hash256]) -> Hash256 {
    if leaves.is_empty() {
        return Hash256::zero();
    }
    let mut layer = leaves.to_vec();
    while layer.len() > 1 {
        if layer.len() % 2 == 1 {
            let last = *layer.last().unwrap();
            layer.push(last);
        }
        let mut next = Vec::new();
        for pair in layer.chunks(2) {
            let mut data = Vec::with_capacity(64);
            data.extend_from_slice(pair[0].as_bytes());
            data.extend_from_slice(pair[1].as_bytes());
            next.push(Hash256::double_sha256(&data));
        }
        layer = next;
    }
    layer[0]
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address {
    pub prefix: String,
    pub payload: [u8; 20],
}
impl Address {
    pub fn new(prefix: impl Into<String>, payload: [u8; 20]) -> Self {
        Self {
            prefix: prefix.into(),
            payload,
        }
    }
    pub fn from_pubkey_hash(prefix: &str, payload: [u8; 20]) -> Self {
        Self::new(prefix, payload)
    }
    pub fn script_pubkey(&self) -> ScriptBuf {
        let mut out = Vec::with_capacity(25);
        out.extend_from_slice(&P2PKH_PREFIX);
        out.extend_from_slice(&self.payload);
        out.extend_from_slice(&P2PKH_SUFFIX);
        ScriptBuf(out)
    }
    pub fn parse_with_prefix(s: &str, prefix: &str) -> Result<Self> {
        let a = Self::from_str(s)?;
        if a.prefix != prefix {
            bail!("address prefix mismatch");
        }
        Ok(a)
    }
}
impl Display for Address {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut body = Vec::with_capacity(24);
        body.extend_from_slice(&self.payload);
        body.extend_from_slice(&address_checksum(&self.prefix, &self.payload));
        write!(f, "{}1{}", self.prefix, hex::encode(body))
    }
}
impl FromStr for Address {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let Some((prefix, rest)) = s.trim().split_once('1') else {
            bail!("address missing separator");
        };
        if prefix.is_empty() || rest.len() != 48 || !prefix.chars().all(|c| c.is_ascii_lowercase())
        {
            bail!("invalid address format");
        }
        let bytes = hex::decode(rest)?;
        let mut payload = [0u8; 20];
        payload.copy_from_slice(&bytes[..20]);
        if &bytes[20..24] != address_checksum(prefix, &payload).as_slice() {
            bail!("invalid address checksum");
        }
        Ok(Self::new(prefix, payload))
    }
}
fn address_checksum(prefix: &str, payload: &[u8; 20]) -> [u8; 4] {
    let mut data = prefix.as_bytes().to_vec();
    data.extend_from_slice(payload);
    let first = Sha256::digest(&data);
    let second = Sha256::digest(first);
    let mut out = [0u8; 4];
    out.copy_from_slice(&second[..4]);
    out
}
fn p2pkh_payload(script: &ScriptBuf) -> Option<[u8; 20]> {
    let b = script.as_bytes();
    if b.len() != 25 || &b[0..3] != P2PKH_PREFIX.as_slice() || &b[23..25] != P2PKH_SUFFIX.as_slice()
    {
        return None;
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&b[3..23]);
    Some(out)
}

pub fn generate_secret_key() -> SecretKey {
    let mut rng = OsRng;
    SecretKey::new(&mut rng)
}
pub fn secret_key_from_hex(s: &str) -> Result<SecretKey> {
    let b = hex::decode(s.trim())?;
    if b.len() != 32 {
        bail!("secret key must be 32 bytes");
    }
    Ok(SecretKey::from_slice(&b)?)
}
pub fn secret_key_to_hex(secret: &SecretKey) -> String {
    hex::encode(secret.secret_bytes())
}
pub fn public_key_from_secret(secret: &SecretKey) -> PublicKey {
    PublicKey::from_secret_key(&Secp256k1::new(), secret)
}
pub fn public_key_hash(pk: &[u8]) -> [u8; 20] {
    let sha = Sha256::digest(pk);
    let ripe = Ripemd160::digest(sha);
    let mut out = [0u8; 20];
    out.copy_from_slice(&ripe);
    out
}
pub fn address_from_public_key(prefix: &str, pk: &PublicKey) -> Address {
    Address::from_pubkey_hash(prefix, public_key_hash(&pk.serialize()))
}
fn sign_hash(secret: &SecretKey, hash: Hash256) -> Result<Vec<u8>> {
    let msg = Message::from_digest_slice(hash.as_bytes())?;
    let sig = Secp256k1::new().sign_ecdsa(&msg, secret);
    Ok(sig.serialize_der().to_vec())
}
fn verify_hash(pk: &[u8], sig: &[u8], hash: Hash256) -> bool {
    let Ok(pk) = PublicKey::from_slice(pk) else {
        return false;
    };
    let Ok(sig) = Signature::from_der(sig) else {
        return false;
    };
    let Ok(msg) = Message::from_digest_slice(hash.as_bytes()) else {
        return false;
    };
    Secp256k1::verification_only()
        .verify_ecdsa(&msg, &sig, &pk)
        .is_ok()
}
fn encode_sig_script(pk: &[u8], sig: &[u8]) -> Result<ScriptBuf> {
    if pk.len() > u16::MAX as usize || sig.len() > u16::MAX as usize {
        bail!("signature script too large");
    }
    let mut out = Vec::new();
    out.extend_from_slice(&(pk.len() as u16).to_le_bytes());
    out.extend_from_slice(pk);
    out.extend_from_slice(&(sig.len() as u16).to_le_bytes());
    out.extend_from_slice(sig);
    Ok(ScriptBuf(out))
}
fn decode_sig_script(script: &ScriptBuf) -> Result<(Vec<u8>, Vec<u8>)> {
    let b = script.as_bytes();
    if b.len() < 4 {
        bail!("signature script too short");
    }
    let pk_len = u16::from_le_bytes([b[0], b[1]]) as usize;
    if b.len() < 2 + pk_len + 2 {
        bail!("bad public key length");
    }
    let sig_off = 2 + pk_len;
    let sig_len = u16::from_le_bytes([b[sig_off], b[sig_off + 1]]) as usize;
    let sig_start = sig_off + 2;
    if sig_start + sig_len != b.len() {
        bail!("bad signature length");
    }
    Ok((b[2..2 + pk_len].to_vec(), b[sig_start..].to_vec()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub network: NetworkSettings,
    pub node: NodeSettings,
    pub p2p: P2PSettings,
    pub rpc: RpcSettings,
    pub mempool: MempoolSettings,
    pub mining: MiningSettings,
    pub wallet: WalletSettings,
    pub consensus: ConsensusSettings,
    pub features: FeatureSettings,
    pub qns: QnsSettings,
    #[serde(default = "default_jin_settings")]
    pub jin: JinSettings,
    #[serde(default = "default_jin_swap_settings")]
    pub jin_swap: JinSwapSettings,
    #[serde(default = "default_pools_settings")]
    pub pools: PoolsSettings,
    #[serde(default = "default_library_settings")]
    pub library: LibrarySettings,
    #[serde(default = "default_verified_governance_settings")]
    pub verified_governance: VerifiedGovernanceSettings,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSettings {
    pub name: String,
    pub magic: String,
    pub default_port: u16,
    pub address_prefix: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSettings {
    pub data_dir: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2PSettings {
    pub enabled: bool,
    pub bind: String,
    pub advertise_addr: String,
    pub max_inbound_peers: usize,
    pub max_outbound_peers: usize,
    pub max_message_bytes: usize,
    pub max_blocks_per_message: usize,
    pub max_peer_errors: u32,
    pub connect_interval_secs: u64,
    pub peer_file: String,
    pub bootnodes: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcSettings {
    pub enabled: bool,
    pub bind: String,
    #[serde(default)]
    pub auth_token: String,
    #[serde(default)]
    pub auth_token_file: String,
    #[serde(default)]
    pub allow_remote: bool,
    #[serde(default)]
    pub allowed_cidrs: Vec<String>,
    #[serde(default = "default_rpc_max_connections")]
    pub max_connections: usize,
    #[serde(default = "default_rpc_requests_per_minute")]
    pub max_requests_per_minute: u32,
    #[serde(default = "default_rpc_max_header_bytes")]
    pub max_header_bytes: usize,
    #[serde(default = "default_rpc_max_body_bytes")]
    pub max_body_bytes: usize,
    #[serde(default = "default_rpc_read_timeout_secs")]
    pub read_timeout_secs: u64,
    #[serde(default = "default_rpc_write_timeout_secs")]
    pub write_timeout_secs: u64,
    #[serde(default = "default_rpc_job_ttl_secs")]
    pub job_ttl_secs: u64,
    #[serde(default = "default_rpc_max_cached_jobs")]
    pub max_cached_jobs: usize,
    #[serde(default = "default_rpc_max_template_batch")]
    pub max_template_batch: usize,
}

fn default_rpc_max_connections() -> usize {
    64
}
fn default_rpc_requests_per_minute() -> u32 {
    600
}
fn default_rpc_max_header_bytes() -> usize {
    16 * 1024
}
fn default_rpc_max_body_bytes() -> usize {
    2 * 1024 * 1024
}
fn default_rpc_read_timeout_secs() -> u64 {
    5
}
fn default_rpc_write_timeout_secs() -> u64 {
    15
}
fn default_rpc_job_ttl_secs() -> u64 {
    90
}
fn default_rpc_max_cached_jobs() -> usize {
    256
}
fn default_rpc_max_template_batch() -> usize {
    32
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolSettings {
    pub max_transactions: usize,
    pub min_relay_fee_atoms: u64,
}

// HF115/v1.7.3 local policy caps. These are not consensus rules. They keep a
// public-sale/Library/mempool burst from turning every block-template rebuild or
// peer mempool exchange into a long CPU/network stall. Transactions not retained
// locally can be rebroadcast by wallets; valid mined blocks remain consensus-valid.
pub const HF115_EFFECTIVE_MEMPOOL_MAX_TRANSACTIONS: usize = 20_000;
pub const HF115_MAX_TEMPLATE_SCAN_TXS: usize = 2_048;

pub fn effective_mempool_max_transactions(settings: &Settings) -> usize {
    settings
        .mempool
        .max_transactions
        .max(1)
        .min(HF115_EFFECTIVE_MEMPOOL_MAX_TRANSACTIONS)
}

pub fn hf115_template_scan_limit(settings: &Settings) -> usize {
    settings
        .consensus
        .max_block_transactions
        .saturating_mul(2)
        .max(256)
        .min(HF115_MAX_TEMPLATE_SCAN_TXS)
}

// HF117/v1.7.5 is a non-consensus hotfix. It protects locally-created
// transactions against stale-chain replacement by keeping exact raw txs in a
// wallet outbox until finality, and by resurrecting non-coinbase txs from
// disconnected suffix blocks during reorg/adoption.
pub const HF117_PENDING_TX_CONFIRMATIONS: u32 = 2;
pub const HF117_PENDING_TX_MAX_RECORDS: usize = 512;
pub const HF117_PENDING_TX_MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningSettings {
    pub enabled: bool,
    pub threads: usize,
    pub duty_cycle_percent: u8,
    pub miner_address: String,
    pub auto_mine_mempool: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletSettings {
    pub plaintext_keys_allowed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusSettings {
    pub version: u32,
    pub max_money_atoms: u64,
    pub subsidy_halving_interval: u64,
    pub initial_subsidy_atoms: u64,
    pub coinbase_maturity: u32,
    pub target_spacing_secs: u32,
    pub difficulty_adjustment_interval: u32,
    pub difficulty_max_adjustment_factor: u32,
    pub max_future_time_secs: u32,
    pub max_block_transactions: usize,
    pub pow_bits: String,
    pub genesis_time: u32,
    pub genesis_bits: String,
    pub genesis_nonce: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSettings {
    pub pooled_mining_enabled: bool,
    pub jin_native_coin_enabled: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QnsSettings {
    pub enabled: bool,
    pub activation_height: u32,
    #[serde(default = "default_disabled_activation_height")]
    pub miner_split_activation_height: u32,
    pub max_label_chars: u8,
    pub marker_output_atoms: u64,
    pub base_registration_atoms: u64,
    pub price_coefficient_atoms: u64,
    pub protocol_address: String,
    pub protocol_name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JinSettings {
    pub enabled: bool,
    pub activation_height: u32,
    #[serde(default = "default_disabled_activation_height")]
    pub conversion_activation_height: u32,
    pub decimals: u8,
    pub total_supply_units: String,
    pub protocol_address: String,
    pub marker_output_atoms: u64,
    pub default_fee_units: String,
    pub allow_fee_in_jin: bool,
    pub bridge_name: String,
}
fn default_disabled_activation_height() -> u32 {
    u32::MAX
}
fn default_jin_settings() -> JinSettings {
    JinSettings {
        enabled: false,
        activation_height: u32::MAX,
        conversion_activation_height: u32::MAX,
        decimals: JIN_DECIMALS,
        total_supply_units: JIN_TOTAL_SUPPLY_UNITS.to_string(),
        protocol_address: String::new(),
        marker_output_atoms: 1,
        default_fee_units: "1000000000000000".to_string(),
        allow_fee_in_jin: false,
        bridge_name: "jin.bridge".to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JinSwapSettings {
    pub enabled: bool,
    pub activation_height: u32,
    pub marker_output_atoms: u64,
    pub sale_total_units: String,
    pub sale_batch_units: String,
    pub sale_start_price_atoms_per_jin: u64,
    pub sale_step_price_atoms_per_jin: u64,
    pub protocol_fee_bps: u64,
}
fn default_jin_swap_settings() -> JinSwapSettings {
    JinSwapSettings {
        enabled: true,
        activation_height: u32::MAX,
        marker_output_atoms: 1,
        sale_total_units: (85_000_000u128 * JIN_UNITS_PER_COIN).to_string(),
        sale_batch_units: (1_000_000u128 * JIN_UNITS_PER_COIN).to_string(),
        sale_start_price_atoms_per_jin: 1_000_000, // 0.01000000 QUB per JIN
        sale_step_price_atoms_per_jin: 100_000,    // +0.00100000 QUB per 1M JIN batch
        protocol_fee_bps: 10, // 0.10% total, split 50/50 between miner fee and JIN protocol address
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySettings {
    pub enabled: bool,
    pub activation_height: u32,
    pub marker_output_atoms: u64,
    pub base_post_fee_atoms: u64,
    pub byte_fee_atoms: u64,
    pub max_title_bytes: usize,
    pub max_category_bytes: usize,
    pub max_page_bytes: usize,
    pub max_pages_per_post: usize,
    pub max_comment_bytes: usize,
    pub max_comment_depth: usize,
}
fn default_library_settings() -> LibrarySettings {
    LibrarySettings {
        enabled: true,
        activation_height: u32::MAX,
        marker_output_atoms: 1,
        base_post_fee_atoms: 10_000,
        byte_fee_atoms: 100,
        max_title_bytes: 120,
        max_category_bytes: 48,
        max_page_bytes: 2048,
        max_pages_per_post: 16,
        max_comment_bytes: 1024,
        max_comment_depth: 16,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedGovernanceSettings {
    pub enabled: bool,
    pub activation_height: u32,
    pub marker_output_atoms: u64,
    pub wallet_bond_units: String,
    pub pool_bond_units: String,
    pub moderator_bond_units: String,
    pub report_bond_units: String,
    pub appeal_bond_units: String,
    pub min_lock_blocks: u32,
    pub max_avatar_bytes: u32,
    pub max_display_name_bytes: usize,
    pub max_avatar_ref_bytes: usize,
    pub max_evidence_ref_bytes: usize,
    pub max_active_reports: usize,
    pub appeal_window_blocks: u32,
    pub max_initial_slash_bps: u16,
}
fn default_verified_governance_settings() -> VerifiedGovernanceSettings {
    VerifiedGovernanceSettings {
        enabled: true,
        activation_height: u32::MAX,
        marker_output_atoms: 1,
        wallet_bond_units: (10_000u128 * JIN_UNITS_PER_COIN).to_string(),
        pool_bond_units: (25_000u128 * JIN_UNITS_PER_COIN).to_string(),
        moderator_bond_units: (50_000u128 * JIN_UNITS_PER_COIN).to_string(),
        report_bond_units: (100u128 * JIN_UNITS_PER_COIN).to_string(),
        appeal_bond_units: (250u128 * JIN_UNITS_PER_COIN).to_string(),
        min_lock_blocks: 7_200,
        max_avatar_bytes: 32 * 1024,
        max_display_name_bytes: 48,
        max_avatar_ref_bytes: 256,
        max_evidence_ref_bytes: 256,
        max_active_reports: 4096,
        appeal_window_blocks: 7_200,
        max_initial_slash_bps: 2500,
    }
}

pub const MAINNET_QNS_ACTIVATION_HEIGHT: u32 = 1000;
pub const MAINNET_JIN_ACTIVATION_HEIGHT: u32 = 5555;
pub const MAINNET_QNS_MINER_SPLIT_ACTIVATION_HEIGHT: u32 = 8305;
pub const MAINNET_JIN_CONVERSION_ACTIVATION_HEIGHT: u32 = 8305;
pub const MAINNET_JIN_CONVERSION_DISABLE_HEIGHT: u32 = 10720;
pub const MAINNET_JIN_SWAP_ACTIVATION_HEIGHT: u32 = 10720;
pub const TESTNET_JIN_SWAP_ACTIVATION_HEIGHT: u32 = 3520;
pub const MAINNET_POOLS_ACTIVATION_HEIGHT: u32 = 9999;
pub const MAINNET_POOLS_PROTOCOL_ADDRESS: &str =
    "qub1b4acf65ce91f786de20baa177cdb643869d1fdd5728eec0e";

// HF49/v1.4.8 fork-safety checkpoint. This is intentionally a mainnet
// consensus checkpoint on the post-pool canonical branch observed by the
// operator seed and the overwhelming majority of active miners after the
// #9999 pooled-mining activation. Any alternate branch that diverged before
// this height is rejected by upgraded nodes, even if a stale/NAT peer reports
// a larger height through the peer registry.
pub const MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT: u32 = 10367;
pub const MAINNET_FORK_SAFETY_CHECKPOINT_HASH: &str =
    "21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000";

// DAA v2 is a deliberate consensus upgrade and therefore activates in the
// future. It uses a rolling window every block instead of waiting a full 60
// block interval. This makes the chain recover toward 60s blocks after sudden
// hashrate changes while preserving the already-mined pre-activation history.
pub const MAINNET_DAA_V2_ACTIVATION_HEIGHT: u32 = 10500;
pub const TESTNET_DAA_V2_ACTIVATION_HEIGHT: u32 = 3330;

// HF57 / v1.4.8: Multi-send is normal multi-output transaction support.
// Blast is a future-height protocol extension. It is intentionally activated
// after DAA v2 so the network can stabilize first.
pub const MAINNET_BLAST_ACTIVATION_HEIGHT: u32 = 10600;
pub const TESTNET_BLAST_ACTIVATION_HEIGHT: u32 = 3420;
pub const MAINNET_LIBRARY_ACTIVATION_HEIGHT: u32 = 10550;
pub const TESTNET_LIBRARY_ACTIVATION_HEIGHT: u32 = 3440;
pub const MAINNET_VERIFIED_GOVERNANCE_ACTIVATION_HEIGHT: u32 = 21000;
pub const TESTNET_VERIFIED_GOVERNANCE_ACTIVATION_HEIGHT: u32 = 5000;

// HF120/v1.7.8: Protocol Epoch 2 is a forward-only mainnet chain upgrade.
// It does not roll back history, blacklist addresses, change DAA, or change
// economics. It simply introduces a post-activation block-version gate so all
// miners must run an updated client to remain on the official QUB mainnet.
pub const MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT: u32 = 24000;
pub const PROTOCOL_EPOCH_1_BLOCK_VERSION: u32 = 1;
pub const PROTOCOL_EPOCH_2_BLOCK_VERSION: u32 = 2;

// HF116/v1.7.4: JIN Coin infusion into QUB Coin. This is a mainnet
// consensus activation. Bootstrap moves the final 42 public-sale batches into
// the QUB infusion vault at #16777, creating an exact 2 JIN/QUB starting ratio.
// Accounting is intentionally per QUB atom to avoid fractional-claim drift:
// every future JIN infusion must divide exactly across the current true max
// QUB atom supply, and every QUB melt pays atoms * units_per_qub_atom.
pub const MAINNET_QUB_JIN_INFUSION_ACTIVATION_HEIGHT: u32 = 16777;
// Safety pre-lock: stop selling the final 42 public-sale batches before the
// bootstrap height, so the #16777 42M JIN infusion cannot be drained by late
// manual buys while the mandatory upgrade is rolling out.
pub const MAINNET_QUB_JIN_SALE_RESERVE_LOCK_HEIGHT: u32 = 16666;
pub const QUB_JIN_RESERVED_SALE_BATCHES: u32 = 42;
pub const QUB_JIN_BOOTSTRAP_INFUSION_JIN_COINS: u128 = 42_000_000u128;
pub const QUB_JIN_BOOTSTRAP_INFUSION_UNITS: u128 =
    QUB_JIN_BOOTSTRAP_INFUSION_JIN_COINS * JIN_UNITS_PER_COIN;
pub const QUB_JIN_INFUSION_VAULT: &str = "__qub_jin_infusion_vault__";
pub const DAA_V2_WINDOW_BLOCKS: usize = 20;
pub const DAA_V2_MAX_ADJUSTMENT_FACTOR: u64 = 4;

impl Settings {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let raw = fs::read_to_string(path.as_ref())
            .with_context(|| format!("failed to read {}", path.as_ref().display()))?;
        let mut s: Self = toml::from_str(&raw)?;
        s.apply_network_consensus_overrides();
        s.validate()?;
        Ok(s)
    }

    fn apply_network_consensus_overrides(&mut self) {
        if self.network.name == "mainnet" {
            // Mainnet consensus checkpoints must not depend on stale per-user config files.
            // Installer upgrades preserve user config, so missing v1.2.5 fields would otherwise
            // make updated binaries reject the post-#8305 canonical chain.
            self.qns.activation_height = MAINNET_QNS_ACTIVATION_HEIGHT;
            self.qns.miner_split_activation_height = MAINNET_QNS_MINER_SPLIT_ACTIVATION_HEIGHT;
            self.jin.activation_height = MAINNET_JIN_ACTIVATION_HEIGHT;
            self.jin.conversion_activation_height = MAINNET_JIN_CONVERSION_ACTIVATION_HEIGHT;
            self.jin_swap.enabled = true;
            self.jin_swap.activation_height = MAINNET_JIN_SWAP_ACTIVATION_HEIGHT;
            self.jin_swap.marker_output_atoms = 1;
            self.jin_swap.sale_total_units = (85_000_000u128 * JIN_UNITS_PER_COIN).to_string();
            self.jin_swap.sale_batch_units = (1_000_000u128 * JIN_UNITS_PER_COIN).to_string();
            self.jin_swap.sale_start_price_atoms_per_jin = 1_000_000;
            self.jin_swap.sale_step_price_atoms_per_jin = 100_000;
            self.jin_swap.protocol_fee_bps = 10;
            self.features.pooled_mining_enabled = true;
            self.pools.enabled = true;
            self.pools.activation_height = MAINNET_POOLS_ACTIVATION_HEIGHT;
            self.pools.protocol_name = "pools.qub".to_string();
            self.pools.protocol_address = MAINNET_POOLS_PROTOCOL_ADDRESS.to_string();
            self.pools.max_name_chars = 32;
            self.pools.max_name_bytes = 128;
            self.pools.marker_output_atoms = 1;
            self.pools.base_create_atoms = 25 * ATOMS_PER_QUB;
            self.pools.base_capacity_slots = 8;
            self.pools.capacity_step_atoms = 10 * ATOMS_PER_QUB;
            self.pools.capacity_step_slots = 8;
            self.pools.max_capacity_slots = 128;
            self.pools.max_active_pools = 1024;
            self.pools.max_commission_bps = 2000;
            self.pools.share_window_blocks = 360;
            self.pools.share_target_bits = "0x1e00ffff".to_string();
            self.pools.max_share_txs_per_block = 128;
            self.pools.share_stale_blocks = 6;
            self.library.enabled = true;
            self.library.activation_height = MAINNET_LIBRARY_ACTIVATION_HEIGHT;
            self.library.marker_output_atoms = 1;
            self.library.base_post_fee_atoms = 10_000;
            self.library.byte_fee_atoms = 100;
            self.library.max_title_bytes = 120;
            self.library.max_category_bytes = 48;
            self.library.max_page_bytes = 2048;
            self.library.max_pages_per_post = 16;
            self.library.max_comment_bytes = 1024;
            self.library.max_comment_depth = 16;
            self.verified_governance.enabled = true;
            self.verified_governance.activation_height =
                MAINNET_VERIFIED_GOVERNANCE_ACTIVATION_HEIGHT;
            self.verified_governance.marker_output_atoms = 1;
            self.verified_governance.wallet_bond_units =
                (10_000u128 * JIN_UNITS_PER_COIN).to_string();
            self.verified_governance.pool_bond_units =
                (25_000u128 * JIN_UNITS_PER_COIN).to_string();
            self.verified_governance.moderator_bond_units =
                (50_000u128 * JIN_UNITS_PER_COIN).to_string();
            self.verified_governance.report_bond_units = (100u128 * JIN_UNITS_PER_COIN).to_string();
            self.verified_governance.appeal_bond_units = (250u128 * JIN_UNITS_PER_COIN).to_string();
            self.verified_governance.max_initial_slash_bps = 2500;
        } else if self.network.name == "testnet" {
            self.library.enabled = true;
            self.library.activation_height = TESTNET_LIBRARY_ACTIVATION_HEIGHT;
            self.jin_swap.enabled = true;
            self.jin_swap.activation_height = TESTNET_JIN_SWAP_ACTIVATION_HEIGHT;
            self.verified_governance.enabled = true;
            self.verified_governance.activation_height =
                TESTNET_VERIFIED_GOVERNANCE_ACTIVATION_HEIGHT;
        }
    }

    pub fn validate(&self) -> Result<()> {
        parse_u32(&self.network.magic)?;
        self.pow_bits()?;
        self.genesis_bits()?;
        if self.mining.duty_cycle_percent == 0 || self.mining.duty_cycle_percent > 100 {
            bail!("invalid duty cycle");
        }
        if self.consensus.subsidy_halving_interval == 0 {
            bail!("zero halving interval");
        }
        if self.consensus.difficulty_adjustment_interval == 0 {
            bail!("zero difficulty adjustment interval");
        }
        if self.consensus.difficulty_max_adjustment_factor < 2 {
            bail!("difficulty max adjustment factor must be >= 2");
        }
        if self.p2p.max_message_bytes < 1024 {
            bail!("p2p max_message_bytes too small");
        }
        if self.p2p.max_blocks_per_message == 0 {
            bail!("p2p max_blocks_per_message must be non-zero");
        }
        if self.qns.enabled {
            if self.qns.max_label_chars == 0 || self.qns.max_label_chars > 64 {
                bail!("qns max_label_chars must be 1..64");
            }
            if self.qns.marker_output_atoms == 0 {
                bail!("qns marker_output_atoms must be non-zero");
            }
            if self.qns.protocol_address.trim().is_empty() {
                bail!("qns protocol_address must be set when QNS is enabled");
            }
            Address::parse_with_prefix(&self.qns.protocol_address, &self.network.address_prefix)
                .context("invalid qns protocol_address")?;
            normalize_qns_name(&self.qns.protocol_name, self.qns.max_label_chars)
                .context("invalid qns protocol_name")?;
        }
        if self.jin.enabled {
            if self.jin.decimals != JIN_DECIMALS {
                bail!("JIN decimals must be 18");
            }
            if self.jin.activation_height == 0 {
                bail!("JIN activation_height must be non-zero");
            }
            if self.jin.marker_output_atoms == 0 {
                bail!("JIN marker_output_atoms must be non-zero");
            }
            let supply = parse_jin_units_raw(&self.jin.total_supply_units)
                .context("invalid JIN total_supply_units")?;
            if supply != JIN_TOTAL_SUPPLY_UNITS {
                bail!("JIN total supply must be exactly 105,000,000 JIN");
            }
            parse_jin_units_raw(&self.jin.default_fee_units)
                .context("invalid JIN default_fee_units")?;
            Address::parse_with_prefix(&self.jin.protocol_address, &self.network.address_prefix)
                .context("invalid JIN protocol_address")?;
            if self.jin.bridge_name.trim().is_empty() {
                bail!("JIN bridge_name must be set");
            }
        }
        if self.jin_swap.enabled {
            if self.jin_swap.marker_output_atoms == 0 {
                bail!("JIN swap marker_output_atoms must be non-zero");
            }
            if self.jin_swap.protocol_fee_bps != 10 {
                bail!("JIN swap protocol fee must be 10 bps (0.1%)");
            }
            parse_jin_units_raw(&self.jin_swap.sale_total_units)
                .context("invalid JIN swap sale_total_units")?;
            parse_jin_units_raw(&self.jin_swap.sale_batch_units)
                .context("invalid JIN swap sale_batch_units")?;
            if self.jin_swap.sale_start_price_atoms_per_jin == 0 {
                bail!("JIN swap start price must be non-zero");
            }
        }
        if self.library.enabled {
            if self.library.marker_output_atoms == 0 {
                bail!("library marker_output_atoms must be non-zero");
            }
            if self.library.max_title_bytes == 0 || self.library.max_title_bytes > 512 {
                bail!("library max_title_bytes must be 1..512");
            }
            if self.library.max_category_bytes == 0 || self.library.max_category_bytes > 128 {
                bail!("library max_category_bytes must be 1..128");
            }
            if self.library.max_page_bytes == 0 || self.library.max_page_bytes > 8192 {
                bail!("library max_page_bytes must be 1..8192");
            }
            if self.library.max_pages_per_post == 0
                || self.library.max_pages_per_post > MAX_SEND_ENTRIES_PER_TX
            {
                bail!("library max_pages_per_post must be 1..256");
            }
            if self.library.max_comment_bytes == 0 || self.library.max_comment_bytes > 4096 {
                bail!("library max_comment_bytes must be 1..4096");
            }
        }
        validate_pools_settings(self)?;
        if self.rpc.max_connections == 0 || self.rpc.max_connections > 4096 {
            bail!("rpc.max_connections must be 1..4096");
        }
        if self.rpc.max_requests_per_minute == 0 {
            bail!("rpc.max_requests_per_minute must be non-zero");
        }
        if self.rpc.max_header_bytes < 1024 || self.rpc.max_header_bytes > 1024 * 1024 {
            bail!("rpc.max_header_bytes must be 1024..1048576");
        }
        if self.rpc.max_body_bytes < 1024 || self.rpc.max_body_bytes > 16 * 1024 * 1024 {
            bail!("rpc.max_body_bytes must be 1024..16777216");
        }
        if self.rpc.read_timeout_secs == 0 || self.rpc.write_timeout_secs == 0 {
            bail!("rpc timeouts must be non-zero");
        }
        if self.rpc.job_ttl_secs < 10 || self.rpc.job_ttl_secs > 3600 {
            bail!("rpc.job_ttl_secs must be 10..3600");
        }
        if self.rpc.max_template_batch == 0 || self.rpc.max_template_batch > 128 {
            bail!("rpc.max_template_batch must be 1..128");
        }
        if self.rpc.max_cached_jobs < self.rpc.max_template_batch || self.rpc.max_cached_jobs > 8192
        {
            bail!("rpc.max_cached_jobs must be >= max_template_batch and <= 8192");
        }
        Ok(())
    }
    pub fn pow_bits(&self) -> Result<u32> {
        parse_u32(&self.consensus.pow_bits)
    }
    pub fn genesis_bits(&self) -> Result<u32> {
        parse_u32(&self.consensus.genesis_bits)
    }
    pub fn required_bits(&self, chain_blocks: &[Block], next_height: u32) -> Result<u32> {
        required_work_bits(self, chain_blocks, next_height)
    }
}
fn parse_u32(s: &str) -> Result<u32> {
    if let Some(hex) = s.trim().strip_prefix("0x") {
        Ok(u32::from_str_radix(hex, 16)?)
    } else {
        Ok(s.trim().parse()?)
    }
}

pub fn consensus_checkpoint_hash(settings: &Settings, height: u32) -> Option<&'static str> {
    match settings.network.name.as_str() {
        "mainnet" if height == MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT => {
            Some(MAINNET_FORK_SAFETY_CHECKPOINT_HASH)
        }
        _ => None,
    }
}

pub fn validate_consensus_checkpoint(
    settings: &Settings,
    height: u32,
    hash: Hash256,
) -> Result<()> {
    if let Some(expected) = consensus_checkpoint_hash(settings, height) {
        let actual = hash.to_string();
        if actual != expected {
            bail!(
                "fork-safety checkpoint mismatch at #{}: expected {}, got {}. Stop mining and resync/repair from the canonical branch.",
                height,
                expected,
                actual
            );
        }
    }
    Ok(())
}

pub fn validate_chain_consensus_checkpoints(settings: &Settings, blocks: &[Block]) -> Result<()> {
    for (height, block) in blocks.iter().enumerate() {
        validate_consensus_checkpoint(settings, height as u32, block.block_hash())?;
    }
    Ok(())
}

pub fn daa_v2_active(settings: &Settings, next_height: u32) -> bool {
    match settings.network.name.as_str() {
        "mainnet" => next_height >= MAINNET_DAA_V2_ACTIVATION_HEIGHT,
        "testnet" => next_height >= TESTNET_DAA_V2_ACTIVATION_HEIGHT,
        _ => false,
    }
}

pub fn protocol_epoch_2_activation_height(settings: &Settings) -> u32 {
    match settings.network.name.as_str() {
        "mainnet" => MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT,
        _ => u32::MAX,
    }
}

pub fn protocol_epoch_2_active(settings: &Settings, height: u32) -> bool {
    let activation = protocol_epoch_2_activation_height(settings);
    activation != u32::MAX && height >= activation
}

pub fn expected_block_version(settings: &Settings, height: u32) -> u32 {
    if protocol_epoch_2_active(settings, height) {
        PROTOCOL_EPOCH_2_BLOCK_VERSION
    } else {
        settings.consensus.version
    }
}

pub fn protocol_epoch_name(settings: &Settings, height: u32) -> &'static str {
    if protocol_epoch_2_active(settings, height) {
        "Protocol Epoch 2"
    } else {
        "Protocol Epoch 1"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoinRecord {
    pub tx_out: TxOut,
    pub height: u32,
    pub is_coinbase: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedUtxo {
    pub outpoint: OutPoint,
    pub coin: CoinRecord,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedChainState {
    pub network: String,
    pub blocks: Vec<Block>,
    pub utxos: Vec<PersistedUtxo>,
    pub mempool: Vec<Transaction>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainState {
    pub network: String,
    pub blocks: Vec<Block>,
    pub utxos: HashMap<OutPoint, CoinRecord>,
    pub mempool: Vec<Transaction>,
}
impl ChainState {
    pub fn new_with_genesis(settings: &Settings) -> Result<Self> {
        Ok(Self {
            network: settings.network.name.clone(),
            blocks: vec![genesis_block(settings)?],
            utxos: HashMap::new(),
            mempool: Vec::new(),
        })
    }

    pub fn from_blocks(blocks: Vec<Block>, settings: &Settings) -> Result<Self> {
        if blocks.is_empty() {
            bail!("empty candidate chain");
        }
        if blocks.first() != Some(&genesis_block(settings)?) {
            bail!("genesis mismatch");
        }
        let mut chain = Self::new_with_genesis(settings)?;
        for block in blocks.into_iter().skip(1) {
            chain.connect_block(block, settings)?;
        }
        Ok(chain)
    }

    pub fn height(&self) -> u32 {
        self.blocks.len().saturating_sub(1) as u32
    }
    pub fn tip_hash(&self) -> Hash256 {
        self.blocks
            .last()
            .map(|b| b.block_hash())
            .unwrap_or_else(Hash256::zero)
    }
    pub fn total_work_hex(&self) -> Result<String> {
        Ok(chain_work_for_blocks(&self.blocks)?.to_str_radix(16))
    }

    pub fn to_persisted(&self) -> PersistedChainState {
        let mut utxos = self
            .utxos
            .iter()
            .map(|(o, c)| PersistedUtxo {
                outpoint: o.clone(),
                coin: c.clone(),
            })
            .collect::<Vec<_>>();
        utxos.sort_by(|a, b| a.outpoint.key().cmp(&b.outpoint.key()));
        PersistedChainState {
            network: self.network.clone(),
            blocks: self.blocks.clone(),
            utxos,
            mempool: self.mempool.clone(),
        }
    }

    pub fn from_persisted(p: PersistedChainState, settings: &Settings) -> Result<Self> {
        if p.network != settings.network.name {
            bail!("network mismatch");
        }
        let mut utxos = HashMap::new();
        for u in p.utxos {
            if utxos.insert(u.outpoint, u.coin).is_some() {
                bail!("duplicate persisted UTXO");
            }
        }
        let s = Self {
            network: p.network,
            blocks: p.blocks,
            utxos,
            mempool: p.mempool,
        };
        s.validate_all(settings)?;
        Ok(s)
    }

    /// HF88/v1.6.2: fast UI-only loader. GUI snapshots must not replay-validate
    /// the full chain every 15-20 seconds; that was causing long snapshot workers
    /// and repeated timeout warnings while the local chain was already visible.
    /// Consensus-sensitive code still uses `from_persisted`.
    pub fn from_persisted_unchecked_for_ui(
        p: PersistedChainState,
        settings: &Settings,
    ) -> Result<Self> {
        if p.network != settings.network.name {
            bail!("network mismatch");
        }
        if p.blocks.is_empty() {
            bail!("empty persisted chain");
        }
        if p.blocks.first() != Some(&genesis_block(settings)?) {
            bail!("genesis mismatch");
        }
        if let Some(expected) =
            consensus_checkpoint_hash(settings, MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT)
        {
            if let Some(block) = p.blocks.get(MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT as usize) {
                let actual = block.block_hash().to_string();
                if actual != expected {
                    bail!(
                        "fork-safety checkpoint mismatch at #{}: expected {}, got {}",
                        MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT,
                        expected,
                        actual
                    );
                }
            }
        }
        let start = p.blocks.len().saturating_sub(96).max(1);
        for i in start..p.blocks.len() {
            let expected_prev = p.blocks[i - 1].block_hash();
            if p.blocks[i].header.prev_block_hash != expected_prev {
                bail!("recent chain link mismatch at #{}", i);
            }
        }
        let mut utxos = HashMap::new();
        for u in p.utxos {
            if utxos.insert(u.outpoint, u.coin).is_some() {
                bail!("duplicate persisted UTXO");
            }
        }
        Ok(Self {
            network: p.network,
            blocks: p.blocks,
            utxos,
            mempool: p.mempool,
        })
    }

    pub fn validate_all(&self, settings: &Settings) -> Result<()> {
        if self.blocks.first() != Some(&genesis_block(settings)?) {
            bail!("genesis mismatch");
        }
        let mut utxos = HashMap::new();
        let mut prev = self.blocks[0].block_hash();
        for (i, block) in self.blocks.iter().enumerate().skip(1) {
            let required_bits = required_work_bits(settings, &self.blocks[..i], i as u32)?;
            validate_block(block, &utxos, prev, i as u32, required_bits, settings)?;
            validate_qns_block(block, &self.blocks[..i], i as u32, settings)?;
            validate_jin_block(block, &self.blocks[..i], &utxos, i as u32, settings)?;
            validate_jin_sale_block(block, &self.blocks[..i], &utxos, i as u32, settings)?;
            validate_pools_block(block, &self.blocks[..i], &utxos, i as u32, settings)?;
            validate_library_block(block, &self.blocks[..i], &utxos, i as u32, settings)?;
            validate_verified_governance_block(
                block,
                &self.blocks[..i],
                &utxos,
                i as u32,
                settings,
            )?;
            validate_consensus_checkpoint(settings, i as u32, block.block_hash())?;
            connect_block_utxos(block, &mut utxos, i as u32)?;
            prev = block.block_hash();
        }
        if utxos != self.utxos {
            bail!("UTXO mismatch after replay");
        }
        Ok(())
    }

    pub fn try_adopt_better_chain(
        &mut self,
        blocks: Vec<Block>,
        settings: &Settings,
    ) -> Result<bool> {
        self.try_adopt_peer_chain(blocks, settings, false)
    }

    pub fn try_adopt_peer_chain(
        &mut self,
        blocks: Vec<Block>,
        settings: &Settings,
        prefer_peer_on_equal_work: bool,
    ) -> Result<bool> {
        let mut candidate = Self::from_blocks(blocks, settings)?;
        let candidate_work = chain_work_for_blocks(&candidate.blocks)?;
        let current_work = chain_work_for_blocks(&self.blocks)?;
        let candidate_height = candidate.height();
        let current_height = self.height();
        let candidate_tip = candidate.tip_hash().to_string();
        let current_tip = self.tip_hash().to_string();
        let should_adopt = candidate_work > current_work
            || (candidate_work == current_work && candidate_height > current_height)
            || (candidate_work == current_work
                && candidate_height == current_height
                && candidate_tip != current_tip
                && prefer_peer_on_equal_work);
        if should_adopt {
            // HF117/v1.7.5: preserve wallet/p2p mempool entries AND resurrect
            // non-coinbase transactions from any local suffix that becomes stale.
            // Before HF117, a QUB tx mined into a losing local block could be
            // removed from mempool by connect_block(), then disappear after the
            // node adopted the winning branch. Rebuild against the candidate tip
            // from the union of: old mempool, candidate mempool, and disconnected
            // local block transactions. Full mempool admission still resolves
            // double-spends and feature-state conflicts deterministically.
            let keep_mempool = self.reorg_mempool_candidates_for(&candidate);
            candidate.rebuild_mempool_from(keep_mempool, settings);
            *self = candidate;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn tx_confirmations(&self, txid: Hash256) -> Option<(u32, u32)> {
        for (height, block) in self.blocks.iter().enumerate().skip(1) {
            if block.transactions.iter().any(|tx| tx.txid() == txid) {
                let h = height as u32;
                let confirmations = self.height().saturating_sub(h).saturating_add(1);
                return Some((h, confirmations));
            }
        }
        None
    }

    pub fn tx_in_mempool(&self, txid: Hash256) -> bool {
        self.mempool.iter().any(|tx| tx.txid() == txid)
    }

    fn common_prefix_height_with(&self, other: &ChainState) -> u32 {
        let max = self.height().min(other.height()) as usize;
        let mut common = 0usize;
        for height in 0..=max {
            if self.blocks.get(height).map(|b| b.block_hash())
                == other.blocks.get(height).map(|b| b.block_hash())
            {
                common = height;
            } else {
                break;
            }
        }
        common as u32
    }

    pub fn disconnected_non_coinbase_transactions_for(
        &self,
        candidate: &ChainState,
    ) -> Vec<Transaction> {
        let common = self.common_prefix_height_with(candidate);
        let candidate_canonical_txids = candidate
            .blocks
            .iter()
            .flat_map(|block| block.transactions.iter().map(|tx| tx.txid()))
            .collect::<HashSet<_>>();
        let mut seen = HashSet::<Hash256>::new();
        let mut out = Vec::<Transaction>::new();
        for block in self.blocks.iter().skip(common.saturating_add(1) as usize) {
            for tx in block.transactions.iter().skip(1) {
                let txid = tx.txid();
                if candidate_canonical_txids.contains(&txid) {
                    continue;
                }
                if seen.insert(txid) {
                    out.push(tx.clone());
                }
            }
        }
        out
    }

    pub fn reorg_mempool_candidates_for(&self, candidate: &ChainState) -> Vec<Transaction> {
        let mut seen = HashSet::<Hash256>::new();
        let mut out = Vec::<Transaction>::new();
        for tx in self.mempool.iter().chain(candidate.mempool.iter()) {
            let txid = tx.txid();
            if seen.insert(txid) {
                out.push(tx.clone());
            }
        }
        for tx in self.disconnected_non_coinbase_transactions_for(candidate) {
            let txid = tx.txid();
            if seen.insert(txid) {
                out.push(tx);
            }
        }
        out
    }

    fn accept_transaction_preview(&self, tx: &Transaction, settings: &Settings) -> Result<()> {
        validate_tx_contextual(tx, &self.utxos, self.height() + 1, settings, true).map(|_| ())?;
        validate_qns_transaction_against_chain(tx, self, self.height() + 1, settings)?;
        validate_jin_transaction_against_chain(tx, self, self.height() + 1, settings)
            .map(|_| ())?;
        validate_qub_jin_infusion_transaction_against_chain(tx, self, self.height() + 1, settings)?;
        validate_jin_sale_transaction_against_chain(tx, self, self.height() + 1, settings)
            .map(|_| ())?;
        validate_pools_transaction_against_chain(tx, self, self.height() + 1, settings)?;
        validate_library_transaction_against_chain(tx, self, self.height() + 1, settings)?;
        validate_verified_governance_transaction_against_chain(
            tx,
            self,
            self.height() + 1,
            settings,
        )
    }

    pub fn accept_transaction_to_mempool(
        &mut self,
        tx: Transaction,
        settings: &Settings,
    ) -> Result<Hash256> {
        if self.mempool.len() >= effective_mempool_max_transactions(settings) {
            bail!("mempool full");
        }
        let txid = tx.txid();
        if self.mempool.iter().any(|t| t.txid() == txid) {
            bail!("duplicate mempool tx");
        }
        hf106_jin_sale_standardness_policy(&tx, settings)?;

        // HF117/v1.7.5: reject mempool input conflicts before the expensive
        // JIN/QUB-JIN/Library/QNS contextual validators. This does not change
        // consensus; it makes local mempool behavior deterministic under fast
        // reorg/rebuild pressure and avoids misleading late failures for normal
        // QUB sends.
        if !is_pool_share_transaction(&tx) {
            let spent = self
                .mempool
                .iter()
                .flat_map(|t| {
                    t.inputs
                        .iter()
                        .filter(|i| i.previous_output != OutPoint::null())
                        .map(|i| i.previous_output.clone())
                })
                .collect::<HashSet<_>>();
            for input in &tx.inputs {
                if input.previous_output != OutPoint::null()
                    && spent.contains(&input.previous_output)
                {
                    bail!("mempool double spend");
                }
            }
        }

        let spend_height = self.height() + 1;
        let raw_fee = validate_tx_contextual(&tx, &self.utxos, spend_height, settings, true)?;
        let qub_melt_burn = qub_jin_melt_burn_atoms_for_fee(settings, &tx, spend_height)?;
        if raw_fee < qub_melt_burn {
            bail!("QUB melt burn exceeds transaction input-output delta");
        }
        let fee = raw_fee - qub_melt_burn;
        validate_pools_transaction_against_chain(&tx, self, spend_height, settings)?;
        let jin_fee_units =
            validate_jin_transaction_against_chain(&tx, self, spend_height, settings)?;
        validate_qub_jin_infusion_transaction_against_chain(&tx, self, spend_height, settings)?;
        let swap_miner_fee =
            validate_jin_sale_transaction_against_chain(&tx, self, spend_height, settings)?;
        let qns_miner_fee = qns_miner_fee_required_in_tx(settings, &tx, spend_height)?;
        let library_miner_fee = library_miner_fee_required_in_tx(settings, &tx, spend_height)?;
        let required_extra_fee = qns_miner_fee
            .checked_add(library_miner_fee)
            .and_then(|v| v.checked_add(swap_miner_fee))
            .context("extra miner fee overflow")?;
        if fee < required_extra_fee {
            bail!(
                "miner fee underpayment: required {} atoms as block fee, got {}",
                required_extra_fee,
                fee
            );
        }
        if !is_pool_share_transaction(&tx)
            && !is_blast_claim_transaction(&tx, settings)
            && fee < settings.mempool.min_relay_fee_atoms
            && jin_fee_units == 0
        {
            bail!("fee below min relay fee");
        }
        validate_qns_transaction_against_chain(&tx, self, self.height() + 1, settings)?;
        validate_library_transaction_against_chain(&tx, self, self.height() + 1, settings)?;
        validate_verified_governance_transaction_against_chain(
            &tx,
            self,
            self.height() + 1,
            settings,
        )?;
        self.mempool.push(tx);
        Ok(txid)
    }

    /// Rebuild the mempool against the current chain tip and feature-state in insertion order.
    /// Returns the number of transactions retained. This intentionally uses full mempool
    /// admission, not the cheaper preview path, so double-spends and contextual JIN/Library
    /// conflicts are resolved deterministically after block connect or fork repair.
    pub fn rebuild_mempool_from<I>(&mut self, txs: I, settings: &Settings) -> usize
    where
        I: IntoIterator<Item = Transaction>,
    {
        let old_len = self.mempool.len();
        let mempool_cap = effective_mempool_max_transactions(settings);
        let mut candidates = Vec::with_capacity(old_len.min(mempool_cap));
        candidates.extend(txs);
        candidates.sort_by_cached_key(|tx| {
            (
                mempool_template_priority(settings, tx),
                tx.txid().to_string(),
            )
        });
        self.mempool.clear();
        let mut seen = HashSet::<Hash256>::new();
        let mut pending = Vec::<Transaction>::new();
        for tx in candidates.into_iter().take(mempool_cap) {
            let txid = tx.txid();
            if seen.insert(txid) {
                pending.push(tx);
            }
        }

        // HF117/v1.7.5: dependency-aware rebuild. A stale-block suffix can contain
        // parent/child wallet txs. The old one-pass rebuild could drop a valid child
        // if its parent was reaccepted later in the sorted candidate order. Keep this
        // bounded and policy-only: every successful admission still uses the normal
        // mempool validator against the current overlay.
        let mut retained = 0usize;
        for _ in 0..8 {
            if pending.is_empty() {
                break;
            }
            let before = pending.len();
            let mut next = Vec::<Transaction>::new();
            for tx in pending.into_iter() {
                if self
                    .accept_transaction_to_mempool(tx.clone(), settings)
                    .is_ok()
                {
                    retained = retained.saturating_add(1);
                } else {
                    next.push(tx);
                }
            }
            if next.len() == before {
                break;
            }
            pending = next;
        }
        retained
    }

    pub fn revalidate_mempool(&mut self, settings: &Settings) -> usize {
        let pending = self.mempool.clone();
        self.rebuild_mempool_from(pending, settings)
    }

    pub fn mempool_txids(&self) -> HashSet<Hash256> {
        self.mempool.iter().map(|tx| tx.txid()).collect()
    }

    pub fn connect_block(&mut self, block: Block, settings: &Settings) -> Result<Hash256> {
        let height = self.height() + 1;
        let required_bits = required_work_bits(settings, &self.blocks, height)?;
        let txids = validate_block(
            &block,
            &self.utxos,
            self.tip_hash(),
            height,
            required_bits,
            settings,
        )?;
        validate_qns_block(&block, &self.blocks, height, settings)?;
        validate_jin_block(&block, &self.blocks, &self.utxos, height, settings)?;
        validate_jin_sale_block(&block, &self.blocks, &self.utxos, height, settings)?;
        validate_pools_block(&block, &self.blocks, &self.utxos, height, settings)?;
        validate_library_block(&block, &self.blocks, &self.utxos, height, settings)?;
        validate_verified_governance_block(&block, &self.blocks, &self.utxos, height, settings)?;
        validate_consensus_checkpoint(settings, height, block.block_hash())?;
        let mut new_utxos = self.utxos.clone();
        connect_block_utxos(&block, &mut new_utxos, height)?;
        let hash = block.block_hash();
        self.blocks.push(block);
        self.utxos = new_utxos;
        self.mempool.retain(|tx| !txids.contains(&tx.txid()));
        // HF76/v1.5.8: a newly-confirmed block can spend inputs or change feature
        // state used by still-pending transactions. Revalidate survivors now so
        // stale local mempool entries do not linger for many blocks and then appear
        // to "revert" from the GUI.
        self.revalidate_mempool(settings);
        Ok(hash)
    }

    pub fn balance_for_scripts(
        &self,
        scripts: &HashSet<Vec<u8>>,
        settings: &Settings,
        include_immature: bool,
    ) -> u64 {
        self.utxos
            .values()
            .filter(|c| scripts.contains(&c.tx_out.script_pubkey.0))
            .filter(|c| include_immature || !is_immature_coinbase(c, self.height(), settings))
            .map(|c| c.tx_out.value.atoms())
            .sum()
    }
    pub fn jin_balance_units_for_address(
        &self,
        settings: &Settings,
        address: &str,
    ) -> Result<u128> {
        jin_balance_units_for_address(settings, self, address)
    }
}

pub fn genesis_block(settings: &Settings) -> Result<Block> {
    let coinbase = Transaction {
        version: 1,
        inputs: vec![TxIn {
            previous_output: OutPoint::null(),
            signature_script: ScriptBuf(
                format!("QUB genesis:{}", settings.network.name).into_bytes(),
            ),
            sequence: u32::MAX,
        }],
        outputs: Vec::new(),
        locktime: 0,
    };
    Ok(Block {
        header: BlockHeader {
            version: settings.consensus.version,
            prev_block_hash: Hash256::zero(),
            merkle_root: merkle_root(&[coinbase.txid()]),
            time: settings.consensus.genesis_time,
            bits: settings.genesis_bits()?,
            nonce: settings.consensus.genesis_nonce,
        },
        transactions: vec![coinbase],
    })
}
pub fn address_from_script_pubkey(prefix: &str, script: &ScriptBuf) -> Option<Address> {
    p2pkh_payload(script).map(|payload| Address::from_pubkey_hash(prefix, payload))
}

const QNS_SCRIPT_PREFIX: &[u8] = b"QNS1|";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QnsRecord {
    pub name: String,
    pub address: String,
    pub height: u32,
    pub txid: Hash256,
    pub price_atoms: u64,
    pub protocol_address: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QnsRegistration {
    pub name: String,
    pub address: String,
}

pub fn qns_active(settings: &Settings, height: u32) -> bool {
    settings.qns.enabled && height >= settings.qns.activation_height
}

pub fn normalize_qns_name(input: &str, max_label_chars: u8) -> Result<String> {
    let raw = input.trim().trim_end_matches('.').to_ascii_lowercase();
    let label = raw.strip_suffix(".qub").unwrap_or(&raw);
    if label.is_empty() {
        bail!("QNS name is empty");
    }
    if label.len() > max_label_chars as usize {
        bail!("QNS label too long; max {} chars", max_label_chars);
    }
    if !label
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        bail!("QNS accepts only latin letters a-z and digits 0-9");
    }
    Ok(format!("{label}.qub"))
}

pub fn qns_label(name: &str, max_label_chars: u8) -> Result<String> {
    let n = normalize_qns_name(name, max_label_chars)?;
    Ok(n.trim_end_matches(".qub").to_string())
}

pub fn qns_registration_price_atoms(settings: &Settings, name: &str) -> Result<u64> {
    let label_len = qns_label(name, settings.qns.max_label_chars)?.len() as u64;
    let max = settings.qns.max_label_chars as u64;
    if label_len == 0 || label_len > max {
        bail!("invalid QNS length");
    }
    let weight = max.saturating_sub(label_len).saturating_add(1);
    let premium = settings
        .qns
        .price_coefficient_atoms
        .checked_mul(weight)
        .and_then(|v| v.checked_mul(weight))
        .context("QNS price overflow")?;
    settings
        .qns
        .base_registration_atoms
        .checked_add(premium)
        .context("QNS price overflow")
}

pub fn qns_miner_split_active(settings: &Settings, height: u32) -> bool {
    settings.qns.enabled && height >= settings.qns.miner_split_activation_height
}

pub fn qns_protocol_share_atoms(settings: &Settings, height: u32, price_atoms: u64) -> u64 {
    if qns_miner_split_active(settings, height) {
        price_atoms / 2
    } else {
        price_atoms
    }
}

pub fn qns_miner_share_atoms(settings: &Settings, height: u32, price_atoms: u64) -> u64 {
    if qns_miner_split_active(settings, height) {
        price_atoms - (price_atoms / 2)
    } else {
        0
    }
}

pub fn qns_miner_fee_required_in_tx(
    settings: &Settings,
    tx: &Transaction,
    height: u32,
) -> Result<u64> {
    let mut required = 0u64;
    for (_, reg, _) in qns_registrations_in_tx(tx, settings) {
        let price = qns_registration_price_atoms(settings, &reg.name)?;
        required = required
            .checked_add(qns_miner_share_atoms(settings, height, price))
            .context("QNS miner split fee overflow")?;
    }
    Ok(required)
}

pub fn qns_marker_script(name: &str, address: &Address, settings: &Settings) -> Result<ScriptBuf> {
    let name = normalize_qns_name(name, settings.qns.max_label_chars)?;
    Ok(ScriptBuf(format!("QNS1|{}|{}", name, address).into_bytes()))
}

pub fn parse_qns_marker_script(script: &ScriptBuf, settings: &Settings) -> Option<QnsRegistration> {
    let b = script.as_bytes();
    if !b.starts_with(QNS_SCRIPT_PREFIX) {
        return None;
    }
    let s = std::str::from_utf8(b).ok()?;
    let mut parts = s.split('|');
    if parts.next()? != "QNS1" {
        return None;
    }
    let name = normalize_qns_name(parts.next()?, settings.qns.max_label_chars).ok()?;
    let address = parts.next()?.trim().to_string();
    if parts.next().is_some() {
        return None;
    }
    Address::parse_with_prefix(&address, &settings.network.address_prefix).ok()?;
    Some(QnsRegistration { name, address })
}

pub fn qns_registrations_in_tx(
    tx: &Transaction,
    settings: &Settings,
) -> Vec<(usize, QnsRegistration, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_qns_marker_script(&out.script_pubkey, settings)
                .map(|r| (idx, r, out.value.atoms()))
        })
        .collect()
}

pub fn qns_registry_from_blocks(
    settings: &Settings,
    blocks: &[Block],
) -> Result<HashMap<String, QnsRecord>> {
    let mut registry = HashMap::new();
    if settings.qns.enabled {
        let protocol_name =
            normalize_qns_name(&settings.qns.protocol_name, settings.qns.max_label_chars)?;
        registry.insert(
            protocol_name.clone(),
            QnsRecord {
                name: protocol_name,
                address: settings.qns.protocol_address.clone(),
                height: 0,
                txid: Hash256::zero(),
                price_atoms: 0,
                protocol_address: settings.qns.protocol_address.clone(),
            },
        );
        if settings.pools.enabled
            && settings.features.pooled_mining_enabled
            && !settings.pools.protocol_address.trim().is_empty()
        {
            if let Ok(pool_protocol_name) =
                normalize_qns_name(&settings.pools.protocol_name, settings.qns.max_label_chars)
            {
                registry
                    .entry(pool_protocol_name.clone())
                    .or_insert(QnsRecord {
                        name: pool_protocol_name,
                        address: settings.pools.protocol_address.clone(),
                        height: 0,
                        txid: Hash256::zero(),
                        price_atoms: 0,
                        protocol_address: settings.pools.protocol_address.clone(),
                    });
            }
        }
    }
    for (height, block) in blocks.iter().enumerate().skip(1) {
        let height = height as u32;
        if !qns_active(settings, height) {
            continue;
        }
        for tx in block.transactions.iter().skip(1) {
            for (_, reg, _) in qns_registrations_in_tx(tx, settings) {
                if registry.contains_key(&reg.name) {
                    continue;
                }
                let price = qns_registration_price_atoms(settings, &reg.name)?;
                registry.insert(
                    reg.name.clone(),
                    QnsRecord {
                        name: reg.name,
                        address: reg.address,
                        height,
                        txid: tx.txid(),
                        price_atoms: price,
                        protocol_address: settings.qns.protocol_address.clone(),
                    },
                );
            }
        }
    }
    Ok(registry)
}

pub fn qns_resolve(
    settings: &Settings,
    chain: &ChainState,
    name: &str,
) -> Result<Option<QnsRecord>> {
    let name = normalize_qns_name(name, settings.qns.max_label_chars)?;
    Ok(qns_registry_from_blocks(settings, &chain.blocks)?
        .get(&name)
        .cloned())
}

pub fn qns_names_for_address(
    settings: &Settings,
    chain: &ChainState,
    address: &str,
) -> Result<Vec<QnsRecord>> {
    let mut rows = qns_registry_from_blocks(settings, &chain.blocks)?
        .into_values()
        .filter(|r| r.address == address)
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(rows)
}

fn validate_qns_transaction_against_chain(
    tx: &Transaction,
    chain: &ChainState,
    spend_height: u32,
    settings: &Settings,
) -> Result<()> {
    let mut seen = HashSet::new();
    let registry = qns_registry_from_blocks(settings, &chain.blocks)?;
    validate_qns_transaction_with_registry(tx, spend_height, &registry, &mut seen, settings)
}

fn validate_qns_block(
    block: &Block,
    prior_blocks: &[Block],
    height: u32,
    settings: &Settings,
) -> Result<()> {
    let mut seen = HashSet::new();
    let registry = qns_registry_from_blocks(settings, prior_blocks)?;
    for tx in block.transactions.iter().skip(1) {
        validate_qns_transaction_with_registry(tx, height, &registry, &mut seen, settings)?;
    }
    Ok(())
}

fn validate_qns_transaction_with_registry(
    tx: &Transaction,
    height: u32,
    registry: &HashMap<String, QnsRecord>,
    block_seen: &mut HashSet<String>,
    settings: &Settings,
) -> Result<()> {
    let regs = qns_registrations_in_tx(tx, settings);
    if regs.is_empty() {
        return Ok(());
    }
    if !qns_active(settings, height) {
        bail!(
            "QNS transaction before activation height {}",
            settings.qns.activation_height
        );
    }
    if tx.is_coinbase() {
        bail!("coinbase cannot register QNS names");
    }
    if regs.len() != 1 {
        bail!("a transaction may register exactly one QNS name");
    }
    let (_idx, reg, marker_atoms) = &regs[0];
    if *marker_atoms != settings.qns.marker_output_atoms {
        bail!(
            "QNS marker output must be exactly {} atom(s)",
            settings.qns.marker_output_atoms
        );
    }
    if registry.contains_key(&reg.name) {
        bail!("QNS name already registered: {}", reg.name);
    }
    if !block_seen.insert(reg.name.clone()) {
        bail!("duplicate QNS name in block: {}", reg.name);
    }
    let price = qns_registration_price_atoms(settings, &reg.name)?;
    let protocol_required = qns_protocol_share_atoms(settings, height, price);
    let protocol = Address::parse_with_prefix(
        &settings.qns.protocol_address,
        &settings.network.address_prefix,
    )?;
    let protocol_script = protocol.script_pubkey().0;
    let paid: u64 = tx
        .outputs
        .iter()
        .filter(|out| out.script_pubkey.0 == protocol_script)
        .map(|out| out.value.atoms())
        .sum();
    if paid < protocol_required {
        bail!(
            "QNS underpayment for {}: need {} QUB to protocol, paid {} QUB",
            reg.name,
            Amount::from_atoms(protocol_required)?,
            Amount::from_atoms(paid)?
        );
    }
    Ok(())
}

const LIBRARY_SCRIPT_PREFIX: &[u8] = b"LIB1|";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryMarker {
    Post {
        author: String,
        title: String,
        category: String,
        page_index: u32,
        page_total: u32,
        body: String,
    },
    Comment {
        post_id: String,
        parent_comment_id: Option<String>,
        author: String,
        body: String,
    },
    Vote {
        target_kind: String,
        target_id: String,
        author: String,
        up: bool,
    },
    Edit {
        target_kind: String,
        target_id: String,
        author: String,
        title: String,
        category: String,
        body: String,
    },
    Delete {
        target_kind: String,
        target_id: String,
        author: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryPost {
    pub id: String,
    pub author: String,
    pub title: String,
    pub category: String,
    pub body: String,
    pub created_height: u32,
    pub created_time: u32,
    pub edited_height: Option<u32>,
    pub edited_time: Option<u32>,
    pub deleted: bool,
    pub upvotes: u32,
    pub downvotes: u32,
    pub comment_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryComment {
    pub id: String,
    pub post_id: String,
    pub parent_comment_id: Option<String>,
    pub author: String,
    pub body: String,
    pub depth: usize,
    pub created_height: u32,
    pub created_time: u32,
    pub edited_height: Option<u32>,
    pub edited_time: Option<u32>,
    pub deleted: bool,
    pub upvotes: u32,
    pub downvotes: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryState {
    pub posts: Vec<LibraryPost>,
    pub comments: Vec<LibraryComment>,
}

fn library_enabled(settings: &Settings) -> bool {
    settings.library.enabled
}
pub fn library_activation_height(settings: &Settings) -> u32 {
    settings.library.activation_height
}
pub fn library_active(settings: &Settings, height: u32) -> bool {
    library_enabled(settings) && height >= library_activation_height(settings)
}

fn library_hex_field(input: &str, max_bytes: usize, label: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.as_bytes().len() > max_bytes {
        bail!("Library {label} exceeds {max_bytes} bytes");
    }
    Ok(hex::encode(trimmed.as_bytes()))
}
fn library_unhex_field(input: &str, max_bytes: usize, label: &str) -> Result<String> {
    if input.len() > max_bytes.saturating_mul(2) {
        bail!("Library {label} exceeds {max_bytes} bytes");
    }
    let bytes = hex::decode(input).with_context(|| format!("invalid Library {label} encoding"))?;
    if bytes.len() > max_bytes {
        bail!("Library {label} exceeds {max_bytes} bytes");
    }
    String::from_utf8(bytes).with_context(|| format!("Library {label} must be UTF-8"))
}
fn library_validate_id(id: &str, label: &str) -> Result<String> {
    let id = id.trim().to_ascii_lowercase();
    if id.len() != 64 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid Library {label} id");
    }
    Ok(id)
}
fn library_validate_target_kind(kind: &str) -> Result<String> {
    let k = kind.trim().to_ascii_lowercase();
    if k != "post" && k != "comment" {
        bail!("Library target kind must be post or comment");
    }
    Ok(k)
}

pub fn library_post_price_atoms(
    settings: &Settings,
    title: &str,
    category: &str,
    body: &str,
) -> Result<u64> {
    let bytes = title
        .as_bytes()
        .len()
        .checked_add(category.as_bytes().len())
        .context("Library price overflow")?
        .checked_add(body.as_bytes().len())
        .context("Library price overflow")?;
    let byte_fee = settings
        .library
        .byte_fee_atoms
        .checked_mul(bytes as u64)
        .context("Library byte fee overflow")?;
    settings
        .library
        .base_post_fee_atoms
        .checked_add(byte_fee)
        .context("Library post fee overflow")
}

pub fn library_post_marker_script(
    author: &Address,
    title: &str,
    category: &str,
    page_index: u32,
    page_total: u32,
    body: &str,
    settings: &Settings,
) -> Result<ScriptBuf> {
    if page_total == 0 || page_total as usize > settings.library.max_pages_per_post {
        bail!(
            "Library post pages must be 1..{}",
            settings.library.max_pages_per_post
        );
    }
    if page_index >= page_total {
        bail!("Library page index out of range");
    }
    let title = library_hex_field(title, settings.library.max_title_bytes, "title")?;
    let category = library_hex_field(category, settings.library.max_category_bytes, "category")?;
    let body = library_hex_field(body, settings.library.max_page_bytes, "page")?;
    Ok(ScriptBuf(
        format!(
            "LIB1|post|v1|{}|{}|{}|{}|{}|{}",
            author, title, category, page_index, page_total, body
        )
        .into_bytes(),
    ))
}

pub fn library_comment_marker_script(
    post_id: &str,
    parent_comment_id: Option<&str>,
    author: &Address,
    body: &str,
    settings: &Settings,
) -> Result<ScriptBuf> {
    let post_id = library_validate_id(post_id, "post")?;
    let parent = match parent_comment_id {
        Some(v) if !v.trim().is_empty() => library_validate_id(v, "parent comment")?,
        _ => "-".to_string(),
    };
    let body = library_hex_field(body, settings.library.max_comment_bytes, "comment")?;
    Ok(ScriptBuf(
        format!("LIB1|comment|v1|{}|{}|{}|{}", post_id, parent, author, body).into_bytes(),
    ))
}

pub fn library_vote_marker_script(
    target_kind: &str,
    target_id: &str,
    author: &Address,
    up: bool,
) -> Result<ScriptBuf> {
    let kind = library_validate_target_kind(target_kind)?;
    let target_id = library_validate_id(target_id, "target")?;
    Ok(ScriptBuf(
        format!(
            "LIB1|vote|v1|{}|{}|{}|{}",
            kind,
            target_id,
            author,
            if up { "up" } else { "down" }
        )
        .into_bytes(),
    ))
}

pub fn library_edit_marker_script(
    target_kind: &str,
    target_id: &str,
    author: &Address,
    title: &str,
    category: &str,
    body: &str,
    settings: &Settings,
) -> Result<ScriptBuf> {
    let kind = library_validate_target_kind(target_kind)?;
    let target_id = library_validate_id(target_id, "target")?;
    let title = library_hex_field(title, settings.library.max_title_bytes, "title")?;
    let category = library_hex_field(category, settings.library.max_category_bytes, "category")?;
    let max_body = if kind == "post" {
        settings.library.max_page_bytes * settings.library.max_pages_per_post
    } else {
        settings.library.max_comment_bytes
    };
    let body = library_hex_field(body, max_body, "edit body")?;
    Ok(ScriptBuf(
        format!(
            "LIB1|edit|v1|{}|{}|{}|{}|{}|{}",
            kind, target_id, author, title, category, body
        )
        .into_bytes(),
    ))
}

pub fn library_delete_marker_script(
    target_kind: &str,
    target_id: &str,
    author: &Address,
) -> Result<ScriptBuf> {
    let kind = library_validate_target_kind(target_kind)?;
    let target_id = library_validate_id(target_id, "target")?;
    Ok(ScriptBuf(
        format!("LIB1|delete|v1|{}|{}|{}", kind, target_id, author).into_bytes(),
    ))
}

pub fn parse_library_marker_script(
    script: &ScriptBuf,
    settings: &Settings,
) -> Option<LibraryMarker> {
    let raw = std::str::from_utf8(script.as_bytes()).ok()?;
    if !raw.as_bytes().starts_with(LIBRARY_SCRIPT_PREFIX) {
        return None;
    }
    let mut parts = raw.split('|');
    if parts.next()? != "LIB1" {
        return None;
    }
    let op = parts.next()?;
    if parts.next()? != "v1" {
        return None;
    }
    match op {
        "post" => {
            let author =
                Address::parse_with_prefix(parts.next()?, &settings.network.address_prefix)
                    .ok()?
                    .to_string();
            let title =
                library_unhex_field(parts.next()?, settings.library.max_title_bytes, "title")
                    .ok()?;
            let category = library_unhex_field(
                parts.next()?,
                settings.library.max_category_bytes,
                "category",
            )
            .ok()?;
            let page_index = parts.next()?.parse::<u32>().ok()?;
            let page_total = parts.next()?.parse::<u32>().ok()?;
            let body =
                library_unhex_field(parts.next()?, settings.library.max_page_bytes, "page").ok()?;
            if parts.next().is_some() {
                return None;
            }
            Some(LibraryMarker::Post {
                author,
                title,
                category,
                page_index,
                page_total,
                body,
            })
        }
        "comment" => {
            let post_id = library_validate_id(parts.next()?, "post").ok()?;
            let parent_raw = parts.next()?;
            let parent_comment_id = if parent_raw == "-" {
                None
            } else {
                Some(library_validate_id(parent_raw, "parent comment").ok()?)
            };
            let author =
                Address::parse_with_prefix(parts.next()?, &settings.network.address_prefix)
                    .ok()?
                    .to_string();
            let body =
                library_unhex_field(parts.next()?, settings.library.max_comment_bytes, "comment")
                    .ok()?;
            if parts.next().is_some() {
                return None;
            }
            Some(LibraryMarker::Comment {
                post_id,
                parent_comment_id,
                author,
                body,
            })
        }
        "vote" => {
            let target_kind = library_validate_target_kind(parts.next()?).ok()?;
            let target_id = library_validate_id(parts.next()?, "target").ok()?;
            let author =
                Address::parse_with_prefix(parts.next()?, &settings.network.address_prefix)
                    .ok()?
                    .to_string();
            let vote = parts.next()?;
            if parts.next().is_some() {
                return None;
            }
            let up = match vote {
                "up" => true,
                "down" => false,
                _ => return None,
            };
            Some(LibraryMarker::Vote {
                target_kind,
                target_id,
                author,
                up,
            })
        }
        "edit" => {
            let target_kind = library_validate_target_kind(parts.next()?).ok()?;
            let target_id = library_validate_id(parts.next()?, "target").ok()?;
            let author =
                Address::parse_with_prefix(parts.next()?, &settings.network.address_prefix)
                    .ok()?
                    .to_string();
            let title =
                library_unhex_field(parts.next()?, settings.library.max_title_bytes, "title")
                    .ok()?;
            let category = library_unhex_field(
                parts.next()?,
                settings.library.max_category_bytes,
                "category",
            )
            .ok()?;
            let max_body = if target_kind == "post" {
                settings.library.max_page_bytes * settings.library.max_pages_per_post
            } else {
                settings.library.max_comment_bytes
            };
            let body = library_unhex_field(parts.next()?, max_body, "edit body").ok()?;
            if parts.next().is_some() {
                return None;
            }
            Some(LibraryMarker::Edit {
                target_kind,
                target_id,
                author,
                title,
                category,
                body,
            })
        }
        "delete" => {
            let target_kind = library_validate_target_kind(parts.next()?).ok()?;
            let target_id = library_validate_id(parts.next()?, "target").ok()?;
            let author =
                Address::parse_with_prefix(parts.next()?, &settings.network.address_prefix)
                    .ok()?
                    .to_string();
            if parts.next().is_some() {
                return None;
            }
            Some(LibraryMarker::Delete {
                target_kind,
                target_id,
                author,
            })
        }
        _ => None,
    }
}

pub fn library_markers_in_tx(
    tx: &Transaction,
    settings: &Settings,
) -> Vec<(usize, LibraryMarker, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_library_marker_script(&out.script_pubkey, settings)
                .map(|m| (idx, m, out.value.atoms()))
        })
        .collect()
}

pub fn library_miner_fee_required_in_tx(
    settings: &Settings,
    tx: &Transaction,
    height: u32,
) -> Result<u64> {
    let markers = library_markers_in_tx(tx, settings);
    if markers.is_empty() {
        return Ok(0);
    }
    if !library_active(settings, height) {
        bail!(
            "Library activates at block #{}",
            library_activation_height(settings)
        );
    }
    let mut has_post = false;
    let mut bytes = 0usize;
    let mut counted_meta = false;
    for (_, marker, atoms) in &markers {
        if *atoms != settings.library.marker_output_atoms {
            bail!(
                "Library marker output must be exactly {} atom(s)",
                settings.library.marker_output_atoms
            );
        }
        if let LibraryMarker::Post {
            title,
            category,
            body,
            ..
        } = marker
        {
            has_post = true;
            if !counted_meta {
                counted_meta = true;
                bytes = bytes
                    .checked_add(title.as_bytes().len())
                    .context("Library fee overflow")?;
                bytes = bytes
                    .checked_add(category.as_bytes().len())
                    .context("Library fee overflow")?;
            }
            bytes = bytes
                .checked_add(body.as_bytes().len())
                .context("Library fee overflow")?;
        }
    }
    if has_post {
        let byte_fee = settings
            .library
            .byte_fee_atoms
            .checked_mul(bytes as u64)
            .context("Library byte fee overflow")?;
        settings
            .library
            .base_post_fee_atoms
            .checked_add(byte_fee)
            .context("Library post fee overflow")
    } else {
        Ok(0)
    }
}

fn library_author_spent(
    tx: &Transaction,
    utxos: &HashMap<OutPoint, CoinRecord>,
    author: &str,
    settings: &Settings,
) -> Result<bool> {
    let address = Address::parse_with_prefix(author, &settings.network.address_prefix)?;
    let script = address.script_pubkey().0;
    Ok(tx.inputs.iter().any(|i| {
        utxos
            .get(&i.previous_output)
            .map(|c| c.tx_out.script_pubkey.0 == script)
            .unwrap_or(false)
    }))
}

fn library_apply_markers(
    state: &mut LibraryState,
    txid: String,
    height: u32,
    time: u32,
    markers: Vec<LibraryMarker>,
    settings: &Settings,
) -> Result<()> {
    if markers.is_empty() {
        return Ok(());
    }
    if let Some(LibraryMarker::Post { .. }) = markers.first() {
        let mut pages: Vec<(u32, String)> = Vec::new();
        let mut author = String::new();
        let mut title = String::new();
        let mut category = String::new();
        let mut total = 0u32;
        for marker in markers {
            let LibraryMarker::Post {
                author: a,
                title: t,
                category: c,
                page_index,
                page_total,
                body,
            } = marker
            else {
                bail!("Library post tx cannot mix actions");
            };
            if author.is_empty() {
                author = a;
                title = t;
                category = c;
                total = page_total;
            } else if author != a || title != t || category != c || total != page_total {
                bail!("Library post pages must share metadata");
            }
            pages.push((page_index, body));
        }
        pages.sort_by_key(|p| p.0);
        let body = pages
            .into_iter()
            .map(|(_, body)| body)
            .collect::<Vec<_>>()
            .join("");
        state.posts.push(LibraryPost {
            id: txid,
            author,
            title,
            category,
            body,
            created_height: height,
            created_time: time,
            edited_height: None,
            edited_time: None,
            deleted: false,
            upvotes: 0,
            downvotes: 0,
            comment_count: 0,
        });
        return Ok(());
    }
    if markers.len() != 1 {
        bail!("Library non-post tx must contain exactly one action");
    }
    match markers.into_iter().next().unwrap() {
        LibraryMarker::Comment {
            post_id,
            parent_comment_id,
            author,
            body,
        } => {
            let depth = if let Some(parent) = &parent_comment_id {
                state
                    .comments
                    .iter()
                    .find(|c| c.id == *parent && !c.deleted)
                    .map(|c| c.depth + 1)
                    .unwrap_or(1)
            } else {
                0
            };
            if depth > settings.library.max_comment_depth {
                bail!(
                    "Library comment nesting exceeds {}",
                    settings.library.max_comment_depth
                );
            }
            state.comments.push(LibraryComment {
                id: txid,
                post_id,
                parent_comment_id,
                author,
                body,
                depth,
                created_height: height,
                created_time: time,
                edited_height: None,
                edited_time: None,
                deleted: false,
                upvotes: 0,
                downvotes: 0,
            });
        }
        LibraryMarker::Edit {
            target_kind,
            target_id,
            author: _,
            title,
            category,
            body,
        } => {
            if target_kind == "post" {
                if let Some(post) = state.posts.iter_mut().find(|p| p.id == target_id) {
                    post.title = title;
                    post.category = category;
                    post.body = body;
                    post.edited_height = Some(height);
                    post.edited_time = Some(time);
                }
            } else if let Some(comment) = state.comments.iter_mut().find(|c| c.id == target_id) {
                comment.body = body;
                comment.edited_height = Some(height);
                comment.edited_time = Some(time);
            }
        }
        LibraryMarker::Delete {
            target_kind,
            target_id,
            ..
        } => {
            if target_kind == "post" {
                if let Some(post) = state.posts.iter_mut().find(|p| p.id == target_id) {
                    post.deleted = true;
                }
            } else if let Some(comment) = state.comments.iter_mut().find(|c| c.id == target_id) {
                comment.deleted = true;
            }
        }
        LibraryMarker::Vote { .. } | LibraryMarker::Post { .. } => {}
    }
    Ok(())
}

pub fn library_state_from_blocks(settings: &Settings, blocks: &[Block]) -> Result<LibraryState> {
    let mut state = LibraryState::default();
    let mut votes: HashMap<(String, String, String), bool> = HashMap::new();
    for (height, block) in blocks.iter().enumerate().skip(1) {
        if !library_active(settings, height as u32) {
            continue;
        }
        for tx in block.transactions.iter().skip(1) {
            let markers = library_markers_in_tx(tx, settings)
                .into_iter()
                .map(|(_, m, _)| m)
                .collect::<Vec<_>>();
            for m in &markers {
                if let LibraryMarker::Vote {
                    target_kind,
                    target_id,
                    author,
                    up,
                } = m
                {
                    votes.insert(
                        (target_kind.clone(), target_id.clone(), author.clone()),
                        *up,
                    );
                }
            }
            library_apply_markers(
                &mut state,
                tx.txid().to_string(),
                height as u32,
                block.header.time,
                markers,
                settings,
            )?;
        }
    }
    for ((kind, target, _author), up) in votes {
        if kind == "post" {
            if let Some(post) = state.posts.iter_mut().find(|p| p.id == target) {
                if up {
                    post.upvotes = post.upvotes.saturating_add(1);
                } else {
                    post.downvotes = post.downvotes.saturating_add(1);
                }
            }
        } else if let Some(comment) = state.comments.iter_mut().find(|c| c.id == target) {
            if up {
                comment.upvotes = comment.upvotes.saturating_add(1);
            } else {
                comment.downvotes = comment.downvotes.saturating_add(1);
            }
        }
    }
    for post in &mut state.posts {
        post.comment_count = state
            .comments
            .iter()
            .filter(|c| c.post_id == post.id && !c.deleted)
            .count();
    }
    state.posts.sort_by(|a, b| {
        b.created_height
            .cmp(&a.created_height)
            .then_with(|| b.id.cmp(&a.id))
    });
    state.comments.sort_by(|a, b| {
        a.created_height
            .cmp(&b.created_height)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(state)
}

fn validate_library_marker_state(
    markers: &[(usize, LibraryMarker, u64)],
    state: &LibraryState,
    tx: &Transaction,
    utxos: &HashMap<OutPoint, CoinRecord>,
    settings: &Settings,
) -> Result<()> {
    if markers.is_empty() {
        return Ok(());
    }
    if markers.len() > MAX_SEND_ENTRIES_PER_TX {
        bail!("Library action count exceeds 256");
    }
    for (_, marker, atoms) in markers {
        if *atoms != settings.library.marker_output_atoms {
            bail!(
                "Library marker output must be exactly {} atom(s)",
                settings.library.marker_output_atoms
            );
        }
        match marker {
            LibraryMarker::Post {
                author,
                page_index,
                page_total,
                ..
            } => {
                if *page_total == 0 || *page_total as usize > settings.library.max_pages_per_post {
                    bail!("Library post page count out of range");
                }
                if *page_index >= *page_total {
                    bail!("Library post page index out of range");
                }
                if !library_author_spent(tx, utxos, author, settings)? {
                    bail!("Library post author must sign/spend from author address");
                }
            }
            LibraryMarker::Comment {
                post_id,
                parent_comment_id,
                author,
                body: _,
            } => {
                if !state.posts.iter().any(|p| p.id == *post_id && !p.deleted) {
                    bail!("Library comment target post not found");
                }
                if let Some(parent) = parent_comment_id {
                    let Some(c) = state
                        .comments
                        .iter()
                        .find(|c| c.id == *parent && !c.deleted)
                    else {
                        bail!("Library parent comment not found");
                    };
                    if c.post_id != *post_id {
                        bail!("Library parent comment belongs to a different post");
                    }
                    if c.depth + 1 > settings.library.max_comment_depth {
                        bail!(
                            "Library comment nesting exceeds {}",
                            settings.library.max_comment_depth
                        );
                    }
                }
                if !library_author_spent(tx, utxos, author, settings)? {
                    bail!("Library comment author must sign/spend from author address");
                }
            }
            LibraryMarker::Vote {
                target_kind,
                target_id,
                author,
                ..
            } => {
                if target_kind == "post" {
                    if !state.posts.iter().any(|p| p.id == *target_id && !p.deleted) {
                        bail!("Library vote target post not found");
                    }
                } else if !state
                    .comments
                    .iter()
                    .any(|c| c.id == *target_id && !c.deleted)
                {
                    bail!("Library vote target comment not found");
                }
                if !library_author_spent(tx, utxos, author, settings)? {
                    bail!("Library vote author must sign/spend from author address");
                }
            }
            LibraryMarker::Edit {
                target_kind,
                target_id,
                author,
                ..
            }
            | LibraryMarker::Delete {
                target_kind,
                target_id,
                author,
            } => {
                let owner = if target_kind == "post" {
                    state
                        .posts
                        .iter()
                        .find(|p| p.id == *target_id && !p.deleted)
                        .map(|p| p.author.clone())
                } else {
                    state
                        .comments
                        .iter()
                        .find(|c| c.id == *target_id && !c.deleted)
                        .map(|c| c.author.clone())
                };
                let owner = owner.context("Library edit/delete target not found")?;
                if owner != *author {
                    bail!("Library edit/delete author mismatch");
                }
                if !library_author_spent(tx, utxos, author, settings)? {
                    bail!("Library edit/delete author must sign/spend from author address");
                }
            }
        }
    }
    if matches!(
        markers.first().map(|(_, m, _)| m),
        Some(LibraryMarker::Post { .. })
    ) {
        let mut seen = HashSet::new();
        let mut total = 0u32;
        let mut meta: Option<(String, String, String)> = None;
        for (_, marker, _) in markers {
            let LibraryMarker::Post {
                author,
                title,
                category,
                page_index,
                page_total,
                ..
            } = marker
            else {
                bail!("Library post tx cannot mix actions");
            };
            let this_meta = (author.clone(), title.clone(), category.clone());
            if let Some(existing) = &meta {
                if existing != &this_meta {
                    bail!("Library post pages must share author/title/category");
                }
            } else {
                meta = Some(this_meta);
            }
            if total == 0 {
                total = *page_total;
            } else if total != *page_total {
                bail!("Library post pages must share page_total");
            }
            if !seen.insert(*page_index) {
                bail!("duplicate Library post page");
            }
        }
        if seen.len() != total as usize {
            bail!("Library post tx must include every page exactly once");
        }
    } else if markers.len() != 1 {
        bail!("Library non-post tx must contain exactly one action");
    }
    Ok(())
}

fn validate_library_transaction_with_state(
    tx: &Transaction,
    state: &LibraryState,
    utxos: &HashMap<OutPoint, CoinRecord>,
    spend_height: u32,
    settings: &Settings,
) -> Result<()> {
    let markers = library_markers_in_tx(tx, settings);
    if markers.is_empty() {
        return Ok(());
    }
    if !library_active(settings, spend_height) {
        bail!(
            "Library activates at block #{}",
            library_activation_height(settings)
        );
    }
    if tx.is_coinbase() {
        bail!("coinbase cannot contain Library actions");
    }
    validate_library_marker_state(&markers, state, tx, utxos, settings)
}

fn validate_library_transaction_against_chain(
    tx: &Transaction,
    chain: &ChainState,
    spend_height: u32,
    settings: &Settings,
) -> Result<()> {
    let state = library_state_from_blocks(settings, &chain.blocks)?;
    validate_library_transaction_with_state(tx, &state, &chain.utxos, spend_height, settings)
}

fn validate_library_block(
    block: &Block,
    prior_blocks: &[Block],
    base_utxos: &HashMap<OutPoint, CoinRecord>,
    height: u32,
    settings: &Settings,
) -> Result<()> {
    let mut state = library_state_from_blocks(settings, prior_blocks)?;
    let mut scratch = base_utxos.clone();
    for tx in block.transactions.iter().skip(1) {
        validate_library_transaction_with_state(tx, &state, &scratch, height, settings)?;
        let markers = library_markers_in_tx(tx, settings)
            .into_iter()
            .map(|(_, m, _)| m)
            .collect::<Vec<_>>();
        library_apply_markers(
            &mut state,
            tx.txid().to_string(),
            height,
            block.header.time,
            markers,
            settings,
        )?;
        let _ = connect_tx_utxos(tx, &mut scratch, height, false);
    }
    Ok(())
}

pub fn block_subsidy(height: u64, settings: &Settings) -> u64 {
    if height == 0 {
        return 0;
    }
    let halvings = (height - 1) / settings.consensus.subsidy_halving_interval;
    if halvings >= 64 {
        0
    } else {
        settings.consensus.initial_subsidy_atoms >> halvings
    }
}
pub fn validate_tx_stateless(tx: &Transaction, settings: &Settings) -> Result<()> {
    if is_pool_share_transaction(tx) {
        return Ok(());
    }
    if tx.inputs.is_empty() || tx.outputs.is_empty() {
        if tx.is_coinbase() {
            return Ok(());
        }
        bail!("tx must have inputs and outputs");
    }
    if !tx.is_coinbase() && tx.outputs.len() > MAX_SEND_ENTRIES_PER_TX + 4 {
        bail!("tx has too many outputs; max send/blast entries is 256");
    }
    let mut seen = HashSet::new();
    for input in &tx.inputs {
        if !seen.insert(input.previous_output.clone()) {
            bail!("duplicate input");
        }
    }
    let mut total = 0u128;
    for output in &tx.outputs {
        let v = output.value.atoms();
        if v == 0 || v > settings.consensus.max_money_atoms || v > MAX_MONEY_ATOMS {
            bail!("amount out of range");
        }
        total += v as u128;
        if total > settings.consensus.max_money_atoms as u128 {
            bail!("output total exceeds max money");
        }
    }
    Ok(())
}
pub fn validate_tx_contextual(
    tx: &Transaction,
    utxos: &HashMap<OutPoint, CoinRecord>,
    spend_height: u32,
    settings: &Settings,
    verify_scripts: bool,
) -> Result<u64> {
    validate_tx_stateless(tx, settings)?;
    if tx.is_coinbase() || is_pool_share_transaction(tx) {
        return Ok(0);
    }
    validate_blast_create_outputs(tx, spend_height, settings)?;
    if is_blast_claim_transaction(tx, settings) {
        return validate_blast_claim_contextual(tx, utxos, spend_height, settings);
    }
    let mut total_in = 0u128;
    let total_out: u128 = tx.outputs.iter().map(|o| o.value.atoms() as u128).sum();
    for (idx, input) in tx.inputs.iter().enumerate() {
        let coin = utxos
            .get(&input.previous_output)
            .with_context(|| format!("missing UTXO {}", input.previous_output.key()))?;
        if coin.is_coinbase
            && spend_height.saturating_sub(coin.height) < settings.consensus.coinbase_maturity
        {
            bail!("premature coinbase spend");
        }
        if verify_scripts {
            verify_p2pkh_input(tx, idx, coin)?;
        }
        total_in += coin.tx_out.value.atoms() as u128;
    }
    if total_in < total_out {
        bail!("insufficient input value");
    }
    Ok((total_in - total_out) as u64)
}
fn verify_p2pkh_input(tx: &Transaction, idx: usize, coin: &CoinRecord) -> Result<()> {
    let expected =
        p2pkh_payload(&coin.tx_out.script_pubkey).context("unsupported script_pubkey")?;
    let (pk, sig) = decode_sig_script(&tx.inputs[idx].signature_script)?;
    if public_key_hash(&pk) != expected {
        bail!("pubkey hash mismatch");
    }
    let sighash = tx.sighash_all(idx, &coin.tx_out.script_pubkey)?;
    if !verify_hash(&pk, &sig, sighash) {
        bail!("signature verification failed");
    }
    Ok(())
}
fn validate_block(
    block: &Block,
    base_utxos: &HashMap<OutPoint, CoinRecord>,
    expected_prev: Hash256,
    height: u32,
    required_bits: u32,
    settings: &Settings,
) -> Result<HashSet<Hash256>> {
    if block.transactions.is_empty() {
        bail!("empty block");
    }
    if block.transactions.len() > settings.consensus.max_block_transactions {
        bail!("too many block transactions");
    }
    let expected_version = expected_block_version(settings, height);
    if block.header.version != expected_version {
        bail!(
            "bad block version for {} at #{}: expected {}, got {}",
            protocol_epoch_name(settings, height),
            height,
            expected_version,
            block.header.version
        );
    }
    if block.header.prev_block_hash != expected_prev {
        bail!("bad prev hash");
    }
    if block.header.bits != required_bits {
        bail!(
            "bad bits: expected {required_bits:#x}, got {:#x}",
            block.header.bits
        );
    }
    if block.compute_merkle_root() != block.header.merkle_root {
        bail!("merkle mismatch");
    }
    if !verify_header_pow(&block.header)? {
        bail!("insufficient PoW");
    }
    if block.header.time > unix_time_u32().saturating_add(settings.consensus.max_future_time_secs) {
        bail!("future timestamp");
    }
    if !block.transactions[0].is_coinbase() {
        bail!("missing coinbase");
    }
    for tx in block.transactions.iter().skip(1) {
        if tx.is_coinbase() {
            bail!("extra coinbase");
        }
    }
    let mut ids = HashSet::new();
    for tx in &block.transactions {
        if !ids.insert(tx.txid()) {
            bail!("duplicate txid");
        }
    }
    let mut scratch = base_utxos.clone();
    let mut fees = 0u128;
    for tx in block.transactions.iter().skip(1) {
        let raw_fee = validate_tx_contextual(tx, &scratch, height, settings, true)?;
        let qub_melt_burn = qub_jin_melt_burn_atoms_for_fee(settings, tx, height)?;
        if raw_fee < qub_melt_burn {
            bail!("QUB melt burn exceeds transaction input-output delta");
        }
        let fee = raw_fee - qub_melt_burn;
        let qns_miner_fee = qns_miner_fee_required_in_tx(settings, tx, height)?;
        let library_miner_fee = library_miner_fee_required_in_tx(settings, tx, height)?;
        let swap_miner_fee = jin_swap_miner_fee_required_in_tx(settings, tx, height)?;
        let required_extra_fee = qns_miner_fee
            .checked_add(library_miner_fee)
            .and_then(|v| v.checked_add(swap_miner_fee))
            .context("extra miner fee overflow")?;
        if fee < required_extra_fee {
            bail!(
                "miner fee underpayment: required {} atoms as block fee, got {}",
                required_extra_fee,
                fee
            );
        }
        fees += fee as u128;
        connect_tx_utxos(tx, &mut scratch, height, false)?;
    }
    validate_tx_stateless(&block.transactions[0], settings)?;
    let claim: u128 = block.transactions[0]
        .outputs
        .iter()
        .map(|o| o.value.atoms() as u128)
        .sum();
    if claim > block_subsidy(height as u64, settings) as u128 + fees {
        bail!("coinbase overclaim");
    }
    Ok(ids)
}
fn connect_block_utxos(
    block: &Block,
    utxos: &mut HashMap<OutPoint, CoinRecord>,
    height: u32,
) -> Result<()> {
    for (idx, tx) in block.transactions.iter().enumerate() {
        connect_tx_utxos(tx, utxos, height, idx == 0)?;
    }
    Ok(())
}
pub(crate) fn connect_tx_utxos(
    tx: &Transaction,
    utxos: &mut HashMap<OutPoint, CoinRecord>,
    height: u32,
    is_coinbase: bool,
) -> Result<()> {
    if is_pool_share_transaction(tx) {
        return Ok(());
    }
    if !is_coinbase {
        for input in &tx.inputs {
            if utxos.remove(&input.previous_output).is_none() {
                bail!("missing UTXO during connect");
            }
        }
    }
    let txid = tx.txid();
    for (vout, output) in tx.outputs.iter().enumerate() {
        let old = utxos.insert(
            OutPoint {
                txid,
                vout: vout as u32,
            },
            CoinRecord {
                tx_out: output.clone(),
                height,
                is_coinbase,
            },
        );
        if old.is_some() {
            bail!("duplicate created outpoint");
        }
    }
    Ok(())
}
pub fn is_immature_coinbase(coin: &CoinRecord, current_height: u32, settings: &Settings) -> bool {
    coin.is_coinbase
        && current_height.saturating_sub(coin.height) < settings.consensus.coinbase_maturity
}
pub fn unix_time_u32() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(u32::MAX as u64) as u32
}

#[derive(Debug, Clone, Copy)]
pub struct MiningOptions {
    pub duty_cycle_percent: u8,
    pub max_hashes: Option<u64>,
}
impl MiningOptions {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            duty_cycle_percent: settings.mining.duty_cycle_percent,
            max_hashes: None,
        }
    }
}
pub fn create_coinbase(
    height: u32,
    value_atoms: u64,
    address: &Address,
    extra_nonce: u64,
) -> Result<Transaction> {
    let mut sig = Vec::new();
    sig.extend_from_slice(&height.to_le_bytes());
    sig.extend_from_slice(&extra_nonce.to_le_bytes());
    sig.extend_from_slice(b"/QUB Core v1.4.8/");
    Ok(Transaction {
        version: 1,
        inputs: vec![TxIn {
            previous_output: OutPoint::null(),
            signature_script: ScriptBuf(sig),
            sequence: u32::MAX,
        }],
        outputs: vec![TxOut {
            value: Amount::from_atoms(value_atoms)?,
            script_pubkey: address.script_pubkey(),
        }],
        locktime: 0,
    })
}

fn candidate_mempool_transactions(
    chain: &ChainState,
    settings: &Settings,
    height: u32,
    solo_miner_for_legacy_jin: Option<&str>,
) -> Result<(Vec<Transaction>, u64)> {
    let mut scratch = chain.utxos.clone();
    let mut jin_ledger = jin_ledger_from_blocks(settings, &chain.blocks)?;
    let mut qub_jin_state = qub_jin_infusion_state_from_blocks(settings, &chain.blocks)?;
    qub_jin_apply_bootstrap_for_context(
        settings,
        &mut jin_ledger,
        &mut qub_jin_state,
        chain.height(),
        height,
    )?;
    let mut txs = Vec::new();
    let mut fees = 0u64;
    let qns_registry = qns_registry_from_blocks(settings, &chain.blocks)?;
    let mut qns_seen = HashSet::new();
    let mut library_state = library_state_from_blocks(settings, &chain.blocks)?;
    let mut verified_governance_state =
        verified_governance_state_from_blocks(settings, &chain.blocks)?;
    let mut jin_sale_sold = jin_sale_sold_by_listing_from_blocks(settings, &chain.blocks)?;

    // HF75/v1.5.8: build candidates with the same contextual validators used
    // by mempool/block acceptance, but maintain scratch state incrementally.
    // Previously candidate building only checked a subset of feature validators;
    // JIN sale / Library / multi-action txs could sit in mempool until a later
    // sync dropped them, or miners could skip them in practice because a stale
    // per-feature state check was not updated for earlier txs in the same block.
    // HF105/v1.6.7: keep pool-share markers first, but also make high-impact
    // protocol txs deterministic. In particular, large JIN public-sale purchases
    // must be selected/revalidated in the same priority order everywhere, instead
    // of bouncing between mempool/block-template positions during fast blocks.
    let mut ordered_mempool = chain.mempool.iter().collect::<Vec<_>>();
    ordered_mempool.sort_by_cached_key(|tx| {
        (
            mempool_template_priority(settings, *tx),
            tx.txid().to_string(),
        )
    });
    let template_scan_limit = hf115_template_scan_limit(settings);
    for tx in ordered_mempool.into_iter().take(template_scan_limit) {
        if txs.len() + 1 >= settings.consensus.max_block_transactions {
            break;
        }

        if hf106_jin_sale_standardness_policy(tx, settings).is_err() {
            continue;
        }
        let Ok(raw_fee) = validate_tx_contextual(tx, &scratch, height, settings, true) else {
            continue;
        };
        let Ok(qub_melt_burn) = qub_jin_melt_burn_atoms_for_fee(settings, tx, height) else {
            continue;
        };
        if raw_fee < qub_melt_burn {
            continue;
        }
        let fee = raw_fee - qub_melt_burn;
        let qns_fee = qns_miner_fee_required_in_tx(settings, tx, height).unwrap_or(u64::MAX);
        let library_fee =
            library_miner_fee_required_in_tx(settings, tx, height).unwrap_or(u64::MAX);
        let swap_fee = jin_swap_miner_fee_required_in_tx(settings, tx, height).unwrap_or(u64::MAX);
        let Some(required_extra_fee) = qns_fee
            .checked_add(library_fee)
            .and_then(|v| v.checked_add(swap_fee))
        else {
            continue;
        };
        if fee < required_extra_fee {
            continue;
        }

        let mut qns_seen_trial = qns_seen.clone();
        if validate_qns_transaction_with_registry(
            tx,
            height,
            &qns_registry,
            &mut qns_seen_trial,
            settings,
        )
        .is_err()
        {
            continue;
        }
        if validate_pools_transaction_against_chain(tx, chain, height, settings).is_err() {
            continue;
        }

        let mut jin_trial = jin_ledger.clone();
        if validate_jin_transaction_with_ledger(
            tx,
            &mut jin_trial,
            &scratch,
            height,
            settings,
            solo_miner_for_legacy_jin,
        )
        .is_err()
        {
            continue;
        }
        let mut qub_jin_trial = qub_jin_state.clone();
        if validate_qub_jin_infusion_transaction_with_state(
            tx,
            &mut jin_trial,
            &mut qub_jin_trial,
            &scratch,
            height,
            settings,
        )
        .is_err()
        {
            continue;
        }

        let purchases = jin_sale_purchases_in_tx(tx, settings);
        let mut jin_sale_sold_trial = jin_sale_sold.clone();
        if !purchases.is_empty()
            && validate_jin_sale_purchase_state(
                &purchases,
                tx,
                &scratch,
                &mut jin_sale_sold_trial,
                height,
                settings,
            )
            .is_err()
        {
            continue;
        }

        if validate_library_transaction_with_state(tx, &library_state, &scratch, height, settings)
            .is_err()
        {
            continue;
        }
        let mut verified_governance_trial = verified_governance_state.clone();
        if validate_verified_governance_transaction_with_state(
            tx,
            &mut verified_governance_trial,
            &mut jin_trial,
            &scratch,
            &chain.blocks,
            height,
            settings,
        )
        .is_err()
        {
            continue;
        }

        let mut scratch_trial = scratch.clone();
        if connect_tx_utxos(tx, &mut scratch_trial, height, false).is_err() {
            continue;
        }

        let markers = library_markers_in_tx(tx, settings)
            .into_iter()
            .map(|(_, m, _)| m)
            .collect::<Vec<_>>();
        let mut library_trial = library_state.clone();
        if !markers.is_empty()
            && library_apply_markers(
                &mut library_trial,
                tx.txid().to_string(),
                height,
                unix_time_u32(),
                markers,
                settings,
            )
            .is_err()
        {
            continue;
        }

        qns_seen = qns_seen_trial;
        jin_ledger = jin_trial;
        qub_jin_state = qub_jin_trial;
        jin_sale_sold = jin_sale_sold_trial;
        library_state = library_trial;
        verified_governance_state = verified_governance_trial;
        scratch = scratch_trial;
        fees = fees.saturating_add(fee);
        txs.push((*tx).clone());
    }
    Ok((txs, fees))
}

#[derive(Debug, Clone)]
pub struct CandidateBlockParts {
    pub height: u32,
    pub prev_block_hash: Hash256,
    pub time: u32,
    pub bits: u32,
    pub version: u32,
    pub reward_atoms: u64,
    pub non_coinbase_transactions: Vec<Transaction>,
}

pub fn build_candidate_block_parts(
    chain: &ChainState,
    settings: &Settings,
    solo_miner_for_legacy_jin: Option<&str>,
) -> Result<CandidateBlockParts> {
    let height = chain.height().saturating_add(1);
    let (transactions, fees) =
        candidate_mempool_transactions(chain, settings, height, solo_miner_for_legacy_jin)?;
    let reward_atoms = block_subsidy(height as u64, settings)
        .checked_add(fees)
        .context("reward overflow")?;
    Ok(CandidateBlockParts {
        height,
        prev_block_hash: chain.tip_hash(),
        time: unix_time_u32(),
        bits: required_work_bits(settings, &chain.blocks, height)?,
        version: expected_block_version(settings, height),
        reward_atoms,
        non_coinbase_transactions: transactions,
    })
}

pub fn block_from_candidate_parts(parts: &CandidateBlockParts, coinbase: Transaction) -> Block {
    let mut transactions =
        Vec::with_capacity(parts.non_coinbase_transactions.len().saturating_add(1));
    transactions.push(coinbase);
    transactions.extend(parts.non_coinbase_transactions.iter().cloned());
    let merkle_root = merkle_root(
        &transactions
            .iter()
            .map(Transaction::txid)
            .collect::<Vec<_>>(),
    );
    Block {
        header: BlockHeader {
            version: parts.version,
            prev_block_hash: parts.prev_block_hash,
            merkle_root,
            time: parts.time,
            bits: parts.bits,
            nonce: 0,
        },
        transactions,
    }
}

pub fn build_candidate_block(
    chain: &ChainState,
    settings: &Settings,
    miner: &Address,
    extra_nonce: u64,
) -> Result<Block> {
    let miner_s = miner.to_string();
    let parts = build_candidate_block_parts(chain, settings, Some(&miner_s))?;
    let coinbase = create_coinbase(parts.height, parts.reward_atoms, miner, extra_nonce)?;
    Ok(block_from_candidate_parts(&parts, coinbase))
}

pub fn build_candidate_pool_block(
    chain: &ChainState,
    settings: &Settings,
    pool_id: Hash256,
    extra_nonce: u64,
) -> Result<Block> {
    let height = chain.height() + 1;
    if !pools_active(settings, height) {
        bail!(
            "pooled mining activates at block #{}",
            settings.pools.activation_height
        );
    }
    let registry = pools_registry_from_blocks(settings, &chain.blocks)?;
    if !registry.contains_key(&pool_id) {
        bail!("unknown pool_id");
    }
    let parts = build_candidate_block_parts(chain, settings, None)?;
    let outputs = expected_pool_coinbase_outputs(
        settings,
        &chain.blocks,
        pool_id,
        parts.reward_atoms as u128,
    )?;
    let coinbase = Transaction {
        version: 1,
        inputs: vec![TxIn {
            previous_output: OutPoint::null(),
            signature_script: pool_block_marker_script(parts.height, extra_nonce, pool_id),
            sequence: u32::MAX,
        }],
        outputs,
        locktime: 0,
    };
    Ok(block_from_candidate_parts(&parts, coinbase))
}

pub fn mine_next_block(
    chain: &ChainState,
    settings: &Settings,
    miner: &Address,
    options: MiningOptions,
) -> Result<Block> {
    if options.duty_cycle_percent == 0 || options.duty_cycle_percent > 100 {
        bail!("invalid duty cycle");
    }
    let mut extra = 0u64;
    let mut hashes = 0u64;
    loop {
        let mut block = build_candidate_block(chain, settings, miner, extra)?;
        for nonce in 0..=u32::MAX {
            block.header.nonce = nonce;
            if verify_header_pow(&block.header)? {
                return Ok(block);
            }
            hashes = hashes.saturating_add(1);
            if let Some(max) = options.max_hashes {
                if hashes >= max {
                    bail!("max_hashes reached");
                }
            }
            if options.duty_cycle_percent < 100 && hashes % 10_000 == 0 {
                std::thread::sleep(std::time::Duration::from_millis(
                    (100 - options.duty_cycle_percent) as u64,
                ));
            }
        }
        extra = extra.wrapping_add(1);
    }
}

pub fn mine_next_pool_block(
    chain: &ChainState,
    settings: &Settings,
    pool_id: Hash256,
    options: MiningOptions,
) -> Result<Block> {
    if options.duty_cycle_percent == 0 || options.duty_cycle_percent > 100 {
        bail!("invalid duty cycle");
    }
    let mut extra = 0u64;
    let mut hashes = 0u64;
    loop {
        let mut block = build_candidate_pool_block(chain, settings, pool_id, extra)?;
        for nonce in 0..=u32::MAX {
            block.header.nonce = nonce;
            if verify_header_pow(&block.header)? {
                return Ok(block);
            }
            hashes = hashes.saturating_add(1);
            if let Some(max) = options.max_hashes {
                if hashes >= max {
                    bail!("max_hashes reached");
                }
            }
            if options.duty_cycle_percent < 100 && hashes % 10_000 == 0 {
                std::thread::sleep(std::time::Duration::from_millis(
                    (100 - options.duty_cycle_percent) as u64,
                ));
            }
        }
        extra = extra.wrapping_add(1);
    }
}

fn mining_payout_label(settings: &Settings, block: &Block) -> (String, String) {
    if let Some(pool_id) = parse_pool_block_marker(block) {
        return (format!("pool:{}", pool_id), "pool".to_string());
    }
    if let Some(address) = block
        .transactions
        .first()
        .and_then(|tx| tx.outputs.first())
        .and_then(|out| {
            address_from_script_pubkey(&settings.network.address_prefix, &out.script_pubkey)
        })
    {
        return (address.to_string(), "solo".to_string());
    }
    ("unknown".to_string(), "unknown".to_string())
}

pub fn mining_stats_json(
    settings: &Settings,
    blocks: &[Block],
    requested_window: usize,
) -> serde_json::Value {
    let available = blocks.len().saturating_sub(1);
    let window = requested_window.max(1).min(4096).min(available.max(1));
    let start_index = blocks.len().saturating_sub(window).max(1);
    let selected = if blocks.len() > 1 {
        &blocks[start_index..]
    } else {
        &blocks[0..0]
    };

    let mut labels = Vec::<String>::with_capacity(selected.len());
    let mut label_types = HashMap::<String, String>::new();
    let mut counts = HashMap::<String, u64>::new();
    let mut coinbase_only = HashMap::<String, u64>::new();
    let mut versions = HashMap::<u32, u64>::new();
    let mut intervals = Vec::<u64>::new();
    let mut total_coinbase_only = 0u64;

    for (offset, block) in selected.iter().enumerate() {
        let (label, kind) = mining_payout_label(settings, block);
        labels.push(label.clone());
        label_types.entry(label.clone()).or_insert(kind);
        *counts.entry(label.clone()).or_insert(0) += 1;
        if block.transactions.len() == 1 {
            total_coinbase_only += 1;
            *coinbase_only.entry(label).or_insert(0) += 1;
        }
        *versions.entry(block.header.version).or_insert(0) += 1;
        if offset > 0 {
            let previous = selected[offset - 1].header.time;
            intervals.push(block.header.time.saturating_sub(previous) as u64);
        }
    }

    let block_count = selected.len() as u64;
    let mut distribution = counts
        .iter()
        .map(|(label, blocks)| {
            let share = if block_count == 0 {
                0.0
            } else {
                *blocks as f64 / block_count as f64
            };
            serde_json::json!({
                "label": label,
                "label_type": label_types.get(label).map(String::as_str).unwrap_or("unknown"),
                "blocks": blocks,
                "share": share,
                "share_percent": share * 100.0,
                "coinbase_only_blocks": coinbase_only.get(label).copied().unwrap_or(0),
            })
        })
        .collect::<Vec<_>>();
    distribution.sort_by(|a, b| {
        b.get("blocks")
            .and_then(serde_json::Value::as_u64)
            .cmp(&a.get("blocks").and_then(serde_json::Value::as_u64))
            .then_with(|| {
                a.get("label")
                    .and_then(serde_json::Value::as_str)
                    .cmp(&b.get("label").and_then(serde_json::Value::as_str))
            })
    });

    let hhi = counts.values().fold(0.0, |acc, count| {
        if block_count == 0 {
            acc
        } else {
            let share = *count as f64 / block_count as f64;
            acc + share * share
        }
    });
    let effective_labels = if hhi > 0.0 { 1.0 / hhi } else { 0.0 };
    let top_share = distribution
        .first()
        .and_then(|row| row.get("share_percent"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);

    let mut longest_streak = 0usize;
    let mut longest_streak_label = String::new();
    let mut current_streak = 0usize;
    let mut current_streak_label = String::new();
    for label in &labels {
        if current_streak_label == *label {
            current_streak += 1;
        } else {
            current_streak_label = label.clone();
            current_streak = 1;
        }
        if current_streak > longest_streak {
            longest_streak = current_streak;
            longest_streak_label = label.clone();
        }
    }

    let mut alt_len = 0usize;
    let mut alt_a = String::new();
    let mut alt_b = String::new();
    let mut longest_alt = usize::from(!labels.is_empty());
    let mut longest_alt_a = labels.first().cloned().unwrap_or_default();
    let mut longest_alt_b = String::new();
    for i in 0..labels.len() {
        if i == 0 {
            alt_len = 1;
            alt_a = labels[i].clone();
            alt_b.clear();
            continue;
        }
        if labels[i] == labels[i - 1] {
            alt_len = 1;
            alt_a = labels[i].clone();
            alt_b.clear();
        } else if alt_len == 1 {
            alt_len = 2;
            alt_a = labels[i - 1].clone();
            alt_b = labels[i].clone();
        } else {
            let expected = if alt_len % 2 == 0 { &alt_a } else { &alt_b };
            if labels[i] == *expected {
                alt_len += 1;
            } else {
                alt_len = 2;
                alt_a = labels[i - 1].clone();
                alt_b = labels[i].clone();
            }
        }
        if alt_len > longest_alt {
            longest_alt = alt_len;
            longest_alt_a = alt_a.clone();
            longest_alt_b = alt_b.clone();
        }
    }

    intervals.sort_unstable();
    let average_interval = if intervals.is_empty() {
        0.0
    } else {
        let total = intervals
            .iter()
            .fold(0u128, |sum, value| sum.saturating_add(u128::from(*value)));
        total as f64 / intervals.len() as f64
    };
    let median_interval = if intervals.is_empty() {
        0u64
    } else {
        intervals[intervals.len() / 2]
    };
    let p90_interval = if intervals.is_empty() {
        0u64
    } else {
        let index = ((intervals.len() - 1) as f64 * 0.90).ceil() as usize;
        intervals[index.min(intervals.len() - 1)]
    };

    let mut version_rows = versions
        .into_iter()
        .map(|(version, count)| serde_json::json!({"version":version,"blocks":count}))
        .collect::<Vec<_>>();
    version_rows.sort_by_key(|row| {
        row.get("version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    });

    let from_height = if selected.is_empty() {
        0
    } else {
        start_index as u32
    };
    let to_height = blocks.len().saturating_sub(1) as u32;
    serde_json::json!({
        "ok": true,
        "network": settings.network.name,
        "requested_window": requested_window,
        "window_blocks": selected.len(),
        "from_height": from_height,
        "to_height": to_height,
        "tip_hash": blocks.last().map(Block::block_hash).unwrap_or_else(Hash256::zero).to_string(),
        "unique_payout_labels": counts.len(),
        "top_label_share_percent": top_share,
        "hhi": hhi,
        "hhi_10000": hhi * 10000.0,
        "effective_label_count": effective_labels,
        "coinbase_only_blocks": total_coinbase_only,
        "coinbase_only_percent": if block_count == 0 { 0.0 } else { total_coinbase_only as f64 * 100.0 / block_count as f64 },
        "longest_same_label_streak": {
            "blocks": longest_streak,
            "label": longest_streak_label,
        },
        "current_same_label_streak": {
            "blocks": current_streak,
            "label": current_streak_label,
        },
        "longest_exact_two_label_alternation": {
            "blocks": longest_alt,
            "label_a": longest_alt_a,
            "label_b": longest_alt_b,
        },
        "current_exact_two_label_alternation": {
            "blocks": alt_len,
            "label_a": alt_a,
            "label_b": alt_b,
        },
        "interval_seconds": {
            "average": average_interval,
            "median": median_interval,
            "p90": p90_interval,
        },
        "block_versions": version_rows,
        "distribution": distribution,
        "interpretation_note": "A payout address or pool label is an observable on-chain label, not proof of a unique human, machine, cluster, or operator."
    })
}

pub fn required_work_bits(
    settings: &Settings,
    chain_blocks: &[Block],
    next_height: u32,
) -> Result<u32> {
    if next_height == 0 || chain_blocks.is_empty() {
        return settings.genesis_bits();
    }
    if next_height <= 1 {
        return settings.pow_bits();
    }
    if daa_v2_active(settings, next_height) {
        return required_work_bits_v2(settings, chain_blocks);
    }
    required_work_bits_legacy(settings, chain_blocks, next_height)
}

fn required_work_bits_legacy(
    settings: &Settings,
    chain_blocks: &[Block],
    next_height: u32,
) -> Result<u32> {
    let prev_bits = chain_blocks
        .last()
        .map(|b| b.header.bits)
        .unwrap_or(settings.pow_bits()?);
    let interval = settings.consensus.difficulty_adjustment_interval.max(1);
    if interval <= 1 || (next_height - 1) % interval != 0 || chain_blocks.len() <= interval as usize
    {
        return Ok(prev_bits);
    }

    retarget_bits(
        settings,
        chain_blocks,
        interval as usize,
        settings.consensus.difficulty_max_adjustment_factor.max(2) as u64,
    )
}

fn required_work_bits_v2(settings: &Settings, chain_blocks: &[Block]) -> Result<u32> {
    let available = chain_blocks.len().saturating_sub(1);
    if available < 2 {
        return Ok(chain_blocks
            .last()
            .map(|b| b.header.bits)
            .unwrap_or(settings.pow_bits()?));
    }
    let window = DAA_V2_WINDOW_BLOCKS.min(available).max(2);
    retarget_bits(settings, chain_blocks, window, DAA_V2_MAX_ADJUSTMENT_FACTOR)
}

fn retarget_bits(
    settings: &Settings,
    chain_blocks: &[Block],
    window: usize,
    max_factor: u64,
) -> Result<u32> {
    let prev_bits = chain_blocks
        .last()
        .map(|b| b.header.bits)
        .unwrap_or(settings.pow_bits()?);
    let last_index = chain_blocks.len() - 1;
    let first_index = last_index.saturating_sub(window);
    let first_time = chain_blocks[first_index].header.time;
    let last_time = chain_blocks[last_index].header.time;
    let target_timespan = settings.consensus.target_spacing_secs as u64 * window as u64;
    let max_factor = max_factor.max(2);
    let min_timespan = (target_timespan / max_factor).max(1);
    let max_timespan = target_timespan.saturating_mul(max_factor).max(1);
    let actual_timespan =
        (last_time.saturating_sub(first_time) as u64).clamp(min_timespan, max_timespan);

    let old_target = compact_to_biguint(prev_bits)?;
    let pow_limit = compact_to_biguint(settings.pow_bits()?)?;
    let mut new_target =
        old_target * BigUint::from(actual_timespan) / BigUint::from(target_timespan.max(1));
    if new_target.is_zero() {
        new_target = BigUint::one();
    }
    if new_target > pow_limit {
        new_target = pow_limit;
    }
    Ok(biguint_to_compact(&new_target))
}

fn compact_to_biguint(bits: u32) -> Result<BigUint> {
    Ok(BigUint::from_bytes_be(&target_from_compact(bits)?))
}

fn biguint_to_compact(target: &BigUint) -> u32 {
    if target.is_zero() {
        return 0;
    }
    let bytes = target.to_bytes_be();
    let mut size = bytes.len();
    let mut compact = if size <= 3 {
        let mut v = 0u32;
        for b in &bytes {
            v = (v << 8) | (*b as u32);
        }
        v << (8 * (3 - size))
    } else {
        ((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | (bytes[2] as u32)
    };
    if compact & 0x0080_0000 != 0 {
        compact >>= 8;
        size += 1;
    }
    ((size as u32) << 24) | (compact & 0x007f_ffff)
}

fn block_work(bits: u32) -> Result<BigUint> {
    let target = compact_to_biguint(bits)?;
    if target.is_zero() {
        bail!("zero work target");
    }
    Ok((BigUint::one() << 256usize) / (target + BigUint::one()))
}

fn chain_work_for_blocks(blocks: &[Block]) -> Result<BigUint> {
    let mut work = BigUint::zero();
    for block in blocks.iter().skip(1) {
        work += block_work(block.header.bits)?;
    }
    Ok(work)
}
pub fn target_from_compact(bits: u32) -> Result<[u8; 32]> {
    let exp = (bits >> 24) as usize;
    let mant = bits & 0x007f_ffff;
    if mant == 0 || bits & 0x0080_0000 != 0 {
        bail!("invalid compact target");
    }
    let mut out = [0u8; 32];
    let mb = [
        ((mant >> 16) & 0xff) as u8,
        ((mant >> 8) & 0xff) as u8,
        (mant & 0xff) as u8,
    ];
    if exp <= 3 {
        let v = mant >> (8 * (3 - exp));
        out[31] = (v & 0xff) as u8;
        if v > 0xff {
            out[30] = ((v >> 8) & 0xff) as u8;
        }
        if v > 0xffff {
            out[29] = ((v >> 16) & 0xff) as u8;
        }
    } else {
        let start = 32usize.checked_sub(exp).context("target overflow")?;
        if start + 3 > 32 {
            bail!("target overflow");
        }
        out[start..start + 3].copy_from_slice(&mb);
    }
    if out.iter().all(|b| *b == 0) {
        bail!("zero target");
    }
    Ok(out)
}
pub fn verify_header_pow(header: &BlockHeader) -> Result<bool> {
    let target = target_from_compact(header.bits)?;
    let mut hash_be = header.hash().0;
    hash_be.reverse();
    Ok(hash_be.as_slice() <= target.as_slice())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletFile {
    pub version: u32,
    pub network: String,
    pub keys: Vec<WalletKey>,
    pub default_index: Option<usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletKey {
    pub address: String,
    pub public_key_hex: String,
    pub secret_key_hex: String,
    pub label: String,
    pub created_height: u32,
}
impl WalletFile {
    pub fn new(network: &str) -> Self {
        Self {
            version: 1,
            network: network.to_string(),
            keys: Vec::new(),
            default_index: None,
        }
    }
    pub fn ensure_network(&self, settings: &Settings) -> Result<()> {
        if self.network != settings.network.name {
            bail!("wallet network mismatch");
        }
        Ok(())
    }
    pub fn create_key(
        &mut self,
        settings: &Settings,
        label: impl Into<String>,
        height: u32,
    ) -> Result<WalletKey> {
        ensure_plaintext_wallet_allowed(settings)?;
        let secret = generate_secret_key();
        let public = public_key_from_secret(&secret);
        let key = WalletKey {
            address: address_from_public_key(&settings.network.address_prefix, &public).to_string(),
            public_key_hex: hex::encode(public.serialize()),
            secret_key_hex: secret_key_to_hex(&secret),
            label: label.into(),
            created_height: height,
        };
        self.keys.push(key.clone());
        if self.default_index.is_none() {
            self.default_index = Some(0);
        }
        Ok(key)
    }
    pub fn default_key(&self) -> Option<&WalletKey> {
        self.default_index.and_then(|i| self.keys.get(i))
    }
    pub fn default_address(&self) -> Option<&str> {
        self.default_key().map(|k| k.address.as_str())
    }
    pub fn scripts(&self) -> Result<HashSet<Vec<u8>>> {
        let mut set = HashSet::new();
        for k in &self.keys {
            set.insert(Address::from_str(&k.address)?.script_pubkey().0);
        }
        Ok(set)
    }
    pub fn balance_atoms(
        &self,
        chain: &ChainState,
        settings: &Settings,
        include_immature: bool,
    ) -> Result<u64> {
        Ok(chain.balance_for_scripts(&self.scripts()?, settings, include_immature))
    }
    fn key_for_coin(&self, coin: &CoinRecord) -> Result<Option<&WalletKey>> {
        let Some(payload) = p2pkh_payload(&coin.tx_out.script_pubkey) else {
            return Ok(None);
        };
        for k in &self.keys {
            let public = PublicKey::from_slice(&hex::decode(&k.public_key_hex)?)?;
            if public_key_hash(&public.serialize()) == payload {
                return Ok(Some(k));
            }
        }
        Ok(None)
    }

    fn pending_spent_outpoints(chain: &ChainState) -> HashSet<OutPoint> {
        chain
            .mempool
            .iter()
            .flat_map(|tx| tx.inputs.iter())
            .filter(|input| input.previous_output != OutPoint::null())
            .map(|input| input.previous_output.clone())
            .collect::<HashSet<_>>()
    }

    fn ordered_wallet_keys_default_first(&self) -> Vec<&WalletKey> {
        let mut out = Vec::new();
        let mut seen = HashSet::<usize>::new();
        if let Some(default_idx) = self.default_index {
            if let Some(key) = self.keys.get(default_idx) {
                out.push(key);
                seen.insert(default_idx);
            }
        }
        for (idx, key) in self.keys.iter().enumerate() {
            if seen.insert(idx) {
                out.push(key);
            }
        }
        out
    }

    fn select_signed_inputs_for_target_from_script(
        &self,
        chain: &ChainState,
        settings: &Settings,
        target_atoms: u64,
        required_script: Option<&[u8]>,
    ) -> Result<(Vec<(OutPoint, CoinRecord, WalletKey)>, u64)> {
        let mut selected: Vec<(OutPoint, CoinRecord, WalletKey)> = Vec::new();
        let mut total = 0u64;
        let pending_spent = Self::pending_spent_outpoints(chain);
        for (outpoint, coin) in &chain.utxos {
            if pending_spent.contains(outpoint) {
                continue;
            }
            if is_immature_coinbase(coin, chain.height(), settings) {
                continue;
            }
            if let Some(script) = required_script {
                if coin.tx_out.script_pubkey.as_bytes() != script {
                    continue;
                }
            }
            if let Some(key) = self.key_for_coin(coin)? {
                total = total
                    .checked_add(coin.tx_out.value.atoms())
                    .context("selection overflow")?;
                selected.push((outpoint.clone(), coin.clone(), key.clone()));
                if total >= target_atoms {
                    break;
                }
            }
        }
        if total < target_atoms {
            if required_script.is_some() {
                bail!("insufficient spendable QUB on the required source address; wait for pending txs to confirm or send tiny QUB dust to that same address");
            }
            bail!("insufficient spendable balance; wait for pending txs to confirm or lower the amount");
        }
        Ok((selected, total))
    }

    fn select_wallet_address_inputs_for_target(
        &self,
        chain: &ChainState,
        settings: &Settings,
        target_atoms: u64,
    ) -> Result<(Address, Vec<(OutPoint, CoinRecord, WalletKey)>, u64)> {
        let mut saw_wallet_address = false;
        for key in self.ordered_wallet_keys_default_first() {
            let address =
                Address::parse_with_prefix(&key.address, &settings.network.address_prefix)?;
            saw_wallet_address = true;
            let script = address.script_pubkey().0;
            if let Ok((selected, total)) = self.select_signed_inputs_for_target_from_script(
                chain,
                settings,
                target_atoms,
                Some(&script),
            ) {
                return Ok((address, selected, total));
            }
        }
        if !saw_wallet_address {
            bail!("wallet empty");
        }
        bail!("no single wallet address has enough spendable QUB for this authorized transaction; wait for pending txs or fund one address with enough QUB dust")
    }

    fn select_jin_source_for_payment(
        &self,
        chain: &ChainState,
        settings: &Settings,
        required_jin_units: u128,
        target_qub_atoms: u64,
    ) -> Result<(Address, Vec<(OutPoint, CoinRecord, WalletKey)>, u64)> {
        let mut first_jin_source_without_qub_dust: Option<String> = None;
        for key in self.ordered_wallet_keys_default_first() {
            let address =
                Address::parse_with_prefix(&key.address, &settings.network.address_prefix)?;
            let address_text = address.to_string();
            let balance = jin_balance_units_for_address(settings, chain, &address_text)?;
            if balance < required_jin_units {
                continue;
            }
            let script = address.script_pubkey().0;
            if let Ok((selected, total)) = self.select_signed_inputs_for_target_from_script(
                chain,
                settings,
                target_qub_atoms,
                Some(&script),
            ) {
                return Ok((address, selected, total));
            }
            if first_jin_source_without_qub_dust.is_none() {
                first_jin_source_without_qub_dust = Some(address_text);
            }
        }
        if let Some(addr) = first_jin_source_without_qub_dust {
            bail!("JIN balance exists at {addr}, but that same source address has no spendable QUB dust for the on-chain marker/authorization; send a tiny QUB amount to {addr} and retry");
        }
        bail!("insufficient JIN balance on any local wallet address");
    }

    pub fn create_signed_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        to: &Address,
        amount: Amount,
        fee: Amount,
    ) -> Result<Transaction> {
        if to.prefix != settings.network.address_prefix {
            bail!("recipient network mismatch");
        }
        let target = amount.checked_add(fee)?.atoms();
        let (selected, total) = self.select_signed_inputs_for_target(chain, settings, target)?;
        let mut outputs = vec![TxOut {
            value: amount,
            script_pubkey: to.script_pubkey(),
        }];
        let change = total - target;
        if change > 0 {
            let change_address = Address::parse_with_prefix(
                self.default_address().context("wallet empty")?,
                &settings.network.address_prefix,
            )?;
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: change_address.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        self.sign_selected_transaction(tx, &selected)
    }

    pub fn create_multi_signed_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        payments: &[(Address, Amount)],
        fee: Amount,
    ) -> Result<Transaction> {
        if payments.is_empty() {
            bail!("multi-send requires at least one recipient");
        }
        if payments.len() > MAX_SEND_ENTRIES_PER_TX {
            bail!(
                "multi-send supports at most {} entries",
                MAX_SEND_ENTRIES_PER_TX
            );
        }
        let mut total_pay = 0u64;
        for (to, amount) in payments {
            if to.prefix != settings.network.address_prefix {
                bail!("recipient network mismatch");
            }
            if amount.atoms() == 0 {
                bail!("multi-send amount must be non-zero");
            }
            total_pay = total_pay
                .checked_add(amount.atoms())
                .context("multi-send amount overflow")?;
        }
        let target = total_pay
            .checked_add(fee.atoms())
            .context("multi-send target overflow")?;
        let (selected, total) = self.select_signed_inputs_for_target(chain, settings, target)?;
        let mut outputs = payments
            .iter()
            .map(|(to, amount)| TxOut {
                value: *amount,
                script_pubkey: to.script_pubkey(),
            })
            .collect::<Vec<_>>();
        let change = total - target;
        if change > 0 {
            let change_address = Address::parse_with_prefix(
                self.default_address().context("wallet empty")?,
                &settings.network.address_prefix,
            )?;
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: change_address.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        self.sign_selected_transaction(tx, &selected)
    }

    pub fn create_jin_multi_transfer_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        payments: &[(Address, u128)],
        qub_fee: Amount,
        jin_fee_units: u128,
        fee_asset: &str,
    ) -> Result<Transaction> {
        if !jin_active(settings, chain.height() + 1) {
            bail!("JIN activates at block #{}", settings.jin.activation_height);
        }
        if payments.is_empty() {
            bail!("JIN multi-send requires at least one recipient");
        }
        if payments.len() > MAX_SEND_ENTRIES_PER_TX {
            bail!(
                "JIN multi-send supports at most {} entries",
                MAX_SEND_ENTRIES_PER_TX
            );
        }
        let fee_asset = fee_asset.trim().to_ascii_uppercase();
        if fee_asset != "QUB" && fee_asset != "JIN" {
            bail!("JIN fee asset must be QUB or JIN");
        }
        let mut total_jin = 0u128;
        for (to, units) in payments {
            if to.prefix != settings.network.address_prefix {
                bail!("recipient network mismatch");
            }
            if *units == 0 {
                bail!("JIN multi-send amount must be non-zero");
            }
            total_jin = total_jin
                .checked_add(*units)
                .context("JIN multi-send amount overflow")?;
        }
        let required_jin = total_jin
            .checked_add(if fee_asset == "JIN" { jin_fee_units } else { 0 })
            .context("JIN multi-send fee overflow")?;
        let marker_atoms_total = settings
            .jin
            .marker_output_atoms
            .checked_mul(payments.len() as u64)
            .context("JIN marker atoms overflow")?;
        let target_qub = marker_atoms_total
            .checked_add(if fee_asset == "QUB" {
                qub_fee.atoms()
            } else {
                0
            })
            .context("QUB fee overflow")?;
        let (from_address, selected, total) =
            self.select_jin_source_for_payment(chain, settings, required_jin, target_qub)?;
        let mut outputs = Vec::with_capacity(payments.len() + 1);
        let fee_base = if fee_asset == "JIN" {
            jin_fee_units / payments.len() as u128
        } else {
            0
        };
        let fee_remainder = if fee_asset == "JIN" {
            jin_fee_units % payments.len() as u128
        } else {
            0
        };
        for (idx, (to, units)) in payments.iter().enumerate() {
            let marker_fee = if fee_asset == "JIN" {
                fee_base + if (idx as u128) < fee_remainder { 1 } else { 0 }
            } else {
                0
            };
            let marker_script = jin_marker_script_transfer(
                settings,
                &from_address,
                to,
                *units,
                marker_fee,
                &fee_asset,
            )?;
            outputs.push(TxOut {
                value: Amount::from_atoms(settings.jin.marker_output_atoms)?,
                script_pubkey: marker_script,
            });
        }
        let change = total - target_qub;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: from_address.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        let tx = self.sign_selected_transaction(tx, &selected)?;
        validate_jin_transaction_against_chain(&tx, chain, chain.height() + 1, settings)?;
        Ok(tx)
    }

    pub fn create_blast_create_transaction_qub(
        &self,
        chain: &ChainState,
        settings: &Settings,
        total: Amount,
        per_claim: Amount,
        max_claims: u32,
        code: &str,
        fee: Amount,
    ) -> Result<Transaction> {
        let spend_height = chain.height() + 1;
        if !blast_active(settings, spend_height) {
            bail!(
                "Blast activates at block #{}",
                blast_activation_height(settings)
            );
        }
        if total.atoms() == 0 || per_claim.atoms() == 0 {
            bail!("Blast total/per-claim amounts must be non-zero");
        }
        if max_claims == 0 || max_claims as usize > MAX_SEND_ENTRIES_PER_TX {
            bail!("Blast max_claims must be 1..256");
        }
        if per_claim
            .atoms()
            .checked_mul(max_claims as u64)
            .context("Blast amount overflow")?
            != total.atoms()
        {
            bail!("Blast total must equal per_claim * max_claims in v1.4.8");
        }
        let manager = Address::parse_with_prefix(
            self.default_address().context("wallet empty")?,
            &settings.network.address_prefix,
        )?;
        let code_hash = blast_code_hash_hex(code)?;
        let target = total
            .atoms()
            .checked_add(fee.atoms())
            .context("Blast target overflow")?;
        let (selected, selected_total) =
            self.select_signed_inputs_for_target(chain, settings, target)?;
        let mut outputs = vec![TxOut {
            value: total,
            script_pubkey: blast_vault_script(
                "QUB",
                &manager,
                &code_hash,
                per_claim.atoms() as u128,
                max_claims,
            )?,
        }];
        let change = selected_total - target;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: manager.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        self.sign_selected_transaction(tx, &selected)
    }

    pub fn create_blast_claim_transaction_qub(
        &self,
        chain: &ChainState,
        settings: &Settings,
        code_payload: &str,
        claimant: Option<&Address>,
    ) -> Result<Transaction> {
        let spend_height = chain.height() + 1;
        if !blast_active(settings, spend_height) {
            bail!(
                "Blast activates at block #{}",
                blast_activation_height(settings)
            );
        }
        let (hint_txid, hint_vout, code) = parse_blast_code_payload(code_payload)?;
        let claimant = match claimant {
            Some(a) => a.clone(),
            None => Address::parse_with_prefix(
                self.default_address().context("wallet empty")?,
                &settings.network.address_prefix,
            )?,
        };
        if claimant.prefix != settings.network.address_prefix {
            bail!("Blast claimant address network mismatch");
        }
        let code_hash = blast_code_hash_hex(&code)?;

        // A Blast claim spends and recreates the remaining vault, so the active
        // vault outpoint changes after every successful claim. The private QR
        // payload may contain the original outpoint as a hint, but if that hint
        // is already spent we search the active UTXO set for the unique remaining
        // vault with the same code hash. This makes one creator QR/code usable
        // for the full Blast campaign without requiring fresh code distribution
        // after each claim. If the creator reused a code across multiple active
        // blasts, the claim is intentionally refused as ambiguous.
        let pending_spent = Self::pending_spent_outpoints(chain);
        let hint_prev = OutPoint {
            txid: hint_txid,
            vout: hint_vout,
        };
        let mut selected: Option<(OutPoint, CoinRecord, BlastVault)> = None;
        if !pending_spent.contains(&hint_prev) {
            if let Some(coin) = chain.utxos.get(&hint_prev) {
                if let Some(vault) = parse_blast_vault_script(&coin.tx_out.script_pubkey, settings)
                {
                    if vault.asset == "QUB"
                        && vault.code_hash == code_hash
                        && vault.remaining_claims > 0
                    {
                        selected = Some((hint_prev.clone(), coin.clone(), vault));
                    }
                }
            }
        }
        if selected.is_none() {
            for (outpoint, coin) in &chain.utxos {
                if pending_spent.contains(outpoint) {
                    continue;
                }
                let Some(vault) = parse_blast_vault_script(&coin.tx_out.script_pubkey, settings)
                else {
                    continue;
                };
                if vault.asset != "QUB"
                    || vault.code_hash != code_hash
                    || vault.remaining_claims == 0
                {
                    continue;
                }
                if selected.is_some() {
                    bail!("multiple active Blast vaults use this code; use a unique Blast code");
                }
                selected = Some((outpoint.clone(), coin.clone(), vault));
            }
        }
        let (prev, coin, vault) = selected.context("active Blast vault output not found in canonical chain, or it is already pending in local mempool")?;
        let per_claim_atoms =
            u64::try_from(vault.per_claim_units).context("Blast QUB amount too large")?;
        if coin.tx_out.value.atoms() < per_claim_atoms {
            bail!("Blast vault has insufficient remaining QUB");
        }
        let mut outputs = vec![TxOut {
            value: Amount::from_atoms(per_claim_atoms)?,
            script_pubkey: claimant.script_pubkey(),
        }];
        let remaining_value = coin.tx_out.value.atoms() - per_claim_atoms;
        let remaining_claims = vault.remaining_claims.saturating_sub(1);
        if remaining_value > 0 && remaining_claims > 0 {
            let manager = Address::parse_with_prefix(
                &vault.manager_address,
                &settings.network.address_prefix,
            )?;
            outputs.push(TxOut {
                value: Amount::from_atoms(remaining_value)?,
                script_pubkey: blast_vault_script(
                    &vault.asset,
                    &manager,
                    &vault.code_hash,
                    vault.per_claim_units,
                    remaining_claims,
                )?,
            });
        }
        Ok(Transaction {
            version: 1,
            inputs: vec![TxIn {
                previous_output: prev,
                signature_script: blast_claim_script(&claimant, &code)?,
                sequence: u32::MAX,
            }],
            outputs,
            locktime: 0,
        })
    }

    pub fn jin_balance_units(&self, chain: &ChainState, settings: &Settings) -> Result<u128> {
        let mut total = 0u128;
        for key in &self.keys {
            total = total
                .checked_add(jin_balance_units_for_address(
                    settings,
                    chain,
                    &key.address,
                )?)
                .context("JIN balance overflow")?;
        }
        Ok(total)
    }

    pub fn create_jin_transfer_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        to: &Address,
        amount_units: u128,
        qub_fee: Amount,
        jin_fee_units: u128,
        fee_asset: &str,
    ) -> Result<Transaction> {
        if !jin_active(settings, chain.height() + 1) {
            bail!("JIN activates at block #{}", settings.jin.activation_height);
        }
        if to.prefix != settings.network.address_prefix {
            bail!("recipient network mismatch");
        }
        if amount_units == 0 {
            bail!("JIN amount must be non-zero");
        }
        let fee_asset = fee_asset.trim().to_ascii_uppercase();
        if fee_asset != "QUB" && fee_asset != "JIN" {
            bail!("JIN fee asset must be QUB or JIN");
        }
        let required_jin = amount_units
            .checked_add(if fee_asset == "JIN" { jin_fee_units } else { 0 })
            .context("JIN amount overflow")?;
        let marker_atoms = settings.jin.marker_output_atoms;
        let target_qub = marker_atoms
            .checked_add(if fee_asset == "QUB" {
                qub_fee.atoms()
            } else {
                0
            })
            .context("QUB fee overflow")?;
        let (from_address, selected, total) =
            self.select_jin_source_for_payment(chain, settings, required_jin, target_qub)?;
        let marker_script = jin_marker_script_transfer(
            settings,
            &from_address,
            to,
            amount_units,
            jin_fee_units,
            &fee_asset,
        )?;
        let mut outputs = vec![TxOut {
            value: Amount::from_atoms(marker_atoms)?,
            script_pubkey: marker_script,
        }];
        let change = total - target_qub;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: from_address.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        let tx = self.sign_selected_transaction(tx, &selected)?;
        validate_jin_transaction_against_chain(&tx, chain, chain.height() + 1, settings)?;
        Ok(tx)
    }

    pub fn create_jin_public_sale_buy_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        listing_id: u32,
        amount_units: u128,
        fee: Amount,
    ) -> Result<Transaction> {
        let spend_height = chain.height() + 1;
        if !jin_swap_active(settings, spend_height) {
            bail!(
                "JIN public sale activates at block #{}",
                jin_swap_activation_height(settings)
            );
        }
        if amount_units > HF106_MAX_STANDARD_JIN_SALE_BUY_UNITS {
            bail!("HF106 safety policy: split JIN buys into <= 50,000 JIN per transaction");
        }
        let price_atoms = jin_swap_sale_price_atoms(settings, listing_id, amount_units)?;
        let (protocol_fee_atoms, miner_fee_atoms) =
            jin_swap_fee_split_atoms(settings, price_atoms)?;
        let protocol = Address::parse_with_prefix(
            &settings.jin.protocol_address,
            &settings.network.address_prefix,
        )?;
        let marker_atoms = settings.jin_swap.marker_output_atoms;
        let protocol_payment = price_atoms
            .checked_add(protocol_fee_atoms)
            .context("JIN sale protocol payment overflow")?;
        let target_qub = marker_atoms
            .checked_add(protocol_payment)
            .context("JIN sale target overflow")?
            .checked_add(miner_fee_atoms)
            .context("JIN sale target overflow")?
            .checked_add(fee.atoms())
            .context("JIN sale target overflow")?;
        let (buyer, selected, total) =
            self.select_wallet_address_inputs_for_target(chain, settings, target_qub)?;
        let marker_script =
            jin_sale_purchase_marker_script(settings, &buyer, listing_id, amount_units)?;
        let mut outputs = vec![
            TxOut {
                value: Amount::from_atoms(protocol_payment)?,
                script_pubkey: protocol.script_pubkey(),
            },
            TxOut {
                value: Amount::from_atoms(marker_atoms)?,
                script_pubkey: marker_script,
            },
        ];
        let change = total - target_qub;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: buyer.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        let tx = self.sign_selected_transaction(tx, &selected)?;
        validate_jin_sale_transaction_against_chain(&tx, chain, spend_height, settings)?;
        Ok(tx)
    }

    pub fn create_qub_jin_infuse_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        amount_units: u128,
        fee: Amount,
    ) -> Result<Transaction> {
        let spend_height = chain.height() + 1;
        if !qub_jin_infusion_active(settings, spend_height) {
            bail!(
                "QUB/JIN infusion activates at block #{}",
                qub_jin_infusion_activation_height(settings)
            );
        }
        let state = qub_jin_infusion_state(settings, chain)?;
        if amount_units == 0 {
            bail!("JIN->QUB infusion amount must be non-zero");
        }
        if state.true_max_qub_atoms == 0 {
            bail!("cannot infuse JIN after all QUB has been melted");
        }
        let step = state.true_max_qub_atoms as u128;
        if amount_units % step != 0 {
            bail!("JIN->QUB infusion amount must be a multiple of {} JIN units ({}) so every QUB atom receives an exact increment", step, format_jin_amount(step));
        }
        let marker_atoms = settings.jin.marker_output_atoms;
        let target_qub = marker_atoms
            .checked_add(fee.atoms())
            .context("QUB infusion fee overflow")?;
        let (from_address, selected, total) =
            self.select_jin_source_for_payment(chain, settings, amount_units, target_qub)?;
        let marker_script = qub_jin_infuse_marker_script(settings, &from_address, amount_units)?;
        let mut outputs = vec![TxOut {
            value: Amount::from_atoms(marker_atoms)?,
            script_pubkey: marker_script,
        }];
        let change = total - target_qub;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: from_address.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        let tx = self.sign_selected_transaction(tx, &selected)?;
        validate_qub_jin_infusion_transaction_against_chain(&tx, chain, spend_height, settings)?;
        Ok(tx)
    }

    pub fn create_qub_melt_for_jin_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        qub_amount: Amount,
        fee: Amount,
        min_jin_units: u128,
    ) -> Result<Transaction> {
        let spend_height = chain.height() + 1;
        if !qub_jin_infusion_active(settings, spend_height) {
            bail!(
                "QUB/JIN infusion activates at block #{}",
                qub_jin_infusion_activation_height(settings)
            );
        }
        let qub_atoms = qub_amount.atoms();
        if qub_atoms == 0 {
            bail!("QUB melt amount must be non-zero");
        }
        let payout = qub_jin_melt_payout_units_for_atoms(settings, chain, qub_atoms)?;
        if payout < min_jin_units {
            bail!("QUB melt payout is below minimum JIN units");
        }
        let marker_atoms = settings.jin.marker_output_atoms;
        let target_qub = qub_atoms
            .checked_add(marker_atoms)
            .context("QUB melt target overflow")?
            .checked_add(fee.atoms())
            .context("QUB melt target overflow")?;
        let (from_address, selected, total) =
            self.select_wallet_address_inputs_for_target(chain, settings, target_qub)?;
        let marker_script =
            qub_jin_melt_marker_script(settings, &from_address, qub_atoms, min_jin_units)?;
        let mut outputs = vec![TxOut {
            value: Amount::from_atoms(marker_atoms)?,
            script_pubkey: marker_script,
        }];
        let change = total - target_qub;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: from_address.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        let tx = self.sign_selected_transaction(tx, &selected)?;
        validate_qub_jin_infusion_transaction_against_chain(&tx, chain, spend_height, settings)?;
        Ok(tx)
    }

    pub fn create_jin_token_conversion_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        matrix_address: &str,
        amount_units: u128,
        qub_fee: Amount,
        jin_fee_units: u128,
        fee_asset: &str,
    ) -> Result<Transaction> {
        let spend_height = chain.height() + 1;
        if !jin_active(settings, spend_height) {
            bail!("JIN activates at block #{}", settings.jin.activation_height);
        }
        if !jin_conversion_active(settings, spend_height) {
            bail!("JIN Coin -> Token conversion is disabled until the Enjin bridge is live");
        }
        let matrix_address = validate_matrix_address_like(matrix_address)?;
        let fee_asset = fee_asset.trim().to_ascii_uppercase();
        if fee_asset != "QUB" && fee_asset != "JIN" {
            bail!("JIN conversion fee asset must be QUB or JIN");
        }
        let required_jin = amount_units
            .checked_add(if fee_asset == "JIN" { jin_fee_units } else { 0 })
            .context("JIN conversion amount overflow")?;
        let marker_atoms = settings.jin.marker_output_atoms;
        let target_qub = marker_atoms
            .checked_add(if fee_asset == "QUB" {
                qub_fee.atoms()
            } else {
                0
            })
            .context("QUB fee overflow")?;
        let (from_address, selected, total) =
            self.select_jin_source_for_payment(chain, settings, required_jin, target_qub)?;
        let marker_script = jin_marker_script_conversion(
            settings,
            &from_address,
            &matrix_address,
            amount_units,
            jin_fee_units,
            &fee_asset,
        )?;
        let mut outputs = vec![TxOut {
            value: Amount::from_atoms(marker_atoms)?,
            script_pubkey: marker_script,
        }];
        let change = total - target_qub;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: from_address.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        let tx = self.sign_selected_transaction(tx, &selected)?;
        validate_jin_transaction_against_chain(&tx, chain, spend_height, settings)?;
        Ok(tx)
    }

    pub fn create_qns_registration_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        name: &str,
        target_address: &Address,
        fee: Amount,
    ) -> Result<Transaction> {
        if !settings.qns.enabled {
            bail!("QNS is disabled on this network");
        }
        let spend_height = chain.height() + 1;
        if spend_height < settings.qns.activation_height {
            bail!("QNS activates at block #{}", settings.qns.activation_height);
        }
        if qns_resolve(settings, chain, name)?.is_some() {
            bail!("QNS name already registered");
        }
        let name = normalize_qns_name(name, settings.qns.max_label_chars)?;
        if target_address.prefix != settings.network.address_prefix {
            bail!("QNS target address network mismatch");
        }
        let protocol = Address::parse_with_prefix(
            &settings.qns.protocol_address,
            &settings.network.address_prefix,
        )?;
        let price_atoms = qns_registration_price_atoms(settings, &name)?;
        let protocol_atoms = qns_protocol_share_atoms(settings, spend_height, price_atoms);
        let marker_atoms = settings.qns.marker_output_atoms;
        // Total cost is unchanged. After split activation, the miner share is paid as block fee;
        // the protocol output receives only the protocol half.
        let target = price_atoms
            .checked_add(marker_atoms)
            .and_then(|v| v.checked_add(fee.atoms()))
            .context("QNS payment overflow")?;
        let mut selected: Vec<(OutPoint, CoinRecord, WalletKey)> = Vec::new();
        let mut total = 0u64;
        for (outpoint, coin) in &chain.utxos {
            if is_immature_coinbase(coin, chain.height(), settings) {
                continue;
            }
            if let Some(key) = self.key_for_coin(coin)? {
                total = total
                    .checked_add(coin.tx_out.value.atoms())
                    .context("selection overflow")?;
                selected.push((outpoint.clone(), coin.clone(), key.clone()));
                if total >= target {
                    break;
                }
            }
        }
        if total < target {
            bail!("insufficient spendable balance for QNS registration");
        }
        let mut outputs = vec![
            TxOut {
                value: Amount::from_atoms(protocol_atoms)?,
                script_pubkey: protocol.script_pubkey(),
            },
            TxOut {
                value: Amount::from_atoms(marker_atoms)?,
                script_pubkey: qns_marker_script(&name, target_address, settings)?,
            },
        ];
        let change = total - target;
        if change > 0 {
            let change_address = Address::parse_with_prefix(
                self.default_address().context("wallet empty")?,
                &settings.network.address_prefix,
            )?;
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: change_address.script_pubkey(),
            });
        }
        let mut tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        for (idx, (_, coin, key)) in selected.iter().enumerate() {
            let secret = secret_key_from_hex(&key.secret_key_hex)?;
            let public = PublicKey::from_slice(&hex::decode(&key.public_key_hex)?)?;
            let sighash = tx.sighash_all(idx, &coin.tx_out.script_pubkey)?;
            let sig = sign_hash(&secret, sighash)?;
            tx.inputs[idx].signature_script = encode_sig_script(&public.serialize(), &sig)?;
        }
        validate_qns_transaction_against_chain(&tx, chain, spend_height, settings)?;
        let fee_atoms = validate_tx_contextual(&tx, &chain.utxos, spend_height, settings, true)?;
        let miner_required = qns_miner_fee_required_in_tx(settings, &tx, spend_height)?;
        if fee_atoms < miner_required {
            bail!(
                "QNS miner split underpayment: required {} atoms as block fee, got {}",
                miner_required,
                fee_atoms
            );
        }
        Ok(tx)
    }

    pub fn create_pool_create_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        name: &str,
        manager: &Address,
        commission_bps: u16,
        capacity_slots: u32,
        fee: Amount,
    ) -> Result<Transaction> {
        if !settings.features.pooled_mining_enabled || !settings.pools.enabled {
            bail!("pooled mining is disabled on this network");
        }
        let spend_height = chain.height() + 1;
        if spend_height < settings.pools.activation_height {
            bail!(
                "pooled mining activates at block #{}",
                settings.pools.activation_height
            );
        }
        if manager.prefix != settings.network.address_prefix {
            bail!("pool manager address network mismatch");
        }
        let name = normalize_pool_name(
            name,
            settings.pools.max_name_chars,
            settings.pools.max_name_bytes,
        )?;
        if commission_bps > settings.pools.max_commission_bps {
            bail!("commission exceeds max_commission_bps");
        }
        let protocol = Address::parse_with_prefix(
            &settings.pools.protocol_address,
            &settings.network.address_prefix,
        )?;
        let price_atoms = pool_create_price_atoms(settings, capacity_slots)?;
        let protocol_atoms = pool_protocol_share_atoms(price_atoms);
        let marker_atoms = settings.pools.marker_output_atoms;
        let target = price_atoms
            .checked_add(marker_atoms)
            .and_then(|v| v.checked_add(fee.atoms()))
            .context("pool create payment overflow")?;
        let mut selected: Vec<(OutPoint, CoinRecord, WalletKey)> = Vec::new();
        let mut total = 0u64;
        for (outpoint, coin) in &chain.utxos {
            if is_immature_coinbase(coin, chain.height(), settings) {
                continue;
            }
            if let Some(key) = self.key_for_coin(coin)? {
                total = total
                    .checked_add(coin.tx_out.value.atoms())
                    .context("selection overflow")?;
                selected.push((outpoint.clone(), coin.clone(), key.clone()));
                if total >= target {
                    break;
                }
            }
        }
        if total < target {
            bail!("insufficient spendable balance for pool creation");
        }
        let mut outputs = vec![
            TxOut {
                value: Amount::from_atoms(protocol_atoms)?,
                script_pubkey: protocol.script_pubkey(),
            },
            TxOut {
                value: Amount::from_atoms(marker_atoms)?,
                script_pubkey: pool_create_marker_script(
                    &name,
                    manager,
                    commission_bps,
                    capacity_slots,
                    settings,
                )?,
            },
        ];
        let change = total - target;
        if change > 0 {
            let change_address = Address::parse_with_prefix(
                self.default_address().context("wallet empty")?,
                &settings.network.address_prefix,
            )?;
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: change_address.script_pubkey(),
            });
        }
        let mut tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        for (idx, (_, coin, key)) in selected.iter().enumerate() {
            let secret = secret_key_from_hex(&key.secret_key_hex)?;
            let public = PublicKey::from_slice(&hex::decode(&key.public_key_hex)?)?;
            let sighash = tx.sighash_all(idx, &coin.tx_out.script_pubkey)?;
            let sig = sign_hash(&secret, sighash)?;
            tx.inputs[idx].signature_script = encode_sig_script(&public.serialize(), &sig)?;
        }
        validate_pools_transaction_against_chain(&tx, chain, spend_height, settings)?;
        Ok(tx)
    }

    pub fn create_pool_topup_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        pool_id: Hash256,
        extra_capacity_slots: u32,
        fee: Amount,
    ) -> Result<Transaction> {
        if !settings.features.pooled_mining_enabled || !settings.pools.enabled {
            bail!("pooled mining is disabled on this network");
        }
        let spend_height = chain.height() + 1;
        if spend_height < settings.pools.activation_height {
            bail!(
                "pooled mining activates at block #{}",
                settings.pools.activation_height
            );
        }
        let registry = pools_registry_from_blocks(settings, &chain.blocks)?;
        let pool = registry.get(&pool_id).context("unknown pool_id")?;
        let manager =
            Address::parse_with_prefix(&pool.manager_address, &settings.network.address_prefix)?;
        let protocol = Address::parse_with_prefix(
            &settings.pools.protocol_address,
            &settings.network.address_prefix,
        )?;
        let new_capacity = pool
            .capacity_slots
            .checked_add(extra_capacity_slots)
            .context("pool capacity overflow")?;
        if new_capacity > settings.pools.max_capacity_slots {
            bail!("pool capacity exceeds max_capacity_slots");
        }
        let price_atoms = pool_topup_price_atoms(settings, extra_capacity_slots)?;
        let protocol_atoms = pool_protocol_share_atoms(price_atoms);
        let marker_atoms = settings.pools.marker_output_atoms;
        let target = price_atoms
            .checked_add(marker_atoms)
            .and_then(|v| v.checked_add(fee.atoms()))
            .context("pool top-up payment overflow")?;
        let manager_script = manager.script_pubkey().0;
        let mut selected: Vec<(OutPoint, CoinRecord, WalletKey)> = Vec::new();
        let mut total = 0u64;
        for (outpoint, coin) in &chain.utxos {
            if is_immature_coinbase(coin, chain.height(), settings) {
                continue;
            }
            if coin.tx_out.script_pubkey.0 != manager_script {
                continue;
            }
            if let Some(key) = self.key_for_coin(coin)? {
                total = total
                    .checked_add(coin.tx_out.value.atoms())
                    .context("selection overflow")?;
                selected.push((outpoint.clone(), coin.clone(), key.clone()));
                if total >= target {
                    break;
                }
            }
        }
        if total < target {
            bail!("insufficient manager-owned spendable balance for pool top-up");
        }
        let mut outputs = vec![
            TxOut {
                value: Amount::from_atoms(protocol_atoms)?,
                script_pubkey: protocol.script_pubkey(),
            },
            TxOut {
                value: Amount::from_atoms(marker_atoms)?,
                script_pubkey: pool_topup_marker_script(
                    pool_id,
                    &manager,
                    extra_capacity_slots,
                    settings,
                )?,
            },
        ];
        let change = total - target;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: manager.script_pubkey(),
            });
        }
        let mut tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        for (idx, (_, coin, key)) in selected.iter().enumerate() {
            let secret = secret_key_from_hex(&key.secret_key_hex)?;
            let public = PublicKey::from_slice(&hex::decode(&key.public_key_hex)?)?;
            let sighash = tx.sighash_all(idx, &coin.tx_out.script_pubkey)?;
            let sig = sign_hash(&secret, sighash)?;
            tx.inputs[idx].signature_script = encode_sig_script(&public.serialize(), &sig)?;
        }
        validate_pools_transaction_against_chain(&tx, chain, spend_height, settings)?;
        Ok(tx)
    }

    pub fn create_pool_set_commission_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        pool_id: Hash256,
        new_commission_bps: u16,
        fee: Amount,
    ) -> Result<Transaction> {
        if !settings.features.pooled_mining_enabled || !settings.pools.enabled {
            bail!("pooled mining is disabled on this network");
        }
        let spend_height = chain.height() + 1;
        if spend_height < settings.pools.activation_height {
            bail!(
                "pooled mining activates at block #{}",
                settings.pools.activation_height
            );
        }
        let registry = pools_registry_from_blocks(settings, &chain.blocks)?;
        let pool = registry.get(&pool_id).context("unknown pool_id")?;
        if new_commission_bps > pool.commission_bps {
            bail!("pool commission can only decrease");
        }
        let manager =
            Address::parse_with_prefix(&pool.manager_address, &settings.network.address_prefix)?;
        let marker_atoms = settings.pools.marker_output_atoms;
        let target = marker_atoms
            .checked_add(fee.atoms())
            .context("pool commission tx payment overflow")?;
        let manager_script = manager.script_pubkey().0;
        let mut selected: Vec<(OutPoint, CoinRecord, WalletKey)> = Vec::new();
        let mut total = 0u64;
        for (outpoint, coin) in &chain.utxos {
            if is_immature_coinbase(coin, chain.height(), settings) {
                continue;
            }
            if coin.tx_out.script_pubkey.0 != manager_script {
                continue;
            }
            if let Some(key) = self.key_for_coin(coin)? {
                total = total
                    .checked_add(coin.tx_out.value.atoms())
                    .context("selection overflow")?;
                selected.push((outpoint.clone(), coin.clone(), key.clone()));
                if total >= target {
                    break;
                }
            }
        }
        if total < target {
            bail!("insufficient manager-owned spendable balance for pool commission update");
        }
        let mut outputs = vec![TxOut {
            value: Amount::from_atoms(marker_atoms)?,
            script_pubkey: pool_commission_marker_script(
                pool_id,
                &manager,
                new_commission_bps,
                settings,
            )?,
        }];
        let change = total - target;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: manager.script_pubkey(),
            });
        }
        let mut tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        for (idx, (_, coin, key)) in selected.iter().enumerate() {
            let secret = secret_key_from_hex(&key.secret_key_hex)?;
            let public = PublicKey::from_slice(&hex::decode(&key.public_key_hex)?)?;
            let sighash = tx.sighash_all(idx, &coin.tx_out.script_pubkey)?;
            let sig = sign_hash(&secret, sighash)?;
            tx.inputs[idx].signature_script = encode_sig_script(&public.serialize(), &sig)?;
        }
        validate_pools_transaction_against_chain(&tx, chain, spend_height, settings)?;
        Ok(tx)
    }

    pub fn create_pool_rename_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        pool_id: Hash256,
        new_name: &str,
        fee: Amount,
    ) -> Result<Transaction> {
        if !settings.features.pooled_mining_enabled || !settings.pools.enabled {
            bail!("pooled mining is disabled on this network");
        }
        let spend_height = chain.height() + 1;
        if spend_height < settings.pools.activation_height {
            bail!(
                "pooled mining activates at block #{}",
                settings.pools.activation_height
            );
        }
        let registry = pools_registry_from_blocks(settings, &chain.blocks)?;
        let pool = registry.get(&pool_id).context("unknown pool_id")?;
        let new_name = normalize_pool_name(
            new_name,
            settings.pools.max_name_chars,
            settings.pools.max_name_bytes,
        )?;
        let manager =
            Address::parse_with_prefix(&pool.manager_address, &settings.network.address_prefix)?;
        let marker_atoms = settings.pools.marker_output_atoms;
        let target = marker_atoms
            .checked_add(fee.atoms())
            .context("pool rename tx payment overflow")?;
        let manager_script = manager.script_pubkey().0;
        let mut selected: Vec<(OutPoint, CoinRecord, WalletKey)> = Vec::new();
        let mut total = 0u64;
        for (outpoint, coin) in &chain.utxos {
            if is_immature_coinbase(coin, chain.height(), settings) {
                continue;
            }
            if coin.tx_out.script_pubkey.0 != manager_script {
                continue;
            }
            if let Some(key) = self.key_for_coin(coin)? {
                total = total
                    .checked_add(coin.tx_out.value.atoms())
                    .context("selection overflow")?;
                selected.push((outpoint.clone(), coin.clone(), key.clone()));
                if total >= target {
                    break;
                }
            }
        }
        if total < target {
            bail!("insufficient manager-owned spendable balance for pool rename");
        }
        let mut outputs = vec![TxOut {
            value: Amount::from_atoms(marker_atoms)?,
            script_pubkey: pool_rename_marker_script(pool_id, &manager, &new_name, settings)?,
        }];
        let change = total - target;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: manager.script_pubkey(),
            });
        }
        let mut tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        for (idx, (_, coin, key)) in selected.iter().enumerate() {
            let secret = secret_key_from_hex(&key.secret_key_hex)?;
            let public = PublicKey::from_slice(&hex::decode(&key.public_key_hex)?)?;
            let sighash = tx.sighash_all(idx, &coin.tx_out.script_pubkey)?;
            let sig = sign_hash(&secret, sighash)?;
            tx.inputs[idx].signature_script = encode_sig_script(&public.serialize(), &sig)?;
        }
        validate_pools_transaction_against_chain(&tx, chain, spend_height, settings)?;
        Ok(tx)
    }

    fn select_signed_inputs_for_target(
        &self,
        chain: &ChainState,
        settings: &Settings,
        target_atoms: u64,
    ) -> Result<(Vec<(OutPoint, CoinRecord, WalletKey)>, u64)> {
        // HF79/v1.6.0: common wallet transaction builders use the same
        // pending-aware UTXO selector so normal send, JIN send, Library and Blast
        // do not accidentally reuse an outpoint already consumed by local mempool.
        self.select_signed_inputs_for_target_from_script(chain, settings, target_atoms, None)
    }

    fn sign_selected_transaction(
        &self,
        mut tx: Transaction,
        selected: &[(OutPoint, CoinRecord, WalletKey)],
    ) -> Result<Transaction> {
        for (idx, (_, coin, key)) in selected.iter().enumerate() {
            let secret = secret_key_from_hex(&key.secret_key_hex)?;
            let public = PublicKey::from_slice(&hex::decode(&key.public_key_hex)?)?;
            let sighash = tx.sighash_all(idx, &coin.tx_out.script_pubkey)?;
            let sig = sign_hash(&secret, sighash)?;
            tx.inputs[idx].signature_script = encode_sig_script(&public.serialize(), &sig)?;
        }
        Ok(tx)
    }

    pub fn create_library_post_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        title: &str,
        category: &str,
        body: &str,
        fee: Amount,
    ) -> Result<Transaction> {
        let spend_height = chain.height() + 1;
        if !library_active(settings, spend_height) {
            bail!(
                "Library activates at block #{}",
                library_activation_height(settings)
            );
        }
        let author = Address::parse_with_prefix(
            self.default_address().context("wallet empty")?,
            &settings.network.address_prefix,
        )?;
        let body_bytes = body.as_bytes();
        let page_size = settings.library.max_page_bytes.max(1);
        let mut pages: Vec<String> = Vec::new();
        if body_bytes.is_empty() {
            pages.push(String::new());
        } else {
            let mut start = 0usize;
            while start < body_bytes.len() {
                let mut end = (start + page_size).min(body_bytes.len());
                while end > start && std::str::from_utf8(&body_bytes[start..end]).is_err() {
                    end -= 1;
                }
                if end == start {
                    bail!("Library post contains a UTF-8 character larger than page size");
                }
                pages.push(String::from_utf8(body_bytes[start..end].to_vec())?);
                start = end;
            }
        }
        if pages.len() > settings.library.max_pages_per_post {
            bail!(
                "Library post exceeds max {} pages",
                settings.library.max_pages_per_post
            );
        }
        let library_fee = library_post_price_atoms(settings, title, category, body)?;
        let marker_atoms = settings
            .library
            .marker_output_atoms
            .checked_mul(pages.len() as u64)
            .context("Library marker overflow")?;
        let target = marker_atoms
            .checked_add(fee.atoms())
            .context("Library target overflow")?
            .checked_add(library_fee)
            .context("Library target overflow")?;
        let (selected, total) = self.select_signed_inputs_for_target(chain, settings, target)?;
        let mut outputs = Vec::new();
        let page_total = pages.len() as u32;
        for (idx, page) in pages.iter().enumerate() {
            outputs.push(TxOut {
                value: Amount::from_atoms(settings.library.marker_output_atoms)?,
                script_pubkey: library_post_marker_script(
                    &author, title, category, idx as u32, page_total, page, settings,
                )?,
            });
        }
        let change = total - target;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: author.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        self.sign_selected_transaction(tx, &selected)
    }

    pub fn create_library_comment_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        post_id: &str,
        parent_comment_id: Option<&str>,
        body: &str,
        fee: Amount,
    ) -> Result<Transaction> {
        let spend_height = chain.height() + 1;
        if !library_active(settings, spend_height) {
            bail!(
                "Library activates at block #{}",
                library_activation_height(settings)
            );
        }
        let author = Address::parse_with_prefix(
            self.default_address().context("wallet empty")?,
            &settings.network.address_prefix,
        )?;
        let target = settings
            .library
            .marker_output_atoms
            .checked_add(fee.atoms())
            .context("Library comment target overflow")?;
        let (selected, total) = self.select_signed_inputs_for_target(chain, settings, target)?;
        let mut outputs = vec![TxOut {
            value: Amount::from_atoms(settings.library.marker_output_atoms)?,
            script_pubkey: library_comment_marker_script(
                post_id,
                parent_comment_id,
                &author,
                body,
                settings,
            )?,
        }];
        let change = total - target;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: author.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        self.sign_selected_transaction(tx, &selected)
    }

    pub fn create_library_vote_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        target_kind: &str,
        target_id: &str,
        up: bool,
        fee: Amount,
    ) -> Result<Transaction> {
        let spend_height = chain.height() + 1;
        if !library_active(settings, spend_height) {
            bail!(
                "Library activates at block #{}",
                library_activation_height(settings)
            );
        }
        let author = Address::parse_with_prefix(
            self.default_address().context("wallet empty")?,
            &settings.network.address_prefix,
        )?;
        let target = settings
            .library
            .marker_output_atoms
            .checked_add(fee.atoms())
            .context("Library vote target overflow")?;
        let (selected, total) = self.select_signed_inputs_for_target(chain, settings, target)?;
        let mut outputs = vec![TxOut {
            value: Amount::from_atoms(settings.library.marker_output_atoms)?,
            script_pubkey: library_vote_marker_script(target_kind, target_id, &author, up)?,
        }];
        let change = total - target;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: author.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        self.sign_selected_transaction(tx, &selected)
    }

    pub fn create_library_edit_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        target_kind: &str,
        target_id: &str,
        title: &str,
        category: &str,
        body: &str,
        fee: Amount,
    ) -> Result<Transaction> {
        let spend_height = chain.height() + 1;
        if !library_active(settings, spend_height) {
            bail!(
                "Library activates at block #{}",
                library_activation_height(settings)
            );
        }
        let author = Address::parse_with_prefix(
            self.default_address().context("wallet empty")?,
            &settings.network.address_prefix,
        )?;
        let target = settings
            .library
            .marker_output_atoms
            .checked_add(fee.atoms())
            .context("Library edit target overflow")?;
        let (selected, total) = self.select_signed_inputs_for_target(chain, settings, target)?;
        let mut outputs = vec![TxOut {
            value: Amount::from_atoms(settings.library.marker_output_atoms)?,
            script_pubkey: library_edit_marker_script(
                target_kind,
                target_id,
                &author,
                title,
                category,
                body,
                settings,
            )?,
        }];
        let change = total - target;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: author.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        self.sign_selected_transaction(tx, &selected)
    }

    pub fn create_library_delete_transaction(
        &self,
        chain: &ChainState,
        settings: &Settings,
        target_kind: &str,
        target_id: &str,
        fee: Amount,
    ) -> Result<Transaction> {
        let spend_height = chain.height() + 1;
        if !library_active(settings, spend_height) {
            bail!(
                "Library activates at block #{}",
                library_activation_height(settings)
            );
        }
        let author = Address::parse_with_prefix(
            self.default_address().context("wallet empty")?,
            &settings.network.address_prefix,
        )?;
        let target = settings
            .library
            .marker_output_atoms
            .checked_add(fee.atoms())
            .context("Library delete target overflow")?;
        let (selected, total) = self.select_signed_inputs_for_target(chain, settings, target)?;
        let mut outputs = vec![TxOut {
            value: Amount::from_atoms(settings.library.marker_output_atoms)?,
            script_pubkey: library_delete_marker_script(target_kind, target_id, &author)?,
        }];
        let change = total - target;
        if change > 0 {
            outputs.push(TxOut {
                value: Amount::from_atoms(change)?,
                script_pubkey: author.script_pubkey(),
            });
        }
        let tx = Transaction {
            version: 1,
            inputs: selected
                .iter()
                .map(|(outpoint, _, _)| TxIn {
                    previous_output: outpoint.clone(),
                    signature_script: ScriptBuf::empty(),
                    sequence: u32::MAX,
                })
                .collect(),
            outputs,
            locktime: 0,
        };
        self.sign_selected_transaction(tx, &selected)
    }
}
pub fn ensure_plaintext_wallet_allowed(settings: &Settings) -> Result<()> {
    if settings.wallet.plaintext_keys_allowed
        || std::env::var("QUB_ALLOW_PLAINTEXT_WALLET").ok().as_deref() == Some("1")
    {
        Ok(())
    } else {
        bail!("plaintext local wallet keys disabled; set QUB_ALLOW_PLAINTEXT_WALLET=1 only if you accept the v1 risk")
    }
}

pub const FAST_CHAIN_STATUS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastChainStatus {
    pub schema_version: u32,
    pub network: String,
    pub height: u32,
    pub tip_hash: Hash256,
    pub tip_block_version: u32,
    pub tip_block_time: u32,
    pub tip_tx_count: usize,
    pub chain_file_bytes: u64,
    pub chain_file_modified_unix_secs: u64,
    pub chain_file_modified_subsec_nanos: u32,
    pub generated_at_unix: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastChainStatusSource {
    Metadata,
    StreamScan,
}

#[derive(Debug, Clone)]
pub struct NodePaths {
    pub data_dir: PathBuf,
    pub chain_file: PathBuf,
    pub chain_status_file: PathBuf,
    pub wallet_file: PathBuf,
    pub pending_txs_file: PathBuf,
}

impl NodePaths {
    pub fn from_settings(settings: &Settings) -> Self {
        let data_dir = PathBuf::from(&settings.node.data_dir);
        Self {
            chain_file: data_dir.join("chain.json"),
            chain_status_file: data_dir.join("chain-status.json"),
            wallet_file: data_dir.join("wallet.json"),
            pending_txs_file: data_dir.join("wallet-pending-txs.json"),
            data_dir,
        }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.data_dir)?;
        Ok(())
    }
}

fn storage_lock() -> &'static StdMutex<()> {
    static LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| StdMutex::new(()))
}

fn unix_time_parts(time: SystemTime) -> (u64, u32) {
    time.duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs(), d.subsec_nanos()))
        .unwrap_or((0, 0))
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn chain_file_stamp(metadata: &fs::Metadata) -> (u64, u32) {
    metadata.modified().map(unix_time_parts).unwrap_or((0, 0))
}

fn fast_status_from_chain(chain: &ChainState, metadata: &fs::Metadata) -> Result<FastChainStatus> {
    let tip = chain.blocks.last().context("chain has no blocks")?;
    let (modified_secs, modified_nanos) = chain_file_stamp(metadata);
    Ok(FastChainStatus {
        schema_version: FAST_CHAIN_STATUS_SCHEMA_VERSION,
        network: chain.network.clone(),
        height: chain.height(),
        tip_hash: tip.block_hash(),
        tip_block_version: tip.header.version,
        tip_block_time: tip.header.time,
        tip_tx_count: tip.transactions.len(),
        chain_file_bytes: metadata.len(),
        chain_file_modified_unix_secs: modified_secs,
        chain_file_modified_subsec_nanos: modified_nanos,
        generated_at_unix: current_unix_secs(),
    })
}

fn fast_status_matches_file(
    status: &FastChainStatus,
    settings: &Settings,
    metadata: &fs::Metadata,
) -> bool {
    let (modified_secs, modified_nanos) = chain_file_stamp(metadata);
    status.schema_version == FAST_CHAIN_STATUS_SCHEMA_VERSION
        && status.network == settings.network.name
        && status.chain_file_bytes == metadata.len()
        && status.chain_file_modified_unix_secs == modified_secs
        && status.chain_file_modified_subsec_nanos == modified_nanos
}

fn write_fast_chain_status_file(paths: &NodePaths, status: &FastChainStatus) -> Result<()> {
    write_text_replace(
        &paths.chain_status_file,
        &serde_json::to_string_pretty(status)?,
    )
}

fn refresh_fast_chain_status_file(paths: &NodePaths, chain: &ChainState) -> Result<()> {
    let metadata = fs::metadata(&paths.chain_file)
        .with_context(|| format!("metadata {}", paths.chain_file.display()))?;
    let status = fast_status_from_chain(chain, &metadata)?;
    write_fast_chain_status_file(paths, &status)
}

#[derive(Default)]
struct ChainStatusScan {
    network: Option<String>,
    block_count: u64,
    tip_header: Option<BlockHeader>,
    tip_tx_count: usize,
}

struct ChainStatusSeed;

impl<'de> DeserializeSeed<'de> for ChainStatusSeed {
    type Value = ChainStatusScan;

    fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(ChainStatusVisitor)
    }
}

struct ChainStatusVisitor;

impl<'de> Visitor<'de> for ChainStatusVisitor {
    type Value = ChainStatusScan;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a persisted QUB chain object")
    }

    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut scan = ChainStatusScan::default();
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "network" => scan.network = Some(map.next_value::<String>()?),
                "blocks" => map.next_value_seed(BlockSequenceSeed { scan: &mut scan })?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(scan)
    }
}

struct BlockSequenceSeed<'a> {
    scan: &'a mut ChainStatusScan,
}

impl<'de, 'a> DeserializeSeed<'de> for BlockSequenceSeed<'a> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(BlockSequenceVisitor { scan: self.scan })
    }
}

struct BlockSequenceVisitor<'a> {
    scan: &'a mut ChainStatusScan,
}

impl<'de, 'a> Visitor<'de> for BlockSequenceVisitor<'a> {
    type Value = ();

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the persisted blocks array")
    }

    fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while let Some(block) = seq.next_element::<Block>()? {
            self.scan.block_count = self
                .scan
                .block_count
                .checked_add(1)
                .ok_or_else(|| <A::Error as DeError>::custom("block count overflow"))?;
            self.scan.tip_tx_count = block.transactions.len();
            self.scan.tip_header = Some(block.header);
        }
        Ok(())
    }
}

fn stream_scan_fast_chain_status(
    settings: &Settings,
    paths: &NodePaths,
) -> Result<FastChainStatus> {
    let file = File::open(&paths.chain_file)
        .with_context(|| format!("open {}", paths.chain_file.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("metadata {}", paths.chain_file.display()))?;
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(file));
    let scan = ChainStatusSeed
        .deserialize(&mut deserializer)
        .with_context(|| format!("stream-scan {}", paths.chain_file.display()))?;
    deserializer
        .end()
        .with_context(|| format!("finish stream-scan {}", paths.chain_file.display()))?;

    let network = scan.network.context("chain.json missing network")?;
    if network != settings.network.name {
        bail!(
            "network mismatch: chain={}, config={}",
            network,
            settings.network.name
        );
    }
    if scan.block_count == 0 {
        bail!("chain has no blocks");
    }
    let height_u64 = scan.block_count - 1;
    if height_u64 > u32::MAX as u64 {
        bail!("chain height exceeds u32");
    }
    let tip_header = scan.tip_header.context("chain has no tip header")?;
    let (modified_secs, modified_nanos) = chain_file_stamp(&metadata);
    Ok(FastChainStatus {
        schema_version: FAST_CHAIN_STATUS_SCHEMA_VERSION,
        network,
        height: height_u64 as u32,
        tip_hash: tip_header.hash(),
        tip_block_version: tip_header.version,
        tip_block_time: tip_header.time,
        tip_tx_count: scan.tip_tx_count,
        chain_file_bytes: metadata.len(),
        chain_file_modified_unix_secs: modified_secs,
        chain_file_modified_subsec_nanos: modified_nanos,
        generated_at_unix: current_unix_secs(),
    })
}

pub fn load_fast_chain_status(
    settings: &Settings,
) -> Result<(FastChainStatus, FastChainStatusSource)> {
    let paths = NodePaths::from_settings(settings);
    if !paths.chain_file.exists() {
        bail!("chain file not found: {}", paths.chain_file.display());
    }
    let metadata = fs::metadata(&paths.chain_file)
        .with_context(|| format!("metadata {}", paths.chain_file.display()))?;

    if paths.chain_status_file.exists() {
        if let Ok(raw) = fs::read_to_string(&paths.chain_status_file) {
            if let Ok(status) = serde_json::from_str::<FastChainStatus>(&raw) {
                if fast_status_matches_file(&status, settings, &metadata) {
                    return Ok((status, FastChainStatusSource::Metadata));
                }
            }
        }
    }

    let status = stream_scan_fast_chain_status(settings, &paths)?;

    // A concurrent node process may replace chain.json while this process is
    // scanning the old inode. Cache the result only if the path still points to
    // the exact file stamp we scanned; otherwise the next call will rescan.
    if let Ok(current_metadata) = fs::metadata(&paths.chain_file) {
        if fast_status_matches_file(&status, settings, &current_metadata) {
            let _ = write_fast_chain_status_file(&paths, &status);
        }
    }

    Ok((status, FastChainStatusSource::StreamScan))
}

pub fn load_or_init_chain(settings: &Settings) -> Result<ChainState> {
    let _guard = storage_lock().lock().expect("storage mutex poisoned");
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    if paths.chain_file.exists() {
        let raw = fs::read_to_string(&paths.chain_file)?;
        ChainState::from_persisted(serde_json::from_str(&raw)?, settings)
    } else {
        let chain = ChainState::new_with_genesis(settings)?;
        write_chain_file(&paths, &chain)?;
        Ok(chain)
    }
}

/// HF88/v1.6.2: UI-only fast chain loader. It reads persisted chain/UTXO data
/// with cheap structural checks instead of a full replay. Do not use this for
/// mining, sync, tx creation, or consensus decisions.
pub fn load_or_init_chain_for_ui_fast(settings: &Settings) -> Result<ChainState> {
    let _guard = storage_lock().lock().expect("storage mutex poisoned");
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    if paths.chain_file.exists() {
        let raw = fs::read_to_string(&paths.chain_file)?;
        ChainState::from_persisted_unchecked_for_ui(serde_json::from_str(&raw)?, settings)
    } else {
        let chain = ChainState::new_with_genesis(settings)?;
        write_chain_file(&paths, &chain)?;
        Ok(chain)
    }
}

pub fn save_chain(settings: &Settings, chain: &ChainState) -> Result<()> {
    let _guard = storage_lock().lock().expect("storage mutex poisoned");
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    write_chain_file(&paths, chain)
}

fn write_chain_file(paths: &NodePaths, chain: &ChainState) -> Result<()> {
    write_text_replace(
        &paths.chain_file,
        &serde_json::to_string_pretty(&chain.to_persisted())?,
    )?;

    // HF121-r2: chain-status.json is an operational cache only. A failure to
    // refresh it must never turn a successfully persisted consensus state into
    // a node failure; status-fast will rebuild it with a bounded-memory scan.
    if let Err(err) = refresh_fast_chain_status_file(paths, chain) {
        eprintln!(
            "warning: could not refresh {}: {err:#}",
            paths.chain_status_file.display()
        );
    }
    Ok(())
}

pub fn load_or_init_wallet(settings: &Settings) -> Result<WalletFile> {
    let _guard = storage_lock().lock().expect("storage mutex poisoned");
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    if paths.wallet_file.exists() {
        let raw = fs::read_to_string(&paths.wallet_file)?;
        let w: WalletFile = serde_json::from_str(&raw)?;
        w.ensure_network(settings)?;
        Ok(w)
    } else {
        let w = WalletFile::new(&settings.network.name);
        write_wallet_file(&paths, &w)?;
        Ok(w)
    }
}

pub fn save_wallet(settings: &Settings, wallet: &WalletFile) -> Result<()> {
    let _guard = storage_lock().lock().expect("storage mutex poisoned");
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    write_wallet_file(&paths, wallet)
}

fn write_wallet_file(paths: &NodePaths, wallet: &WalletFile) -> Result<()> {
    write_text_replace(&paths.wallet_file, &serde_json::to_string_pretty(wallet)?)
}

fn write_text_replace(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file_name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("qub-state");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));

    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .with_context(|| format!("create temporary state file {}", tmp.display()))?;
        file.write_all(body.as_bytes())?;
        file.flush()?;
        // Wallets, pending outboxes, and metadata are small and worth an
        // explicit durability sync. The canonical chain file can be hundreds
        // of megabytes and is rewritten frequently; forcing a full fsync on
        // every block would harm liveness and recreate the observed I/O stalls.
        if body.len() <= 4 * 1024 * 1024 {
            file.sync_all()?;
        }
        drop(file);

        // Unix rename-over-target is atomic. On platforms where that operation
        // is rejected, move the old file aside first; if the second rename fails,
        // restore the old file instead of truncating it in place.
        match fs::rename(&tmp, path) {
            Ok(()) => {}
            Err(first_err) => {
                if !path.exists() {
                    return Err(anyhow::Error::new(first_err)).with_context(|| {
                        format!("replace {} with {}", path.display(), tmp.display())
                    });
                }

                let backup = path.with_file_name(format!(
                    ".{file_name}.{}.{}.bak",
                    std::process::id(),
                    nonce
                ));
                fs::rename(path, &backup).with_context(|| {
                    format!(
                        "move existing state {} aside after rename failure: {first_err}",
                        path.display()
                    )
                })?;

                if let Err(second_err) = fs::rename(&tmp, path) {
                    let _ = fs::rename(&backup, path);
                    return Err(anyhow::Error::new(second_err))
                        .with_context(|| format!("install replacement state {}", path.display()));
                }
                let _ = fs::remove_file(&backup);
            }
        }

        if let Some(parent) = path.parent() {
            if let Ok(dir) = File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    write_result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTxRecord {
    pub txid: Hash256,
    pub tx: Transaction,
    pub label: String,
    pub created_height: u32,
    pub created_unix: u64,
    pub last_rebroadcast_unix: u64,
    pub confirmations_required: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTxFile {
    pub version: u32,
    pub network: String,
    pub txs: Vec<PendingTxRecord>,
}

impl PendingTxFile {
    pub fn new(network: &str) -> Self {
        Self {
            version: 1,
            network: network.to_string(),
            txs: Vec::new(),
        }
    }
    pub fn ensure_network(&self, settings: &Settings) -> Result<()> {
        if self.network != settings.network.name {
            bail!("pending tx network mismatch");
        }
        Ok(())
    }
}

pub fn load_pending_txs(settings: &Settings) -> Result<PendingTxFile> {
    let _guard = storage_lock().lock().expect("storage mutex poisoned");
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    if !paths.pending_txs_file.exists() {
        return Ok(PendingTxFile::new(&settings.network.name));
    }
    let raw = fs::read_to_string(&paths.pending_txs_file)?;
    let file: PendingTxFile = serde_json::from_str(&raw)?;
    file.ensure_network(settings)?;
    Ok(file)
}

pub fn save_pending_txs(settings: &Settings, pending: &PendingTxFile) -> Result<()> {
    let _guard = storage_lock().lock().expect("storage mutex poisoned");
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    write_text_replace(
        &paths.pending_txs_file,
        &serde_json::to_string_pretty(pending)?,
    )
}

pub fn remember_pending_tx(
    settings: &Settings,
    chain: &ChainState,
    tx: &Transaction,
    label: impl Into<String>,
) -> Result<()> {
    let mut pending =
        load_pending_txs(settings).unwrap_or_else(|_| PendingTxFile::new(&settings.network.name));
    pending.ensure_network(settings)?;
    let txid = tx.txid();
    pending.txs.retain(|rec| rec.txid != txid);
    pending.txs.push(PendingTxRecord {
        txid,
        tx: tx.clone(),
        label: label.into(),
        created_height: chain.height(),
        created_unix: unix_time_u32() as u64,
        last_rebroadcast_unix: 0,
        confirmations_required: HF117_PENDING_TX_CONFIRMATIONS,
    });
    if pending.txs.len() > HF117_PENDING_TX_MAX_RECORDS {
        let excess = pending
            .txs
            .len()
            .saturating_sub(HF117_PENDING_TX_MAX_RECORDS);
        pending.txs.drain(0..excess);
    }
    save_pending_txs(settings, &pending)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PendingTxReconcileReport {
    pub retained: usize,
    pub confirmed: usize,
    pub reaccepted: usize,
    pub dropped: usize,
}

pub fn reconcile_pending_txs(
    settings: &Settings,
    chain: &mut ChainState,
) -> Result<PendingTxReconcileReport> {
    let mut pending =
        load_pending_txs(settings).unwrap_or_else(|_| PendingTxFile::new(&settings.network.name));
    pending.ensure_network(settings)?;
    let now = unix_time_u32() as u64;
    let mut report = PendingTxReconcileReport::default();
    let mut next = Vec::<PendingTxRecord>::new();
    let mut seen = HashSet::<Hash256>::new();
    for mut rec in pending.txs.into_iter() {
        if !seen.insert(rec.txid) {
            continue;
        }
        if let Some((_height, confirmations)) = chain.tx_confirmations(rec.txid) {
            if confirmations >= rec.confirmations_required.max(1) {
                report.confirmed = report.confirmed.saturating_add(1);
                continue;
            }
            report.retained = report.retained.saturating_add(1);
            next.push(rec);
            continue;
        }
        if chain.tx_in_mempool(rec.txid) {
            report.retained = report.retained.saturating_add(1);
            next.push(rec);
            continue;
        }
        if now.saturating_sub(rec.created_unix) > HF117_PENDING_TX_MAX_AGE_SECS {
            report.dropped = report.dropped.saturating_add(1);
            continue;
        }
        match chain.accept_transaction_to_mempool(rec.tx.clone(), settings) {
            Ok(_) => {
                rec.last_rebroadcast_unix = now;
                report.reaccepted = report.reaccepted.saturating_add(1);
                next.push(rec);
            }
            Err(_) => {
                // Keep still-recent records in the outbox. A stale local view can
                // temporarily reject the tx; it should not be forgotten until it
                // is confirmed, reaccepted after catch-up, or ages out.
                report.retained = report.retained.saturating_add(1);
                next.push(rec);
            }
        }
    }
    pending = PendingTxFile {
        version: 1,
        network: settings.network.name.clone(),
        txs: next,
    };
    save_pending_txs(settings, &pending)?;
    Ok(report)
}

pub fn pending_tx_raw(settings: &Settings, txid: Hash256) -> Result<Option<Transaction>> {
    let pending = load_pending_txs(settings)?;
    Ok(pending
        .txs
        .into_iter()
        .find(|rec| rec.txid == txid)
        .map(|rec| rec.tx))
}

const JIN_SCRIPT_PREFIX: &[u8] = b"JIN1|";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JinTransfer {
    pub from: String,
    pub to: String,
    pub amount_units: u128,
    pub fee_units: u128,
    pub fee_asset: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JinConversion {
    pub from: String,
    pub matrix_address: String,
    pub amount_units: u128,
    pub fee_units: u128,
    pub fee_asset: String,
}

pub fn parse_jin_units_raw(input: &str) -> Result<u128> {
    let s = input.trim();
    if s.is_empty() || s.starts_with('-') {
        bail!("invalid JIN units");
    }
    Ok(s.parse::<u128>()?)
}

pub fn parse_jin_amount(input: &str) -> Result<u128> {
    let input = input.trim();
    if input.is_empty() || input.starts_with('-') {
        bail!("invalid JIN amount");
    }
    let mut parts = input.split('.');
    let whole = parts.next().unwrap_or("0").parse::<u128>()?;
    let frac = parts.next();
    if parts.next().is_some() {
        bail!("invalid JIN amount");
    }
    let mut units = whole
        .checked_mul(JIN_UNITS_PER_COIN)
        .context("JIN amount overflow")?;
    if let Some(frac) = frac {
        if frac.len() > JIN_DECIMALS as usize || !frac.chars().all(|c| c.is_ascii_digit()) {
            bail!("invalid JIN fractional amount");
        }
        let mut padded = frac.to_string();
        while padded.len() < JIN_DECIMALS as usize {
            padded.push('0');
        }
        units = units
            .checked_add(padded.parse::<u128>()?)
            .context("JIN amount overflow")?;
    }
    if units > JIN_TOTAL_SUPPLY_UNITS {
        bail!("JIN amount exceeds total supply");
    }
    Ok(units)
}

pub fn format_jin_amount(units: u128) -> String {
    let whole = units / JIN_UNITS_PER_COIN;
    let frac = units % JIN_UNITS_PER_COIN;
    if frac == 0 {
        return whole.to_string();
    }
    let mut s = format!("{:018}", frac);
    while s.ends_with('0') {
        s.pop();
    }
    format!("{whole}.{s}")
}

pub fn block_jin_fee_units(settings: &Settings, block: &Block) -> u128 {
    block
        .transactions
        .iter()
        .skip(1)
        .map(|tx| {
            let transfer_fees = jin_transfers_in_tx(tx, settings)
                .into_iter()
                .filter(|(_, t, _)| t.fee_asset == "JIN")
                .map(|(_, t, _)| t.fee_units)
                .fold(0u128, |a, b| a.saturating_add(b));
            let conversion_fees = jin_conversions_in_tx(tx, settings)
                .into_iter()
                .filter(|(_, t, _)| t.fee_asset == "JIN")
                .map(|(_, t, _)| t.fee_units)
                .fold(0u128, |a, b| a.saturating_add(b));
            transfer_fees.saturating_add(conversion_fees)
        })
        .fold(0u128, |a, b| a.saturating_add(b))
}

pub fn jin_active(settings: &Settings, height: u32) -> bool {
    settings.jin.enabled
        && settings.features.jin_native_coin_enabled
        && height >= settings.jin.activation_height
}

pub fn jin_marker_script_transfer(
    settings: &Settings,
    from: &Address,
    to: &Address,
    amount_units: u128,
    fee_units: u128,
    fee_asset: &str,
) -> Result<ScriptBuf> {
    if from.prefix != settings.network.address_prefix
        || to.prefix != settings.network.address_prefix
    {
        bail!("JIN transfer address network mismatch");
    }
    if amount_units == 0 {
        bail!("JIN amount must be non-zero");
    }
    if amount_units > JIN_TOTAL_SUPPLY_UNITS {
        bail!("JIN amount exceeds supply");
    }
    let fee_asset = fee_asset.trim().to_ascii_uppercase();
    if fee_asset != "QUB" && fee_asset != "JIN" {
        bail!("JIN fee asset must be QUB or JIN");
    }
    if fee_asset == "JIN" && !settings.jin.allow_fee_in_jin {
        bail!("JIN fee payment is disabled on this network");
    }
    Ok(ScriptBuf(
        format!(
            "JIN1|transfer|{}|{}|{}|{}|{}",
            from, to, amount_units, fee_units, fee_asset
        )
        .into_bytes(),
    ))
}

pub fn jin_conversion_active(settings: &Settings, height: u32) -> bool {
    if !(settings.jin.enabled
        && settings.features.jin_native_coin_enabled
        && height >= settings.jin.conversion_activation_height)
    {
        return false;
    }
    // HF74/v1.5.7: disable future JIN Coin -> Token conversion until the Enjin bridge is live.
    // Historical conversions before this safety cutoff remain valid if any existed.
    if settings.network.name == "mainnet" && height >= MAINNET_JIN_CONVERSION_DISABLE_HEIGHT {
        return false;
    }
    true
}

fn validate_matrix_address_like(input: &str) -> Result<String> {
    let s = input.trim();
    if s.len() < 16 || s.len() > 128 {
        bail!("invalid Matrixchain address length");
    }
    if !s.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("Matrixchain address may contain only latin letters and digits");
    }
    Ok(s.to_string())
}

pub fn jin_marker_script_conversion(
    settings: &Settings,
    from: &Address,
    matrix_address: &str,
    amount_units: u128,
    fee_units: u128,
    fee_asset: &str,
) -> Result<ScriptBuf> {
    if from.prefix != settings.network.address_prefix {
        bail!("JIN conversion source address network mismatch");
    }
    if amount_units == 0 {
        bail!("JIN conversion amount must be non-zero");
    }
    if amount_units > JIN_TOTAL_SUPPLY_UNITS {
        bail!("JIN conversion amount exceeds supply");
    }
    if amount_units % JIN_UNITS_PER_COIN != 0 {
        bail!("JIN Token conversion requires whole JIN because JIN Token has no decimals");
    }
    let matrix_address = validate_matrix_address_like(matrix_address)?;
    let fee_asset = fee_asset.trim().to_ascii_uppercase();
    if fee_asset != "QUB" && fee_asset != "JIN" {
        bail!("JIN conversion fee asset must be QUB or JIN");
    }
    if fee_asset == "JIN" && !settings.jin.allow_fee_in_jin {
        bail!("JIN fee payment is disabled on this network");
    }
    Ok(ScriptBuf(
        format!(
            "JIN1|convert-token|{}|{}|{}|{}|{}",
            from, matrix_address, amount_units, fee_units, fee_asset
        )
        .into_bytes(),
    ))
}

pub fn parse_jin_marker_script(script: &ScriptBuf, settings: &Settings) -> Option<JinTransfer> {
    let b = script.as_bytes();
    if !b.starts_with(JIN_SCRIPT_PREFIX) {
        return None;
    }
    let s = std::str::from_utf8(b).ok()?;
    let mut parts = s.split('|');
    if parts.next()? != "JIN1" {
        return None;
    }
    if parts.next()? != "transfer" {
        return None;
    }
    let from = parts.next()?.trim().to_string();
    let to = parts.next()?.trim().to_string();
    Address::parse_with_prefix(&from, &settings.network.address_prefix).ok()?;
    Address::parse_with_prefix(&to, &settings.network.address_prefix).ok()?;
    let amount_units = parts.next()?.parse::<u128>().ok()?;
    let fee_units = parts.next()?.parse::<u128>().ok()?;
    let fee_asset = parts.next()?.trim().to_ascii_uppercase();
    if parts.next().is_some() {
        return None;
    }
    if fee_asset != "QUB" && fee_asset != "JIN" {
        return None;
    }
    Some(JinTransfer {
        from,
        to,
        amount_units,
        fee_units,
        fee_asset,
    })
}

pub fn parse_jin_conversion_script(
    script: &ScriptBuf,
    settings: &Settings,
) -> Option<JinConversion> {
    let b = script.as_bytes();
    if !b.starts_with(JIN_SCRIPT_PREFIX) {
        return None;
    }
    let s = std::str::from_utf8(b).ok()?;
    let mut parts = s.split('|');
    if parts.next()? != "JIN1" {
        return None;
    }
    if parts.next()? != "convert-token" {
        return None;
    }
    let from = parts.next()?.trim().to_string();
    let matrix_address = parts.next()?.trim().to_string();
    Address::parse_with_prefix(&from, &settings.network.address_prefix).ok()?;
    validate_matrix_address_like(&matrix_address).ok()?;
    let amount_units = parts.next()?.parse::<u128>().ok()?;
    let fee_units = parts.next()?.parse::<u128>().ok()?;
    let fee_asset = parts.next()?.trim().to_ascii_uppercase();
    if parts.next().is_some() {
        return None;
    }
    if fee_asset != "QUB" && fee_asset != "JIN" {
        return None;
    }
    Some(JinConversion {
        from,
        matrix_address,
        amount_units,
        fee_units,
        fee_asset,
    })
}

pub fn jin_transfers_in_tx(
    tx: &Transaction,
    settings: &Settings,
) -> Vec<(usize, JinTransfer, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_jin_marker_script(&out.script_pubkey, settings)
                .map(|j| (idx, j, out.value.atoms()))
        })
        .collect()
}

pub fn jin_conversions_in_tx(
    tx: &Transaction,
    settings: &Settings,
) -> Vec<(usize, JinConversion, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_jin_conversion_script(&out.script_pubkey, settings)
                .map(|j| (idx, j, out.value.atoms()))
        })
        .collect()
}

const QUB_JIN_INFUSION_SCRIPT_PREFIX: &[u8] = b"QUBJIN1|";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QubJinInfusionMarker {
    InfuseJin {
        from: String,
        amount_units: u128,
    },
    MeltQub {
        from: String,
        qub_atoms: u64,
        min_jin_units: u128,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QubJinInfusionState {
    pub active: bool,
    pub activation_height: u32,
    pub melted_qub_atoms: u64,
    pub true_max_qub_atoms: u64,
    pub lifetime_infused_jin_units: u128,
    pub active_infused_jin_units: u128,
    pub units_per_qub_atom: u128,
    pub bootstrap_units: u128,
}

impl QubJinInfusionState {
    fn inactive(settings: &Settings) -> Self {
        Self {
            active: false,
            activation_height: qub_jin_infusion_activation_height(settings),
            melted_qub_atoms: 0,
            true_max_qub_atoms: MAX_MONEY_ATOMS,
            lifetime_infused_jin_units: 0,
            active_infused_jin_units: 0,
            units_per_qub_atom: 0,
            bootstrap_units: qub_jin_bootstrap_units(settings),
        }
    }
}

pub fn qub_jin_infusion_activation_height(settings: &Settings) -> u32 {
    if settings.network.name == "mainnet" {
        MAINNET_QUB_JIN_INFUSION_ACTIVATION_HEIGHT
    } else {
        u32::MAX
    }
}

pub fn qub_jin_sale_reserve_lock_height(settings: &Settings) -> u32 {
    if settings.network.name == "mainnet" {
        MAINNET_QUB_JIN_SALE_RESERVE_LOCK_HEIGHT
    } else {
        u32::MAX
    }
}

pub fn qub_jin_bootstrap_units(settings: &Settings) -> u128 {
    if settings.network.name == "mainnet" {
        QUB_JIN_BOOTSTRAP_INFUSION_UNITS
    } else {
        0
    }
}

pub fn qub_jin_infusion_active(settings: &Settings, height: u32) -> bool {
    settings.jin.enabled
        && settings.features.jin_native_coin_enabled
        && settings.network.name == "mainnet"
        && height >= qub_jin_infusion_activation_height(settings)
}

pub fn qub_jin_bootstrap_units_per_atom(settings: &Settings) -> Result<u128> {
    let units = qub_jin_bootstrap_units(settings);
    if units == 0 {
        return Ok(0);
    }
    let denom = MAX_MONEY_ATOMS as u128;
    if units % denom != 0 {
        bail!("HF116 bootstrap JIN infusion is not exact per QUB atom");
    }
    Ok(units / denom)
}

pub fn qub_jin_infuse_marker_script(
    settings: &Settings,
    from: &Address,
    amount_units: u128,
) -> Result<ScriptBuf> {
    if from.prefix != settings.network.address_prefix {
        bail!("JIN->QUB infusion source address network mismatch");
    }
    if amount_units == 0 {
        bail!("JIN->QUB infusion amount must be non-zero");
    }
    if amount_units > JIN_TOTAL_SUPPLY_UNITS {
        bail!("JIN->QUB infusion amount exceeds JIN supply");
    }
    Ok(ScriptBuf(
        format!("QUBJIN1|infuse-jin|v1|{}|{}", from, amount_units).into_bytes(),
    ))
}

pub fn qub_jin_melt_marker_script(
    settings: &Settings,
    from: &Address,
    qub_atoms: u64,
    min_jin_units: u128,
) -> Result<ScriptBuf> {
    if from.prefix != settings.network.address_prefix {
        bail!("QUB melt source address network mismatch");
    }
    if qub_atoms == 0 {
        bail!("QUB melt amount must be non-zero");
    }
    if qub_atoms > MAX_MONEY_ATOMS {
        bail!("QUB melt amount exceeds QUB max supply");
    }
    Ok(ScriptBuf(
        format!(
            "QUBJIN1|melt-qub|v1|{}|{}|{}",
            from, qub_atoms, min_jin_units
        )
        .into_bytes(),
    ))
}

pub fn parse_qub_jin_infusion_script(
    script: &ScriptBuf,
    settings: &Settings,
) -> Option<QubJinInfusionMarker> {
    let raw = std::str::from_utf8(script.as_bytes()).ok()?;
    if !raw.as_bytes().starts_with(QUB_JIN_INFUSION_SCRIPT_PREFIX) {
        return None;
    }
    let mut parts = raw.split('|');
    if parts.next()? != "QUBJIN1" {
        return None;
    }
    let op = parts.next()?;
    if parts.next()? != "v1" {
        return None;
    }
    match op {
        "infuse-jin" => {
            let from = Address::parse_with_prefix(parts.next()?, &settings.network.address_prefix)
                .ok()?
                .to_string();
            let amount_units = parts.next()?.parse::<u128>().ok()?;
            if parts.next().is_some() {
                return None;
            }
            Some(QubJinInfusionMarker::InfuseJin { from, amount_units })
        }
        "melt-qub" => {
            let from = Address::parse_with_prefix(parts.next()?, &settings.network.address_prefix)
                .ok()?
                .to_string();
            let qub_atoms = parts.next()?.parse::<u64>().ok()?;
            let min_jin_units = parts.next()?.parse::<u128>().ok()?;
            if parts.next().is_some() {
                return None;
            }
            Some(QubJinInfusionMarker::MeltQub {
                from,
                qub_atoms,
                min_jin_units,
            })
        }
        _ => None,
    }
}

pub fn qub_jin_infusion_markers_in_tx(
    tx: &Transaction,
    settings: &Settings,
) -> Vec<(usize, QubJinInfusionMarker, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_qub_jin_infusion_script(&out.script_pubkey, settings)
                .map(|m| (idx, m, out.value.atoms()))
        })
        .collect()
}

pub fn is_qub_jin_infusion_transaction(tx: &Transaction, settings: &Settings) -> bool {
    !qub_jin_infusion_markers_in_tx(tx, settings).is_empty()
}

fn qub_jin_apply_bootstrap(
    settings: &Settings,
    ledger: &mut HashMap<String, u128>,
    state: &mut QubJinInfusionState,
) -> Result<()> {
    if state.active {
        return Ok(());
    }
    let bootstrap = qub_jin_bootstrap_units(settings);
    state.active = true;
    if bootstrap == 0 {
        return Ok(());
    }
    let per_atom = qub_jin_bootstrap_units_per_atom(settings)?;
    jin_debit(ledger, &settings.jin.protocol_address, bootstrap)?;
    jin_credit(ledger, QUB_JIN_INFUSION_VAULT, bootstrap)?;
    state.lifetime_infused_jin_units = state
        .lifetime_infused_jin_units
        .checked_add(bootstrap)
        .context("HF116 bootstrap lifetime overflow")?;
    state.active_infused_jin_units = state
        .active_infused_jin_units
        .checked_add(bootstrap)
        .context("HF116 bootstrap active overflow")?;
    state.units_per_qub_atom = state
        .units_per_qub_atom
        .checked_add(per_atom)
        .context("HF116 bootstrap per-atom overflow")?;
    Ok(())
}

fn qub_jin_apply_bootstrap_for_context(
    settings: &Settings,
    ledger: &mut HashMap<String, u128>,
    state: &mut QubJinInfusionState,
    prior_visible_height: u32,
    spend_height: u32,
) -> Result<()> {
    let activation = qub_jin_infusion_activation_height(settings);
    if qub_jin_infusion_active(settings, spend_height) && prior_visible_height < activation {
        qub_jin_apply_bootstrap(settings, ledger, state)?;
    }
    Ok(())
}

fn qub_jin_apply_infuse_units(state: &mut QubJinInfusionState, amount_units: u128) -> Result<u128> {
    if amount_units == 0 {
        bail!("JIN->QUB infusion amount must be non-zero");
    }
    let denom = state.true_max_qub_atoms as u128;
    if denom == 0 {
        bail!("cannot infuse JIN after all QUB has been melted");
    }
    if amount_units % denom != 0 {
        bail!(
            "JIN->QUB infusion amount must divide exactly across the current true max QUB atom supply; minimum/current step is {} JIN units ({})",
            denom,
            format_jin_amount(denom)
        );
    }
    let delta_per_atom = amount_units / denom;
    if delta_per_atom == 0 {
        bail!("JIN->QUB infusion amount is below one unit per current true QUB atom");
    }
    state.units_per_qub_atom = state
        .units_per_qub_atom
        .checked_add(delta_per_atom)
        .context("JIN per-QUB-atom infusion overflow")?;
    state.lifetime_infused_jin_units = state
        .lifetime_infused_jin_units
        .checked_add(amount_units)
        .context("JIN lifetime infusion overflow")?;
    state.active_infused_jin_units = state
        .active_infused_jin_units
        .checked_add(amount_units)
        .context("JIN active infusion overflow")?;
    Ok(delta_per_atom)
}

fn qub_jin_apply_melt_atoms(
    state: &mut QubJinInfusionState,
    qub_atoms: u64,
    min_jin_units: u128,
) -> Result<u128> {
    if qub_atoms == 0 {
        bail!("QUB melt amount must be non-zero");
    }
    if qub_atoms > state.true_max_qub_atoms {
        bail!("QUB melt amount exceeds current true max QUB supply");
    }
    if state.units_per_qub_atom == 0 {
        bail!("QUB melt is unavailable because no JIN is infused into QUB");
    }
    let payout_units = (qub_atoms as u128)
        .checked_mul(state.units_per_qub_atom)
        .context("QUB melt payout overflow")?;
    if payout_units == 0 {
        bail!("QUB melt payout would be zero");
    }
    if payout_units < min_jin_units {
        bail!("QUB melt slippage check failed: payout below minimum JIN units");
    }
    if state.active_infused_jin_units < payout_units {
        bail!("QUB melt exceeds active infused JIN reserve");
    }
    state.melted_qub_atoms = state
        .melted_qub_atoms
        .checked_add(qub_atoms)
        .context("melted QUB overflow")?;
    state.true_max_qub_atoms = state
        .true_max_qub_atoms
        .checked_sub(qub_atoms)
        .context("true max QUB underflow")?;
    state.active_infused_jin_units = state
        .active_infused_jin_units
        .checked_sub(payout_units)
        .context("active infused JIN underflow")?;
    Ok(payout_units)
}

fn qub_jin_apply_marker_to_state_only(
    state: &mut QubJinInfusionState,
    marker: &QubJinInfusionMarker,
) -> Result<()> {
    match marker {
        QubJinInfusionMarker::InfuseJin { amount_units, .. } => {
            qub_jin_apply_infuse_units(state, *amount_units)?;
        }
        QubJinInfusionMarker::MeltQub {
            qub_atoms,
            min_jin_units,
            ..
        } => {
            let _ = qub_jin_apply_melt_atoms(state, *qub_atoms, *min_jin_units)?;
        }
    }
    Ok(())
}

fn qub_jin_apply_marker_unchecked(
    settings: &Settings,
    ledger: &mut HashMap<String, u128>,
    state: &mut QubJinInfusionState,
    marker: &QubJinInfusionMarker,
) -> Result<()> {
    match marker {
        QubJinInfusionMarker::InfuseJin { from, amount_units } => {
            qub_jin_apply_infuse_units(state, *amount_units)?;
            jin_debit(ledger, from, *amount_units)?;
            jin_credit(ledger, QUB_JIN_INFUSION_VAULT, *amount_units)?;
        }
        QubJinInfusionMarker::MeltQub {
            from,
            qub_atoms,
            min_jin_units,
        } => {
            let payout_units = qub_jin_apply_melt_atoms(state, *qub_atoms, *min_jin_units)?;
            jin_debit(ledger, QUB_JIN_INFUSION_VAULT, payout_units)?;
            jin_credit(ledger, from, payout_units)?;
        }
    }
    if settings.network.name == "mainnet"
        && state.active_infused_jin_units > state.lifetime_infused_jin_units
    {
        bail!("HF116 infusion accounting invariant failed");
    }
    Ok(())
}

pub fn qub_jin_infusion_state_from_blocks(
    settings: &Settings,
    blocks: &[Block],
) -> Result<QubJinInfusionState> {
    let mut state = QubJinInfusionState::inactive(settings);
    for (height, block) in blocks.iter().enumerate().skip(1) {
        let height = height as u32;
        if height == qub_jin_infusion_activation_height(settings) {
            state.active = true;
            let bootstrap = qub_jin_bootstrap_units(settings);
            let per_atom = qub_jin_bootstrap_units_per_atom(settings)?;
            state.lifetime_infused_jin_units = state
                .lifetime_infused_jin_units
                .checked_add(bootstrap)
                .context("HF116 bootstrap lifetime overflow")?;
            state.active_infused_jin_units = state
                .active_infused_jin_units
                .checked_add(bootstrap)
                .context("HF116 bootstrap active overflow")?;
            state.units_per_qub_atom = state
                .units_per_qub_atom
                .checked_add(per_atom)
                .context("HF116 bootstrap per-atom overflow")?;
        }
        if !qub_jin_infusion_active(settings, height) {
            continue;
        }
        for tx in block.transactions.iter().skip(1) {
            for (_, marker, marker_atoms) in qub_jin_infusion_markers_in_tx(tx, settings) {
                if marker_atoms != settings.jin.marker_output_atoms {
                    bail!(
                        "QUB/JIN infusion marker output must be exactly {} atom(s)",
                        settings.jin.marker_output_atoms
                    );
                }
                qub_jin_apply_marker_to_state_only(&mut state, &marker)?;
            }
        }
    }
    Ok(state)
}

pub fn qub_jin_infusion_state(
    settings: &Settings,
    chain: &ChainState,
) -> Result<QubJinInfusionState> {
    qub_jin_infusion_state_from_blocks(settings, &chain.blocks)
}

pub fn qub_jin_melt_payout_units_for_atoms(
    settings: &Settings,
    chain: &ChainState,
    qub_atoms: u64,
) -> Result<u128> {
    let state = qub_jin_infusion_state(settings, chain)?;
    if qub_atoms == 0 {
        bail!("QUB melt amount must be non-zero");
    }
    (qub_atoms as u128)
        .checked_mul(state.units_per_qub_atom)
        .context("QUB melt payout overflow")
}

pub fn qub_jin_infusion_minimum_step_units(
    settings: &Settings,
    chain: &ChainState,
) -> Result<u128> {
    Ok(qub_jin_infusion_state(settings, chain)?.true_max_qub_atoms as u128)
}

fn qub_jin_melt_burn_atoms_for_fee(
    settings: &Settings,
    tx: &Transaction,
    height: u32,
) -> Result<u64> {
    let markers = qub_jin_infusion_markers_in_tx(tx, settings);
    if markers.is_empty() {
        return Ok(0);
    }
    if !qub_jin_infusion_active(settings, height) {
        bail!(
            "QUB/JIN infusion activates at block #{}",
            qub_jin_infusion_activation_height(settings)
        );
    }
    if markers.len() != 1 {
        bail!("a transaction may contain exactly one QUB/JIN infusion marker");
    }
    match &markers[0].1 {
        QubJinInfusionMarker::MeltQub { qub_atoms, .. } => Ok(*qub_atoms),
        QubJinInfusionMarker::InfuseJin { .. } => Ok(0),
    }
}

fn validate_qub_jin_infusion_transaction_with_state(
    tx: &Transaction,
    ledger: &mut HashMap<String, u128>,
    state: &mut QubJinInfusionState,
    base_utxos: &HashMap<OutPoint, CoinRecord>,
    height: u32,
    settings: &Settings,
) -> Result<()> {
    let markers = qub_jin_infusion_markers_in_tx(tx, settings);
    if markers.is_empty() {
        return Ok(());
    }
    if !qub_jin_infusion_active(settings, height) {
        bail!(
            "QUB/JIN infusion activates at block #{}",
            qub_jin_infusion_activation_height(settings)
        );
    }
    if tx.is_coinbase() {
        bail!("coinbase cannot contain QUB/JIN infusion actions");
    }
    if markers.len() != 1 {
        bail!("a transaction may contain exactly one QUB/JIN infusion marker");
    }
    if !jin_transfers_in_tx(tx, settings).is_empty()
        || !jin_conversions_in_tx(tx, settings).is_empty()
        || !jin_sale_purchases_in_tx(tx, settings).is_empty()
    {
        bail!("QUB/JIN infusion actions must be standalone JIN-accounting transactions");
    }
    let (_idx, marker, marker_atoms) = &markers[0];
    if *marker_atoms != settings.jin.marker_output_atoms {
        bail!(
            "QUB/JIN infusion marker output must be exactly {} atom(s)",
            settings.jin.marker_output_atoms
        );
    }
    let from_text = match marker {
        QubJinInfusionMarker::InfuseJin { from, .. } => from,
        QubJinInfusionMarker::MeltQub { from, .. } => from,
    };
    let from = Address::parse_with_prefix(from_text, &settings.network.address_prefix)?;
    if !input_authorizes_address(tx, base_utxos, &from) {
        bail!("QUB/JIN infusion action is not authorized by a QUB input from the source address");
    }
    if let QubJinInfusionMarker::MeltQub { qub_atoms, .. } = marker {
        let raw_fee_plus_burn = validate_tx_contextual(tx, base_utxos, height, settings, true)?;
        if raw_fee_plus_burn < *qub_atoms {
            bail!(
                "QUB melt transaction must burn the declared QUB amount in addition to miner fee"
            );
        }
    }
    qub_jin_apply_marker_unchecked(settings, ledger, state, marker)
}

fn validate_qub_jin_infusion_transaction_against_chain(
    tx: &Transaction,
    chain: &ChainState,
    spend_height: u32,
    settings: &Settings,
) -> Result<()> {
    let mut ledger = jin_ledger_from_blocks(settings, &chain.blocks)?;
    let mut state = qub_jin_infusion_state_from_blocks(settings, &chain.blocks)?;
    qub_jin_apply_bootstrap_for_context(
        settings,
        &mut ledger,
        &mut state,
        chain.height(),
        spend_height,
    )?;
    let mut scratch = chain.utxos.clone();
    let mut pending = chain.mempool.iter().collect::<Vec<_>>();
    pending.sort_by_cached_key(|tx| {
        (
            mempool_template_priority(settings, *tx),
            tx.txid().to_string(),
        )
    });
    for mem in pending {
        if mem.txid() == tx.txid() {
            continue;
        }
        if validate_jin_transaction_with_ledger(
            mem,
            &mut ledger,
            &scratch,
            spend_height,
            settings,
            None,
        )
        .is_ok()
        {
            let _ = validate_qub_jin_infusion_transaction_with_state(
                mem,
                &mut ledger,
                &mut state,
                &scratch,
                spend_height,
                settings,
            );
            let _ = connect_tx_utxos(mem, &mut scratch, spend_height, false);
        }
    }
    validate_qub_jin_infusion_transaction_with_state(
        tx,
        &mut ledger,
        &mut state,
        &scratch,
        spend_height,
        settings,
    )
}

const JIN_SWAP_SCRIPT_PREFIX: &[u8] = b"JINSWAP1|";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JinSaleListing {
    pub listing_id: u32,
    pub price_atoms_per_jin: u64,
    pub total_units: u128,
    pub sold_units: u128,
    pub remaining_units: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JinSalePurchase {
    pub buyer: String,
    pub listing_id: u32,
    pub amount_units: u128,
    pub price_atoms: u64,
    pub protocol_fee_atoms: u64,
}

fn ceil_mul_div_u64(value: u64, mul: u64, div: u64) -> Result<u64> {
    if div == 0 {
        bail!("division by zero");
    }
    let v = (value as u128)
        .checked_mul(mul as u128)
        .context("fee overflow")?;
    Ok(((v + div as u128 - 1) / div as u128)
        .try_into()
        .context("fee overflow")?)
}

fn jin_swap_enabled(settings: &Settings) -> bool {
    settings.jin_swap.enabled && settings.jin.enabled && settings.features.jin_native_coin_enabled
}
pub fn jin_swap_activation_height(settings: &Settings) -> u32 {
    settings.jin_swap.activation_height
}
pub fn jin_swap_active(settings: &Settings, height: u32) -> bool {
    jin_swap_enabled(settings) && height >= settings.jin_swap.activation_height
}

pub fn jin_swap_sale_total_units(settings: &Settings) -> Result<u128> {
    parse_jin_units_raw(&settings.jin_swap.sale_total_units)
}
pub fn jin_swap_sale_batch_units(settings: &Settings) -> Result<u128> {
    parse_jin_units_raw(&settings.jin_swap.sale_batch_units)
}

pub fn jin_swap_sale_batch_count(settings: &Settings) -> Result<u32> {
    let total = jin_swap_sale_total_units(settings)?;
    let batch = jin_swap_sale_batch_units(settings)?;
    if batch == 0 {
        bail!("JIN sale batch size is zero");
    }
    Ok(((total + batch - 1) / batch)
        .try_into()
        .context("too many JIN sale batches")?)
}

pub fn jin_swap_sale_listing_price_atoms_per_jin(
    settings: &Settings,
    listing_id: u32,
) -> Result<u64> {
    let batches = jin_swap_sale_batch_count(settings)?;
    if listing_id >= batches {
        bail!("invalid JIN sale listing id");
    }
    settings
        .jin_swap
        .sale_start_price_atoms_per_jin
        .checked_add(
            settings
                .jin_swap
                .sale_step_price_atoms_per_jin
                .checked_mul(listing_id as u64)
                .context("JIN sale price overflow")?,
        )
        .context("JIN sale price overflow")
}

pub fn jin_swap_sale_listing_total_units(settings: &Settings, listing_id: u32) -> Result<u128> {
    let total = jin_swap_sale_total_units(settings)?;
    let batch = jin_swap_sale_batch_units(settings)?;
    let start = batch
        .checked_mul(listing_id as u128)
        .context("JIN sale batch overflow")?;
    if start >= total {
        bail!("invalid JIN sale listing id");
    }
    Ok((total - start).min(batch))
}

pub fn jin_swap_sale_listing_total_units_at_height(
    settings: &Settings,
    listing_id: u32,
    height: u32,
) -> Result<u128> {
    let batches = jin_swap_sale_batch_count(settings)?;
    if listing_id >= batches {
        bail!("invalid JIN sale listing id");
    }
    if settings.network.name == "mainnet" && height >= qub_jin_sale_reserve_lock_height(settings) {
        let reserved_start = batches.saturating_sub(QUB_JIN_RESERVED_SALE_BATCHES);
        if listing_id >= reserved_start {
            return Ok(0);
        }
    }
    jin_swap_sale_listing_total_units(settings, listing_id)
}

pub fn jin_swap_sale_price_atoms(
    settings: &Settings,
    listing_id: u32,
    amount_units: u128,
) -> Result<u64> {
    if amount_units == 0 {
        bail!("JIN sale amount must be non-zero");
    }
    if amount_units % JIN_UNITS_PER_COIN != 0 {
        bail!("JIN public sale uses whole JIN units only");
    }
    let whole_jin = amount_units / JIN_UNITS_PER_COIN;
    let price_per_jin = jin_swap_sale_listing_price_atoms_per_jin(settings, listing_id)? as u128;
    let atoms = price_per_jin
        .checked_mul(whole_jin)
        .context("JIN sale price overflow")?;
    if atoms == 0 || atoms > u64::MAX as u128 {
        bail!("JIN sale price out of range");
    }
    Ok(atoms as u64)
}

pub fn jin_swap_fee_split_atoms(settings: &Settings, price_atoms: u64) -> Result<(u64, u64)> {
    // 0.1% total fee, split 50/50: 0.05% protocol, 0.05% miner.
    let total_bps = settings.jin_swap.protocol_fee_bps;
    if total_bps != 10 {
        bail!("JIN swap protocol fee must be 10 bps");
    }
    let protocol = ceil_mul_div_u64(price_atoms, total_bps / 2, 10_000)?;
    let miner = ceil_mul_div_u64(price_atoms, total_bps - (total_bps / 2), 10_000)?;
    Ok((protocol, miner))
}

pub fn jin_sale_purchase_marker_script(
    settings: &Settings,
    buyer: &Address,
    listing_id: u32,
    amount_units: u128,
) -> Result<ScriptBuf> {
    if buyer.prefix != settings.network.address_prefix {
        bail!("JIN sale buyer network mismatch");
    }
    let price_atoms = jin_swap_sale_price_atoms(settings, listing_id, amount_units)?;
    let (protocol_fee_atoms, _miner_fee_atoms) = jin_swap_fee_split_atoms(settings, price_atoms)?;
    Ok(ScriptBuf(
        format!(
            "JINSWAP1|sale-buy|v1|{}|{}|{}|{}|{}",
            buyer, listing_id, amount_units, price_atoms, protocol_fee_atoms
        )
        .into_bytes(),
    ))
}

pub fn parse_jin_sale_purchase_script(
    script: &ScriptBuf,
    settings: &Settings,
) -> Option<JinSalePurchase> {
    let raw = std::str::from_utf8(script.as_bytes()).ok()?;
    if !raw.as_bytes().starts_with(JIN_SWAP_SCRIPT_PREFIX) {
        return None;
    }
    let mut parts = raw.split('|');
    if parts.next()? != "JINSWAP1" {
        return None;
    }
    if parts.next()? != "sale-buy" {
        return None;
    }
    if parts.next()? != "v1" {
        return None;
    }
    let buyer = Address::parse_with_prefix(parts.next()?, &settings.network.address_prefix)
        .ok()?
        .to_string();
    let listing_id = parts.next()?.parse::<u32>().ok()?;
    let amount_units = parts.next()?.parse::<u128>().ok()?;
    let price_atoms = parts.next()?.parse::<u64>().ok()?;
    let protocol_fee_atoms = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(JinSalePurchase {
        buyer,
        listing_id,
        amount_units,
        price_atoms,
        protocol_fee_atoms,
    })
}

pub fn jin_sale_purchases_in_tx(
    tx: &Transaction,
    settings: &Settings,
) -> Vec<(usize, JinSalePurchase, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_jin_sale_purchase_script(&out.script_pubkey, settings)
                .map(|p| (idx, p, out.value.atoms()))
        })
        .collect()
}

pub fn is_jin_sale_purchase_transaction(tx: &Transaction, settings: &Settings) -> bool {
    !jin_sale_purchases_in_tx(tx, settings).is_empty()
}

pub fn is_jin_protocol_transaction(tx: &Transaction, settings: &Settings) -> bool {
    is_jin_sale_purchase_transaction(tx, settings)
        || is_qub_jin_infusion_transaction(tx, settings)
        || !jin_transfers_in_tx(tx, settings).is_empty()
        || !jin_conversions_in_tx(tx, settings).is_empty()
}

pub fn mempool_template_priority(settings: &Settings, tx: &Transaction) -> u8 {
    if is_pool_share_transaction(tx) {
        return 0;
    }
    if is_jin_sale_purchase_transaction(tx, settings) {
        return 1;
    }
    if is_qub_jin_infusion_transaction(tx, settings) {
        return 2;
    }
    if is_jin_protocol_transaction(tx, settings) {
        return 3;
    }
    if is_blast_claim_transaction(tx, settings) {
        return 4;
    }
    if !library_markers_in_tx(tx, settings).is_empty() {
        return 5;
    }
    16
}

// HF106/v1.6.9 mempool standardness: the old high-value JIN public-sale
// purchase that users called the "hot potato" was consensus-valid-looking but
// repeatedly raced/staled and then re-entered mempools, teasing 90+ QUB reward
// candidates and poisoning local UX. This is NOT consensus validation: blocks
// containing valid large purchases still validate. It is local mempool/mining
// policy so updated seeds/miners stop relaying/building enormous sale buys.
// Large buyers can split into multiple smaller buys; this keeps propagation and
// orphan-risk sane while the network is still small.
pub const HF106_MAX_STANDARD_JIN_SALE_BUY_UNITS: u128 = 50_000u128 * JIN_UNITS_PER_COIN;
pub const HF106_MAX_STANDARD_JIN_SALE_MINER_FEE_ATOMS: u64 = 5 * ATOMS_PER_QUB;

pub fn hf106_jin_sale_standardness_policy(tx: &Transaction, settings: &Settings) -> Result<()> {
    let purchases = jin_sale_purchases_in_tx(tx, settings);
    if purchases.is_empty() {
        return Ok(());
    }
    if purchases.len() > 1 {
        bail!("HF106 mempool policy: one JIN public sale purchase per tx");
    }
    let (_, purchase, marker_atoms) = &purchases[0];
    if *marker_atoms != settings.jin_swap.marker_output_atoms {
        bail!("HF106 mempool policy: invalid JIN sale marker output");
    }
    if purchase.amount_units > HF106_MAX_STANDARD_JIN_SALE_BUY_UNITS {
        bail!(
            "HF106 mempool policy: split large JIN buys into <= 50,000 JIN per transaction; this protects miners from stale high-fee hot-potato candidates"
        );
    }
    let expected_price =
        jin_swap_sale_price_atoms(settings, purchase.listing_id, purchase.amount_units)?;
    let (_protocol_fee, miner_fee) = jin_swap_fee_split_atoms(settings, expected_price)?;
    if miner_fee > HF106_MAX_STANDARD_JIN_SALE_MINER_FEE_ATOMS {
        bail!(
            "HF106 mempool policy: JIN sale miner fee {} atoms exceeds standard limit {}; split the buy into smaller transactions",
            miner_fee,
            HF106_MAX_STANDARD_JIN_SALE_MINER_FEE_ATOMS
        );
    }
    Ok(())
}

fn jin_sale_sold_by_listing_from_blocks(
    settings: &Settings,
    blocks: &[Block],
) -> Result<HashMap<u32, u128>> {
    let mut sold = HashMap::<u32, u128>::new();
    for (height, block) in blocks.iter().enumerate().skip(1) {
        let height = height as u32;
        if !jin_swap_active(settings, height) {
            continue;
        }
        for tx in block.transactions.iter().skip(1) {
            for (_, purchase, _) in jin_sale_purchases_in_tx(tx, settings) {
                let cur = sold.get(&purchase.listing_id).copied().unwrap_or(0);
                sold.insert(
                    purchase.listing_id,
                    cur.checked_add(purchase.amount_units)
                        .context("JIN sale sold overflow")?,
                );
            }
        }
    }
    Ok(sold)
}

fn jin_sale_sold_by_listing_with_mempool_except(
    settings: &Settings,
    chain: &ChainState,
    exclude_txid: Option<Hash256>,
) -> Result<HashMap<u32, u128>> {
    let mut sold = jin_sale_sold_by_listing_from_blocks(settings, &chain.blocks)?;
    // HF105/v1.6.7: only valid, spendable, non-conflicting pending sale buys count
    // as temporarily sold in wallet/listing/mempool views. Before this, stale sale
    // txs left in a repaired mempool could be counted blindly, making real buys
    // appear/drop/reappear and causing miners to build inconsistent high-fee blocks.
    let mut spent = HashSet::<OutPoint>::new();
    let mut pending = chain.mempool.iter().collect::<Vec<_>>();
    pending.sort_by_cached_key(|tx| {
        (
            mempool_template_priority(settings, *tx),
            tx.txid().to_string(),
        )
    });
    for tx in pending {
        let txid = tx.txid();
        if exclude_txid == Some(txid) {
            continue;
        }
        let purchases = jin_sale_purchases_in_tx(tx, settings);
        if purchases.is_empty() {
            continue;
        }
        let mut conflict = false;
        for input in &tx.inputs {
            if input.previous_output == OutPoint::null() {
                continue;
            }
            if !chain.utxos.contains_key(&input.previous_output)
                || spent.contains(&input.previous_output)
            {
                conflict = true;
                break;
            }
        }
        if conflict {
            continue;
        }
        if hf106_jin_sale_standardness_policy(tx, settings).is_err() {
            continue;
        }
        if validate_jin_sale_purchase_state(
            &purchases,
            tx,
            &chain.utxos,
            &mut sold,
            chain.height() + 1,
            settings,
        )
        .is_ok()
        {
            for input in &tx.inputs {
                if input.previous_output != OutPoint::null() {
                    spent.insert(input.previous_output.clone());
                }
            }
        }
    }
    Ok(sold)
}

fn jin_sale_sold_by_listing_with_mempool(
    settings: &Settings,
    chain: &ChainState,
) -> Result<HashMap<u32, u128>> {
    jin_sale_sold_by_listing_with_mempool_except(settings, chain, None)
}

pub fn jin_sale_listings(settings: &Settings, chain: &ChainState) -> Result<Vec<JinSaleListing>> {
    let sold = jin_sale_sold_by_listing_with_mempool(settings, chain)?;
    let count = jin_swap_sale_batch_count(settings)?;
    let display_height = chain.height() + 1;
    let mut out = Vec::new();
    for listing_id in 0..count {
        let total_units =
            jin_swap_sale_listing_total_units_at_height(settings, listing_id, display_height)?;
        let sold_units = sold.get(&listing_id).copied().unwrap_or(0).min(total_units);
        let remaining_units = total_units.saturating_sub(sold_units);
        out.push(JinSaleListing {
            listing_id,
            price_atoms_per_jin: jin_swap_sale_listing_price_atoms_per_jin(settings, listing_id)?,
            total_units,
            sold_units,
            remaining_units,
        });
    }
    Ok(out)
}

fn apply_jin_sale_purchase(
    map: &mut HashMap<String, u128>,
    purchase: &JinSalePurchase,
    settings: &Settings,
) -> Result<()> {
    jin_debit(map, &settings.jin.protocol_address, purchase.amount_units)?;
    jin_credit(map, &purchase.buyer, purchase.amount_units)?;
    Ok(())
}

fn validate_jin_sale_purchase_state(
    purchases: &[(usize, JinSalePurchase, u64)],
    tx: &Transaction,
    utxos: &HashMap<OutPoint, CoinRecord>,
    sold: &mut HashMap<u32, u128>,
    spend_height: u32,
    settings: &Settings,
) -> Result<u64> {
    if purchases.is_empty() {
        return Ok(0);
    }
    if !jin_swap_active(settings, spend_height) {
        bail!(
            "JIN public sale activates at block #{}",
            jin_swap_activation_height(settings)
        );
    }
    if tx.is_coinbase() {
        bail!("coinbase cannot buy JIN public sale listings");
    }
    if purchases.len() > 1 {
        bail!("a transaction may buy from one JIN sale listing at a time");
    }
    let (idx, purchase, marker_atoms) = &purchases[0];
    if *marker_atoms != settings.jin_swap.marker_output_atoms {
        bail!(
            "JIN sale marker output must be exactly {} atom(s)",
            settings.jin_swap.marker_output_atoms
        );
    }
    let buyer = Address::parse_with_prefix(&purchase.buyer, &settings.network.address_prefix)?;
    if !input_authorizes_address(tx, utxos, &buyer) {
        bail!("JIN sale purchase must be authorized by a QUB input from buyer address");
    }
    if purchase.amount_units == 0 {
        bail!("JIN sale amount must be non-zero");
    }
    if purchase.amount_units % JIN_UNITS_PER_COIN != 0 {
        bail!("JIN public sale uses whole JIN units only");
    }
    let listing_total =
        jin_swap_sale_listing_total_units_at_height(settings, purchase.listing_id, spend_height)?;
    let already_sold = sold.get(&purchase.listing_id).copied().unwrap_or(0);
    let remaining = listing_total.saturating_sub(already_sold);
    if purchase.amount_units > remaining {
        bail!("JIN sale listing has insufficient remaining JIN");
    }
    let expected_price =
        jin_swap_sale_price_atoms(settings, purchase.listing_id, purchase.amount_units)?;
    if purchase.price_atoms != expected_price {
        bail!("JIN sale price mismatch");
    }
    let (expected_protocol_fee, expected_miner_fee) =
        jin_swap_fee_split_atoms(settings, expected_price)?;
    if purchase.protocol_fee_atoms != expected_protocol_fee {
        bail!("JIN sale protocol fee mismatch");
    }
    let protocol = Address::parse_with_prefix(
        &settings.jin.protocol_address,
        &settings.network.address_prefix,
    )?;
    let protocol_script = protocol.script_pubkey().0;
    let paid_to_protocol: u64 = tx
        .outputs
        .iter()
        .enumerate()
        .filter(|(out_idx, out)| *out_idx != *idx && out.script_pubkey.0 == protocol_script)
        .map(|(_, out)| out.value.atoms())
        .sum();
    let required_protocol_payment = expected_price
        .checked_add(expected_protocol_fee)
        .context("JIN sale protocol payment overflow")?;
    if paid_to_protocol < required_protocol_payment {
        bail!(
            "JIN sale underpayment to protocol: need {}, paid {}",
            required_protocol_payment,
            paid_to_protocol
        );
    }
    sold.insert(
        purchase.listing_id,
        already_sold
            .checked_add(purchase.amount_units)
            .context("JIN sale sold overflow")?,
    );
    Ok(expected_miner_fee)
}

fn validate_jin_sale_transaction_against_chain(
    tx: &Transaction,
    chain: &ChainState,
    spend_height: u32,
    settings: &Settings,
) -> Result<u64> {
    let purchases = jin_sale_purchases_in_tx(tx, settings);
    if purchases.is_empty() {
        return Ok(0);
    }
    // HF75/v1.5.8: when re-validating an already-pending JIN sale tx, do not
    // count that same tx as already sold from mempool. Otherwise mempool
    // retention/snapshot repair can reject and silently drop valid pending buys.
    let mut sold = jin_sale_sold_by_listing_with_mempool_except(settings, chain, Some(tx.txid()))?;
    validate_jin_sale_purchase_state(
        &purchases,
        tx,
        &chain.utxos,
        &mut sold,
        spend_height,
        settings,
    )
}

fn validate_jin_sale_block(
    block: &Block,
    prior_blocks: &[Block],
    base_utxos: &HashMap<OutPoint, CoinRecord>,
    height: u32,
    settings: &Settings,
) -> Result<()> {
    let mut sold = jin_sale_sold_by_listing_from_blocks(settings, prior_blocks)?;
    let mut scratch = base_utxos.clone();
    for tx in block.transactions.iter().skip(1) {
        let purchases = jin_sale_purchases_in_tx(tx, settings);
        validate_jin_sale_purchase_state(&purchases, tx, &scratch, &mut sold, height, settings)?;
        validate_tx_contextual(tx, &scratch, height, settings, true)?;
        connect_tx_utxos(tx, &mut scratch, height, false)?;
    }
    Ok(())
}

pub fn jin_swap_miner_fee_required_in_tx(
    settings: &Settings,
    tx: &Transaction,
    height: u32,
) -> Result<u64> {
    let purchases = jin_sale_purchases_in_tx(tx, settings);
    if purchases.is_empty() {
        return Ok(0);
    }
    if !jin_swap_active(settings, height) {
        return Ok(0);
    }
    let (_, purchase, _) = &purchases[0];
    let expected_price =
        jin_swap_sale_price_atoms(settings, purchase.listing_id, purchase.amount_units)?;
    let (_protocol_fee, miner_fee) = jin_swap_fee_split_atoms(settings, expected_price)?;
    Ok(miner_fee)
}

fn jin_credit(map: &mut HashMap<String, u128>, address: &str, units: u128) -> Result<()> {
    let cur = map.get(address).copied().unwrap_or(0);
    map.insert(
        address.to_string(),
        cur.checked_add(units).context("JIN balance overflow")?,
    );
    Ok(())
}
fn jin_debit(map: &mut HashMap<String, u128>, address: &str, units: u128) -> Result<()> {
    let cur = map.get(address).copied().unwrap_or(0);
    if cur < units {
        bail!("insufficient JIN balance");
    }
    map.insert(address.to_string(), cur - units);
    Ok(())
}

fn apply_jin_transfer(
    map: &mut HashMap<String, u128>,
    transfer: &JinTransfer,
    miner_address: Option<&str>,
) -> Result<()> {
    let mut debit = transfer.amount_units;
    if transfer.fee_asset == "JIN" {
        debit = debit
            .checked_add(transfer.fee_units)
            .context("JIN fee overflow")?;
    }
    jin_debit(map, &transfer.from, debit)?;
    jin_credit(map, &transfer.to, transfer.amount_units)?;
    if transfer.fee_asset == "JIN" && transfer.fee_units > 0 {
        if let Some(miner) = miner_address {
            jin_credit(map, miner, transfer.fee_units)?;
        }
    }
    Ok(())
}

fn apply_jin_conversion(
    map: &mut HashMap<String, u128>,
    conversion: &JinConversion,
    settings: &Settings,
    miner_address: Option<&str>,
) -> Result<()> {
    let mut debit = conversion.amount_units;
    if conversion.fee_asset == "JIN" {
        debit = debit
            .checked_add(conversion.fee_units)
            .context("JIN conversion fee overflow")?;
    }
    jin_debit(map, &conversion.from, debit)?;
    // Coin -> Token conversion locks native JIN back into the bridge/protocol reserve.
    // The Enjin Matrixchain token payout is handled by the bridge service after this on-chain event finalizes.
    jin_credit(map, &settings.jin.protocol_address, conversion.amount_units)?;
    if conversion.fee_asset == "JIN" && conversion.fee_units > 0 {
        if let Some(miner) = miner_address {
            jin_credit(map, miner, conversion.fee_units)?;
        }
    }
    Ok(())
}

fn coinbase_miner_address(settings: &Settings, block: &Block) -> Option<String> {
    block.transactions.first()?.outputs.first().and_then(|out| {
        address_from_script_pubkey(&settings.network.address_prefix, &out.script_pubkey)
            .map(|a| a.to_string())
    })
}

pub fn jin_ledger_from_blocks(
    settings: &Settings,
    blocks: &[Block],
) -> Result<HashMap<String, u128>> {
    let mut ledger = HashMap::new();
    if !settings.jin.enabled || !settings.features.jin_native_coin_enabled {
        return Ok(ledger);
    }
    let visible_height = blocks.len().saturating_sub(1) as u32;
    if visible_height < settings.jin.activation_height {
        return Ok(ledger);
    }
    jin_credit(
        &mut ledger,
        &settings.jin.protocol_address,
        JIN_TOTAL_SUPPLY_UNITS,
    )?;
    let mut qub_jin_state = QubJinInfusionState::inactive(settings);
    for (height, block) in blocks.iter().enumerate().skip(1) {
        let height = height as u32;
        if height == qub_jin_infusion_activation_height(settings) {
            qub_jin_apply_bootstrap(settings, &mut ledger, &mut qub_jin_state)?;
        }
        if height < settings.jin.activation_height {
            continue;
        }
        let pool_block = parse_pool_block_marker(block).is_some();
        let miner = if pool_block {
            None
        } else {
            coinbase_miner_address(settings, block)
        };
        let mut block_jin_fees = 0u128;
        for tx in block.transactions.iter().skip(1) {
            for (_, purchase, marker_atoms) in jin_sale_purchases_in_tx(tx, settings) {
                if height < settings.jin_swap.activation_height {
                    bail!(
                        "JIN public sale activates at block #{}",
                        settings.jin_swap.activation_height
                    );
                }
                if marker_atoms != settings.jin_swap.marker_output_atoms {
                    bail!(
                        "JIN sale marker output must be exactly {} atom(s)",
                        settings.jin_swap.marker_output_atoms
                    );
                }
                apply_jin_sale_purchase(&mut ledger, &purchase, settings)?;
            }
            for (_, transfer, marker_atoms) in jin_transfers_in_tx(tx, settings) {
                if marker_atoms != settings.jin.marker_output_atoms {
                    bail!(
                        "JIN marker output must be exactly {} atom(s)",
                        settings.jin.marker_output_atoms
                    );
                }
                if pool_block && transfer.fee_asset == "JIN" {
                    block_jin_fees = block_jin_fees
                        .checked_add(transfer.fee_units)
                        .context("JIN fee overflow")?;
                }
                apply_jin_transfer(&mut ledger, &transfer, miner.as_deref())?;
            }
            for (_, conversion, marker_atoms) in jin_conversions_in_tx(tx, settings) {
                if height < settings.jin.conversion_activation_height {
                    bail!(
                        "JIN Coin -> Token conversion activates at block #{}",
                        settings.jin.conversion_activation_height
                    );
                }
                if marker_atoms != settings.jin.marker_output_atoms {
                    bail!(
                        "JIN conversion marker output must be exactly {} atom(s)",
                        settings.jin.marker_output_atoms
                    );
                }
                if pool_block && conversion.fee_asset == "JIN" {
                    block_jin_fees = block_jin_fees
                        .checked_add(conversion.fee_units)
                        .context("JIN conversion fee overflow")?;
                }
                apply_jin_conversion(&mut ledger, &conversion, settings, miner.as_deref())?;
            }
            for (_, marker, marker_atoms) in qub_jin_infusion_markers_in_tx(tx, settings) {
                if !qub_jin_infusion_active(settings, height) {
                    bail!(
                        "QUB/JIN infusion activates at block #{}",
                        qub_jin_infusion_activation_height(settings)
                    );
                }
                if marker_atoms != settings.jin.marker_output_atoms {
                    bail!(
                        "QUB/JIN infusion marker output must be exactly {} atom(s)",
                        settings.jin.marker_output_atoms
                    );
                }
                qub_jin_apply_marker_unchecked(settings, &mut ledger, &mut qub_jin_state, &marker)?;
            }
        }
        if pool_block && block_jin_fees > 0 {
            for (addr, units) in jin_fee_receivers_for_block(
                settings,
                &blocks[..height as usize],
                block,
                block_jin_fees,
            )? {
                jin_credit(&mut ledger, &addr, units)?;
            }
        }
    }
    apply_verified_governance_locks_to_jin_ledger(&mut ledger, settings, blocks)?;
    Ok(ledger)
}

pub fn jin_balance_units_for_address(
    settings: &Settings,
    chain: &ChainState,
    address: &str,
) -> Result<u128> {
    Address::parse_with_prefix(address, &settings.network.address_prefix)?;
    let mut ledger = jin_ledger_from_blocks(settings, &chain.blocks)?;
    let mut qub_jin_state = qub_jin_infusion_state_from_blocks(settings, &chain.blocks)?;
    let spend_height = chain.height() + 1;
    let _ = qub_jin_apply_bootstrap_for_context(
        settings,
        &mut ledger,
        &mut qub_jin_state,
        chain.height(),
        spend_height,
    );
    // Show local mempool effects optimistically for the selected address.
    for tx in &chain.mempool {
        for (_, purchase, _) in jin_sale_purchases_in_tx(tx, settings) {
            let _ = apply_jin_sale_purchase(&mut ledger, &purchase, settings);
        }
        for (_, transfer, _) in jin_transfers_in_tx(tx, settings) {
            let _ = apply_jin_transfer(&mut ledger, &transfer, None);
        }
        for (_, conversion, _) in jin_conversions_in_tx(tx, settings) {
            let _ = apply_jin_conversion(&mut ledger, &conversion, settings, None);
        }
        for (_, marker, _) in qub_jin_infusion_markers_in_tx(tx, settings) {
            let mut ledger_trial = ledger.clone();
            let mut state_trial = qub_jin_state.clone();
            if qub_jin_apply_marker_unchecked(
                settings,
                &mut ledger_trial,
                &mut state_trial,
                &marker,
            )
            .is_ok()
            {
                ledger = ledger_trial;
                qub_jin_state = state_trial;
            }
        }
    }
    Ok(ledger.get(address).copied().unwrap_or(0))
}

fn input_authorizes_address(
    tx: &Transaction,
    base_utxos: &HashMap<OutPoint, CoinRecord>,
    address: &Address,
) -> bool {
    let script = address.script_pubkey().0;
    tx.inputs.iter().any(|input| {
        base_utxos
            .get(&input.previous_output)
            .map(|coin| coin.tx_out.script_pubkey.0 == script)
            .unwrap_or(false)
    })
}

fn validate_jin_transaction_with_ledger(
    tx: &Transaction,
    ledger: &mut HashMap<String, u128>,
    base_utxos: &HashMap<OutPoint, CoinRecord>,
    height: u32,
    settings: &Settings,
    miner_address: Option<&str>,
) -> Result<u128> {
    let transfers = jin_transfers_in_tx(tx, settings);
    let conversions = jin_conversions_in_tx(tx, settings);
    let purchases = jin_sale_purchases_in_tx(tx, settings);
    if transfers.is_empty() && conversions.is_empty() && purchases.is_empty() {
        return Ok(0);
    }
    if !jin_active(settings, height) {
        bail!("JIN activates at block #{}", settings.jin.activation_height);
    }
    let mut total_jin_fee = 0u128;
    for (_, transfer, marker_atoms) in transfers {
        if marker_atoms != settings.jin.marker_output_atoms {
            bail!(
                "JIN marker output must be exactly {} atom(s)",
                settings.jin.marker_output_atoms
            );
        }
        let from = Address::parse_with_prefix(&transfer.from, &settings.network.address_prefix)?;
        Address::parse_with_prefix(&transfer.to, &settings.network.address_prefix)?;
        if !input_authorizes_address(tx, base_utxos, &from) {
            bail!("JIN transfer is not authorized by a QUB input from the source address");
        }
        if transfer.fee_asset == "JIN" {
            total_jin_fee = total_jin_fee
                .checked_add(transfer.fee_units)
                .context("JIN fee overflow")?;
        }
        apply_jin_transfer(ledger, &transfer, miner_address)?;
    }
    for (_, conversion, marker_atoms) in conversions {
        if !jin_conversion_active(settings, height) {
            bail!("JIN Coin -> Token conversion is disabled until the Enjin bridge is live");
        }
        if marker_atoms != settings.jin.marker_output_atoms {
            bail!(
                "JIN conversion marker output must be exactly {} atom(s)",
                settings.jin.marker_output_atoms
            );
        }
        let from = Address::parse_with_prefix(&conversion.from, &settings.network.address_prefix)?;
        if !input_authorizes_address(tx, base_utxos, &from) {
            bail!("JIN conversion is not authorized by a QUB input from the source address");
        }
        if conversion.fee_asset == "JIN" {
            total_jin_fee = total_jin_fee
                .checked_add(conversion.fee_units)
                .context("JIN conversion fee overflow")?;
        }
        apply_jin_conversion(ledger, &conversion, settings, miner_address)?;
    }
    for (_, purchase, marker_atoms) in purchases {
        if !jin_swap_active(settings, height) {
            bail!(
                "JIN public sale activates at block #{}",
                settings.jin_swap.activation_height
            );
        }
        if marker_atoms != settings.jin_swap.marker_output_atoms {
            bail!(
                "JIN sale marker output must be exactly {} atom(s)",
                settings.jin_swap.marker_output_atoms
            );
        }
        apply_jin_sale_purchase(ledger, &purchase, settings)?;
    }
    Ok(total_jin_fee)
}

fn validate_jin_transaction_against_chain(
    tx: &Transaction,
    chain: &ChainState,
    spend_height: u32,
    settings: &Settings,
) -> Result<u128> {
    let mut ledger = jin_ledger_from_blocks(settings, &chain.blocks)?;
    let mut qub_jin_state = qub_jin_infusion_state_from_blocks(settings, &chain.blocks)?;
    qub_jin_apply_bootstrap_for_context(
        settings,
        &mut ledger,
        &mut qub_jin_state,
        chain.height(),
        spend_height,
    )?;
    for mem in &chain.mempool {
        if mem.txid() == tx.txid() {
            continue;
        }
        if validate_jin_transaction_with_ledger(
            mem,
            &mut ledger,
            &chain.utxos,
            spend_height,
            settings,
            None,
        )
        .is_ok()
        {
            let _ = validate_qub_jin_infusion_transaction_with_state(
                mem,
                &mut ledger,
                &mut qub_jin_state,
                &chain.utxos,
                spend_height,
                settings,
            );
        }
    }
    validate_jin_transaction_with_ledger(
        tx,
        &mut ledger,
        &chain.utxos,
        spend_height,
        settings,
        None,
    )
}

fn validate_jin_block(
    block: &Block,
    prior_blocks: &[Block],
    base_utxos: &HashMap<OutPoint, CoinRecord>,
    height: u32,
    settings: &Settings,
) -> Result<()> {
    let mut ledger = jin_ledger_from_blocks(settings, prior_blocks)?;
    let mut qub_jin_state = qub_jin_infusion_state_from_blocks(settings, prior_blocks)?;
    let prior_visible_height = prior_blocks.len().saturating_sub(1) as u32;
    qub_jin_apply_bootstrap_for_context(
        settings,
        &mut ledger,
        &mut qub_jin_state,
        prior_visible_height,
        height,
    )?;
    let mut scratch = base_utxos.clone();
    let pool_block = parse_pool_block_marker(block).is_some();
    let miner = if pool_block {
        None
    } else {
        coinbase_miner_address(settings, block)
    };
    let mut block_jin_fees = 0u128;
    for tx in block.transactions.iter().skip(1) {
        if !jin_active(settings, height)
            && (!jin_transfers_in_tx(tx, settings).is_empty()
                || !jin_conversions_in_tx(tx, settings).is_empty())
        {
            bail!("JIN activates at block #{}", settings.jin.activation_height);
        }
        if !jin_conversion_active(settings, height)
            && !jin_conversions_in_tx(tx, settings).is_empty()
        {
            bail!("JIN Coin -> Token conversion is disabled until the Enjin bridge is live");
        }
        validate_tx_contextual(tx, &scratch, height, settings, true)?;
        let fee_units = validate_jin_transaction_with_ledger(
            tx,
            &mut ledger,
            &scratch,
            height,
            settings,
            miner.as_deref(),
        )?;
        validate_qub_jin_infusion_transaction_with_state(
            tx,
            &mut ledger,
            &mut qub_jin_state,
            &scratch,
            height,
            settings,
        )?;
        if pool_block {
            block_jin_fees = block_jin_fees
                .checked_add(fee_units)
                .context("JIN block fee overflow")?;
        }
        connect_tx_utxos(tx, &mut scratch, height, false)?;
    }
    if pool_block && block_jin_fees > 0 {
        for (addr, units) in
            jin_fee_receivers_for_block(settings, prior_blocks, block, block_jin_fees)?
        {
            jin_credit(&mut ledger, &addr, units)?;
        }
    }
    Ok(())
}

const VERIFIED_GOVERNANCE_SCRIPT_PREFIX: &[u8] = b"VGOV1|";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifiedStatus {
    Pending,
    Active,
    UnderReview,
    Suspended,
    Revoked,
    Expired,
}

impl Display for VerifiedStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            VerifiedStatus::Pending => "pending",
            VerifiedStatus::Active => "active",
            VerifiedStatus::UnderReview => "under_review",
            VerifiedStatus::Suspended => "suspended",
            VerifiedStatus::Revoked => "revoked",
            VerifiedStatus::Expired => "expired",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedAvatarMeta {
    pub hash_hex: String,
    pub mime: String,
    pub size_bytes: u32,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedWalletProfile {
    pub address: String,
    pub status: VerifiedStatus,
    pub display_name: String,
    pub avatar: Option<VerifiedAvatarMeta>,
    pub locked_jin_units: u128,
    pub lock_until_height: u32,
    pub verified_since_height: u32,
    pub last_review_height: u32,
    pub strikes: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedPoolProfile {
    pub pool_id: Hash256,
    pub owner_address: String,
    pub status: VerifiedStatus,
    pub display_name: String,
    pub avatar: Option<VerifiedAvatarMeta>,
    pub locked_jin_units: u128,
    pub lock_until_height: u32,
    pub verified_since_height: u32,
    pub last_review_height: u32,
    pub strikes: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeratorProfile {
    pub address: String,
    pub status: VerifiedStatus,
    pub statement_hash: String,
    pub statement_ref: String,
    pub locked_jin_units: u128,
    pub lock_until_height: u32,
    pub elected_height: Option<u32>,
    pub support_power_units: u128,
    pub support_voters: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportCase {
    pub report_id: String,
    pub target_kind: String,
    pub target_id: String,
    pub reporter: String,
    pub category: String,
    pub severity: String,
    pub evidence_hash: String,
    pub evidence_ref: String,
    pub report_bond_units: u128,
    pub status: String,
    pub opened_height: u32,
    pub decision_height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedJinLock {
    pub owner: String,
    pub units: u128,
    pub lock_until_height: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedGovernanceState {
    pub wallets: HashMap<String, VerifiedWalletProfile>,
    pub pools: HashMap<Hash256, VerifiedPoolProfile>,
    pub moderators: HashMap<String, ModeratorProfile>,
    pub reports: HashMap<String, ReportCase>,
    pub votes: HashSet<String>,
    pub locks: Vec<VerifiedJinLock>,
    pub pending_slash_units: HashMap<String, u128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifiedGovernanceMarker {
    WalletApply {
        address: String,
        display_name: String,
        avatar_hash: String,
        avatar_mime: String,
        avatar_size_bytes: u32,
        avatar_ref: String,
        bond_units: u128,
        lock_until_height: u32,
    },
    PoolApply {
        pool_id: Hash256,
        owner: String,
        display_name: String,
        avatar_hash: String,
        avatar_mime: String,
        avatar_size_bytes: u32,
        avatar_ref: String,
        bond_units: u128,
        lock_until_height: u32,
    },
    Report {
        reporter: String,
        target_kind: String,
        target_id: String,
        category: String,
        severity: String,
        evidence_hash: String,
        evidence_ref: String,
        bond_units: u128,
    },
    ModeratorApply {
        candidate: String,
        statement_hash: String,
        statement_ref: String,
        bond_units: u128,
        lock_until_height: u32,
    },
    Vote {
        voter: String,
        proposal_kind: String,
        proposal_id: String,
        support: bool,
        jin_units: u128,
        conviction: u8,
        lock_until_height: u32,
    },
    Decision {
        moderator: String,
        report_id: String,
        decision: String,
        slash_bps: u16,
    },
}

pub fn verified_governance_active(settings: &Settings, height: u32) -> bool {
    settings.verified_governance.enabled && height >= settings.verified_governance.activation_height
}

fn verified_governance_bond_units(raw: &str) -> Result<u128> {
    parse_jin_units_raw(raw)
}

fn verified_governance_hash_hex(input: &str, label: &str) -> Result<String> {
    let h = input.trim().to_ascii_lowercase();
    if h.len() != 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid {label} hash");
    }
    Ok(h)
}

fn verified_governance_clean_ascii(input: &str, max_bytes: usize, label: &str) -> Result<String> {
    let s = input.trim();
    if s.len() > max_bytes {
        bail!("{label} too long; max {max_bytes} bytes");
    }
    if !s
        .chars()
        .all(|c| c.is_ascii() && (c.is_ascii_graphic() || c == ' '))
    {
        bail!("{label} must be printable ASCII in v1");
    }
    if s.contains('|') {
        bail!("{label} cannot contain pipe characters");
    }
    Ok(s.to_string())
}

fn verified_governance_decode_hex_text(
    input: &str,
    max_bytes: usize,
    label: &str,
) -> Result<String> {
    let raw = input.trim();
    if raw == "-" || raw.is_empty() {
        return Ok(String::new());
    }
    let bytes = hex::decode(raw).with_context(|| format!("{label} must be hex encoded"))?;
    if bytes.len() > max_bytes {
        bail!("{label} too long; max {max_bytes} bytes");
    }
    let s = String::from_utf8(bytes).with_context(|| format!("{label} must be UTF-8"))?;
    verified_governance_clean_ascii(&s, max_bytes, label)
}

fn verified_governance_encode_hex_text(
    input: &str,
    max_bytes: usize,
    label: &str,
) -> Result<String> {
    let s = verified_governance_clean_ascii(input, max_bytes, label)?;
    if s.is_empty() {
        Ok("-".to_string())
    } else {
        Ok(hex::encode(s.as_bytes()))
    }
}

fn verified_avatar_meta(
    hash: &str,
    mime: &str,
    size: u32,
    reference: &str,
    settings: &Settings,
) -> Result<Option<VerifiedAvatarMeta>> {
    let hash = hash.trim();
    let mime = mime.trim().to_ascii_lowercase();
    let reference = reference.trim();
    if hash == "-" || hash.is_empty() {
        if mime != "-" && !mime.is_empty() {
            bail!("avatar mime supplied without avatar hash");
        }
        if size != 0 {
            bail!("avatar size supplied without avatar hash");
        }
        if reference != "-" && !reference.is_empty() {
            bail!("avatar reference supplied without avatar hash");
        }
        return Ok(None);
    }
    let hash_hex = verified_governance_hash_hex(hash, "avatar")?;
    if mime != "image/png" && mime != "image/webp" {
        bail!("avatar mime must be image/png or image/webp");
    }
    if size == 0 || size > settings.verified_governance.max_avatar_bytes {
        bail!(
            "avatar size must be 1..{} bytes",
            settings.verified_governance.max_avatar_bytes
        );
    }
    let reference = verified_governance_clean_ascii(
        reference,
        settings.verified_governance.max_avatar_ref_bytes,
        "avatar reference",
    )?;
    Ok(Some(VerifiedAvatarMeta {
        hash_hex,
        mime,
        size_bytes: size,
        reference,
    }))
}

pub fn verified_governance_wallet_apply_marker_script(
    settings: &Settings,
    address: &Address,
    display_name: &str,
    avatar_hash: &str,
    avatar_mime: &str,
    avatar_size_bytes: u32,
    avatar_ref: &str,
    bond_units: u128,
    lock_until_height: u32,
) -> Result<ScriptBuf> {
    let display_hex = verified_governance_encode_hex_text(
        display_name,
        settings.verified_governance.max_display_name_bytes,
        "display name",
    )?;
    let avatar_ref_hex = verified_governance_encode_hex_text(
        avatar_ref,
        settings.verified_governance.max_avatar_ref_bytes,
        "avatar reference",
    )?;
    Ok(ScriptBuf(
        format!(
            "VGOV1|wallet|{}|{}|{}|{}|{}|{}|{}|{}",
            address,
            display_hex,
            avatar_hash.trim(),
            avatar_mime.trim(),
            avatar_size_bytes,
            avatar_ref_hex,
            bond_units,
            lock_until_height
        )
        .into_bytes(),
    ))
}

pub fn verified_governance_pool_apply_marker_script(
    settings: &Settings,
    pool_id: Hash256,
    owner: &Address,
    display_name: &str,
    avatar_hash: &str,
    avatar_mime: &str,
    avatar_size_bytes: u32,
    avatar_ref: &str,
    bond_units: u128,
    lock_until_height: u32,
) -> Result<ScriptBuf> {
    let display_hex = verified_governance_encode_hex_text(
        display_name,
        settings.verified_governance.max_display_name_bytes,
        "display name",
    )?;
    let avatar_ref_hex = verified_governance_encode_hex_text(
        avatar_ref,
        settings.verified_governance.max_avatar_ref_bytes,
        "avatar reference",
    )?;
    Ok(ScriptBuf(
        format!(
            "VGOV1|pool|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            pool_id,
            owner,
            display_hex,
            avatar_hash.trim(),
            avatar_mime.trim(),
            avatar_size_bytes,
            avatar_ref_hex,
            bond_units,
            lock_until_height
        )
        .into_bytes(),
    ))
}

pub fn verified_governance_report_marker_script(
    settings: &Settings,
    reporter: &Address,
    target_kind: &str,
    target_id: &str,
    category: &str,
    severity: &str,
    evidence_hash: &str,
    evidence_ref: &str,
    bond_units: u128,
) -> Result<ScriptBuf> {
    let target_kind =
        verified_governance_clean_ascii(target_kind, 32, "target kind")?.to_ascii_lowercase();
    let target_id = verified_governance_clean_ascii(target_id, 96, "target id")?;
    let category = verified_governance_clean_ascii(category, 48, "report category")?;
    let severity =
        verified_governance_clean_ascii(severity, 16, "report severity")?.to_ascii_lowercase();
    let evidence_ref_hex = verified_governance_encode_hex_text(
        evidence_ref,
        settings.verified_governance.max_evidence_ref_bytes,
        "evidence reference",
    )?;
    Ok(ScriptBuf(
        format!(
            "VGOV1|report|{}|{}|{}|{}|{}|{}|{}|{}",
            reporter,
            target_kind,
            target_id,
            category,
            severity,
            evidence_hash.trim(),
            evidence_ref_hex,
            bond_units
        )
        .into_bytes(),
    ))
}

pub fn verified_governance_moderator_apply_marker_script(
    settings: &Settings,
    candidate: &Address,
    statement_hash: &str,
    statement_ref: &str,
    bond_units: u128,
    lock_until_height: u32,
) -> Result<ScriptBuf> {
    let statement_ref_hex = verified_governance_encode_hex_text(
        statement_ref,
        settings.verified_governance.max_evidence_ref_bytes,
        "statement reference",
    )?;
    Ok(ScriptBuf(
        format!(
            "VGOV1|mod_apply|{}|{}|{}|{}|{}",
            candidate,
            statement_hash.trim(),
            statement_ref_hex,
            bond_units,
            lock_until_height
        )
        .into_bytes(),
    ))
}

pub fn verified_governance_vote_marker_script(
    voter: &Address,
    proposal_kind: &str,
    proposal_id: &str,
    support: bool,
    jin_units: u128,
    conviction: u8,
    lock_until_height: u32,
) -> Result<ScriptBuf> {
    let proposal_kind =
        verified_governance_clean_ascii(proposal_kind, 32, "proposal kind")?.to_ascii_lowercase();
    let proposal_id = verified_governance_clean_ascii(proposal_id, 96, "proposal id")?;
    Ok(ScriptBuf(
        format!(
            "VGOV1|vote|{}|{}|{}|{}|{}|{}|{}",
            voter,
            proposal_kind,
            proposal_id,
            if support { "yes" } else { "no" },
            jin_units,
            conviction,
            lock_until_height
        )
        .into_bytes(),
    ))
}

pub fn verified_governance_decision_marker_script(
    moderator: &Address,
    report_id: &str,
    decision: &str,
    slash_bps: u16,
) -> Result<ScriptBuf> {
    let report_id = verified_governance_clean_ascii(report_id, 96, "report id")?;
    let decision = verified_governance_clean_ascii(decision, 32, "decision")?.to_ascii_lowercase();
    Ok(ScriptBuf(
        format!(
            "VGOV1|decision|{}|{}|{}|{}",
            moderator, report_id, decision, slash_bps
        )
        .into_bytes(),
    ))
}

// HF112 / USDJ Bridge v1 marker scaffold.
// This is intentionally separated from consensus activation. Full QUB-chain
// USDJ accounting should be enabled only by a future explicit bridge activation.
pub const USDJ_BRIDGE_TOLL_BPS: u16 = 100;
pub const MAINNET_USDJ_BRIDGE_PROTOCOL_ADDRESS: &str =
    "qub1a229a209ca3fc2b3066f6f31d4b27c9d663c46959346d1";
const USDJ_BRIDGE_SCRIPT_PREFIX: &[u8] = b"BRDG1|";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsdjBridgeMarker {
    EthToQubClaim {
        eth_tx: String,
        eth_log_index: u32,
        recipient: Address,
        gross_units: u128,
        toll_units: u128,
        net_units: u128,
    },
    QubToEthExit {
        sender: Address,
        eth_recipient: String,
        release_units: u128,
        toll_units: u128,
        debit_units: u128,
    },
}

pub fn usdj_bridge_toll_units(amount_units: u128) -> u128 {
    amount_units.saturating_mul(USDJ_BRIDGE_TOLL_BPS as u128) / 10_000u128
}

fn bridge_clean_ascii(input: &str, max_bytes: usize, label: &str) -> Result<String> {
    let s = input.trim();
    if s.is_empty() {
        bail!("{label} cannot be empty");
    }
    if s.len() > max_bytes {
        bail!("{label} too long; max {max_bytes} bytes");
    }
    if !s.chars().all(|c| c.is_ascii() && c.is_ascii_graphic()) {
        bail!("{label} must be printable ASCII");
    }
    if s.contains('|') {
        bail!("{label} cannot contain pipe characters");
    }
    Ok(s.to_string())
}

pub fn usdj_bridge_eth_to_qub_claim_marker_script(
    eth_tx: &str,
    eth_log_index: u32,
    recipient: &Address,
    gross_units: u128,
) -> Result<ScriptBuf> {
    let eth_tx = bridge_clean_ascii(eth_tx, 96, "Ethereum tx hash")?;
    if gross_units == 0 {
        bail!("gross USDJ amount must be non-zero");
    }
    let toll_units = usdj_bridge_toll_units(gross_units);
    let net_units = gross_units.saturating_sub(toll_units);
    Ok(ScriptBuf(
        format!(
            "BRDG1|eth_to_qub|{}|{}|{}|{}|{}|{}",
            eth_tx, eth_log_index, recipient, gross_units, toll_units, net_units
        )
        .into_bytes(),
    ))
}

pub fn usdj_bridge_qub_to_eth_exit_marker_script(
    sender: &Address,
    eth_recipient: &str,
    release_units: u128,
) -> Result<ScriptBuf> {
    let eth_recipient = bridge_clean_ascii(eth_recipient, 64, "Ethereum recipient")?;
    if !eth_recipient.starts_with("0x") || eth_recipient.len() != 42 {
        bail!("Ethereum recipient must be a 0x address");
    }
    if release_units == 0 {
        bail!("release USDJ amount must be non-zero");
    }
    let toll_units = usdj_bridge_toll_units(release_units);
    let debit_units = release_units.saturating_add(toll_units);
    Ok(ScriptBuf(
        format!(
            "BRDG1|qub_to_eth|{}|{}|{}|{}|{}",
            sender, eth_recipient, release_units, toll_units, debit_units
        )
        .into_bytes(),
    ))
}

pub fn parse_usdj_bridge_marker_script(script: &ScriptBuf) -> Option<UsdjBridgeMarker> {
    let b = script.as_bytes();
    if !b.starts_with(USDJ_BRIDGE_SCRIPT_PREFIX) {
        return None;
    }
    let raw = std::str::from_utf8(b).ok()?;
    let mut parts = raw.split('|');
    if parts.next()? != "BRDG1" {
        return None;
    }
    match parts.next()? {
        "eth_to_qub" => {
            let eth_tx = parts.next()?.to_string();
            let eth_log_index = parts.next()?.parse::<u32>().ok()?;
            let recipient = Address::from_str(parts.next()?).ok()?;
            let gross_units = parts.next()?.parse::<u128>().ok()?;
            let toll_units = parts.next()?.parse::<u128>().ok()?;
            let net_units = parts.next()?.parse::<u128>().ok()?;
            Some(UsdjBridgeMarker::EthToQubClaim {
                eth_tx,
                eth_log_index,
                recipient,
                gross_units,
                toll_units,
                net_units,
            })
        }
        "qub_to_eth" => {
            let sender = Address::from_str(parts.next()?).ok()?;
            let eth_recipient = parts.next()?.to_string();
            let release_units = parts.next()?.parse::<u128>().ok()?;
            let toll_units = parts.next()?.parse::<u128>().ok()?;
            let debit_units = parts.next()?.parse::<u128>().ok()?;
            Some(UsdjBridgeMarker::QubToEthExit {
                sender,
                eth_recipient,
                release_units,
                toll_units,
                debit_units,
            })
        }
        _ => None,
    }
}

pub fn parse_verified_governance_marker_script(
    script: &ScriptBuf,
    settings: &Settings,
) -> Option<VerifiedGovernanceMarker> {
    let b = script.as_bytes();
    if !b.starts_with(VERIFIED_GOVERNANCE_SCRIPT_PREFIX) {
        return None;
    }
    let s = std::str::from_utf8(b).ok()?;
    let mut parts = s.split('|');
    if parts.next()? != "VGOV1" {
        return None;
    }
    let action = parts.next()?;
    let marker = match action {
        "wallet" => {
            let address = parts.next()?.trim().to_string();
            Address::parse_with_prefix(&address, &settings.network.address_prefix).ok()?;
            let display_name = verified_governance_decode_hex_text(
                parts.next()?,
                settings.verified_governance.max_display_name_bytes,
                "display name",
            )
            .ok()?;
            let avatar_hash = parts.next()?.trim().to_string();
            let avatar_mime = parts.next()?.trim().to_string();
            let avatar_size_bytes = parts.next()?.parse::<u32>().ok()?;
            let avatar_ref = verified_governance_decode_hex_text(
                parts.next()?,
                settings.verified_governance.max_avatar_ref_bytes,
                "avatar reference",
            )
            .ok()?;
            let bond_units = parts.next()?.parse::<u128>().ok()?;
            let lock_until_height = parts.next()?.parse::<u32>().ok()?;
            VerifiedGovernanceMarker::WalletApply {
                address,
                display_name,
                avatar_hash,
                avatar_mime,
                avatar_size_bytes,
                avatar_ref,
                bond_units,
                lock_until_height,
            }
        }
        "pool" => {
            let pool_id = Hash256::from_hex(parts.next()?).ok()?;
            let owner = parts.next()?.trim().to_string();
            Address::parse_with_prefix(&owner, &settings.network.address_prefix).ok()?;
            let display_name = verified_governance_decode_hex_text(
                parts.next()?,
                settings.verified_governance.max_display_name_bytes,
                "display name",
            )
            .ok()?;
            let avatar_hash = parts.next()?.trim().to_string();
            let avatar_mime = parts.next()?.trim().to_string();
            let avatar_size_bytes = parts.next()?.parse::<u32>().ok()?;
            let avatar_ref = verified_governance_decode_hex_text(
                parts.next()?,
                settings.verified_governance.max_avatar_ref_bytes,
                "avatar reference",
            )
            .ok()?;
            let bond_units = parts.next()?.parse::<u128>().ok()?;
            let lock_until_height = parts.next()?.parse::<u32>().ok()?;
            VerifiedGovernanceMarker::PoolApply {
                pool_id,
                owner,
                display_name,
                avatar_hash,
                avatar_mime,
                avatar_size_bytes,
                avatar_ref,
                bond_units,
                lock_until_height,
            }
        }
        "report" => {
            let reporter = parts.next()?.trim().to_string();
            Address::parse_with_prefix(&reporter, &settings.network.address_prefix).ok()?;
            let target_kind = verified_governance_clean_ascii(parts.next()?, 32, "target kind")
                .ok()?
                .to_ascii_lowercase();
            let target_id = verified_governance_clean_ascii(parts.next()?, 96, "target id").ok()?;
            let category =
                verified_governance_clean_ascii(parts.next()?, 48, "report category").ok()?;
            let severity = verified_governance_clean_ascii(parts.next()?, 16, "report severity")
                .ok()?
                .to_ascii_lowercase();
            let evidence_hash = parts.next()?.trim().to_string();
            let evidence_ref = verified_governance_decode_hex_text(
                parts.next()?,
                settings.verified_governance.max_evidence_ref_bytes,
                "evidence reference",
            )
            .ok()?;
            let bond_units = parts.next()?.parse::<u128>().ok()?;
            VerifiedGovernanceMarker::Report {
                reporter,
                target_kind,
                target_id,
                category,
                severity,
                evidence_hash,
                evidence_ref,
                bond_units,
            }
        }
        "mod_apply" => {
            let candidate = parts.next()?.trim().to_string();
            Address::parse_with_prefix(&candidate, &settings.network.address_prefix).ok()?;
            let statement_hash = parts.next()?.trim().to_string();
            let statement_ref = verified_governance_decode_hex_text(
                parts.next()?,
                settings.verified_governance.max_evidence_ref_bytes,
                "statement reference",
            )
            .ok()?;
            let bond_units = parts.next()?.parse::<u128>().ok()?;
            let lock_until_height = parts.next()?.parse::<u32>().ok()?;
            VerifiedGovernanceMarker::ModeratorApply {
                candidate,
                statement_hash,
                statement_ref,
                bond_units,
                lock_until_height,
            }
        }
        "vote" => {
            let voter = parts.next()?.trim().to_string();
            Address::parse_with_prefix(&voter, &settings.network.address_prefix).ok()?;
            let proposal_kind = verified_governance_clean_ascii(parts.next()?, 32, "proposal kind")
                .ok()?
                .to_ascii_lowercase();
            let proposal_id =
                verified_governance_clean_ascii(parts.next()?, 96, "proposal id").ok()?;
            let support_raw = parts.next()?.trim();
            let support = match support_raw {
                "yes" | "true" | "1" => true,
                "no" | "false" | "0" => false,
                _ => return None,
            };
            let jin_units = parts.next()?.parse::<u128>().ok()?;
            let conviction = parts.next()?.parse::<u8>().ok()?;
            let lock_until_height = parts.next()?.parse::<u32>().ok()?;
            VerifiedGovernanceMarker::Vote {
                voter,
                proposal_kind,
                proposal_id,
                support,
                jin_units,
                conviction,
                lock_until_height,
            }
        }
        "decision" => {
            let moderator = parts.next()?.trim().to_string();
            Address::parse_with_prefix(&moderator, &settings.network.address_prefix).ok()?;
            let report_id = verified_governance_clean_ascii(parts.next()?, 96, "report id").ok()?;
            let decision = verified_governance_clean_ascii(parts.next()?, 32, "decision")
                .ok()?
                .to_ascii_lowercase();
            let slash_bps = parts.next()?.parse::<u16>().ok()?;
            VerifiedGovernanceMarker::Decision {
                moderator,
                report_id,
                decision,
                slash_bps,
            }
        }
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(marker)
}

pub fn verified_governance_markers_in_tx(
    tx: &Transaction,
    settings: &Settings,
) -> Vec<(usize, VerifiedGovernanceMarker, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_verified_governance_marker_script(&out.script_pubkey, settings)
                .map(|m| (idx, m, out.value.atoms()))
        })
        .collect()
}

fn conviction_multiplier(conviction: u8) -> Result<u128> {
    Ok(match conviction {
        0 => 1,
        1 => 2,
        2 => 3,
        3 => 4,
        4 => 6,
        _ => bail!("conviction must be 0..4"),
    })
}

fn verified_governance_active_locks_from_state(
    state: &VerifiedGovernanceState,
    visible_height: u32,
) -> HashMap<String, u128> {
    let mut out = HashMap::new();
    for lock in &state.locks {
        if lock.lock_until_height > visible_height {
            let entry = out.entry(lock.owner.clone()).or_insert(0u128);
            *entry = entry.saturating_add(lock.units);
        }
    }
    out
}

fn apply_verified_governance_locks_to_jin_ledger(
    ledger: &mut HashMap<String, u128>,
    settings: &Settings,
    blocks: &[Block],
) -> Result<()> {
    if !settings.verified_governance.enabled {
        return Ok(());
    }
    let visible_height = blocks.len().saturating_sub(1) as u32;
    if visible_height < settings.verified_governance.activation_height {
        return Ok(());
    }
    let state = verified_governance_state_from_blocks_without_jin_lock_feedback(settings, blocks)?;
    for (owner, units) in verified_governance_active_locks_from_state(&state, visible_height) {
        let cur = ledger.get(&owner).copied().unwrap_or(0);
        if cur < units {
            bail!("verified governance lock exceeds JIN balance for {owner}");
        }
        ledger.insert(owner, cur - units);
    }
    Ok(())
}

fn verified_governance_available_jin(ledger: &HashMap<String, u128>, owner: &str) -> u128 {
    ledger.get(owner).copied().unwrap_or(0)
}

fn require_verified_governance_lock(
    ledger: &mut HashMap<String, u128>,
    state: &mut VerifiedGovernanceState,
    owner: &str,
    units: u128,
    lock_until_height: u32,
    height: u32,
    reason: &str,
    settings: &Settings,
) -> Result<()> {
    if units == 0 {
        bail!("JIN lock must be non-zero");
    }
    if lock_until_height < height.saturating_add(settings.verified_governance.min_lock_blocks) {
        bail!(
            "JIN lock must last at least {} blocks",
            settings.verified_governance.min_lock_blocks
        );
    }
    let available = verified_governance_available_jin(ledger, owner);
    if available < units {
        bail!(
            "insufficient unlocked JIN for verified governance lock: required {}, available {}",
            units,
            available
        );
    }
    ledger.insert(owner.to_string(), available - units);
    state.locks.push(VerifiedJinLock {
        owner: owner.to_string(),
        units,
        lock_until_height,
        reason: reason.to_string(),
    });
    Ok(())
}

fn verified_governance_refresh_moderators(
    state: &mut VerifiedGovernanceState,
    settings: &Settings,
    height: u32,
) -> Result<()> {
    let threshold =
        verified_governance_bond_units(&settings.verified_governance.moderator_bond_units)?
            .saturating_mul(2);
    let mut support: HashMap<String, (u128, u32)> = HashMap::new();
    for key in &state.votes {
        let parts = key.split('|').collect::<Vec<_>>();
        if parts.len() != 5 {
            continue;
        }
        if parts[0] == "moderator" && parts[4] == "yes" {
            let power = parts[3].parse::<u128>().unwrap_or(0);
            let entry = support.entry(parts[1].to_string()).or_insert((0, 0));
            entry.0 = entry.0.saturating_add(power);
            entry.1 = entry.1.saturating_add(1);
        }
    }
    for (candidate, moderator) in state.moderators.iter_mut() {
        let (power, voters) = support.get(candidate).copied().unwrap_or((0, 0));
        moderator.support_power_units = power;
        moderator.support_voters = voters;
        if moderator.status == VerifiedStatus::Pending
            && moderator.lock_until_height > height
            && voters >= 2
            && power >= threshold
        {
            moderator.status = VerifiedStatus::Active;
            moderator.elected_height = Some(height);
        }
        if moderator.lock_until_height <= height && moderator.status != VerifiedStatus::Revoked {
            moderator.status = VerifiedStatus::Expired;
        }
    }
    Ok(())
}

fn validate_verified_governance_marker_with_state(
    tx: &Transaction,
    marker: &VerifiedGovernanceMarker,
    state: &mut VerifiedGovernanceState,
    ledger: &mut HashMap<String, u128>,
    base_utxos: &HashMap<OutPoint, CoinRecord>,
    prior_blocks: &[Block],
    height: u32,
    settings: &Settings,
    txid: Hash256,
    marker_index: usize,
) -> Result<()> {
    match marker {
        VerifiedGovernanceMarker::WalletApply {
            address,
            display_name,
            avatar_hash,
            avatar_mime,
            avatar_size_bytes,
            avatar_ref,
            bond_units,
            lock_until_height,
        } => {
            let addr = Address::parse_with_prefix(address, &settings.network.address_prefix)?;
            if !input_authorizes_address(tx, base_utxos, &addr) {
                bail!("verified wallet application must be authorized by a QUB input from that wallet");
            }
            if *bond_units
                < verified_governance_bond_units(&settings.verified_governance.wallet_bond_units)?
            {
                bail!("wallet verification bond below minimum");
            }
            let avatar = verified_avatar_meta(
                avatar_hash,
                avatar_mime,
                *avatar_size_bytes,
                avatar_ref,
                settings,
            )?;
            if state
                .wallets
                .get(address)
                .map(|p| p.status != VerifiedStatus::Revoked && p.status != VerifiedStatus::Expired)
                .unwrap_or(false)
            {
                bail!("wallet already has active/pending verification");
            }
            require_verified_governance_lock(
                ledger,
                state,
                address,
                *bond_units,
                *lock_until_height,
                height,
                "wallet_verification",
                settings,
            )?;
            state.wallets.insert(
                address.clone(),
                VerifiedWalletProfile {
                    address: address.clone(),
                    status: VerifiedStatus::Pending,
                    display_name: display_name.clone(),
                    avatar,
                    locked_jin_units: *bond_units,
                    lock_until_height: *lock_until_height,
                    verified_since_height: 0,
                    last_review_height: height,
                    strikes: 0,
                },
            );
        }
        VerifiedGovernanceMarker::PoolApply {
            pool_id,
            owner,
            display_name,
            avatar_hash,
            avatar_mime,
            avatar_size_bytes,
            avatar_ref,
            bond_units,
            lock_until_height,
        } => {
            let owner_addr = Address::parse_with_prefix(owner, &settings.network.address_prefix)?;
            if !input_authorizes_address(tx, base_utxos, &owner_addr) {
                bail!("verified pool application must be authorized by a QUB input from the pool owner");
            }
            let pools = pools_registry_from_blocks(settings, prior_blocks)?;
            let pool = pools
                .get(pool_id)
                .context("verified pool application target pool not found")?;
            if pool.manager_address != *owner {
                bail!("verified pool application owner must match pool manager");
            }
            if *bond_units
                < verified_governance_bond_units(&settings.verified_governance.pool_bond_units)?
            {
                bail!("pool verification bond below minimum");
            }
            let avatar = verified_avatar_meta(
                avatar_hash,
                avatar_mime,
                *avatar_size_bytes,
                avatar_ref,
                settings,
            )?;
            if state
                .pools
                .get(pool_id)
                .map(|p| p.status != VerifiedStatus::Revoked && p.status != VerifiedStatus::Expired)
                .unwrap_or(false)
            {
                bail!("pool already has active/pending verification");
            }
            require_verified_governance_lock(
                ledger,
                state,
                owner,
                *bond_units,
                *lock_until_height,
                height,
                "pool_verification",
                settings,
            )?;
            state.pools.insert(
                *pool_id,
                VerifiedPoolProfile {
                    pool_id: *pool_id,
                    owner_address: owner.clone(),
                    status: VerifiedStatus::Pending,
                    display_name: display_name.clone(),
                    avatar,
                    locked_jin_units: *bond_units,
                    lock_until_height: *lock_until_height,
                    verified_since_height: 0,
                    last_review_height: height,
                    strikes: 0,
                },
            );
        }
        VerifiedGovernanceMarker::Report {
            reporter,
            target_kind,
            target_id,
            category,
            severity,
            evidence_hash,
            evidence_ref,
            bond_units,
        } => {
            let reporter_addr =
                Address::parse_with_prefix(reporter, &settings.network.address_prefix)?;
            if !input_authorizes_address(tx, base_utxos, &reporter_addr) {
                bail!("report must be authorized by reporter-owned QUB input");
            }
            if state.reports.len() >= settings.verified_governance.max_active_reports {
                bail!("too many open verified-governance reports");
            }
            if *bond_units
                < verified_governance_bond_units(&settings.verified_governance.report_bond_units)?
            {
                bail!("report bond below minimum");
            }
            verified_governance_hash_hex(evidence_hash, "evidence")?;
            require_verified_governance_lock(
                ledger,
                state,
                reporter,
                *bond_units,
                height.saturating_add(settings.verified_governance.appeal_window_blocks),
                height,
                "report_bond",
                settings,
            )?;
            let report_id = format!("{}:{}", txid, marker_index);
            state.reports.insert(
                report_id.clone(),
                ReportCase {
                    report_id,
                    target_kind: target_kind.clone(),
                    target_id: target_id.clone(),
                    reporter: reporter.clone(),
                    category: category.clone(),
                    severity: severity.clone(),
                    evidence_hash: evidence_hash.to_ascii_lowercase(),
                    evidence_ref: evidence_ref.clone(),
                    report_bond_units: *bond_units,
                    status: "open".to_string(),
                    opened_height: height,
                    decision_height: None,
                },
            );
        }
        VerifiedGovernanceMarker::ModeratorApply {
            candidate,
            statement_hash,
            statement_ref,
            bond_units,
            lock_until_height,
        } => {
            let candidate_addr =
                Address::parse_with_prefix(candidate, &settings.network.address_prefix)?;
            if !input_authorizes_address(tx, base_utxos, &candidate_addr) {
                bail!("moderator application must be authorized by candidate-owned QUB input");
            }
            if *bond_units
                < verified_governance_bond_units(
                    &settings.verified_governance.moderator_bond_units,
                )?
            {
                bail!("moderator bond below minimum");
            }
            verified_governance_hash_hex(statement_hash, "statement")?;
            if state
                .moderators
                .get(candidate)
                .map(|m| m.status != VerifiedStatus::Revoked && m.status != VerifiedStatus::Expired)
                .unwrap_or(false)
            {
                bail!("moderator candidate already active/pending");
            }
            require_verified_governance_lock(
                ledger,
                state,
                candidate,
                *bond_units,
                *lock_until_height,
                height,
                "moderator_candidate",
                settings,
            )?;
            state.moderators.insert(
                candidate.clone(),
                ModeratorProfile {
                    address: candidate.clone(),
                    status: VerifiedStatus::Pending,
                    statement_hash: statement_hash.to_ascii_lowercase(),
                    statement_ref: statement_ref.clone(),
                    locked_jin_units: *bond_units,
                    lock_until_height: *lock_until_height,
                    elected_height: None,
                    support_power_units: 0,
                    support_voters: 0,
                },
            );
        }
        VerifiedGovernanceMarker::Vote {
            voter,
            proposal_kind,
            proposal_id,
            support,
            jin_units,
            conviction,
            lock_until_height,
        } => {
            let voter_addr = Address::parse_with_prefix(voter, &settings.network.address_prefix)?;
            if !input_authorizes_address(tx, base_utxos, &voter_addr) {
                bail!("governance vote must be authorized by voter-owned QUB input");
            }
            if *jin_units == 0 {
                bail!("vote lock must be non-zero");
            }
            if proposal_kind == "moderator" && !state.moderators.contains_key(proposal_id) {
                bail!("moderator vote target is not a candidate");
            }
            let power = jin_units
                .checked_mul(conviction_multiplier(*conviction)?)
                .context("vote power overflow")?;
            require_verified_governance_lock(
                ledger,
                state,
                voter,
                *jin_units,
                *lock_until_height,
                height,
                "governance_vote",
                settings,
            )?;
            let dup_prefix = format!("{}|{}|{}|", proposal_kind, proposal_id, voter);
            if state.votes.iter().any(|k| k.starts_with(&dup_prefix)) {
                bail!("duplicate governance vote");
            }
            let key = format!(
                "{}|{}|{}|{}|{}",
                proposal_kind,
                proposal_id,
                voter,
                power,
                if *support { "yes" } else { "no" }
            );
            if !state.votes.insert(key) {
                bail!("duplicate governance vote");
            }
            verified_governance_refresh_moderators(state, settings, height)?;
        }
        VerifiedGovernanceMarker::Decision {
            moderator,
            report_id,
            decision,
            slash_bps,
        } => {
            let mod_addr = Address::parse_with_prefix(moderator, &settings.network.address_prefix)?;
            if !input_authorizes_address(tx, base_utxos, &mod_addr) {
                bail!("report decision must be authorized by moderator-owned QUB input");
            }
            let Some(mod_profile) = state.moderators.get(moderator) else {
                bail!("unknown moderator");
            };
            if mod_profile.status != VerifiedStatus::Active
                || mod_profile.lock_until_height <= height
            {
                bail!("moderator is not active");
            }
            if *slash_bps > settings.verified_governance.max_initial_slash_bps {
                bail!(
                    "slash exceeds HF101 maximum {} bps",
                    settings.verified_governance.max_initial_slash_bps
                );
            }
            let Some(report) = state.reports.get_mut(report_id) else {
                bail!("unknown report id");
            };
            if report.status != "open" && report.status != "under_review" {
                bail!("report already decided");
            }
            match decision.as_str() {
                "reject" => {
                    report.status = "rejected".to_string();
                    report.decision_height = Some(height);
                }
                "warning" | "minor" | "major" | "suspend" | "revoke" => {
                    report.status = decision.clone();
                    report.decision_height = Some(height);
                    if *slash_bps > 0 {
                        let key = format!("pending:{}", report_id);
                        state.pending_slash_units.insert(key, *slash_bps as u128);
                    }
                }
                _ => bail!("unsupported decision"),
            }
        }
    }
    Ok(())
}

fn validate_verified_governance_transaction_with_state(
    tx: &Transaction,
    state: &mut VerifiedGovernanceState,
    ledger: &mut HashMap<String, u128>,
    base_utxos: &HashMap<OutPoint, CoinRecord>,
    prior_blocks: &[Block],
    height: u32,
    settings: &Settings,
) -> Result<()> {
    let markers = verified_governance_markers_in_tx(tx, settings);
    if markers.is_empty() {
        return Ok(());
    }
    if !verified_governance_active(settings, height) {
        bail!(
            "Verified Governance v1 activates at block #{}",
            settings.verified_governance.activation_height
        );
    }
    if tx.is_coinbase() {
        bail!("coinbase cannot carry verified governance markers");
    }
    if markers.len() != 1 {
        bail!("a transaction may contain exactly one verified governance action");
    }
    let (idx, marker, atoms) = &markers[0];
    if *atoms != settings.verified_governance.marker_output_atoms {
        bail!(
            "Verified Governance marker output must be exactly {} atom(s)",
            settings.verified_governance.marker_output_atoms
        );
    }
    validate_verified_governance_marker_with_state(
        tx,
        marker,
        state,
        ledger,
        base_utxos,
        prior_blocks,
        height,
        settings,
        tx.txid(),
        *idx,
    )
}

fn validate_verified_governance_transaction_against_chain(
    tx: &Transaction,
    chain: &ChainState,
    spend_height: u32,
    settings: &Settings,
) -> Result<()> {
    let mut state = verified_governance_state_from_blocks(settings, &chain.blocks)?;
    let mut ledger = jin_ledger_from_blocks(settings, &chain.blocks)?;
    validate_verified_governance_transaction_with_state(
        tx,
        &mut state,
        &mut ledger,
        &chain.utxos,
        &chain.blocks,
        spend_height,
        settings,
    )
}

fn validate_verified_governance_block(
    block: &Block,
    prior_blocks: &[Block],
    base_utxos: &HashMap<OutPoint, CoinRecord>,
    height: u32,
    settings: &Settings,
) -> Result<()> {
    let mut state = verified_governance_state_from_blocks(settings, prior_blocks)?;
    let mut ledger = jin_ledger_from_blocks(settings, prior_blocks)?;
    let mut scratch = base_utxos.clone();
    let pool_block = parse_pool_block_marker(block).is_some();
    let miner = if pool_block {
        None
    } else {
        coinbase_miner_address(settings, block)
    };
    for (tx_index, tx) in block.transactions.iter().enumerate().skip(1) {
        validate_tx_contextual(tx, &scratch, height, settings, true)?;
        let _ = validate_jin_transaction_with_ledger(
            tx,
            &mut ledger,
            &scratch,
            height,
            settings,
            miner.as_deref(),
        )?;
        validate_verified_governance_transaction_with_state(
            tx,
            &mut state,
            &mut ledger,
            &scratch,
            prior_blocks,
            height,
            settings,
        )?;
        let _ = tx_index;
        connect_tx_utxos(tx, &mut scratch, height, false)?;
    }
    Ok(())
}

fn apply_verified_governance_marker_unchecked(
    state: &mut VerifiedGovernanceState,
    marker: VerifiedGovernanceMarker,
    txid: Hash256,
    marker_index: usize,
    height: u32,
    settings: &Settings,
) -> Result<()> {
    match marker {
        VerifiedGovernanceMarker::WalletApply {
            address,
            display_name,
            avatar_hash,
            avatar_mime,
            avatar_size_bytes,
            avatar_ref,
            bond_units,
            lock_until_height,
        } => {
            let avatar = verified_avatar_meta(
                &avatar_hash,
                &avatar_mime,
                avatar_size_bytes,
                &avatar_ref,
                settings,
            )?;
            state.locks.push(VerifiedJinLock {
                owner: address.clone(),
                units: bond_units,
                lock_until_height,
                reason: "wallet_verification".to_string(),
            });
            state.wallets.insert(
                address.clone(),
                VerifiedWalletProfile {
                    address,
                    status: VerifiedStatus::Pending,
                    display_name,
                    avatar,
                    locked_jin_units: bond_units,
                    lock_until_height,
                    verified_since_height: 0,
                    last_review_height: height,
                    strikes: 0,
                },
            );
        }
        VerifiedGovernanceMarker::PoolApply {
            pool_id,
            owner,
            display_name,
            avatar_hash,
            avatar_mime,
            avatar_size_bytes,
            avatar_ref,
            bond_units,
            lock_until_height,
        } => {
            let avatar = verified_avatar_meta(
                &avatar_hash,
                &avatar_mime,
                avatar_size_bytes,
                &avatar_ref,
                settings,
            )?;
            state.locks.push(VerifiedJinLock {
                owner: owner.clone(),
                units: bond_units,
                lock_until_height,
                reason: "pool_verification".to_string(),
            });
            state.pools.insert(
                pool_id,
                VerifiedPoolProfile {
                    pool_id,
                    owner_address: owner,
                    status: VerifiedStatus::Pending,
                    display_name,
                    avatar,
                    locked_jin_units: bond_units,
                    lock_until_height,
                    verified_since_height: 0,
                    last_review_height: height,
                    strikes: 0,
                },
            );
        }
        VerifiedGovernanceMarker::Report {
            reporter,
            target_kind,
            target_id,
            category,
            severity,
            evidence_hash,
            evidence_ref,
            bond_units,
        } => {
            state.locks.push(VerifiedJinLock {
                owner: reporter.clone(),
                units: bond_units,
                lock_until_height: height
                    .saturating_add(settings.verified_governance.appeal_window_blocks),
                reason: "report_bond".to_string(),
            });
            let report_id = format!("{}:{}", txid, marker_index);
            state.reports.insert(
                report_id.clone(),
                ReportCase {
                    report_id,
                    target_kind,
                    target_id,
                    reporter,
                    category,
                    severity,
                    evidence_hash: evidence_hash.to_ascii_lowercase(),
                    evidence_ref,
                    report_bond_units: bond_units,
                    status: "open".to_string(),
                    opened_height: height,
                    decision_height: None,
                },
            );
        }
        VerifiedGovernanceMarker::ModeratorApply {
            candidate,
            statement_hash,
            statement_ref,
            bond_units,
            lock_until_height,
        } => {
            state.locks.push(VerifiedJinLock {
                owner: candidate.clone(),
                units: bond_units,
                lock_until_height,
                reason: "moderator_candidate".to_string(),
            });
            state.moderators.insert(
                candidate.clone(),
                ModeratorProfile {
                    address: candidate,
                    status: VerifiedStatus::Pending,
                    statement_hash: statement_hash.to_ascii_lowercase(),
                    statement_ref,
                    locked_jin_units: bond_units,
                    lock_until_height,
                    elected_height: None,
                    support_power_units: 0,
                    support_voters: 0,
                },
            );
        }
        VerifiedGovernanceMarker::Vote {
            voter,
            proposal_kind,
            proposal_id,
            support,
            jin_units,
            conviction,
            lock_until_height,
        } => {
            state.locks.push(VerifiedJinLock {
                owner: voter.clone(),
                units: jin_units,
                lock_until_height,
                reason: "governance_vote".to_string(),
            });
            let power = jin_units.saturating_mul(conviction_multiplier(conviction).unwrap_or(1));
            let key = format!(
                "{}|{}|{}|{}|{}",
                proposal_kind,
                proposal_id,
                voter,
                power,
                if support { "yes" } else { "no" }
            );
            state.votes.insert(key);
            verified_governance_refresh_moderators(state, settings, height)?;
        }
        VerifiedGovernanceMarker::Decision {
            moderator: _,
            report_id,
            decision,
            slash_bps,
        } => {
            if let Some(report) = state.reports.get_mut(&report_id) {
                report.status = decision;
                report.decision_height = Some(height);
                if slash_bps > 0 {
                    state
                        .pending_slash_units
                        .insert(format!("pending:{}", report_id), slash_bps as u128);
                }
            }
        }
    }
    Ok(())
}

fn verified_governance_state_from_blocks_without_jin_lock_feedback(
    settings: &Settings,
    blocks: &[Block],
) -> Result<VerifiedGovernanceState> {
    let mut state = VerifiedGovernanceState::default();
    if !settings.verified_governance.enabled {
        return Ok(state);
    }
    for (height, block) in blocks.iter().enumerate().skip(1) {
        let height = height as u32;
        if !verified_governance_active(settings, height) {
            continue;
        }
        for tx in block.transactions.iter().skip(1) {
            for (idx, marker, atoms) in verified_governance_markers_in_tx(tx, settings) {
                if atoms != settings.verified_governance.marker_output_atoms {
                    bail!(
                        "Verified Governance marker output must be exactly {} atom(s)",
                        settings.verified_governance.marker_output_atoms
                    );
                }
                apply_verified_governance_marker_unchecked(
                    &mut state,
                    marker,
                    tx.txid(),
                    idx,
                    height,
                    settings,
                )?;
            }
        }
    }
    Ok(state)
}

pub fn verified_governance_state_from_blocks(
    settings: &Settings,
    blocks: &[Block],
) -> Result<VerifiedGovernanceState> {
    verified_governance_state_from_blocks_without_jin_lock_feedback(settings, blocks)
}

pub fn verified_wallet_profile_for_address(
    settings: &Settings,
    chain: &ChainState,
    address: &str,
) -> Result<Option<VerifiedWalletProfile>> {
    Address::parse_with_prefix(address, &settings.network.address_prefix)?;
    Ok(
        verified_governance_state_from_blocks(settings, &chain.blocks)?
            .wallets
            .get(address)
            .cloned(),
    )
}

pub fn verified_pool_profile_for_pool(
    settings: &Settings,
    chain: &ChainState,
    pool_id: Hash256,
) -> Result<Option<VerifiedPoolProfile>> {
    Ok(
        verified_governance_state_from_blocks(settings, &chain.blocks)?
            .pools
            .get(&pool_id)
            .cloned(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlastVault {
    pub asset: String,
    pub manager_address: String,
    pub code_hash: String,
    pub per_claim_units: u128,
    pub remaining_claims: u32,
}

pub fn blast_activation_height(settings: &Settings) -> u32 {
    match settings.network.name.as_str() {
        "mainnet" => MAINNET_BLAST_ACTIVATION_HEIGHT,
        "testnet" => TESTNET_BLAST_ACTIVATION_HEIGHT,
        _ => 1,
    }
}

pub fn blast_active(settings: &Settings, height: u32) -> bool {
    height >= blast_activation_height(settings)
}

pub fn blast_code_hash_hex(code: &str) -> Result<String> {
    let code = code.trim();
    if code.len() < 12 || code.len() > 128 {
        bail!("Blast code must be 12..128 characters");
    }
    if !code
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
    {
        bail!("Blast code may contain only letters, digits, '-' and '_'");
    }
    Ok(hex::encode(Sha256::digest(code.as_bytes())))
}

pub fn blast_vault_script(
    asset: &str,
    manager: &Address,
    code_hash: &str,
    per_claim_units: u128,
    remaining_claims: u32,
) -> Result<ScriptBuf> {
    let asset = asset.trim().to_ascii_uppercase();
    if asset != "QUB" && asset != "JIN" {
        bail!("Blast asset must be QUB or JIN");
    }
    if code_hash.len() != 64 || !code_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid Blast code hash");
    }
    if per_claim_units == 0 {
        bail!("Blast per-claim amount must be non-zero");
    }
    if remaining_claims == 0 || remaining_claims as usize > MAX_SEND_ENTRIES_PER_TX {
        bail!("Blast remaining claims must be 1..256");
    }
    Ok(ScriptBuf(
        format!(
            "BLAST1|vault|{}|{}|{}|{}|{}",
            asset,
            manager,
            code_hash.to_ascii_lowercase(),
            per_claim_units,
            remaining_claims
        )
        .into_bytes(),
    ))
}

pub fn parse_blast_vault_script(script: &ScriptBuf, settings: &Settings) -> Option<BlastVault> {
    let b = script.as_bytes();
    if !b.starts_with(BLAST_SCRIPT_PREFIX) {
        return None;
    }
    let s = std::str::from_utf8(b).ok()?;
    let mut parts = s.split('|');
    if parts.next()? != "BLAST1" {
        return None;
    }
    if parts.next()? != "vault" {
        return None;
    }
    let asset = parts.next()?.trim().to_ascii_uppercase();
    if asset != "QUB" && asset != "JIN" {
        return None;
    }
    let manager_address = parts.next()?.trim().to_string();
    Address::parse_with_prefix(&manager_address, &settings.network.address_prefix).ok()?;
    let code_hash = parts.next()?.trim().to_ascii_lowercase();
    if code_hash.len() != 64 || !code_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let per_claim_units = parts.next()?.parse::<u128>().ok()?;
    let remaining_claims = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() || per_claim_units == 0 || remaining_claims == 0 {
        return None;
    }
    Some(BlastVault {
        asset,
        manager_address,
        code_hash,
        per_claim_units,
        remaining_claims,
    })
}

pub fn blast_claim_script(claimant: &Address, code: &str) -> Result<ScriptBuf> {
    // This reveals the code on-chain. For public blast campaigns the creator
    // should generate one-time codes. A single reusable public code cannot be
    // private after the first claim on any transparent chain.
    blast_code_hash_hex(code)?;
    Ok(ScriptBuf(
        format!("BLAST1|claim|{}|{}", claimant, code.trim()).into_bytes(),
    ))
}

fn parse_blast_claim_script(script: &ScriptBuf, settings: &Settings) -> Option<(Address, String)> {
    let b = script.as_bytes();
    if !b.starts_with(BLAST_SCRIPT_PREFIX) {
        return None;
    }
    let s = std::str::from_utf8(b).ok()?;
    let mut parts = s.split('|');
    if parts.next()? != "BLAST1" {
        return None;
    }
    if parts.next()? != "claim" {
        return None;
    }
    let claimant_s = parts.next()?.trim();
    let claimant = Address::parse_with_prefix(claimant_s, &settings.network.address_prefix).ok()?;
    let code = parts.next()?.trim().to_string();
    if parts.next().is_some() {
        return None;
    }
    blast_code_hash_hex(&code).ok()?;
    Some((claimant, code))
}

pub fn make_blast_code_payload(txid: Hash256, vout: u32, code: &str) -> Result<String> {
    blast_code_hash_hex(code)?;
    Ok(format!("QUBBLAST1|{}|{}|{}", txid, vout, code.trim()))
}

pub fn parse_blast_code_payload(payload: &str) -> Result<(Hash256, u32, String)> {
    let mut parts = payload.trim().split('|');
    if parts.next().unwrap_or("") != "QUBBLAST1" {
        bail!("Blast code must start with QUBBLAST1");
    }
    let txid = Hash256::from_hex(parts.next().context("Blast code missing txid")?)?;
    let vout = parts
        .next()
        .context("Blast code missing vout")?
        .parse::<u32>()?;
    let code = parts
        .next()
        .context("Blast code missing private code")?
        .to_string();
    if parts.next().is_some() {
        bail!("Blast code has extra fields");
    }
    blast_code_hash_hex(&code)?;
    Ok((txid, vout, code))
}

pub fn is_blast_claim_transaction(tx: &Transaction, settings: &Settings) -> bool {
    tx.inputs.len() == 1
        && parse_blast_claim_script(&tx.inputs[0].signature_script, settings).is_some()
}

pub fn blast_creates_in_tx(tx: &Transaction, settings: &Settings) -> Vec<(usize, BlastVault, u64)> {
    tx.outputs
        .iter()
        .enumerate()
        .filter_map(|(idx, out)| {
            parse_blast_vault_script(&out.script_pubkey, settings)
                .map(|b| (idx, b, out.value.atoms()))
        })
        .collect()
}

fn validate_blast_create_outputs(
    tx: &Transaction,
    spend_height: u32,
    settings: &Settings,
) -> Result<()> {
    let creates = blast_creates_in_tx(tx, settings);
    if creates.is_empty() {
        return Ok(());
    }
    if !blast_active(settings, spend_height) {
        bail!(
            "Blast activates at block #{}",
            blast_activation_height(settings)
        );
    }
    if creates.len() > 1 {
        bail!("only one Blast vault output per tx is allowed in v1.4.8");
    }
    for (_, vault, atoms) in creates {
        if vault.remaining_claims as usize > MAX_SEND_ENTRIES_PER_TX {
            bail!("Blast entries exceed 256");
        }
        if vault.asset == "QUB" {
            let expected = u64::try_from(vault.per_claim_units)
                .context("Blast per-claim amount too large")?
                .checked_mul(vault.remaining_claims as u64)
                .context("Blast total overflow")?;
            if atoms != expected {
                bail!("Blast QUB vault output must equal per_claim * claims");
            }
        }
    }
    Ok(())
}

fn validate_blast_claim_contextual(
    tx: &Transaction,
    utxos: &HashMap<OutPoint, CoinRecord>,
    spend_height: u32,
    settings: &Settings,
) -> Result<u64> {
    if !blast_active(settings, spend_height) {
        bail!(
            "Blast activates at block #{}",
            blast_activation_height(settings)
        );
    }
    if tx.inputs.len() != 1 {
        bail!("Blast claim must spend exactly one vault input");
    }
    if tx.outputs.len() > 2 || tx.outputs.is_empty() {
        bail!("Blast claim must have claimant output and optional remaining vault output");
    }
    let (claimant, code) = parse_blast_claim_script(&tx.inputs[0].signature_script, settings)
        .context("invalid Blast claim script")?;
    let coin = utxos
        .get(&tx.inputs[0].previous_output)
        .context("missing Blast vault input")?;
    let vault = parse_blast_vault_script(&coin.tx_out.script_pubkey, settings)
        .context("Blast claim input is not a vault")?;
    if vault.asset != "QUB" {
        bail!("only QUB Blast claims are supported in v1.4.8");
    }
    if blast_code_hash_hex(&code)? != vault.code_hash {
        bail!("invalid Blast code");
    }
    let per_claim_atoms =
        u64::try_from(vault.per_claim_units).context("Blast claim amount too large")?;
    if coin.tx_out.value.atoms() < per_claim_atoms {
        bail!("Blast vault insufficient value");
    }
    if tx.outputs[0].value.atoms() != per_claim_atoms {
        bail!("Blast claimant output amount mismatch");
    }
    if tx.outputs[0].script_pubkey != claimant.script_pubkey() {
        bail!("Blast claimant output address mismatch");
    }
    let expected_remaining_value = coin.tx_out.value.atoms() - per_claim_atoms;
    let expected_remaining_claims = vault.remaining_claims.saturating_sub(1);
    if expected_remaining_value == 0 || expected_remaining_claims == 0 {
        if tx.outputs.len() != 1 {
            bail!("Blast fully-claimed tx must not recreate vault");
        }
    } else {
        if tx.outputs.len() != 2 {
            bail!("Blast claim must recreate remaining vault");
        }
        if tx.outputs[1].value.atoms() != expected_remaining_value {
            bail!("Blast remaining vault amount mismatch");
        }
        let manager =
            Address::parse_with_prefix(&vault.manager_address, &settings.network.address_prefix)?;
        let expected_script = blast_vault_script(
            &vault.asset,
            &manager,
            &vault.code_hash,
            vault.per_claim_units,
            expected_remaining_claims,
        )?;
        if tx.outputs[1].script_pubkey != expected_script {
            bail!("Blast remaining vault script mismatch");
        }
    }
    Ok(0)
}

pub fn v1_feature_notice(settings: &Settings) -> String {
    format!("local address mining: create/paste enabled | pooled mining: {} at #{} | JIN native coin: {} at #{} | QNS: {} at #{} | Library: {} at #{} | Verified Governance: {} at #{}", if settings.features.pooled_mining_enabled && settings.pools.enabled { "enabled" } else { "disabled" }, settings.pools.activation_height, if settings.features.jin_native_coin_enabled && settings.jin.enabled { "enabled" } else { "disabled" }, settings.jin.activation_height, if settings.qns.enabled { "enabled" } else { "disabled" }, settings.qns.activation_height, if settings.library.enabled { "enabled" } else { "disabled" }, settings.library.activation_height, if settings.verified_governance.enabled { "enabled" } else { "disabled" }, settings.verified_governance.activation_height)
}
