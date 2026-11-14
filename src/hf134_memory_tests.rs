//! Offline MAINNET fixtures and isolated adversarial UTXO/transaction tests.
//! No listener, wallet loading, mining, testnet, or regtest.
use super::*;
use crate::hf134_memory_reference as reference;

fn same_state(a: &ChainState, b: &ChainState) {
    assert_eq!(a.network, b.network);
    assert_eq!(a.blocks, b.blocks);
    assert_eq!(a.utxos, b.utxos);
    assert_eq!(a.mempool, b.mempool);
    assert_eq!(a.total_work_hex().unwrap(), b.total_work_hex().unwrap());
}

#[test]
fn hf134_memory_mainnet_history_matches_frozen_r6() {
    let (settings, history) = crate::hf134_tests::fixture();
    let mut old = ChainState::from_blocks(vec![genesis_block(&settings).unwrap()], &settings).unwrap();
    let mut new = old.clone();
    for block in history.blocks.iter().skip(1) {
        let expected = reference::connect_block(&mut old, block.clone(), &settings).unwrap();
        assert_eq!(new.connect_block(block.clone(), &settings).unwrap(), expected);
        same_state(&old, &new);
    }
    new.validate_all(&settings).unwrap();
}

#[test]
fn hf134_memory_private_coinbase_replay_reuses_utxo_allocation() {
    let (settings, history) = crate::hf134_tests::fixture();
    let mut chain = ChainState::from_blocks(vec![genesis_block(&settings).unwrap()], &settings).unwrap();
    let allocation = Arc::as_ptr(&chain.utxos);
    for block in history.blocks.iter().skip(1) {
        assert_eq!(block.transactions.len(), 1);
        chain.connect_block(block.clone(), &settings).unwrap();
        assert_eq!(Arc::as_ptr(&chain.utxos), allocation, "private replay copied the UTXO map");
    }
    same_state(&chain, &history);
}

#[test]
fn hf134_memory_retained_reader_is_unchanged_and_copies_only_once() {
    let (settings, history) = crate::hf134_tests::fixture();
    let reader = ChainState::from_blocks(history.blocks[..2].to_vec(), &settings).unwrap();
    let before = serde_json::to_vec(&reader.to_persisted()).unwrap();
    let mut candidate = reader.clone();
    assert!(Arc::ptr_eq(&reader.utxos, &candidate.utxos));
    candidate.connect_block(history.blocks[2].clone(), &settings).unwrap();
    assert!(!Arc::ptr_eq(&reader.utxos, &candidate.utxos));
    let allocation = Arc::as_ptr(&candidate.utxos);
    candidate.connect_block(history.blocks[3].clone(), &settings).unwrap();
    assert_eq!(allocation, Arc::as_ptr(&candidate.utxos));
    assert_eq!(before, serde_json::to_vec(&reader.to_persisted()).unwrap());
    same_state(&candidate, &history);
}

#[test]
fn hf134_memory_rejected_mainnet_blocks_match_r6_and_leave_state_intact() {
    let (settings, history) = crate::hf134_tests::fixture();
    let baseline = ChainState::from_blocks(history.blocks[..2].to_vec(), &settings).unwrap();
    let original = history.blocks[2].clone();
    let mut invalid = Vec::new();
    let mut b = original.clone(); b.header.nonce = b.header.nonce.wrapping_add(1); invalid.push(b);
    let mut b = original.clone(); b.header.prev_block_hash = Hash256::zero(); invalid.push(b);
    let mut b = original.clone(); b.header.version ^= 1; invalid.push(b);
    let mut b = original.clone(); b.header.bits ^= 1; invalid.push(b);
    let mut b = original.clone(); b.transactions[0].outputs[0].value = Amount::from_atoms(1).unwrap(); invalid.push(b);
    let mut b = original; b.transactions.clear(); invalid.push(b);
    for block in invalid {
        let mut old = baseline.clone(); let mut new = baseline.clone();
        let old_error = reference::connect_block(&mut old, block.clone(), &settings).unwrap_err().to_string();
        let new_error = new.connect_block(block, &settings).unwrap_err().to_string();
        assert_eq!(old_error, new_error);
        same_state(&new, &baseline);
        assert!(Arc::ptr_eq(&new.utxos, &baseline.utxos));
    }
}

#[test]
fn hf134_memory_late_coinbase_collision_rolls_back_entire_update() {
    let (_, history) = crate::hf134_tests::fixture();
    let mut tx = history.blocks[1].transactions[0].clone();
    tx.outputs.push(tx.outputs[0].clone());
    let collision = OutPoint { txid: tx.txid(), vout: 1 };
    let occupied = CoinRecord { tx_out: tx.outputs[1].clone(), height: 99, is_coinbase: false };
    for retained in [false, true] {
        let mut actual = Arc::new(HashMap::from([(collision.clone(), occupied.clone())]));
        let reader = if retained { Some(actual.clone()) } else { None };
        let allocation = Arc::as_ptr(&actual);
        let before = actual.as_ref().clone();
        let mut legacy_scratch = before.clone();
        let expected = connect_tx_utxos(&tx, &mut legacy_scratch, 1, true).unwrap_err().to_string();
        assert_eq!(coinbase_utxos::connect(&tx, &mut actual, 1).unwrap_err().to_string(), expected);
        assert_eq!(actual.as_ref(), &before);
        assert_eq!(Arc::as_ptr(&actual), allocation);
        if let Some(reader) = reader { assert!(Arc::ptr_eq(&reader, &actual)); }
    }
}

#[test]
fn hf134_memory_multioutput_coinbase_matches_legacy_application() {
    let (_, history) = crate::hf134_tests::fixture();
    let mut tx = history.blocks[1].transactions[0].clone();
    tx.outputs.extend(vec![tx.outputs[0].clone(); 17]);
    let mut expected = history.utxos.as_ref().clone();
    let mut actual = Arc::new(expected.clone());
    let allocation = Arc::as_ptr(&actual);
    connect_tx_utxos(&tx, &mut expected, 4, true).unwrap();
    coinbase_utxos::connect(&tx, &mut actual, 4).unwrap();
    assert_eq!(actual.as_ref(), &expected);
    assert_eq!(Arc::as_ptr(&actual), allocation);
}

#[test]
fn hf134_memory_empty_coinbase_and_noncoinbase_boundaries() {
    let (_, history) = crate::hf134_tests::fixture();
    let mut tx = history.blocks[1].transactions[0].clone();
    tx.outputs.clear();
    let mut actual = history.utxos.clone();
    let allocation = Arc::as_ptr(&actual);
    coinbase_utxos::connect(&tx, &mut actual, 4).unwrap();
    assert_eq!(Arc::as_ptr(&actual), allocation);
    tx.version = TX_VERSION_POOL_SHARE;
    assert!(coinbase_utxos::connect(&tx, &mut actual, 4).is_err());
    assert_eq!(Arc::as_ptr(&actual), allocation);
    assert_eq!(actual, history.utxos);
}

fn signed_spend(secret: &SecretKey, previous: OutPoint, coin: &CoinRecord, value: u64) -> Transaction {
    let mut tx = Transaction {
        version: 1,
        inputs: vec![TxIn { previous_output: previous, signature_script: ScriptBuf::empty(), sequence: u32::MAX }],
        outputs: vec![TxOut { value: Amount::from_atoms(value).unwrap(), script_pubkey: coin.tx_out.script_pubkey.clone() }],
        locktime: 0,
    };
    let hash = tx.sighash_all(0, &coin.tx_out.script_pubkey).unwrap();
    let sig = sign_hash(secret, hash).unwrap();
    tx.inputs[0].signature_script = encode_sig_script(&public_key_from_secret(secret).serialize(), &sig).unwrap();
    tx
}

fn transaction_fixture() -> (Settings, Block, HashMap<OutPoint, CoinRecord>) {
    let (settings, history) = crate::hf134_tests::fixture();
    // Public deterministic unit-test key; never loaded from a wallet or funded.
    let secret = SecretKey::from_slice(&[1; 32]).unwrap();
    let address = address_from_public_key(&settings.network.address_prefix, &public_key_from_secret(&secret));
    let coin = CoinRecord { tx_out: TxOut { value: Amount::from_atoms(1_000).unwrap(), script_pubkey: address.script_pubkey() }, height: 0, is_coinbase: false };
    let outpoint = OutPoint { txid: Hash256([42; 32]), vout: 0 };
    let first = signed_spend(&secret, outpoint.clone(), &coin, 900);
    let next_coin = CoinRecord { tx_out: first.outputs[0].clone(), height: 1, is_coinbase: false };
    let second = signed_spend(&secret, OutPoint { txid: first.txid(), vout: 0 }, &next_coin, 800);
    let mut block = history.blocks[1].clone();
    block.transactions.extend([first, second]);
    (settings, block, HashMap::from([(outpoint, coin)]))
}

#[test]
fn hf134_memory_signed_intrablock_spend_matches_r6_transaction_stage() {
    let (settings, block, utxos) = transaction_fixture();
    let before = utxos.clone();
    let expected = reference::validate_transactions(&block, &utxos, 1, &settings).unwrap();
    let actual = validate_block_transactions(&block, &utxos, 1, &settings).unwrap();
    assert_eq!(actual.len(), 3);
    assert_eq!(actual, expected);
    assert_eq!(utxos, before);
}

#[test]
fn hf134_memory_invalid_transaction_stages_match_r6_without_mutation() {
    let (settings, block, utxos) = transaction_fixture();
    let mut variants = Vec::new();
    let mut b = block.clone(); b.transactions.swap(1, 2); variants.push((b, utxos.clone()));
    let mut b = block.clone(); b.transactions[2] = b.transactions[1].clone(); variants.push((b, utxos.clone()));
    let mut b = block.clone(); b.transactions[1].inputs[0].signature_script = ScriptBuf::empty(); variants.push((b, utxos.clone()));
    let mut b = block.clone(); b.transactions[0].outputs[0].value = Amount::from_atoms(block_subsidy(1, &settings) + 201).unwrap(); variants.push((b, utxos.clone()));
    let mut immature = utxos.clone(); immature.values_mut().next().unwrap().is_coinbase = true; variants.push((block.clone(), immature));
    let secret = SecretKey::from_slice(&[1; 32]).unwrap();
    let (point, coin) = utxos.iter().next().unwrap();
    let mut b = block; b.transactions[2] = signed_spend(&secret, point.clone(), coin, 850); variants.push((b, utxos));
    for (candidate, base) in variants {
        let before = base.clone();
        let expected = reference::validate_transactions(&candidate, &base, 1, &settings).unwrap_err().to_string();
        assert_eq!(validate_block_transactions(&candidate, &base, 1, &settings).unwrap_err().to_string(), expected);
        assert_eq!(base, before);
    }
}

#[test]
fn hf134_memory_candidate_failure_after_valid_block_preserves_reader() {
    let (settings, history) = crate::hf134_tests::fixture();
    let reader = ChainState::from_blocks(history.blocks[..2].to_vec(), &settings).unwrap();
    let before = serde_json::to_vec(&reader.to_persisted()).unwrap();
    let mut candidate = reader.clone();
    candidate.connect_block(history.blocks[2].clone(), &settings).unwrap();
    let candidate_before = serde_json::to_vec(&candidate.to_persisted()).unwrap();
    let mut bad = history.blocks[3].clone(); bad.transactions[0].outputs[0].value = Amount::from_atoms(1).unwrap();
    assert!(candidate.connect_block(bad, &settings).is_err());
    assert_eq!(candidate_before, serde_json::to_vec(&candidate.to_persisted()).unwrap());
    assert_eq!(before, serde_json::to_vec(&reader.to_persisted()).unwrap());
}

#[test]
#[ignore = "explicit bounded synthetic-UTXO microbenchmark, not mainnet replay or RSS proof"]
fn hf134_memory_microbenchmark_when_requested() {
    let (settings, history) = crate::hf134_tests::fixture();
    let mut baseline = ChainState::from_blocks(vec![genesis_block(&settings).unwrap()], &settings).unwrap();
    let exemplar = history.utxos.values().next().unwrap().clone();
    let mut padding = HashMap::new();
    for id in 0..50_000u32 {
        let mut hash = [0x77; 32]; hash[..4].copy_from_slice(&id.to_le_bytes());
        padding.insert(OutPoint { txid: Hash256(hash), vout: 0 }, exemplar.clone());
    }
    baseline.utxos = Arc::new(padding);
    let mut old_times = Vec::new(); let mut new_times = Vec::new();
    for round in 0..5 {
        let mut old = baseline.clone(); let mut new = baseline.clone();
        // Alternate order to reduce warm-cache/order bias. This is not an RSS benchmark.
        for legacy in if round % 2 == 0 { [true, false] } else { [false, true] } {
            let start = std::time::Instant::now();
            if legacy {
                for b in history.blocks.iter().skip(1) { reference::connect_block(&mut old, b.clone(), &settings).unwrap(); }
                old_times.push(start.elapsed().as_micros() as u64);
            } else {
                for b in history.blocks.iter().skip(1) { new.connect_block(b.clone(), &settings).unwrap(); }
                new_times.push(start.elapsed().as_micros() as u64);
            }
        }
        same_state(&old, &new);
        assert_eq!(baseline.height(), 0);
        assert_eq!(baseline.utxos.len(), 50_000);
    }
    old_times.sort_unstable(); new_times.sort_unstable();
    let report = serde_json::json!({
        "schema": "QUB-HF134-R7-MICROBENCH-1", "synthetic_utxos": 50_000,
        "historical_mainnet_blocks": 3, "repetitions": 5,
        "r6_median_microseconds": old_times[2], "r7_median_microseconds": new_times[2],
        "states_equal": true, "mainnet_replay": false, "peak_rss_measured": false
    });
    let path = std::env::var("QUB_HF134_R7_BENCH_REPORT").expect("explicit isolated benchmark report path required");
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();
    serde_json::to_writer_pretty(file, &report).unwrap();
}

