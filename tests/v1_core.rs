use qubd::*;
use std::str::FromStr;

fn regtest() -> Settings {
    toml::from_str(include_str!("../config/regtest.toml")).expect("regtest config parses")
}

#[test]
fn address_checksum_roundtrip() {
    let s = regtest();
    let secret = generate_secret_key();
    let pk = public_key_from_secret(&secret);
    let addr = address_from_public_key(&s.network.address_prefix, &pk);
    let enc = addr.to_string();
    assert_eq!(Address::parse_with_prefix(&enc, &s.network.address_prefix).unwrap(), addr);
    let mut bad = enc.clone();
    let last = bad.pop().unwrap();
    bad.push(if last == '0' { '1' } else { '0' });
    assert!(Address::from_str(&bad).is_err());
}

#[test]
fn target_decodes() {
    let s = regtest();
    assert!(target_from_compact(s.pow_bits().unwrap()).unwrap().iter().any(|b| *b != 0));
}

#[test]
fn economics_are_btc_like_x10_speed() {
    let s = regtest();
    assert_eq!(s.consensus.target_spacing_secs, 60);
    assert_eq!(s.consensus.subsidy_halving_interval, 2_100_000);
    assert_eq!(block_subsidy(1, &s), 5 * ATOMS_PER_QUB);
    assert_eq!(block_subsidy(2_100_000, &s), 5 * ATOMS_PER_QUB);
    assert_eq!(block_subsidy(2_100_001, &s), 250_000_000);

    let mut issued: u128 = 0;
    for era in 0..64u64 {
        let subsidy = s.consensus.initial_subsidy_atoms >> era;
        issued += subsidy as u128 * s.consensus.subsidy_halving_interval as u128;
    }
    assert!(issued <= MAX_MONEY_ATOMS as u128);
}

#[test]
fn mine_send_confirm_roundtrip() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut miner_wallet = WalletFile::new(&s.network.name);
    let miner_key = miner_wallet.create_key(&s, "miner", 0).unwrap();
    let miner = Address::parse_with_prefix(&miner_key.address, &s.network.address_prefix).unwrap();
    let mut recipient_wallet = WalletFile::new(&s.network.name);
    let rec_key = recipient_wallet.create_key(&s, "recipient", 0).unwrap();
    let recipient = Address::parse_with_prefix(&rec_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };
    for _ in 0..3 {
        let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }
    assert!(miner_wallet.balance_atoms(&chain, &s, false).unwrap() > 0);
    let tx = miner_wallet.create_signed_transaction(&chain, &s, &recipient, Amount::from_str("1.25").unwrap(), Amount::from_str("0.00001").unwrap()).unwrap();
    let txid = tx.txid();
    chain.accept_transaction_to_mempool(tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    assert!(b.transactions.iter().any(|t| t.txid() == txid));
    chain.connect_block(b, &s).unwrap();
    assert_eq!(chain.mempool.len(), 0);
    assert_eq!(recipient_wallet.balance_atoms(&chain, &s, true).unwrap(), Amount::from_str("1.25").unwrap().atoms());
}

#[test]
fn tampered_signature_is_rejected() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut miner_wallet = WalletFile::new(&s.network.name);
    let miner_key = miner_wallet.create_key(&s, "miner", 0).unwrap();
    let miner = Address::parse_with_prefix(&miner_key.address, &s.network.address_prefix).unwrap();
    let mut recipient_wallet = WalletFile::new(&s.network.name);
    let rec_key = recipient_wallet.create_key(&s, "recipient", 0).unwrap();
    let recipient = Address::parse_with_prefix(&rec_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };
    for _ in 0..3 {
        let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }
    let mut tx = miner_wallet.create_signed_transaction(&chain, &s, &recipient, Amount::from_str("1").unwrap(), Amount::from_str("0.00001").unwrap()).unwrap();
    tx.outputs[0].value = Amount::from_str("2").unwrap();
    assert!(validate_tx_contextual(&tx, &chain.utxos, chain.height() + 1, &s, true).is_err());
}

#[test]
fn equal_work_forks_do_not_flip_without_explicit_preference() {
    let s = regtest();
    let base = ChainState::new_with_genesis(&s).unwrap();
    let mut w1 = WalletFile::new(&s.network.name);
    let a1 = Address::parse_with_prefix(&w1.create_key(&s, "a", 0).unwrap().address, &s.network.address_prefix).unwrap();
    let mut w2 = WalletFile::new(&s.network.name);
    let a2 = Address::parse_with_prefix(&w2.create_key(&s, "b", 0).unwrap().address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };

    let b1 = mine_next_block(&base, &s, &a1, opts).unwrap();
    let b2 = mine_next_block(&base, &s, &a2, opts).unwrap();
    assert_ne!(b1.block_hash(), b2.block_hash());

    let mut c1 = ChainState::new_with_genesis(&s).unwrap();
    c1.connect_block(b1, &s).unwrap();
    let mut c2 = ChainState::new_with_genesis(&s).unwrap();
    c2.connect_block(b2, &s).unwrap();

    let c1_tip = c1.tip_hash().to_string();
    let c2_tip = c2.tip_hash().to_string();
    let mut higher_tip_chain = if c1_tip > c2_tip { c1.clone() } else { c2.clone() };
    let lower_tip_blocks = if c1_tip < c2_tip { c1.blocks.clone() } else { c2.blocks.clone() };

    let original_tip = higher_tip_chain.tip_hash().to_string();
    assert!(!higher_tip_chain.try_adopt_peer_chain(lower_tip_blocks.clone(), &s, false).unwrap());
    assert_eq!(higher_tip_chain.tip_hash().to_string(), original_tip);
    assert!(higher_tip_chain.try_adopt_peer_chain(lower_tip_blocks, &s, true).unwrap());
    assert_eq!(higher_tip_chain.tip_hash().to_string(), c1_tip.min(c2_tip));
}


#[test]
fn reorg_resurrects_tx_from_abandoned_suffix_into_mempool() {
    let s = regtest();
    let opts = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };

    let mut funder_wallet = WalletFile::new(&s.network.name);
    let funder_key = funder_wallet.create_key(&s, "funder", 0).unwrap();
    let funder = Address::parse_with_prefix(&funder_key.address, &s.network.address_prefix).unwrap();

    let mut recipient_wallet = WalletFile::new(&s.network.name);
    let recipient_key = recipient_wallet.create_key(&s, "recipient", 0).unwrap();
    let recipient = Address::parse_with_prefix(&recipient_key.address, &s.network.address_prefix).unwrap();

    let mut other_wallet = WalletFile::new(&s.network.name);
    let other_key = other_wallet.create_key(&s, "other", 0).unwrap();
    let other = Address::parse_with_prefix(&other_key.address, &s.network.address_prefix).unwrap();

    let mut base = ChainState::new_with_genesis(&s).unwrap();
    for _ in 0..3 {
        let b = mine_next_block(&base, &s, &funder, opts).unwrap();
        base.connect_block(b, &s).unwrap();
    }

    let tx = funder_wallet
        .create_signed_transaction(
            &base,
            &s,
            &recipient,
            Amount::from_str("1.00").unwrap(),
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let txid = tx.txid();

    let mut stale = base.clone();
    stale.accept_transaction_to_mempool(tx.clone(), &s).unwrap();
    let stale_block = mine_next_block(&stale, &s, &funder, opts).unwrap();
    assert!(stale_block.transactions.iter().any(|t| t.txid() == txid));
    stale.connect_block(stale_block, &s).unwrap();
    assert!(!stale.tx_in_mempool(txid));

    let mut winning = base.clone();
    for _ in 0..2 {
        let b = mine_next_block(&winning, &s, &other, opts).unwrap();
        winning.connect_block(b, &s).unwrap();
    }

    assert!(stale.try_adopt_peer_chain(winning.blocks.clone(), &s, false).unwrap());
    assert!(stale.tx_in_mempool(txid));
}

#[test]
fn qns_price_and_registration_are_deterministic() {
    let s = regtest();
    assert_eq!(normalize_qns_name("Alice.QUB", s.qns.max_label_chars).unwrap(), "alice.qub");
    assert!(normalize_qns_name("bad-name.qub", s.qns.max_label_chars).is_err());

    let short = qns_registration_price_atoms(&s, "a.qub").unwrap();
    let long = qns_registration_price_atoms(&s, "abcdefghijklmnopqrstuvwx12345678.qub").unwrap();
    assert!(short > long);

    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut wallet = WalletFile::new(&s.network.name);
    let miner_key = wallet.create_key(&s, "miner", 0).unwrap();
    let miner = Address::parse_with_prefix(&miner_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };

    for _ in 0..s.qns.activation_height.max(s.consensus.coinbase_maturity + 1) {
        let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }

    // Keep the short-vs-long pricing assertions above, but use a max-length
    // affordable name for the actual registration roundtrip. Production-style
    // short-name pricing intentionally costs far more than this small unit-test balance.
    let affordable_name = "abcdefghijklmnopqrstuvwx12345678.qub";

    let tx = wallet
        .create_qns_registration_transaction(
            &chain,
            &s,
            affordable_name,
            &miner,
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();

    let txid = tx.txid();
    chain.accept_transaction_to_mempool(tx, &s).unwrap();

    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    assert!(b.transactions.iter().any(|t| t.txid() == txid));
    chain.connect_block(b, &s).unwrap();

    let rec = qns_resolve(&s, &chain, affordable_name).unwrap().unwrap();
    assert_eq!(rec.address, miner.to_string());

    let dup = wallet.create_qns_registration_transaction(
        &chain,
        &s,
        affordable_name,
        &miner,
        Amount::from_str("0.00001").unwrap(),
    );
    assert!(dup.is_err());
}

#[test]
fn jin_supply_and_activation_are_deterministic() {
    let s = regtest();
    assert!(s.jin.enabled);
    assert_eq!(s.jin.decimals, JIN_DECIMALS);
    assert_eq!(parse_jin_units_raw(&s.jin.total_supply_units).unwrap(), JIN_TOTAL_SUPPLY_UNITS);
    assert_eq!(parse_jin_amount("1").unwrap(), JIN_UNITS_PER_COIN);
    assert_eq!(parse_jin_amount("0.000000000000000001").unwrap(), 1);
    let chain = ChainState::new_with_genesis(&s).unwrap();
    assert_eq!(jin_balance_units_for_address(&s, &chain, &s.jin.protocol_address).unwrap(), 0);
}

#[test]
fn pools_settings_and_names_are_deterministic() {
    let s = regtest();
    assert!(s.features.pooled_mining_enabled);
    assert!(s.pools.enabled);
    assert_eq!(s.pools.activation_height, 1);
    assert_eq!(s.pools.protocol_name, "pools.qub");
    assert_eq!(normalize_pool_name("🔥Dragon Pool🔥", s.pools.max_name_chars, s.pools.max_name_bytes).unwrap(), "🔥Dragon Pool🔥");
    assert!(normalize_pool_name("bad\u{200b}pool", s.pools.max_name_chars, s.pools.max_name_bytes).is_err());
    assert!(normalize_pool_name("bad\nname", s.pools.max_name_chars, s.pools.max_name_bytes).is_err());
    assert!(capacity_slots_valid(&s, s.pools.base_capacity_slots));
    assert!(extra_capacity_slots_valid(&s, s.pools.capacity_step_slots));
}

#[test]
fn pool_create_share_and_payout_are_consensus_valid() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut manager_wallet = WalletFile::new(&s.network.name);
    let manager_key = manager_wallet.create_key(&s, "manager", 0).unwrap();
    let manager = Address::parse_with_prefix(&manager_key.address, &s.network.address_prefix).unwrap();
    let mut miner_wallet = WalletFile::new(&s.network.name);
    let share_miner_key = miner_wallet.create_key(&s, "share-miner", 0).unwrap();
    let opts = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };

    for _ in 0..8 {
        let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }
    assert_eq!(miner_wallet.balance_atoms(&chain, &s, true).unwrap(), 0);

    let create_tx = manager_wallet.create_pool_create_transaction(
        &chain,
        &s,
        "🔥Fair Pool🔥",
        &manager,
        500,
        s.pools.base_capacity_slots,
        Amount::from_str("0.00001").unwrap(),
    ).unwrap();
    let pool_id = create_tx.txid();
    chain.accept_transaction_to_mempool(create_tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    assert!(b.transactions.iter().any(|tx| tx.txid() == pool_id));
    chain.connect_block(b, &s).unwrap();
    assert!(pools_registry_from_blocks(&s, &chain.blocks).unwrap().contains_key(&pool_id));

    let parent_height = chain.height();
    let parent_hash = chain.tip_hash();
    let mut nonce = 0u64;
    while !pool_share_meets_target(&s, pool_id, &share_miner_key.address, parent_height, parent_hash, nonce).unwrap() {
        nonce = nonce.wrapping_add(1);
    }
    let share_tx = create_pool_share_transaction(&s, pool_id, &share_miner_key, parent_height, parent_hash, nonce).unwrap();
    let share_txid = share_tx.txid();
    chain.accept_transaction_to_mempool(share_tx, &s).unwrap();

    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    assert!(b.transactions.iter().any(|tx| tx.txid() == share_txid));
    chain.connect_block(b, &s).unwrap();

    let pool_block = mine_next_pool_block(&chain, &s, pool_id, opts).unwrap();
    assert_eq!(parse_pool_block_marker(&pool_block), Some(pool_id));
    let expected_outputs = expected_pool_coinbase_outputs(&s, &chain.blocks, pool_id, block_subsidy((chain.height() + 1) as u64, &s) as u128).unwrap();
    assert_eq!(pool_block.transactions[0].outputs, expected_outputs);
    chain.connect_block(pool_block, &s).unwrap();
}

#[test]
fn pool_commission_can_only_decrease() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut wallet = WalletFile::new(&s.network.name);
    let manager_key = wallet.create_key(&s, "manager", 0).unwrap();
    let manager = Address::parse_with_prefix(&manager_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };
    for _ in 0..8 {
        let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }
    let create_tx = wallet.create_pool_create_transaction(&chain, &s, "Commission Test", &manager, 1000, s.pools.base_capacity_slots, Amount::from_str("0.00001").unwrap()).unwrap();
    let pool_id = create_tx.txid();
    chain.accept_transaction_to_mempool(create_tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    chain.connect_block(b, &s).unwrap();

    assert!(wallet.create_pool_set_commission_transaction(&chain, &s, pool_id, 1500, Amount::from_str("0.00001").unwrap()).is_err());
    assert!(wallet.create_pool_set_commission_transaction(&chain, &s, pool_id, 500, Amount::from_str("0.00001").unwrap()).is_ok());
}

#[test]
fn pool_manager_can_rename_and_top_up_capacity() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut wallet = WalletFile::new(&s.network.name);
    let manager_key = wallet.create_key(&s, "manager", 0).unwrap();
    let manager = Address::parse_with_prefix(&manager_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };

    for _ in 0..20 {
        let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }

    let create_tx = wallet.create_pool_create_transaction(
        &chain,
        &s,
        "Original Pool",
        &manager,
        600,
        s.pools.base_capacity_slots,
        Amount::from_str("0.00001").unwrap(),
    ).unwrap();
    let pool_id = create_tx.txid();
    chain.accept_transaction_to_mempool(create_tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    chain.connect_block(b, &s).unwrap();

    let rename_tx = wallet.create_pool_rename_transaction(&chain, &s, pool_id, "🔥Renamed Pool🔥", Amount::from_str("0.00001").unwrap()).unwrap();
    chain.accept_transaction_to_mempool(rename_tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    chain.connect_block(b, &s).unwrap();

    let topup_tx = wallet.create_pool_topup_transaction(&chain, &s, pool_id, s.pools.capacity_step_slots, Amount::from_str("0.00001").unwrap()).unwrap();
    chain.accept_transaction_to_mempool(topup_tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    chain.connect_block(b, &s).unwrap();

    let registry = pools_registry_from_blocks(&s, &chain.blocks).unwrap();
    let pool = registry.get(&pool_id).unwrap();
    assert_eq!(pool.name, "🔥Renamed Pool🔥");
    assert_eq!(pool.capacity_slots, s.pools.base_capacity_slots + s.pools.capacity_step_slots);
}

#[test]
fn multi_send_and_blast_qub_are_consensus_valid() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut wallet = WalletFile::new(&s.network.name);
    let miner_key = wallet.create_key(&s, "miner", 0).unwrap();
    let miner = Address::parse_with_prefix(&miner_key.address, &s.network.address_prefix).unwrap();
    let mut r1w = WalletFile::new(&s.network.name);
    let r1 = Address::parse_with_prefix(&r1w.create_key(&s, "r1", 0).unwrap().address, &s.network.address_prefix).unwrap();
    let mut r2w = WalletFile::new(&s.network.name);
    let r2 = Address::parse_with_prefix(&r2w.create_key(&s, "r2", 0).unwrap().address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };
    for _ in 0..8 {
        let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }

    let multi = wallet.create_multi_signed_transaction(&chain, &s, &[(r1.clone(), Amount::from_str("1").unwrap()), (r2.clone(), Amount::from_str("2").unwrap())], Amount::from_str("0.00001").unwrap()).unwrap();
    let multi_id = multi.txid();
    chain.accept_transaction_to_mempool(multi, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    assert!(b.transactions.iter().any(|tx| tx.txid() == multi_id));
    chain.connect_block(b, &s).unwrap();
    assert_eq!(r1w.balance_atoms(&chain, &s, true).unwrap(), ATOMS_PER_QUB);
    assert_eq!(r2w.balance_atoms(&chain, &s, true).unwrap(), 2 * ATOMS_PER_QUB);

    let blast = wallet.create_blast_create_transaction_qub(&chain, &s, Amount::from_str("5").unwrap(), Amount::from_str("1").unwrap(), 5, "testBlastCode_123", Amount::from_str("0.00001").unwrap()).unwrap();
    let blast_id = blast.txid();
    chain.accept_transaction_to_mempool(blast, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    chain.connect_block(b, &s).unwrap();
    let code = make_blast_code_payload(blast_id, 0, "testBlastCode_123").unwrap();
    let claim = r1w.create_blast_claim_transaction_qub(&chain, &s, &code, None).unwrap();
    let claim_id = claim.txid();
    chain.accept_transaction_to_mempool(claim, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    assert!(b.transactions.iter().any(|tx| tx.txid() == claim_id));
    chain.connect_block(b, &s).unwrap();
    assert!(r1w.balance_atoms(&chain, &s, true).unwrap() >= 2 * ATOMS_PER_QUB);

    // The same creator QR/code remains usable after the vault outpoint moves:
    // the wallet helper falls back to finding the active vault by code hash.
    let claim2 = r2w.create_blast_claim_transaction_qub(&chain, &s, &code, None).unwrap();
    let claim2_id = claim2.txid();
    chain.accept_transaction_to_mempool(claim2, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    assert!(b.transactions.iter().any(|tx| tx.txid() == claim2_id));
    chain.connect_block(b, &s).unwrap();
    assert!(r2w.balance_atoms(&chain, &s, true).unwrap() >= 3 * ATOMS_PER_QUB);
}

#[test]
fn library_post_comment_vote_and_delete_are_consensus_valid() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut wallet = WalletFile::new(&s.network.name);
    let miner_key = wallet.create_key(&s, "library-author", 0).unwrap();
    let miner = Address::parse_with_prefix(&miner_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };
    for _ in 0..8 {
        let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }

    let post = wallet.create_library_post_transaction(&chain, &s, "Hello QUB", "general", "First Library post", Amount::from_str("0.00001").unwrap()).unwrap();
    let post_id = post.txid().to_string();
    chain.accept_transaction_to_mempool(post, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    chain.connect_block(b, &s).unwrap();
    let state = library_state_from_blocks(&s, &chain.blocks).unwrap();
    assert_eq!(state.posts.iter().filter(|p| !p.deleted).count(), 1);

    let comment = wallet.create_library_comment_transaction(&chain, &s, &post_id, None, "Nice post", Amount::from_str("0.00001").unwrap()).unwrap();
    chain.accept_transaction_to_mempool(comment, &s).unwrap();
    let vote = wallet.create_library_vote_transaction(&chain, &s, "post", &post_id, true, Amount::from_str("0.00001").unwrap()).unwrap();
    chain.accept_transaction_to_mempool(vote, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    chain.connect_block(b, &s).unwrap();
    let state = library_state_from_blocks(&s, &chain.blocks).unwrap();
    let post = state.posts.iter().find(|p| p.id == post_id).unwrap();
    assert_eq!(post.upvotes, 1);
    assert_eq!(post.comment_count, 1);

    let del = wallet.create_library_delete_transaction(&chain, &s, "post", &post_id, Amount::from_str("0.00001").unwrap()).unwrap();
    chain.accept_transaction_to_mempool(del, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    chain.connect_block(b, &s).unwrap();
    let state = library_state_from_blocks(&s, &chain.blocks).unwrap();
    assert!(state.posts.iter().find(|p| p.id == post_id).unwrap().deleted);
}

#[test]
fn jin_public_sale_buy_is_consensus_valid() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut buyer_wallet = WalletFile::new(&s.network.name);
    let buyer_key = buyer_wallet.create_key(&s, "buyer", 0).unwrap();
    let buyer = Address::parse_with_prefix(&buyer_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };
    for _ in 0..4 {
        let b = mine_next_block(&chain, &s, &buyer, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }
    let amount_units = parse_jin_amount("100").unwrap();
    let tx = buyer_wallet.create_jin_public_sale_buy_transaction(&chain, &s, 0, amount_units, Amount::from_str("0.00001").unwrap()).unwrap();
    let txid = tx.txid();
    chain.accept_transaction_to_mempool(tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &buyer, opts).unwrap();
    assert!(b.transactions.iter().any(|t| t.txid() == txid));
    chain.connect_block(b, &s).unwrap();
    assert_eq!(jin_balance_units_for_address(&s, &chain, &buyer.to_string()).unwrap(), amount_units);
    let listings = jin_sale_listings(&s, &chain).unwrap();
    assert!(listings[0].sold_units >= amount_units);
}

#[test]
fn hf117_reorg_resurrects_qub_tx_from_stale_block() {
    let s = regtest();
    let mut base = ChainState::new_with_genesis(&s).unwrap();
    let mut miner_wallet = WalletFile::new(&s.network.name);
    let miner_key = miner_wallet.create_key(&s, "miner", 0).unwrap();
    let miner = Address::parse_with_prefix(&miner_key.address, &s.network.address_prefix).unwrap();
    let mut other_wallet = WalletFile::new(&s.network.name);
    let other = Address::parse_with_prefix(&other_wallet.create_key(&s, "other", 0).unwrap().address, &s.network.address_prefix).unwrap();
    let mut recipient_wallet = WalletFile::new(&s.network.name);
    let recipient = Address::parse_with_prefix(&recipient_wallet.create_key(&s, "recipient", 0).unwrap().address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };

    for _ in 0..s.consensus.coinbase_maturity.saturating_add(2) {
        let b = mine_next_block(&base, &s, &miner, opts).unwrap();
        base.connect_block(b, &s).unwrap();
    }

    let tx = miner_wallet.create_signed_transaction(
        &base,
        &s,
        &recipient,
        Amount::from_str("1.0").unwrap(),
        Amount::from_str("0.00001").unwrap(),
    ).unwrap();
    let txid = tx.txid();

    let mut stale_local = base.clone();
    stale_local.accept_transaction_to_mempool(tx.clone(), &s).unwrap();
    let stale_block = mine_next_block(&stale_local, &s, &miner, opts).unwrap();
    assert!(stale_block.transactions.iter().any(|t| t.txid() == txid));
    stale_local.connect_block(stale_block, &s).unwrap();
    assert!(!stale_local.tx_in_mempool(txid));

    let mut winning = base.clone();
    for _ in 0..2 {
        let b = mine_next_block(&winning, &s, &other, opts).unwrap();
        winning.connect_block(b, &s).unwrap();
    }
    assert!(winning.height() > stale_local.height());
    assert!(stale_local.try_adopt_peer_chain(winning.blocks.clone(), &s, false).unwrap());
    assert!(stale_local.tx_in_mempool(txid), "HF117 should reaccept the QUB tx from the disconnected stale block");
}

#[test]
fn hf120_protocol_epoch_2_version_gate_is_forward_only() {
    let s: Settings = toml::from_str(include_str!("../config/mainnet.toml")).expect("mainnet config parses");
    assert_eq!(MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT, 24000);
    assert_eq!(protocol_epoch_2_activation_height(&s), 24000);
    assert_eq!(expected_block_version(&s, MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT - 1), s.consensus.version);
    assert_eq!(expected_block_version(&s, MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT), PROTOCOL_EPOCH_2_BLOCK_VERSION);
    assert_eq!(expected_block_version(&s, MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT + 1), PROTOCOL_EPOCH_2_BLOCK_VERSION);
}
