//! Offline historical MAINNET fixtures. No listener, miner, testnet or regtest.
use super::*;

pub(crate) fn fixture() -> (Settings, ChainState) {
    let mut settings: Settings = toml::from_str(include_str!("../config/mainnet.toml")).unwrap();
    settings.apply_network_consensus_overrides();
    settings.p2p.enabled = false;
    settings.mining.enabled = false;
    let rows: serde_json::Value = serde_json::from_str(include_str!("../tests/fixtures/hf134-mainnet-blocks.json")).unwrap();
    let history: Vec<Block> = serde_json::from_value(rows["blocks"].clone()).unwrap();
    let mut blocks = vec![genesis_block(&settings).unwrap()];
    blocks.extend(history);
    let chain = ChainState::from_blocks(blocks, &settings).unwrap();
    assert_eq!(chain.height(), 3);
    assert_eq!(chain.tip_hash().to_string(), rows["hashes"][2].as_str().unwrap());
    (settings, chain)
}

#[test]
fn hf134_mainnet_prefix_utxos_equal_full_consensus_replay() {
    let (settings, baseline) = fixture();
    for keep in 1..=baseline.blocks.len() {
        let blocks = baseline.blocks[..keep].to_vec();
        let expected = ChainState::from_blocks(blocks.clone(), &settings).unwrap();
        let actual = ChainState::hf134_from_validated_prefix(&baseline, blocks, &settings).unwrap();
        assert_eq!(actual.blocks, expected.blocks);
        assert_eq!(actual.utxos, expected.utxos, "rollback to {}", keep - 1);
        assert!(actual.mempool.is_empty());
    }
}

#[test]
fn hf134_mainnet_extension_validates_suffix_and_matches_full_replay() {
    let (settings, baseline) = fixture();
    let prefix = ChainState::from_blocks(baseline.blocks[..2].to_vec(), &settings).unwrap();
    let actual = ChainState::hf134_from_validated_prefix(&prefix, baseline.blocks.to_vec(), &settings).unwrap();
    assert_eq!(actual.blocks, baseline.blocks);
    assert_eq!(actual.utxos, baseline.utxos);
}

#[test]
fn hf134_replacement_body_same_header_is_not_trusted() {
    let (settings, baseline) = fixture();
    let mut blocks = baseline.blocks.to_vec();
    let output = &mut blocks[2].transactions[0].outputs[0];
    output.value = output.value.checked_add(Amount::from_atoms(1).unwrap()).unwrap();
    assert_eq!(blocks[2].block_hash(), baseline.blocks[2].block_hash());
    assert!(ChainState::hf134_from_validated_prefix(&baseline, blocks.clone(), &settings).is_err());
    assert!(ChainState::from_blocks(blocks, &settings).is_err());
    baseline.validate_all(&settings).unwrap();
}

#[test]
fn hf134_replacement_invalid_pow_keeps_parent_unchanged() {
    let (settings, baseline) = fixture();
    let original_utxos = baseline.utxos.clone();
    let original_tip = baseline.tip_hash();
    let mut blocks = baseline.blocks.to_vec();
    blocks[3].header.nonce = blocks[3].header.nonce.wrapping_add(1);
    assert!(!verify_header_pow(&blocks[3].header).unwrap());
    assert!(ChainState::hf134_from_validated_prefix(&baseline, blocks, &settings).is_err());
    assert_eq!(baseline.tip_hash(), original_tip);
    assert_eq!(baseline.utxos, original_utxos);
}

#[test]
fn hf134_valid_suffix_then_invalid_block_is_atomic() {
    let (settings, baseline) = fixture();
    let prefix = ChainState::from_blocks(baseline.blocks[..2].to_vec(), &settings).unwrap();
    let parent = prefix.tip_hash();
    let mut blocks = baseline.blocks.to_vec();
    let output = &mut blocks[3].transactions[0].outputs[0];
    output.value = output.value.checked_add(Amount::from_atoms(1).unwrap()).unwrap();
    assert!(ChainState::hf134_from_validated_prefix(&prefix, blocks, &settings).is_err());
    assert_eq!(prefix.tip_hash(), parent);
    assert_eq!(prefix.height(), 1);
}

#[test]
fn hf134_cancelled_rebuild_keeps_original_chain() {
    let (settings, baseline) = fixture();
    let result = p2p::hf130_with_mining_cancellation(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)), || {
        ChainState::hf134_from_validated_prefix(&baseline, baseline.blocks[..3].to_vec(), &settings)
    });
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert_eq!(baseline.height(), 3);
    baseline.validate_all(&settings).unwrap();
}

#[test]
fn hf134_wrong_network_and_genesis_fail_closed() {
    let (settings, mut baseline) = fixture();
    let blocks = baseline.blocks.to_vec();
    baseline.network = "untrusted".into();
    assert!(ChainState::hf134_from_validated_prefix(&baseline, blocks, &settings).is_err());
    baseline.network = "mainnet".into();
    let mut blocks = baseline.blocks.to_vec();
    blocks[0].header.nonce += 1;
    assert!(ChainState::hf134_from_validated_prefix(&baseline, blocks, &settings).is_err());
}

// Opt-in native replay gate: export a COPY of the operator's real mainnet
// history first. No live wallet/data directory is opened by this test.
#[test]
#[ignore = "requires an explicitly supplied isolated mainnet export"]
fn hf134_real_mainnet_prefix_replay_when_requested() {
    let path = std::env::var("QUB_HF134_REPLAY_JSON").expect("QUB_HF134_REPLAY_JSON required");
    let (settings, _) = fixture();
    let persisted: PersistedChainState = serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();
    assert_eq!(persisted.network, "mainnet");
    let baseline = ChainState::from_persisted(persisted, &settings).unwrap();
    for removed in [1, 2, 12] {
        let len = baseline.blocks.len().saturating_sub(removed).max(1);
        let result = ChainState::hf134_from_validated_prefix(&baseline, baseline.blocks[..len].to_vec(), &settings).unwrap();
        result.validate_all(&settings).unwrap();
        let restored = ChainState::hf134_from_validated_prefix(&result, baseline.blocks.to_vec(), &settings).unwrap();
        assert_eq!(restored.blocks, baseline.blocks);
        assert_eq!(restored.utxos, baseline.utxos);
    }
}
