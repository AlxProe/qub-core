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
    assert_eq!(
        Address::parse_with_prefix(&enc, &s.network.address_prefix).unwrap(),
        addr
    );
    let mut bad = enc.clone();
    let last = bad.pop().unwrap();
    bad.push(if last == '0' { '1' } else { '0' });
    assert!(Address::from_str(&bad).is_err());
}

#[test]
fn target_decodes() {
    let s = regtest();
    assert!(target_from_compact(s.pow_bits().unwrap())
        .unwrap()
        .iter()
        .any(|b| *b != 0));
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
    let recipient =
        Address::parse_with_prefix(&rec_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };
    for _ in 0..3 {
        let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }
    assert!(miner_wallet.balance_atoms(&chain, &s, false).unwrap() > 0);
    let tx = miner_wallet
        .create_signed_transaction(
            &chain,
            &s,
            &recipient,
            Amount::from_str("1.25").unwrap(),
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let txid = tx.txid();
    chain.accept_transaction_to_mempool(tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    assert!(b.transactions.iter().any(|t| t.txid() == txid));
    chain.connect_block(b, &s).unwrap();
    assert_eq!(chain.mempool.len(), 0);
    assert_eq!(
        recipient_wallet.balance_atoms(&chain, &s, true).unwrap(),
        Amount::from_str("1.25").unwrap().atoms()
    );
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
    let recipient =
        Address::parse_with_prefix(&rec_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };
    for _ in 0..3 {
        let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }
    let mut tx = miner_wallet
        .create_signed_transaction(
            &chain,
            &s,
            &recipient,
            Amount::from_str("1").unwrap(),
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    tx.outputs[0].value = Amount::from_str("2").unwrap();
    assert!(validate_tx_contextual(&tx, &chain.utxos, chain.height() + 1, &s, true).is_err());
}

#[test]
fn equal_work_forks_do_not_flip_without_explicit_preference() {
    let s = regtest();
    let base = ChainState::new_with_genesis(&s).unwrap();
    let mut w1 = WalletFile::new(&s.network.name);
    let a1 = Address::parse_with_prefix(
        &w1.create_key(&s, "a", 0).unwrap().address,
        &s.network.address_prefix,
    )
    .unwrap();
    let mut w2 = WalletFile::new(&s.network.name);
    let a2 = Address::parse_with_prefix(
        &w2.create_key(&s, "b", 0).unwrap().address,
        &s.network.address_prefix,
    )
    .unwrap();
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };

    let b1 = mine_next_block(&base, &s, &a1, opts).unwrap();
    let b2 = mine_next_block(&base, &s, &a2, opts).unwrap();
    assert_ne!(b1.block_hash(), b2.block_hash());

    let mut c1 = ChainState::new_with_genesis(&s).unwrap();
    c1.connect_block(b1, &s).unwrap();
    let mut c2 = ChainState::new_with_genesis(&s).unwrap();
    c2.connect_block(b2, &s).unwrap();

    let c1_tip = c1.tip_hash().to_string();
    let c2_tip = c2.tip_hash().to_string();
    let mut higher_tip_chain = if c1_tip > c2_tip {
        c1.clone()
    } else {
        c2.clone()
    };
    let lower_tip_blocks = if c1_tip < c2_tip {
        c1.blocks.as_ref().clone()
    } else {
        c2.blocks.as_ref().clone()
    };

    let original_tip = higher_tip_chain.tip_hash().to_string();
    assert!(!higher_tip_chain
        .try_adopt_peer_chain(lower_tip_blocks.clone(), &s, false)
        .unwrap());
    assert_eq!(higher_tip_chain.tip_hash().to_string(), original_tip);
    assert!(higher_tip_chain
        .try_adopt_peer_chain(lower_tip_blocks, &s, true)
        .unwrap());
    assert_eq!(higher_tip_chain.tip_hash().to_string(), c1_tip.min(c2_tip));
}

#[test]
fn reorg_resurrects_tx_from_abandoned_suffix_into_mempool() {
    let s = regtest();
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };

    let mut funder_wallet = WalletFile::new(&s.network.name);
    let funder_key = funder_wallet.create_key(&s, "funder", 0).unwrap();
    let funder =
        Address::parse_with_prefix(&funder_key.address, &s.network.address_prefix).unwrap();

    let mut recipient_wallet = WalletFile::new(&s.network.name);
    let recipient_key = recipient_wallet.create_key(&s, "recipient", 0).unwrap();
    let recipient =
        Address::parse_with_prefix(&recipient_key.address, &s.network.address_prefix).unwrap();

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

    assert!(stale
        .try_adopt_peer_chain(winning.blocks.as_ref().clone(), &s, false)
        .unwrap());
    assert!(stale.tx_in_mempool(txid));
}

#[test]
fn qns_price_and_registration_are_deterministic() {
    let s = regtest();
    assert_eq!(
        normalize_qns_name("Alice.QUB", s.qns.max_label_chars).unwrap(),
        "alice.qub"
    );
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

    for _ in 0..s
        .qns
        .activation_height
        .max(s.consensus.coinbase_maturity + 1)
    {
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
    assert_eq!(
        parse_jin_units_raw(&s.jin.total_supply_units).unwrap(),
        JIN_TOTAL_SUPPLY_UNITS
    );
    assert_eq!(parse_jin_amount("1").unwrap(), JIN_UNITS_PER_COIN);
    assert_eq!(parse_jin_amount("0.000000000000000001").unwrap(), 1);
    let chain = ChainState::new_with_genesis(&s).unwrap();
    assert_eq!(
        jin_balance_units_for_address(&s, &chain, &s.jin.protocol_address).unwrap(),
        0
    );
}

#[test]
fn pools_settings_and_names_are_deterministic() {
    let s = regtest();
    assert!(s.features.pooled_mining_enabled);
    assert!(s.pools.enabled);
    assert_eq!(s.pools.activation_height, 1);
    assert_eq!(s.pools.protocol_name, "pools.qub");
    assert_eq!(
        normalize_pool_name(
            "🔥Dragon Pool🔥",
            s.pools.max_name_chars,
            s.pools.max_name_bytes
        )
        .unwrap(),
        "🔥Dragon Pool🔥"
    );
    assert!(normalize_pool_name(
        "bad\u{200b}pool",
        s.pools.max_name_chars,
        s.pools.max_name_bytes
    )
    .is_err());
    assert!(
        normalize_pool_name("bad\nname", s.pools.max_name_chars, s.pools.max_name_bytes).is_err()
    );
    assert!(capacity_slots_valid(&s, s.pools.base_capacity_slots));
    assert!(extra_capacity_slots_valid(&s, s.pools.capacity_step_slots));
}

#[test]
fn pool_create_share_and_payout_are_consensus_valid() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut manager_wallet = WalletFile::new(&s.network.name);
    let manager_key = manager_wallet.create_key(&s, "manager", 0).unwrap();
    let manager =
        Address::parse_with_prefix(&manager_key.address, &s.network.address_prefix).unwrap();
    let mut miner_wallet = WalletFile::new(&s.network.name);
    let share_miner_key = miner_wallet.create_key(&s, "share-miner", 0).unwrap();
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };

    for _ in 0..8 {
        let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }
    assert_eq!(miner_wallet.balance_atoms(&chain, &s, true).unwrap(), 0);

    let create_tx = manager_wallet
        .create_pool_create_transaction(
            &chain,
            &s,
            "🔥Fair Pool🔥",
            &manager,
            500,
            s.pools.base_capacity_slots,
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let pool_id = create_tx.txid();
    chain.accept_transaction_to_mempool(create_tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    assert!(b.transactions.iter().any(|tx| tx.txid() == pool_id));
    chain.connect_block(b, &s).unwrap();
    assert!(pools_registry_from_blocks(&s, &chain.blocks)
        .unwrap()
        .contains_key(&pool_id));

    let parent_height = chain.height();
    let parent_hash = chain.tip_hash();
    let mut nonce = 0u64;
    while !pool_share_meets_target(
        &s,
        pool_id,
        &share_miner_key.address,
        parent_height,
        parent_hash,
        nonce,
    )
    .unwrap()
    {
        nonce = nonce.wrapping_add(1);
    }
    let share_tx = create_pool_share_transaction(
        &s,
        pool_id,
        &share_miner_key,
        parent_height,
        parent_hash,
        nonce,
    )
    .unwrap();
    let share_txid = share_tx.txid();
    chain.accept_transaction_to_mempool(share_tx, &s).unwrap();

    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    assert!(b.transactions.iter().any(|tx| tx.txid() == share_txid));
    chain.connect_block(b, &s).unwrap();

    let pool_block = mine_next_pool_block(&chain, &s, pool_id, opts).unwrap();
    assert_eq!(parse_pool_block_marker(&pool_block), Some(pool_id));
    let expected_outputs = expected_pool_coinbase_outputs(
        &s,
        &chain.blocks,
        pool_id,
        block_subsidy((chain.height() + 1) as u64, &s) as u128,
    )
    .unwrap();
    assert_eq!(pool_block.transactions[0].outputs, expected_outputs);
    chain.connect_block(pool_block, &s).unwrap();
}

#[test]
fn pool_commission_can_only_decrease() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut wallet = WalletFile::new(&s.network.name);
    let manager_key = wallet.create_key(&s, "manager", 0).unwrap();
    let manager =
        Address::parse_with_prefix(&manager_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };
    for _ in 0..8 {
        let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }
    let create_tx = wallet
        .create_pool_create_transaction(
            &chain,
            &s,
            "Commission Test",
            &manager,
            1000,
            s.pools.base_capacity_slots,
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let pool_id = create_tx.txid();
    chain.accept_transaction_to_mempool(create_tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    chain.connect_block(b, &s).unwrap();

    assert!(wallet
        .create_pool_set_commission_transaction(
            &chain,
            &s,
            pool_id,
            1500,
            Amount::from_str("0.00001").unwrap()
        )
        .is_err());
    assert!(wallet
        .create_pool_set_commission_transaction(
            &chain,
            &s,
            pool_id,
            500,
            Amount::from_str("0.00001").unwrap()
        )
        .is_ok());
}

#[test]
fn pool_manager_can_rename_and_top_up_capacity() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut wallet = WalletFile::new(&s.network.name);
    let manager_key = wallet.create_key(&s, "manager", 0).unwrap();
    let manager =
        Address::parse_with_prefix(&manager_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };

    for _ in 0..20 {
        let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }

    let create_tx = wallet
        .create_pool_create_transaction(
            &chain,
            &s,
            "Original Pool",
            &manager,
            600,
            s.pools.base_capacity_slots,
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let pool_id = create_tx.txid();
    chain.accept_transaction_to_mempool(create_tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    chain.connect_block(b, &s).unwrap();

    let rename_tx = wallet
        .create_pool_rename_transaction(
            &chain,
            &s,
            pool_id,
            "🔥Renamed Pool🔥",
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    chain.accept_transaction_to_mempool(rename_tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    chain.connect_block(b, &s).unwrap();

    let topup_tx = wallet
        .create_pool_topup_transaction(
            &chain,
            &s,
            pool_id,
            s.pools.capacity_step_slots,
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    chain.accept_transaction_to_mempool(topup_tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &manager, opts).unwrap();
    chain.connect_block(b, &s).unwrap();

    let registry = pools_registry_from_blocks(&s, &chain.blocks).unwrap();
    let pool = registry.get(&pool_id).unwrap();
    assert_eq!(pool.name, "🔥Renamed Pool🔥");
    assert_eq!(
        pool.capacity_slots,
        s.pools.base_capacity_slots + s.pools.capacity_step_slots
    );
}

#[test]
fn multi_send_and_blast_qub_are_consensus_valid() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut wallet = WalletFile::new(&s.network.name);
    let miner_key = wallet.create_key(&s, "miner", 0).unwrap();
    let miner = Address::parse_with_prefix(&miner_key.address, &s.network.address_prefix).unwrap();
    let mut r1w = WalletFile::new(&s.network.name);
    let r1 = Address::parse_with_prefix(
        &r1w.create_key(&s, "r1", 0).unwrap().address,
        &s.network.address_prefix,
    )
    .unwrap();
    let mut r2w = WalletFile::new(&s.network.name);
    let r2 = Address::parse_with_prefix(
        &r2w.create_key(&s, "r2", 0).unwrap().address,
        &s.network.address_prefix,
    )
    .unwrap();
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };
    for _ in 0..8 {
        let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }

    let multi = wallet
        .create_multi_signed_transaction(
            &chain,
            &s,
            &[
                (r1.clone(), Amount::from_str("1").unwrap()),
                (r2.clone(), Amount::from_str("2").unwrap()),
            ],
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let multi_id = multi.txid();
    chain.accept_transaction_to_mempool(multi, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    assert!(b.transactions.iter().any(|tx| tx.txid() == multi_id));
    chain.connect_block(b, &s).unwrap();
    assert_eq!(r1w.balance_atoms(&chain, &s, true).unwrap(), ATOMS_PER_QUB);
    assert_eq!(
        r2w.balance_atoms(&chain, &s, true).unwrap(),
        2 * ATOMS_PER_QUB
    );

    let blast = wallet
        .create_blast_create_transaction_qub(
            &chain,
            &s,
            Amount::from_str("5").unwrap(),
            Amount::from_str("1").unwrap(),
            5,
            "testBlastCode_123",
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let blast_id = blast.txid();
    chain.accept_transaction_to_mempool(blast, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    chain.connect_block(b, &s).unwrap();
    let code = make_blast_code_payload(blast_id, 0, "testBlastCode_123").unwrap();
    let claim = r1w
        .create_blast_claim_transaction_qub(&chain, &s, &code, None)
        .unwrap();
    let claim_id = claim.txid();
    chain.accept_transaction_to_mempool(claim, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    assert!(b.transactions.iter().any(|tx| tx.txid() == claim_id));
    chain.connect_block(b, &s).unwrap();
    assert!(r1w.balance_atoms(&chain, &s, true).unwrap() >= 2 * ATOMS_PER_QUB);

    // The same creator QR/code remains usable after the vault outpoint moves:
    // the wallet helper falls back to finding the active vault by code hash.
    let claim2 = r2w
        .create_blast_claim_transaction_qub(&chain, &s, &code, None)
        .unwrap();
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
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };
    for _ in 0..8 {
        let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }

    let post = wallet
        .create_library_post_transaction(
            &chain,
            &s,
            "Hello QUB",
            "general",
            "First Library post",
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let post_id = post.txid().to_string();
    chain.accept_transaction_to_mempool(post, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    chain.connect_block(b, &s).unwrap();
    let state = library_state_from_blocks(&s, &chain.blocks).unwrap();
    assert_eq!(state.posts.iter().filter(|p| !p.deleted).count(), 1);

    let comment = wallet
        .create_library_comment_transaction(
            &chain,
            &s,
            &post_id,
            None,
            "Nice post",
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    chain.accept_transaction_to_mempool(comment, &s).unwrap();
    let vote = wallet
        .create_library_vote_transaction(
            &chain,
            &s,
            "post",
            &post_id,
            true,
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    chain.accept_transaction_to_mempool(vote, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    chain.connect_block(b, &s).unwrap();
    let state = library_state_from_blocks(&s, &chain.blocks).unwrap();
    let post = state.posts.iter().find(|p| p.id == post_id).unwrap();
    assert_eq!(post.upvotes, 1);
    assert_eq!(post.comment_count, 1);

    let del = wallet
        .create_library_delete_transaction(
            &chain,
            &s,
            "post",
            &post_id,
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    chain.accept_transaction_to_mempool(del, &s).unwrap();
    let b = mine_next_block(&chain, &s, &miner, opts).unwrap();
    chain.connect_block(b, &s).unwrap();
    let state = library_state_from_blocks(&s, &chain.blocks).unwrap();
    assert!(
        state
            .posts
            .iter()
            .find(|p| p.id == post_id)
            .unwrap()
            .deleted
    );
}

#[test]
fn jin_public_sale_buy_is_consensus_valid() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut buyer_wallet = WalletFile::new(&s.network.name);
    let buyer_key = buyer_wallet.create_key(&s, "buyer", 0).unwrap();
    let buyer = Address::parse_with_prefix(&buyer_key.address, &s.network.address_prefix).unwrap();
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };
    for _ in 0..4 {
        let b = mine_next_block(&chain, &s, &buyer, opts).unwrap();
        chain.connect_block(b, &s).unwrap();
    }
    let amount_units = parse_jin_amount("100").unwrap();
    let tx = buyer_wallet
        .create_jin_public_sale_buy_transaction(
            &chain,
            &s,
            0,
            amount_units,
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let txid = tx.txid();
    chain.accept_transaction_to_mempool(tx, &s).unwrap();
    let b = mine_next_block(&chain, &s, &buyer, opts).unwrap();
    assert!(b.transactions.iter().any(|t| t.txid() == txid));
    chain.connect_block(b, &s).unwrap();
    assert_eq!(
        jin_balance_units_for_address(&s, &chain, &buyer.to_string()).unwrap(),
        amount_units
    );
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
    let other = Address::parse_with_prefix(
        &other_wallet.create_key(&s, "other", 0).unwrap().address,
        &s.network.address_prefix,
    )
    .unwrap();
    let mut recipient_wallet = WalletFile::new(&s.network.name);
    let recipient = Address::parse_with_prefix(
        &recipient_wallet
            .create_key(&s, "recipient", 0)
            .unwrap()
            .address,
        &s.network.address_prefix,
    )
    .unwrap();
    let opts = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };

    for _ in 0..s.consensus.coinbase_maturity.saturating_add(2) {
        let b = mine_next_block(&base, &s, &miner, opts).unwrap();
        base.connect_block(b, &s).unwrap();
    }

    let tx = miner_wallet
        .create_signed_transaction(
            &base,
            &s,
            &recipient,
            Amount::from_str("1.0").unwrap(),
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let txid = tx.txid();

    let mut stale_local = base.clone();
    stale_local
        .accept_transaction_to_mempool(tx.clone(), &s)
        .unwrap();
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
    assert!(stale_local
        .try_adopt_peer_chain(winning.blocks.as_ref().clone(), &s, false)
        .unwrap());
    assert!(
        stale_local.tx_in_mempool(txid),
        "HF117 should reaccept the QUB tx from the disconnected stale block"
    );
}

#[test]
fn hf120_protocol_epoch_2_version_gate_is_forward_only() {
    let s: Settings =
        toml::from_str(include_str!("../config/mainnet.toml")).expect("mainnet config parses");
    assert_eq!(MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT, 24000);
    assert_eq!(protocol_epoch_2_activation_height(&s), 24000);
    assert_eq!(
        expected_block_version(&s, MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT - 1),
        s.consensus.version
    );
    assert_eq!(
        expected_block_version(&s, MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT),
        PROTOCOL_EPOCH_2_BLOCK_VERSION
    );
    assert_eq!(
        expected_block_version(&s, MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT + 1),
        PROTOCOL_EPOCH_2_BLOCK_VERSION
    );
}

#[test]
fn hf121_keeps_hf120_epoch2_anchor_for_pre_and_post_24000() {
    let s: Settings =
        toml::from_str(include_str!("../config/mainnet.toml")).expect("mainnet config parses");
    assert_eq!(MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT, 24000);
    assert_eq!(protocol_epoch_2_activation_height(&s), 24000);
    assert_eq!(
        expected_block_version(&s, 23999),
        PROTOCOL_EPOCH_1_BLOCK_VERSION
    );
    assert_eq!(
        expected_block_version(&s, 24000),
        PROTOCOL_EPOCH_2_BLOCK_VERSION
    );
    assert_eq!(
        expected_block_version(&s, 24001),
        PROTOCOL_EPOCH_2_BLOCK_VERSION
    );
    assert!(!protocol_epoch_2_active(&s, 23999));
    assert!(protocol_epoch_2_active(&s, 24000));
    assert!(protocol_epoch_2_active(&s, 24001));
}

#[test]
fn hf123_fast_storage_status_metadata_and_export() {
    let mut s = regtest();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let data_dir = std::env::temp_dir().join(format!(
        "qub-hf123-fast-status-{}-{unique}",
        std::process::id()
    ));
    s.node.data_dir = data_dir.display().to_string();

    let chain = ChainState::new_with_genesis(&s).unwrap();
    save_chain(&s, &chain).unwrap();

    let paths = NodePaths::from_settings(&s);
    assert!(paths.fast_storage_exists());
    assert!(paths.fast_pointer_file.exists());
    assert!(paths.chain_file.exists());
    assert!(paths.chain_status_file.exists());

    let (cached, cached_source) = load_fast_chain_status(&s).unwrap();
    assert_eq!(cached_source, FastChainStatusSource::FastStorageMetadata);
    assert_eq!(cached.schema_version, FAST_CHAIN_STATUS_SCHEMA_VERSION);
    assert_eq!(cached.network, s.network.name);
    assert_eq!(cached.height, 0);
    assert_eq!(cached.tip_hash, chain.tip_hash());
    assert_eq!(cached.tip_block_version, PROTOCOL_EPOCH_1_BLOCK_VERSION);
    assert_eq!(cached.storage_engine, HF123_FAST_STORAGE_MAGIC);

    std::fs::remove_file(&paths.chain_status_file).unwrap();
    let (recovered, recovered_source) = load_fast_chain_status(&s).unwrap();
    assert_eq!(recovered_source, FastChainStatusSource::FastStorageMetadata);
    assert_eq!(recovered.height, cached.height);
    assert_eq!(recovered.tip_hash, cached.tip_hash);
    assert!(paths.chain_status_file.exists());

    let export = data_dir.join("explicit-export.json");
    let (height, tip, bytes) = export_chain_json(&s, &export).unwrap();
    assert_eq!(height, 0);
    assert_eq!(tip, chain.tip_hash());
    assert!(bytes > 0);
    let persisted: PersistedChainState =
        serde_json::from_slice(&std::fs::read(&export).unwrap()).unwrap();
    assert_eq!(persisted.network, s.network.name);
    assert_eq!(&persisted.blocks, chain.blocks.as_ref());

    let stats = fast_storage_stats(&s).unwrap();
    assert!(stats.ok);
    assert_eq!(stats.storage_engine, HF123_FAST_STORAGE_MAGIC);
    assert_eq!(stats.height, 0);

    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn hf123_mining_observability_is_neutral_aggregation_only() {
    let s = regtest();
    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    let mut wallet_a = WalletFile::new(&s.network.name);
    let mut wallet_b = WalletFile::new(&s.network.name);
    let a = Address::parse_with_prefix(
        &wallet_a.create_key(&s, "miner-a", 0).unwrap().address,
        &s.network.address_prefix,
    )
    .unwrap();
    let b = Address::parse_with_prefix(
        &wallet_b.create_key(&s, "miner-b", 0).unwrap().address,
        &s.network.address_prefix,
    )
    .unwrap();
    let options = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };

    for index in 0..8 {
        let miner = if index % 2 == 0 { &a } else { &b };
        let block = mine_next_block(&chain, &s, miner, options).unwrap();
        chain.connect_block(block, &s).unwrap();
    }

    let stats = mining_stats_json(&s, &chain.blocks, 8);
    assert_eq!(stats["window_blocks"].as_u64(), Some(8));
    assert_eq!(stats["unique_payout_labels"].as_u64(), Some(2));
    assert_eq!(stats["distribution"].as_array().map(Vec::len), Some(2));
    let keys = stats.as_object().unwrap().keys().cloned().collect::<std::collections::BTreeSet<_>>();
    let expected = [
        "block_versions",
        "coinbase_only_blocks",
        "coinbase_only_percent",
        "distribution",
        "effective_label_count",
        "from_height",
        "hhi",
        "hhi_10000",
        "interpretation_note",
        "interval_seconds",
        "network",
        "ok",
        "requested_window",
        "tip_hash",
        "to_height",
        "top_label_share_percent",
        "unique_payout_labels",
        "window_blocks",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(keys, expected);
}

#[test]
fn hf122_rpc_config_is_bounded_and_protocol_epoch_2_is_unchanged() {
    let mainnet: Settings =
        toml::from_str(include_str!("../config/mainnet.toml")).expect("mainnet config parses");
    mainnet.validate().expect("mainnet settings validate");
    assert!(!mainnet.rpc.enabled);
    assert!(!mainnet.rpc.allow_remote);
    assert!(mainnet.rpc.max_cached_jobs >= mainnet.rpc.max_template_batch);
    assert_eq!(protocol_epoch_2_activation_height(&mainnet), 24000);
    assert_eq!(expected_block_version(&mainnet, 23999), 1);
    assert_eq!(expected_block_version(&mainnet, 24000), 2);
    assert_eq!(expected_block_version(&mainnet, 24001), 2);

    let headless: Settings = toml::from_str(include_str!("../config/headless-mainnet.toml"))
        .expect("headless mainnet config parses");
    headless.validate().expect("headless settings validate");
    assert!(headless.rpc.enabled);
    assert_eq!(headless.rpc.bind, "127.0.0.1:17445");
    assert_eq!(headless.node.data_dir, "/opt/qub/headless/data/mainnet");
}

#[test]
fn hf123_chainstate_snapshots_share_immutable_state() {
    let s = regtest();
    let original = ChainState::new_with_genesis(&s).unwrap();
    let snapshot = original.clone();

    assert!(std::sync::Arc::ptr_eq(&original.blocks, &snapshot.blocks));
    assert!(std::sync::Arc::ptr_eq(&original.utxos, &snapshot.utxos));
    assert!(std::sync::Arc::ptr_eq(&original.mempool, &snapshot.mempool));

    let mut advanced = snapshot;
    let mut wallet = WalletFile::new(&s.network.name);
    let miner = Address::parse_with_prefix(
        &wallet.create_key(&s, "hf123-cow-miner", 0).unwrap().address,
        &s.network.address_prefix,
    )
    .unwrap();
    let block = mine_next_block(
        &advanced,
        &s,
        &miner,
        MiningOptions {
            duty_cycle_percent: 100,
            max_hashes: Some(5_000_000),
        },
    )
    .unwrap();
    advanced.connect_block(block, &s).unwrap();

    assert_eq!(original.height(), 0);
    assert_eq!(advanced.height(), 1);
    assert!(!std::sync::Arc::ptr_eq(&original.blocks, &advanced.blocks));
    assert!(!std::sync::Arc::ptr_eq(&original.utxos, &advanced.utxos));
}

#[test]
fn hf123_fast_storage_truncates_uncommitted_suffix_and_recovers_previous_pointer() {
    use std::io::Write;

    let mut s = regtest();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let data_dir = std::env::temp_dir().join(format!(
        "qub-hf123-recovery-{}-{unique}",
        std::process::id()
    ));
    s.node.data_dir = data_dir.display().to_string();

    let mut wallet = WalletFile::new(&s.network.name);
    let miner = Address::parse_with_prefix(
        &wallet.create_key(&s, "hf123-recovery-miner", 0).unwrap().address,
        &s.network.address_prefix,
    )
    .unwrap();
    let options = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };

    let mut chain = ChainState::new_with_genesis(&s).unwrap();
    save_chain(&s, &chain).unwrap();
    let block1 = mine_next_block(&chain, &s, &miner, options).unwrap();
    chain.connect_block(block1, &s).unwrap();
    save_chain(&s, &chain).unwrap();

    let paths = NodePaths::from_settings(&s);
    let current: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&paths.fast_pointer_file).unwrap(),
    )
    .unwrap();
    let journal_name = current["blocks_file"].as_str().unwrap();
    let committed_bytes = current["journal_bytes"].as_u64().unwrap();
    let journal_path = paths.fast_storage_dir.join(journal_name);

    {
        let mut journal = std::fs::OpenOptions::new()
            .append(true)
            .open(&journal_path)
            .unwrap();
        journal.write_all(b"{uncommitted-crash-suffix}\n").unwrap();
        journal.flush().unwrap();
    }
    assert!(std::fs::metadata(&journal_path).unwrap().len() > committed_bytes);

    let block2 = mine_next_block(&chain, &s, &miner, options).unwrap();
    chain.connect_block(block2, &s).unwrap();
    save_chain(&s, &chain).unwrap();
    let loaded = load_committed_chain(&s, true).unwrap();
    assert_eq!(loaded.height(), 2);
    assert_eq!(loaded.tip_hash(), chain.tip_hash());

    // A syntactically valid CURRENT pointer with a corrupt/missing state must
    // also recover through PREVIOUS, not only a malformed CURRENT.json file.
    let current_pointer: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&paths.fast_pointer_file).unwrap(),
    )
    .unwrap();
    let current_state_name = current_pointer["state_file"].as_str().unwrap();
    let current_state_path = paths.fast_storage_dir.join(current_state_name);
    let current_state_backup = std::fs::read(&current_state_path).unwrap();
    std::fs::write(&current_state_path, b"{corrupt-current-state").unwrap();
    let state_recovered = load_committed_chain(&s, true).unwrap();
    assert_eq!(state_recovered.height(), 1);
    assert_eq!(state_recovered.tip_hash(), chain.blocks[1].block_hash());
    std::fs::write(&current_state_path, current_state_backup).unwrap();

    std::fs::write(&paths.fast_pointer_file, b"{broken-current").unwrap();
    let recovered = load_committed_chain(&s, true).unwrap();
    assert_eq!(recovered.height(), 1);
    assert_eq!(recovered.tip_hash(), chain.blocks[1].block_hash());

    save_chain(&s, &chain).unwrap();
    let restored = load_committed_chain(&s, true).unwrap();
    assert_eq!(restored.height(), 2);
    assert_eq!(restored.tip_hash(), chain.tip_hash());

    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn hf123_migrates_valid_legacy_chain_once() {
    let mut s = regtest();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let data_dir = std::env::temp_dir().join(format!(
        "qub-hf123-migration-{}-{unique}",
        std::process::id()
    ));
    s.node.data_dir = data_dir.display().to_string();

    let paths = NodePaths::from_settings(&s);
    paths.ensure_dirs().unwrap();
    let chain = ChainState::new_with_genesis(&s).unwrap();
    std::fs::write(
        &paths.chain_file,
        serde_json::to_vec_pretty(&chain.to_persisted()).unwrap(),
    )
    .unwrap();

    assert!(!paths.fast_storage_exists());
    let migrated = load_or_init_chain(&s).unwrap();
    assert_eq!(migrated.tip_hash(), chain.tip_hash());
    assert!(paths.fast_storage_exists());

    let second = load_or_init_chain(&s).unwrap();
    assert_eq!(second.tip_hash(), chain.tip_hash());
    assert_eq!(fast_storage_stats(&s).unwrap().height, 0);

    let _ = std::fs::remove_dir_all(data_dir);
}

fn hf124_pool_fixture(
) -> (
    Settings,
    ChainState,
    WalletFile,
    Address,
    Hash256,
    WalletKey,
    MiningOptions,
) {
    let mut settings = regtest();
    // Keep the template scan budget small enough to prove that a backlog of
    // priority-0 pool shares cannot hide an ordinary transaction beyond it.
    settings.consensus.max_block_transactions = 160;
    let mut chain = ChainState::new_with_genesis(&settings).unwrap();
    let mut manager_wallet = WalletFile::new(&settings.network.name);
    let manager_key = manager_wallet.create_key(&settings, "manager", 0).unwrap();
    let manager = Address::parse_with_prefix(
        &manager_key.address,
        &settings.network.address_prefix,
    )
    .unwrap();
    let mut share_wallet = WalletFile::new(&settings.network.name);
    let share_key = share_wallet
        .create_key(&settings, "share-miner", 0)
        .unwrap();
    let options = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };

    for _ in 0..8 {
        let block = mine_next_block(&chain, &settings, &manager, options).unwrap();
        chain.connect_block(block, &settings).unwrap();
    }

    let create = manager_wallet
        .create_pool_create_transaction(
            &chain,
            &settings,
            "HF124 Liveness Pool",
            &manager,
            500,
            settings.pools.base_capacity_slots,
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let pool_id = create.txid();
    chain
        .accept_transaction_to_mempool(create, &settings)
        .unwrap();
    let block = mine_next_block(&chain, &settings, &manager, options).unwrap();
    chain.connect_block(block, &settings).unwrap();

    (
        settings,
        chain,
        manager_wallet,
        manager,
        pool_id,
        share_key,
        options,
    )
}

fn hf124_make_pool_shares(
    settings: &Settings,
    chain: &ChainState,
    pool_id: Hash256,
    share_key: &WalletKey,
    count: usize,
    nonce_cursor: &mut u64,
) -> Vec<Transaction> {
    let parent_height = chain.height();
    let parent_hash = chain.tip_hash();
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let nonce = *nonce_cursor;
        *nonce_cursor = nonce.wrapping_add(1);
        if pool_share_meets_target(
            settings,
            pool_id,
            &share_key.address,
            parent_height,
            parent_hash,
            nonce,
        )
        .unwrap()
        {
            out.push(
                create_pool_share_transaction(
                    settings,
                    pool_id,
                    share_key,
                    parent_height,
                    parent_hash,
                    nonce,
                )
                .unwrap(),
            );
        }
    }
    out
}

#[test]
fn hf124_candidate_parts_are_reused_without_rebuilding_transaction_selection() {
    let (settings, mut chain, manager_wallet, manager, _pool_id, _share_key, _options) =
        hf124_pool_fixture();

    let mut recipient_wallet = WalletFile::new(&settings.network.name);
    let recipient_key = recipient_wallet
        .create_key(&settings, "candidate-parts-recipient", 0)
        .unwrap();
    let recipient = Address::parse_with_prefix(
        &recipient_key.address,
        &settings.network.address_prefix,
    )
    .unwrap();
    let ordinary = manager_wallet
        .create_signed_transaction(
            &chain,
            &settings,
            &recipient,
            Amount::from_str("1").unwrap(),
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let ordinary_txid = ordinary.txid();
    chain
        .accept_transaction_to_mempool(ordinary, &settings)
        .unwrap();

    let parts = build_candidate_block_parts(
        &chain,
        &settings,
        Some(&manager.to_string()),
    )
    .unwrap();
    assert!(parts
        .non_coinbase_transactions
        .iter()
        .any(|tx| tx.txid() == ordinary_txid));

    let coinbase_a = create_coinbase(parts.height, parts.reward_atoms, &manager, 11).unwrap();
    let coinbase_b = create_coinbase(parts.height, parts.reward_atoms, &manager, 12).unwrap();
    let block_a = block_from_candidate_parts(&parts, coinbase_a);
    let block_b = block_from_candidate_parts(&parts, coinbase_b);

    let expected_tail = parts
        .non_coinbase_transactions
        .iter()
        .map(Transaction::txid)
        .collect::<Vec<_>>();
    let tail_a = block_a
        .transactions
        .iter()
        .skip(1)
        .map(Transaction::txid)
        .collect::<Vec<_>>();
    let tail_b = block_b
        .transactions
        .iter()
        .skip(1)
        .map(Transaction::txid)
        .collect::<Vec<_>>();

    assert_eq!(tail_a, expected_tail);
    assert_eq!(tail_b, expected_tail);
    assert_ne!(block_a.transactions[0].txid(), block_b.transactions[0].txid());
    assert_ne!(block_a.header.merkle_root, block_b.header.merkle_root);
    assert_eq!(block_a.header.prev_block_hash, parts.prev_block_hash);
    assert_eq!(block_b.header.prev_block_hash, parts.prev_block_hash);
    assert_eq!(block_a.header.version, parts.version);
    assert_eq!(block_b.header.version, parts.version);
    assert_eq!(block_a.header.bits, parts.bits);
    assert_eq!(block_b.header.bits, parts.bits);
}

#[test]
fn hf124_candidate_caps_pool_shares_and_keeps_ordinary_transactions() {
    let (
        settings,
        mut chain,
        manager_wallet,
        manager,
        pool_id,
        share_key,
        options,
    ) = hf124_pool_fixture();
    let mut nonce = 0u64;
    let shares = hf124_make_pool_shares(
        &settings,
        &chain,
        pool_id,
        &share_key,
        400,
        &mut nonce,
    );
    let accepted = chain
        .accept_transactions_to_mempool_batch(shares, &settings)
        .unwrap();
    assert_eq!(accepted.len(), 400);
    assert!(400 > hf115_template_scan_limit(&settings));

    let mut recipient_wallet = WalletFile::new(&settings.network.name);
    let recipient_key = recipient_wallet
        .create_key(&settings, "recipient", 0)
        .unwrap();
    let recipient = Address::parse_with_prefix(
        &recipient_key.address,
        &settings.network.address_prefix,
    )
    .unwrap();
    let ordinary = manager_wallet
        .create_signed_transaction(
            &chain,
            &settings,
            &recipient,
            Amount::from_str("1").unwrap(),
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let ordinary_txid = ordinary.txid();
    chain
        .accept_transaction_to_mempool(ordinary, &settings)
        .unwrap();

    let parts = build_candidate_block_parts(
        &chain,
        &settings,
        Some(&manager.to_string()),
    )
    .unwrap();
    let selected_shares = parts
        .non_coinbase_transactions
        .iter()
        .filter(|tx| is_pool_share_transaction(tx))
        .count();
    assert_eq!(selected_shares, settings.pools.max_share_txs_per_block);
    assert!(parts
        .non_coinbase_transactions
        .iter()
        .any(|tx| tx.txid() == ordinary_txid));

    let block = mine_next_block(&chain, &settings, &manager, options).unwrap();
    assert_eq!(
        block
            .transactions
            .iter()
            .skip(1)
            .filter(|tx| is_pool_share_transaction(tx))
            .count(),
        settings.pools.max_share_txs_per_block
    );
    assert!(block
        .transactions
        .iter()
        .any(|tx| tx.txid() == ordinary_txid));
    chain.connect_block(block, &settings).unwrap();
}

#[test]
fn hf124_pool_share_mempool_policy_is_bounded_to_confirmable_horizon() {
    let (settings, mut chain, _wallet, _manager, pool_id, share_key, _options) =
        hf124_pool_fixture();
    let limit = hf124_pool_share_mempool_limit(&settings);
    assert_eq!(
        limit,
        settings.pools.max_share_txs_per_block * settings.pools.share_stale_blocks as usize
    );

    let mut nonce = 0u64;
    let shares = hf124_make_pool_shares(
        &settings,
        &chain,
        pool_id,
        &share_key,
        limit + 1,
        &mut nonce,
    );
    let accepted = chain
        .accept_transactions_to_mempool_batch(shares, &settings)
        .unwrap();
    assert_eq!(accepted.len(), limit);
    assert_eq!(
        chain
            .mempool
            .iter()
            .filter(|tx| is_pool_share_transaction(tx))
            .count(),
        limit
    );
}

#[test]
fn hf124_consensus_still_rejects_more_than_128_pool_shares() {
    let (settings, mut chain, _wallet, manager, pool_id, share_key, options) =
        hf124_pool_fixture();
    let mut nonce = 0u64;
    let shares = hf124_make_pool_shares(
        &settings,
        &chain,
        pool_id,
        &share_key,
        settings.pools.max_share_txs_per_block + 1,
        &mut nonce,
    );
    chain
        .accept_transactions_to_mempool_batch(shares, &settings)
        .unwrap();

    let mut block = mine_next_block(&chain, &settings, &manager, options).unwrap();
    let selected = block
        .transactions
        .iter()
        .map(Transaction::txid)
        .collect::<std::collections::HashSet<_>>();
    let extra = chain
        .mempool
        .iter()
        .find(|tx| is_pool_share_transaction(tx) && !selected.contains(&tx.txid()))
        .cloned()
        .expect("one capped share remains outside the candidate");
    block.transactions.push(extra);
    block.header.merkle_root = merkle_root(
        &block
            .transactions
            .iter()
            .map(Transaction::txid)
            .collect::<Vec<_>>(),
    );
    block.header.nonce = 0;
    while !verify_header_pow(&block.header).unwrap() {
        block.header.nonce = block.header.nonce.wrapping_add(1);
    }

    let err = chain.connect_block(block, &settings).unwrap_err();
    assert!(err.to_string().contains("too many pool share txs in block"));
}

#[test]
fn hf124_candidate_drains_oldest_confirmable_shares_first() {
    let (settings, mut chain, _wallet, manager, pool_id, share_key, options) =
        hf124_pool_fixture();
    let first_parent = chain.height();
    let mut nonce = 0u64;
    let old_shares = hf124_make_pool_shares(
        &settings,
        &chain,
        pool_id,
        &share_key,
        200,
        &mut nonce,
    );
    assert_eq!(
        chain
            .accept_transactions_to_mempool_batch(old_shares, &settings)
            .unwrap()
            .len(),
        200
    );

    let first_block = mine_next_block(&chain, &settings, &manager, options).unwrap();
    assert_eq!(
        first_block
            .transactions
            .iter()
            .skip(1)
            .filter(|tx| is_pool_share_transaction(tx))
            .count(),
        settings.pools.max_share_txs_per_block
    );
    chain.connect_block(first_block, &settings).unwrap();

    let old_remaining = chain
        .mempool
        .iter()
        .filter_map(parse_pool_share_tx)
        .filter(|share| share.parent_height == first_parent)
        .count();
    assert_eq!(old_remaining, 72);

    let second_parent = chain.height();
    let new_shares = hf124_make_pool_shares(
        &settings,
        &chain,
        pool_id,
        &share_key,
        100,
        &mut nonce,
    );
    assert_eq!(
        chain
            .accept_transactions_to_mempool_batch(new_shares, &settings)
            .unwrap()
            .len(),
        100
    );

    let parts = build_candidate_block_parts(
        &chain,
        &settings,
        Some(&manager.to_string()),
    )
    .unwrap();
    let selected = parts
        .non_coinbase_transactions
        .iter()
        .filter_map(parse_pool_share_tx)
        .collect::<Vec<_>>();

    assert_eq!(selected.len(), settings.pools.max_share_txs_per_block);
    assert_eq!(
        selected
            .iter()
            .filter(|share| share.parent_height == first_parent)
            .count(),
        72
    );
    assert_eq!(
        selected
            .iter()
            .filter(|share| share.parent_height == second_parent)
            .count(),
        settings.pools.max_share_txs_per_block - 72
    );
    assert!(selected
        .windows(2)
        .all(|pair| pair[0].parent_height <= pair[1].parent_height));
}

#[test]
fn hf124_same_tip_persistence_merges_concurrent_mempool_entries() {
    let (
        mut settings,
        chain,
        _wallet,
        _manager,
        pool_id,
        share_key,
        _options,
    ) = hf124_pool_fixture();

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let data_dir = std::env::temp_dir().join(format!(
        "qub-hf124-live-merge-{}-{unique}",
        std::process::id()
    ));
    settings.node.data_dir = data_dir.to_string_lossy().to_string();

    save_chain(&settings, &chain).unwrap();

    let mut nonce = 0u64;
    let shares = hf124_make_pool_shares(
        &settings,
        &chain,
        pool_id,
        &share_key,
        2,
        &mut nonce,
    );
    let first_txid = shares[0].txid();
    let second_txid = shares[1].txid();

    let mut live_state = chain.clone();
    live_state
        .accept_transaction_to_mempool(shares[0].clone(), &settings)
        .unwrap();
    let live = std::sync::Arc::new(std::sync::Mutex::new(live_state));
    register_live_chain(&settings, &live);

    let mut candidate = chain.clone();
    candidate
        .accept_transaction_to_mempool(shares[1].clone(), &settings)
        .unwrap();
    save_chain(&settings, &candidate).unwrap();

    let snapshot = live.lock().unwrap().clone();
    assert!(snapshot.tx_in_mempool(first_txid));
    assert!(snapshot.tx_in_mempool(second_txid));
    assert_eq!(snapshot.mempool.len(), 2);

    // Persist the merged owner once and prove both sides survive a fresh load.
    save_chain(&settings, &snapshot).unwrap();
    unregister_live_chain(&settings);
    let reloaded = load_or_init_chain(&settings).unwrap();
    assert!(reloaded.tx_in_mempool(first_txid));
    assert!(reloaded.tx_in_mempool(second_txid));

    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn hf124_stale_pool_share_does_not_block_ordinary_mempool_admission() {
    let (
        mut settings,
        mut chain,
        manager_wallet,
        manager,
        pool_id,
        share_key,
        options,
    ) = hf124_pool_fixture();

    let stale_parent = chain.height();
    let mut nonce = 0u64;
    let stale_share = hf124_make_pool_shares(
        &settings,
        &chain,
        pool_id,
        &share_key,
        1,
        &mut nonce,
    )
    .pop()
    .expect("share generated");

    // Advance beyond the confirmable share horizon without ever admitting the
    // share. Then emulate an older persisted mempool containing that stale item.
    for _ in 0..=settings.pools.share_stale_blocks {
        let block = mine_next_block(&chain, &settings, &manager, options).unwrap();
        chain.connect_block(block, &settings).unwrap();
    }
    assert!(chain.height().saturating_sub(stale_parent) > settings.pools.share_stale_blocks);
    std::sync::Arc::make_mut(&mut chain.mempool).push(stale_share);

    // Force the general mempool-full path. HF124 must prune the unconfirmable
    // share before applying the ordinary-transaction capacity check.
    settings.mempool.max_transactions = 1;
    let mut recipient_wallet = WalletFile::new(&settings.network.name);
    let recipient_key = recipient_wallet
        .create_key(&settings, "stale-share-recipient", 0)
        .unwrap();
    let recipient = Address::parse_with_prefix(
        &recipient_key.address,
        &settings.network.address_prefix,
    )
    .unwrap();
    let ordinary = manager_wallet
        .create_signed_transaction(
            &chain,
            &settings,
            &recipient,
            Amount::from_str("1").unwrap(),
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let ordinary_txid = ordinary.txid();

    chain
        .accept_transaction_to_mempool(ordinary, &settings)
        .unwrap();

    assert_eq!(chain.mempool.len(), 1);
    assert!(chain.tx_in_mempool(ordinary_txid));
    assert!(!chain.mempool.iter().any(is_pool_share_transaction));
}

#[test]
fn hf124_rebuild_skips_stale_legacy_prefix_and_retains_newer_valid_shares() {
    let (
        mut settings,
        mut chain,
        _manager_wallet,
        manager,
        pool_id,
        share_key,
        options,
    ) = hf124_pool_fixture();

    settings.pools.max_share_txs_per_block = 4;
    settings.pools.share_stale_blocks = 2;
    settings.mempool.max_transactions = 8;
    let share_limit = hf124_pool_share_mempool_limit(&settings);
    assert_eq!(share_limit, 8);

    let stale_parent_height = chain.height();
    let mut nonce = 0u64;
    let stale_shares = hf124_make_pool_shares(
        &settings,
        &chain,
        pool_id,
        &share_key,
        share_limit,
        &mut nonce,
    );

    for _ in 0..=settings.pools.share_stale_blocks {
        let block = mine_next_block(&chain, &settings, &manager, options).unwrap();
        chain.connect_block(block, &settings).unwrap();
    }
    assert!(
        chain.height().saturating_sub(stale_parent_height)
            > settings.pools.share_stale_blocks
    );

    let valid_parent_height = chain.height();
    let valid_parent_hash = chain.tip_hash();
    let valid_shares = hf124_make_pool_shares(
        &settings,
        &chain,
        pool_id,
        &share_key,
        share_limit,
        &mut nonce,
    );

    // Emulate a pre-HF124 persisted queue sorted with a full stale prefix.
    // Rebuild must continue scanning past rejected stale entries until the
    // dedicated share queue is filled by valid newer entries.
    let mut legacy = stale_shares;
    legacy.extend(valid_shares);
    std::sync::Arc::make_mut(&mut chain.mempool).extend(legacy.clone());

    let retained = chain.rebuild_mempool_from(legacy, &settings);
    assert_eq!(retained, share_limit);
    assert_eq!(chain.mempool.len(), share_limit);
    assert!(chain.mempool.iter().all(|tx| {
        parse_pool_share_tx(tx)
            .map(|share| {
                share.parent_height == valid_parent_height
                    && share.parent_hash == valid_parent_hash
            })
            .unwrap_or(false)
    }));
}

#[test]
fn hf124_rebuild_caps_legacy_share_queue_before_general_mempool_limit() {
    let (
        mut settings,
        mut chain,
        manager_wallet,
        _manager,
        pool_id,
        share_key,
        _options,
    ) = hf124_pool_fixture();

    settings.pools.max_share_txs_per_block = 4;
    settings.pools.share_stale_blocks = 2;
    settings.mempool.max_transactions = 9;
    let share_limit = hf124_pool_share_mempool_limit(&settings);
    assert_eq!(share_limit, 8);

    let mut nonce = 0u64;
    let shares = hf124_make_pool_shares(
        &settings,
        &chain,
        pool_id,
        &share_key,
        share_limit + 4,
        &mut nonce,
    );

    let mut recipient_wallet = WalletFile::new(&settings.network.name);
    let recipient_key = recipient_wallet
        .create_key(&settings, "rebuild-recipient", 0)
        .unwrap();
    let recipient = Address::parse_with_prefix(
        &recipient_key.address,
        &settings.network.address_prefix,
    )
    .unwrap();
    let ordinary = manager_wallet
        .create_signed_transaction(
            &chain,
            &settings,
            &recipient,
            Amount::from_str("1").unwrap(),
            Amount::from_str("0.00001").unwrap(),
        )
        .unwrap();
    let ordinary_txid = ordinary.txid();

    // Emulate a pre-HF124 persisted queue that exceeded the dedicated share
    // policy before startup/block-connect revalidation was available.
    let mut legacy = shares;
    legacy.push(ordinary);
    std::sync::Arc::make_mut(&mut chain.mempool).extend(legacy.clone());

    let retained = chain.rebuild_mempool_from(legacy, &settings);
    assert_eq!(retained, 9);
    assert_eq!(chain.mempool.len(), 9);
    assert_eq!(
        chain
            .mempool
            .iter()
            .filter(|tx| is_pool_share_transaction(tx))
            .count(),
        share_limit
    );
    assert!(chain.tx_in_mempool(ordinary_txid));
}

#[test]
fn hf124_pool_shares_are_not_persisted_or_resurrected_by_wallet_outbox() {
    let mut settings = regtest();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let data_dir = std::env::temp_dir().join(format!(
        "qub-hf124-share-outbox-{}-{unique}",
        std::process::id()
    ));
    settings.node.data_dir = data_dir.to_string_lossy().to_string();
    let mut chain = ChainState::new_with_genesis(&settings).unwrap();

    let share = Transaction {
        version: TX_VERSION_POOL_SHARE,
        inputs: vec![TxIn {
            previous_output: OutPoint::null(),
            signature_script: ScriptBuf(b"POOLSHARE1|legacy".to_vec()),
            sequence: u32::MAX,
        }],
        outputs: Vec::new(),
        locktime: 0,
    };

    remember_pending_tx(&settings, &chain, &share, "legacy-gui-pool-share").unwrap();
    assert!(load_pending_txs(&settings).unwrap().txs.is_empty());

    // Emulate an old HF117 outbox created before HF124 and prove startup/
    // heartbeat reconciliation removes the ephemeral share deterministically.
    let legacy = PendingTxFile {
        version: 1,
        network: settings.network.name.clone(),
        txs: vec![PendingTxRecord {
            txid: share.txid(),
            tx: share,
            label: "legacy-gui-pool-share".to_string(),
            created_height: chain.height(),
            created_unix: unix_time_u32() as u64,
            last_rebroadcast_unix: 0,
            confirmations_required: HF117_PENDING_TX_CONFIRMATIONS,
        }],
    };
    save_pending_txs(&settings, &legacy).unwrap();
    let report = reconcile_pending_txs(&settings, &mut chain).unwrap();
    assert_eq!(report.dropped, 1);
    assert!(load_pending_txs(&settings).unwrap().txs.is_empty());

    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn hf125_atomic_block_connect_persists_before_publishing_caller_state() {
    let mut settings = regtest();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let data_dir = std::env::temp_dir().join(format!(
        "qub-hf125-atomic-connect-{}-{unique}",
        std::process::id()
    ));
    settings.node.data_dir = data_dir.to_string_lossy().to_string();

    let mut chain = ChainState::new_with_genesis(&settings).unwrap();
    let mut wallet = WalletFile::new(&settings.network.name);
    let key = wallet.create_key(&settings, "hf125-miner", 0).unwrap();
    let miner = Address::parse_with_prefix(&key.address, &settings.network.address_prefix).unwrap();
    let options = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(5_000_000),
    };
    let block = mine_next_block(&chain, &settings, &miner, options).unwrap();
    let expected_hash = block.block_hash();

    let hash = connect_block_persist_atomic(&settings, &mut chain, block).unwrap();
    assert_eq!(hash, expected_hash);
    assert_eq!(chain.height(), 1);
    assert_eq!(chain.tip_hash(), expected_hash);

    let reloaded = load_or_init_chain_for_ui_fast(&settings).unwrap();
    assert_eq!(reloaded.height(), 1);
    assert_eq!(reloaded.tip_hash(), expected_hash);

    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn hf125_mainnet_storage_rejects_equal_work_same_height_tip_overwrite() {
    let mut settings = regtest();
    settings.network.name = "mainnet".to_string();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let data_dir = std::env::temp_dir().join(format!(
        "qub-hf125-equal-work-storage-{}-{unique}",
        std::process::id()
    ));
    settings.node.data_dir = data_dir.to_string_lossy().to_string();

    let base = ChainState::new_with_genesis(&settings).unwrap();
    let mut wallet_a = WalletFile::new(&settings.network.name);
    let mut wallet_b = WalletFile::new(&settings.network.name);
    let a = Address::parse_with_prefix(
        &wallet_a.create_key(&settings, "a", 0).unwrap().address,
        &settings.network.address_prefix,
    )
    .unwrap();
    let b = Address::parse_with_prefix(
        &wallet_b.create_key(&settings, "b", 0).unwrap().address,
        &settings.network.address_prefix,
    )
    .unwrap();
    let options = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(10_000_000),
    };

    let block_a = mine_next_block(&base, &settings, &a, options).unwrap();
    let block_b = mine_next_block(&base, &settings, &b, options).unwrap();
    assert_ne!(block_a.block_hash(), block_b.block_hash());

    let mut chain_a = base.clone();
    chain_a.connect_block(block_a, &settings).unwrap();
    save_chain(&settings, &chain_a).unwrap();

    let mut chain_b = base;
    chain_b.connect_block(block_b, &settings).unwrap();
    let err = save_chain(&settings, &chain_b).unwrap_err();
    assert!(err.to_string().contains("stale Fast Chain Engine persistence rejected"));

    let reloaded = load_or_init_chain_for_ui_fast(&settings).unwrap();
    assert_eq!(reloaded.tip_hash(), chain_a.tip_hash());

    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn hf126_verified_equal_work_reanchor_persists_and_updates_live_owner() {
    let mut settings = regtest();
    settings.network.name = "mainnet".to_string();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let data_dir = std::env::temp_dir().join(format!(
        "qub-hf126-equal-work-reanchor-{}-{unique}",
        std::process::id()
    ));
    settings.node.data_dir = data_dir.to_string_lossy().to_string();

    let base = ChainState::new_with_genesis(&settings).unwrap();
    let mut wallet_a = WalletFile::new(&settings.network.name);
    let mut wallet_b = WalletFile::new(&settings.network.name);
    let a = Address::parse_with_prefix(
        &wallet_a
            .create_key(&settings, "hf126-a", 0)
            .unwrap()
            .address,
        &settings.network.address_prefix,
    )
    .unwrap();
    let b = Address::parse_with_prefix(
        &wallet_b
            .create_key(&settings, "hf126-b", 0)
            .unwrap()
            .address,
        &settings.network.address_prefix,
    )
    .unwrap();
    let options = MiningOptions {
        duty_cycle_percent: 100,
        max_hashes: Some(10_000_000),
    };

    let block_a = mine_next_block(&base, &settings, &a, options).unwrap();
    let block_b = mine_next_block(&base, &settings, &b, options).unwrap();
    assert_ne!(block_a.block_hash(), block_b.block_hash());

    let mut chain_a = base.clone();
    chain_a.connect_block(block_a, &settings).unwrap();
    save_chain(&settings, &chain_a).unwrap();

    let live = std::sync::Arc::new(std::sync::Mutex::new(chain_a.clone()));
    register_live_chain(&settings, &live);

    let mut chain_b = base;
    chain_b.connect_block(block_b, &settings).unwrap();

    // Ordinary persistence must remain fork-monotonic.
    assert!(save_chain(&settings, &chain_b).is_err());

    // The explicit HF126 path is the only equal-work sibling replacement.
    save_chain_verified_equal_work_reanchor(&settings, &chain_b).unwrap();

    let reloaded = load_or_init_chain_for_ui_fast(&settings).unwrap();
    assert_eq!(reloaded.height(), chain_b.height());
    assert_eq!(reloaded.tip_hash(), chain_b.tip_hash());

    let live_snapshot = live_chain_snapshot(&settings).expect("live chain registered");
    assert_eq!(live_snapshot.height(), chain_b.height());
    assert_eq!(live_snapshot.tip_hash(), chain_b.tip_hash());

    unregister_live_chain(&settings);
    let _ = std::fs::remove_dir_all(data_dir);
}
