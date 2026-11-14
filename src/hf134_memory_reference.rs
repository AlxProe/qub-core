//! Frozen R6 reference functions, compiled only in tests.
//! Source lib.rs SHA256: f0d94ad3e0022ed6b9f27db94c3c306a73cd582bacef7ea088456ea2503047db
use super::*;

pub(super) fn connect_block(chain: &mut ChainState, block: Block, settings: &Settings) -> Result<Hash256> {
    let height = chain.height() + 1;
    let required_bits = required_work_bits(settings, &chain.blocks, height)?;
    daa_v3::validate_daa_v3_timestamp(settings, &chain.blocks, height, block.header.time)?;
    let txids = validate_block(
        &block,
        &chain.utxos,
        chain.tip_hash(),
        height,
        required_bits,
        settings,
    )?;
    validate_qns_block(&block, &chain.blocks, height, settings)?;
    validate_jin_block(&block, &chain.blocks, &chain.utxos, height, settings)?;
    validate_jin_sale_block(&block, &chain.blocks, &chain.utxos, height, settings)?;
    validate_pools_block(&block, &chain.blocks, &chain.utxos, height, settings)?;
    validate_library_block(&block, &chain.blocks, &chain.utxos, height, settings)?;
    validate_verified_governance_block(&block, &chain.blocks, &chain.utxos, height, settings)?;
    validate_consensus_checkpoint(settings, height, block.block_hash())?;
    let mut new_utxos = chain.utxos.as_ref().clone();
    connect_block_utxos(&block, &mut new_utxos, height)?;
    let hash = block.block_hash();
    Arc::make_mut(&mut chain.blocks).push(block);
    chain.utxos = Arc::new(new_utxos);
    Arc::make_mut(&mut chain.mempool).retain(|tx| !txids.contains(&tx.txid()));
    // HF76/v1.5.8: a newly-confirmed block can spend inputs or change feature
    // state used by still-pending transactions. Revalidate survivors now so
    // stale local mempool entries do not linger for many blocks and then appear
    // to "revert" from the GUI.
    chain.revalidate_mempool(settings);
    Ok(hash)
}


pub(super) fn validate_block(
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

// The exact R6 transaction stage, isolated for signed intra-block fixtures.
pub(super) fn validate_transactions(
    block: &Block, base_utxos: &HashMap<OutPoint, CoinRecord>,
    height: u32, settings: &Settings,
) -> Result<HashSet<Hash256>> {
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