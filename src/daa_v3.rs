//! HF133 mainnet ASERT-MTP consensus. No wall clock enters target calculation.
//!
//! Integer exponential arithmetic follows the BCHN ASERT specification (CC0):
//! https://upgradespecs.bitcoincashnode.org/2020-11-15-asert/
//! QUB's adapter deliberately differs: height activation, 60-second spacing,
//! 1-hour half-life, and median-of-11 schedule clocks. The final pre-upgrade
//! target is carried into the first upgraded block without another v2 retarget.
//! Using median clocks also bounds the influence of an isolated pre-upgrade
//! backdated header: historical QUB did not require median-time-past.

use crate::{biguint_to_compact, compact_to_biguint, Block, Settings};
use anyhow::{bail, Context, Result};
use num_bigint::BigUint;
use num_traits::{One, Zero};

/// Candidate release height. Public publication must verify sufficient notice.
pub const MAINNET_DAA_V3_ACTIVATION_HEIGHT: u32 = 100_000;
pub const DAA_V3_TARGET_SPACING_SECS: u32 = 60;
pub const DAA_V3_HALF_LIFE_SECS: i64 = 3_600;
pub const DAA_V3_POW_LIMIT_BITS: u32 = 0x1d07_ffff;
pub const DAA_V3_MTP_WINDOW: usize = 11;
pub const PROTOCOL_EPOCH_3_BLOCK_VERSION: u32 = 3;

pub fn daa_v3_active(settings: &Settings, next_height: u32) -> bool {
    settings.network.name == "mainnet" && next_height >= MAINNET_DAA_V3_ACTIVATION_HEIGHT
}

/// Read at most 11 headers, with no chain clone and no cache that can survive a
/// reorganization. `end` is exclusive, and includes the evaluation block.
fn median_time_at(blocks: &[Block], end: usize) -> Result<u32> {
    if end == 0 || end > blocks.len() {
        bail!("missing ASERT median-time history");
    }
    let start = end.saturating_sub(DAA_V3_MTP_WINDOW);
    let mut times = [0u32; DAA_V3_MTP_WINDOW];
    let count = end - start;
    for (slot, block) in times.iter_mut().zip(&blocks[start..end]) {
        *slot = block.header.time;
    }
    times[..count].sort_unstable();
    Ok(times[count / 2])
}

pub fn median_time_past(blocks: &[Block]) -> Result<u32> {
    median_time_at(blocks, blocks.len())
}

fn require_complete_prefix(blocks: &[Block], next_height: u32) -> Result<()> {
    if usize::try_from(next_height).ok() != Some(blocks.len()) {
        bail!("ASERT requires the complete branch prefix through its parent");
    }
    Ok(())
}

/// Generic arithmetic kernel. All divisions and shifts are specified integer
/// operations; even extreme time errors perform only bounded-size shifts.
pub fn asert_target_bits(
    reference_bits: u32,
    time_error: i128,
    half_life: i64,
    pow_limit_bits: u32,
) -> Result<u32> {
    if half_life <= 0 {
        bail!("invalid ASERT half-life");
    }
    let reference = compact_to_biguint(reference_bits)?;
    let pow_limit = compact_to_biguint(pow_limit_bits)?;
    if reference > pow_limit {
        bail!("ASERT anchor target exceeds mainnet proof-of-work limit");
    }
    // Rust signed division truncates toward zero, as required by ASERT.
    let exponent = time_error
        .checked_mul(65_536)
        .context("ASERT exponent overflow")?
        / i128::from(half_life);
    let shifts = exponent >> 16;
    if shifts >= 256 {
        return Ok(biguint_to_compact(&pow_limit));
    }
    if shifts <= -256 {
        return Ok(biguint_to_compact(&BigUint::one()));
    }
    let fraction = (exponent - shifts * 65_536) as u128;
    debug_assert!(fraction < 65_536);
    let factor = 65_536u128
        + ((195_766_423_245_049u128 * fraction
            + 971_821_376u128 * fraction * fraction
            + 5_127u128 * fraction * fraction * fraction
            + (1u128 << 47))
            >> 48);
    let mut result = reference * BigUint::from(factor);
    if shifts < 0 {
        result >>= (-shifts) as usize;
    } else {
        result <<= shifts as usize;
    }
    result >>= 16usize;
    if result.is_zero() {
        result = BigUint::one();
    }
    if result > pow_limit {
        result = pow_limit;
    }
    Ok(biguint_to_compact(&result))
}

pub(crate) fn required_work_bits_v3(blocks: &[Block], next_height: u32) -> Result<u32> {
    require_complete_prefix(blocks, next_height)?;
    let anchor_end = MAINNET_DAA_V3_ACTIVATION_HEIGHT as usize;
    if next_height < MAINNET_DAA_V3_ACTIVATION_HEIGHT || blocks.len() < anchor_end {
        bail!("missing ASERT activation anchor");
    }
    let anchor = &blocks[anchor_end - 1];
    let elapsed = i128::from(median_time_at(blocks, blocks.len())?)
        - i128::from(median_time_at(blocks, anchor_end)?);
    let height_delta = i128::from(next_height - MAINNET_DAA_V3_ACTIVATION_HEIGHT);
    let time_error = elapsed - i128::from(DAA_V3_TARGET_SPACING_SECS) * height_delta;
    asert_target_bits(
        anchor.header.bits,
        time_error,
        DAA_V3_HALF_LIFE_SECS,
        DAA_V3_POW_LIMIT_BITS,
    )
}

pub(crate) fn validate_daa_v3_timestamp(
    settings: &Settings,
    blocks: &[Block],
    height: u32,
    timestamp: u32,
) -> Result<()> {
    if daa_v3_active(settings, height) {
        require_complete_prefix(blocks, height)?;
        if timestamp <= median_time_past(blocks)? {
            bail!("HF133 timestamp must be greater than the median of the previous 11 blocks");
        }
    }
    Ok(())
}

pub(crate) fn candidate_timestamp(
    settings: &Settings,
    blocks: &[Block],
    next_height: u32,
    now: u32,
) -> Result<u32> {
    if !daa_v3_active(settings, next_height) {
        return Ok(now);
    }
    require_complete_prefix(blocks, next_height)?;
    let minimum = median_time_past(blocks)?
        .checked_add(1)
        .context("HF133 median-time overflow")?;
    let result = now.max(minimum);
    if result > now.saturating_add(settings.consensus.max_future_time_secs) {
        bail!("HF133 candidate clock is behind the valid timestamp window");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockHeader, ChainState, Hash256};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn settings() -> Settings {
        let mut settings: Settings = toml::from_str(include_str!("../config/mainnet.toml")).unwrap();
        settings.apply_network_consensus_overrides();
        settings.p2p.enabled = false;
        settings.mining.enabled = false;
        settings
    }

    // In-memory header fixtures only. They are not mined or accepted as a chain.
    fn prefix(length: usize) -> Vec<Block> {
        let block = Block {
            header: BlockHeader {
                version: 2,
                prev_block_hash: Hash256([0; 32]),
                merkle_root: Hash256([0; 32]),
                time: 1_000_000,
                bits: 0x1c10_0000,
                nonce: 0,
            },
            transactions: Vec::new(),
        };
        let mut blocks = vec![block; length];
        for (index, block) in blocks.iter_mut().enumerate() {
            block.header.time = 1_000_000 + (index as u32) * 60;
        }
        blocks
    }

    #[test]
    fn hf133_asert_cross_language_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/hf133-asert-vectors.json"
        )).unwrap();
        let rows = vectors["vectors"].as_array().unwrap();
        assert!(rows.len() >= 4000);
        for row in rows {
            let actual = asert_target_bits(
                row["anchor_bits"].as_u64().unwrap() as u32,
                row["time_error"].as_str().unwrap().parse::<i128>().unwrap(),
                row["half_life"].as_i64().unwrap(),
                row["pow_limit_bits"].as_u64().unwrap() as u32,
            ).unwrap();
            assert_eq!(actual, row["expected_bits"].as_u64().unwrap() as u32, "{}", row);
        }
    }

    #[test]
    fn hf133_asert_exact_powers_and_minimum_target() {
        let bits = 0x1c10_0000;
        let base = compact_to_biguint(bits).unwrap();
        assert_eq!(asert_target_bits(bits, 0, 3600, DAA_V3_POW_LIMIT_BITS).unwrap(), bits);
        for power in -12i128..=6 {
            let n = if power < 0 { &base >> (-power) as usize } else { &base << power as usize };
            assert_eq!(asert_target_bits(bits, power * 3600, 3600, DAA_V3_POW_LIMIT_BITS).unwrap(), biguint_to_compact(&n));
        }
        assert_eq!(asert_target_bits(bits, -10_000_000_000, 3600, DAA_V3_POW_LIMIT_BITS).unwrap(), 0x0101_0000);
        assert_eq!(asert_target_bits(bits, 10_000_000_000, 3600, DAA_V3_POW_LIMIT_BITS).unwrap(), DAA_V3_POW_LIMIT_BITS);
    }

    #[test]
    fn hf133_asert_rejects_invalid_inputs_and_overflow() {
        for bits in [0, 0x1d80ffff, 0x22010000, 0x01000001] {
            assert!(asert_target_bits(bits, 0, 3600, DAA_V3_POW_LIMIT_BITS).is_err());
        }
        assert!(asert_target_bits(0x1e010000, 0, 3600, DAA_V3_POW_LIMIT_BITS).is_err());
        assert!(asert_target_bits(0x1c100000, 0, 0, DAA_V3_POW_LIMIT_BITS).is_err());
        assert!(asert_target_bits(0x1c100000, i128::MAX, 3600, DAA_V3_POW_LIMIT_BITS).is_err());
    }

    #[test]
    fn hf133_real_mainnet_headers_retain_107_legacy_targets() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/hf133-mainnet-headers.json"
        )).unwrap();
        let mut blocks = Vec::new();
        let mut checked = 0;
        for row in rows.as_array().unwrap() {
            let header: BlockHeader = serde_json::from_value(row.clone()).unwrap();
            assert_eq!(header.hash().to_string(), row["hash"].as_str().unwrap());
            if blocks.len() >= 21 {
                let height = row["height"].as_u64().unwrap() as u32;
                assert_eq!(crate::required_work_bits(&settings(), &blocks, height).unwrap(), header.bits);
                checked += 1;
            }
            blocks.push(Block { header, transactions: Vec::new() });
        }
        assert_eq!(checked, 107);
    }

    #[test]
    fn hf133_mainnet_activation_and_block_version_boundary() {
        let settings = settings();
        let h = MAINNET_DAA_V3_ACTIVATION_HEIGHT;
        let blocks = prefix(h as usize);
        // The unpublished r3 height must now retain all legacy rules.
        assert_eq!(h, 100_000);
        let previous_candidate = 80_000u32;
        assert!(!daa_v3_active(&settings, previous_candidate));
        assert_eq!(crate::expected_block_version(&settings, previous_candidate), 2);
        assert_eq!(crate::required_work_bits(&settings, &blocks[..previous_candidate as usize], previous_candidate).unwrap(),
                   crate::required_work_bits_v2(&settings, &blocks[..previous_candidate as usize]).unwrap());
        assert!(validate_daa_v3_timestamp(&settings, &blocks[..previous_candidate as usize], previous_candidate, 0).is_ok());
        assert!(!daa_v3_active(&settings, h - 1));
        assert!(daa_v3_active(&settings, h));
        assert_eq!(crate::expected_block_version(&settings, h - 1), 2);
        assert_eq!(crate::expected_block_version(&settings, h), 3);
        assert_eq!(crate::protocol_epoch_name(&settings, h), "Protocol Epoch 3");
        assert_eq!(crate::required_work_bits(&settings, &blocks[..blocks.len() - 1], h - 1).unwrap(),
                   crate::required_work_bits_v2(&settings, &blocks[..blocks.len() - 1]).unwrap());
        assert_eq!(crate::required_work_bits(&settings, &blocks, h).unwrap(), blocks.last().unwrap().header.bits);
    }

    #[test]
    fn hf133_activation_is_not_a_retroactive_mtp_rule() {
        let h = MAINNET_DAA_V3_ACTIVATION_HEIGHT;
        let mut blocks = prefix(h as usize);
        blocks.last_mut().unwrap().header.time = 0;
        assert!(validate_daa_v3_timestamp(&settings(), &blocks[..blocks.len() - 1], h - 1, 0).is_ok());
        assert_eq!(crate::required_work_bits(&settings(), &blocks, h).unwrap(), 0x1c100000);
        assert!(validate_daa_v3_timestamp(&settings(), &blocks, h, 0).is_err());
    }

    #[test]
    fn hf133_mtp_equality_and_connect_path_reject_before_mutation() {
        let h = MAINNET_DAA_V3_ACTIVATION_HEIGHT;
        let blocks = prefix(h as usize);
        let median = median_time_past(&blocks).unwrap();
        assert!(validate_daa_v3_timestamp(&settings(), &blocks, h, median).is_err());
        assert!(validate_daa_v3_timestamp(&settings(), &blocks, h, median + 1).is_ok());
        let mut candidate = blocks.last().unwrap().clone();
        candidate.header.time = median;
        let mut chain = ChainState { network: "mainnet".into(), blocks: Arc::new(blocks),
            utxos: Arc::new(HashMap::new()), mempool: Arc::new(Vec::new()) };
        let before = chain.tip_hash();
        let error = chain.connect_block(candidate, &settings()).unwrap_err().to_string();
        assert!(error.contains("median of the previous 11"), "{}", error);
        assert_eq!(chain.tip_hash(), before);
        assert_eq!(chain.height(), h - 1);
    }

    #[test]
    fn hf133_template_clock_uses_mtp_and_checks_overflow() {
        let h = MAINNET_DAA_V3_ACTIVATION_HEIGHT;
        let mut blocks = prefix(h as usize);
        let median = median_time_past(&blocks).unwrap();
        assert_eq!(candidate_timestamp(&settings(), &blocks, h, median).unwrap(), median + 1);
        assert_eq!(candidate_timestamp(&settings(), &blocks, h, median + 20).unwrap(), median + 20);
        assert!(candidate_timestamp(&settings(), &blocks, h, 0).is_err());
        for b in blocks.iter_mut().rev().take(11) { b.header.time = u32::MAX; }
        assert!(candidate_timestamp(&settings(), &blocks, h, u32::MAX).is_err());
        assert_eq!(candidate_timestamp(&settings(), &[], h - 1, 123).unwrap(), 123);
    }

    #[test]
    fn hf133_reorganization_recomputes_anchor_from_supplied_branch() {
        let h = MAINNET_DAA_V3_ACTIVATION_HEIGHT;
        let mut a = prefix(h as usize + 8);
        let mut b = a.clone();
        b[h as usize - 1].header.bits = 0x1c200000;
        let first = crate::required_work_bits(&settings(), &a, h + 8).unwrap();
        let other = crate::required_work_bits(&settings(), &b, h + 8).unwrap();
        assert_ne!(first, other);
        assert_eq!(first, crate::required_work_bits(&settings(), &a, h + 8).unwrap());
        for block in a.iter_mut().rev().take(6) { block.header.time += 3600; }
        assert_ne!(first, crate::required_work_bits(&settings(), &a, h + 8).unwrap());
    }

    #[test]
    fn hf133_stale_config_cannot_change_mainnet_asert_parameters() {
        let h = MAINNET_DAA_V3_ACTIVATION_HEIGHT;
        let blocks = prefix(h as usize + 20);
        let mut stale = settings();
        stale.consensus.target_spacing_secs = 300;
        stale.consensus.pow_bits = "0x1e07ffff".into();
        stale.consensus.difficulty_adjustment_interval = 3;
        assert_eq!(crate::required_work_bits(&settings(), &blocks, h + 20).unwrap(),
                   crate::required_work_bits(&stale, &blocks, h + 20).unwrap());
    }

    #[test]
    fn hf133_missing_or_extra_history_fails_closed() {
        let h = MAINNET_DAA_V3_ACTIVATION_HEIGHT;
        assert!(crate::required_work_bits(&settings(), &[], h).is_err());
        let blocks = prefix(h as usize + 1);
        assert!(crate::required_work_bits(&settings(), &blocks[..10], h).is_err());
        assert!(crate::required_work_bits(&settings(), &blocks, h).is_err());
        assert!(crate::required_work_bits(&settings(), &blocks, h + 2).is_err());
    }

    #[test]
    fn hf133_single_timestamp_outlier_has_bounded_clock_influence() {
        let h = MAINNET_DAA_V3_ACTIVATION_HEIGHT;
        let mut blocks = prefix(h as usize + 20);
        let before = median_time_past(&blocks).unwrap();
        blocks.last_mut().unwrap().header.time = u32::MAX;
        assert_eq!(median_time_past(&blocks).unwrap(), before);
        blocks.last_mut().unwrap().header.time = 0;
        assert_eq!(median_time_past(&blocks).unwrap(), before - 60);
        let bits = crate::required_work_bits(&settings(), &blocks, h + 20).unwrap();
        assert!(compact_to_biguint(bits).unwrap() > BigUint::one());
    }

    #[test]
    fn hf133_long_perfect_schedule_does_not_accumulate_target_rounding() {
        let h = MAINNET_DAA_V3_ACTIVATION_HEIGHT;
        let mut blocks = prefix(h as usize);
        let expected = blocks.last().unwrap().header.bits;
        for offset in 0..2000u32 {
            let bits = crate::required_work_bits(&settings(), &blocks, h + offset).unwrap();
            assert_eq!(bits, expected);
            let mut next = blocks.last().unwrap().clone();
            next.header.time += 60;
            next.header.bits = bits;
            next.header.version = 3;
            blocks.push(next);
        }
    }
    #[test]
    fn hf133_upgrade_rejects_old_block_version_and_wrong_target() {
        let h = MAINNET_DAA_V3_ACTIVATION_HEIGHT;
        let settings = settings();
        let blocks = prefix(h as usize);
        let mut candidate = blocks.last().unwrap().clone();
        candidate.header.time += 60;
        candidate.transactions.push(crate::Transaction {
            version: 1, inputs: Vec::new(), outputs: Vec::new(), locktime: 0,
        });
        let expected = crate::required_work_bits(&settings, &blocks, h).unwrap();
        let error = crate::validate_block(&candidate, &HashMap::new(), Hash256([0; 32]),
            h, expected, &settings).unwrap_err().to_string();
        assert!(error.contains("bad block version"), "{}", error);
        candidate.header.version = 3;
        candidate.header.bits = 0x1c200000;
        let error = crate::validate_block(&candidate, &HashMap::new(), Hash256([0; 32]),
            h, expected, &settings).unwrap_err().to_string();
        assert!(error.contains("bad bits"), "{}", error);
    }

}
