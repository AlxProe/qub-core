use anyhow::{bail, Context, Result};
use qubd::*;
use std::collections::HashMap;
use std::env;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::str::FromStr;
use std::thread;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let config_path = take_flag(&mut args, "--config").unwrap_or_else(|| "config/regtest.toml".to_string());
    let settings = Settings::load_from_path(&config_path)?;
    if args.is_empty() || args[0] == "help" || args[0] == "--help" {
        help(&config_path);
        return Ok(());
    }
    match args[0].as_str() {
        "init" => cmd_init(&settings),
        "info" => cmd_info(&settings),
        "validate" => { let chain = load_or_init_chain(&settings)?; chain.validate_all(&settings)?; println!("OK height={} bestblockhash={}", chain.height(), chain.tip_hash()); Ok(()) },
        "node" => p2p::run_node(settings.clone()),
        "sync" => cmd_sync(&settings),
        "peers" => cmd_peers(&settings),
        "peers-raw" => cmd_peers_raw(&settings),
        "preflight" => cmd_preflight(&settings),
        "wallet-new" => cmd_wallet_new(&settings),
        "wallet-address" => { let w = load_or_init_wallet(&settings)?; println!("{}", w.default_address().unwrap_or("wallet empty; run wallet-new")); Ok(()) },
        "wallet-list" => cmd_wallet_list(&settings),
        "balance" => cmd_balance(&settings),
        "jin-balance" => cmd_jin_balance(&settings, args.get(1).map(String::as_str)),
        "jin-sale-list" => cmd_jin_sale_list(&settings),
        "buy-jin" => cmd_buy_jin(&settings, args.get(1).context("usage: buy-jin <listing-id> <amount_jin> [fee]")?, args.get(2).context("usage: buy-jin <listing-id> <amount_jin> [fee]")?, args.get(3).map(String::as_str).unwrap_or("0.00001")),
        "mempool" => cmd_mempool(&settings),
        "relay-mempool" => cmd_relay_mempool(&settings),
        "send" => cmd_send(&settings, args.get(1).context("usage: send <address-or-name.qub> <amount> [fee]")?, args.get(2).context("usage: send <address-or-name.qub> <amount> [fee]")?, args.get(3).map(String::as_str).unwrap_or("0.00001")),
        "send-jin" => cmd_send_jin(&settings, args.get(1).context("usage: send-jin <address-or-name.qub> <amount_jin> [fee] [fee_asset=JIN|QUB]")?, args.get(2).context("usage: send-jin <address-or-name.qub> <amount_jin> [fee] [fee_asset=JIN|QUB]")?, args.get(3).map(String::as_str).unwrap_or("0.001"), args.get(4).map(String::as_str).unwrap_or("JIN")),
        "send-multi" => cmd_send_multi(&settings, args.get(1).context("usage: send-multi <asset=QUB|JIN> <entries addr:amount,...> [fee] [fee_asset=JIN|QUB]")?, args.get(2).context("usage: send-multi <asset=QUB|JIN> <entries addr:amount,...> [fee] [fee_asset=JIN|QUB]")?, args.get(3).map(String::as_str).unwrap_or("0.00001"), args.get(4).map(String::as_str).unwrap_or("QUB")),
        "blast-create" => cmd_blast_create(&settings, args.get(1).context("usage: blast-create <total_qub> <per_claim_qub> <max_claims> [private_code] [fee]")?, args.get(2).context("usage: blast-create <total_qub> <per_claim_qub> <max_claims> [private_code] [fee]")?, args.get(3).context("usage: blast-create <total_qub> <per_claim_qub> <max_claims> [private_code] [fee]")?, args.get(4).map(String::as_str), args.get(5).map(String::as_str).unwrap_or("0.00001")),
        "blast-claim" => cmd_blast_claim(&settings, args.get(1).context("usage: blast-claim <QUBBLAST1|txid|vout|code> [claimant-address]")?, args.get(2).map(String::as_str)),
        "convert-jin-token" => cmd_convert_jin_token(&settings, args.get(1).context("usage: convert-jin-token <matrix-address> <amount_jin> [fee] [fee_asset=JIN|QUB]")?, args.get(2).context("usage: convert-jin-token <matrix-address> <amount_jin> [fee] [fee_asset=JIN|QUB]")?, args.get(3).map(String::as_str).unwrap_or("0.001"), args.get(4).map(String::as_str).unwrap_or("JIN")),
        "qns-resolve" => cmd_qns_resolve(&settings, args.get(1).context("usage: qns-resolve <name.qub>")?),
        "qns-price" => cmd_qns_price(&settings, args.get(1).context("usage: qns-price <name.qub>")?),
        "qns-list" => cmd_qns_list(&settings, args.get(1).map(String::as_str)),
        "qns-register" => cmd_qns_register(&settings, args.get(1).context("usage: qns-register <name.qub> [target-address] [fee]")?, args.get(2).map(String::as_str), args.get(3).map(String::as_str).unwrap_or("0.00001")),
        "library-list" => cmd_library_list(&settings),
        "library-read" => cmd_library_read(&settings, args.get(1).context("usage: library-read <post-id>")?),
        "library-create" => cmd_library_create(&settings, args.get(1).context("usage: library-create <title> <category> <body> [fee]")?, args.get(2).context("usage: library-create <title> <category> <body> [fee]")?, args.get(3).context("usage: library-create <title> <category> <body> [fee]")?, args.get(4).map(String::as_str).unwrap_or("0.00001")),
        "library-comment" => cmd_library_comment(&settings, args.get(1).context("usage: library-comment <post-id> <body> [parent-comment-id|-] [fee]")?, args.get(2).context("usage: library-comment <post-id> <body> [parent-comment-id|-] [fee]")?, args.get(3).map(String::as_str), args.get(4).map(String::as_str).unwrap_or("0.00001")),
        "library-vote" => cmd_library_vote(&settings, args.get(1).context("usage: library-vote <post|comment> <target-id> <up|down> [fee]")?, args.get(2).context("usage: library-vote <post|comment> <target-id> <up|down> [fee]")?, args.get(3).context("usage: library-vote <post|comment> <target-id> <up|down> [fee]")?, args.get(4).map(String::as_str).unwrap_or("0.00001")),
        "library-edit" => cmd_library_edit(&settings, args.get(1).context("usage: library-edit <post|comment> <target-id> <title> <category> <body> [fee]")?, args.get(2).context("usage: library-edit <post|comment> <target-id> <title> <category> <body> [fee]")?, args.get(3).context("usage: library-edit <post|comment> <target-id> <title> <category> <body> [fee]")?, args.get(4).context("usage: library-edit <post|comment> <target-id> <title> <category> <body> [fee]")?, args.get(5).context("usage: library-edit <post|comment> <target-id> <title> <category> <body> [fee]")?, args.get(6).map(String::as_str).unwrap_or("0.00001")),
        "library-delete" => cmd_library_delete(&settings, args.get(1).context("usage: library-delete <post|comment> <target-id> [fee]")?, args.get(2).context("usage: library-delete <post|comment> <target-id> [fee]")?, args.get(3).map(String::as_str).unwrap_or("0.00001")),
        "pool-list" => cmd_pool_list(&settings),
        "pool-info" => cmd_pool_info(&settings, args.get(1).context("usage: pool-info <pool-id>")?),
        "pool-create" => cmd_pool_create(&settings, args.get(1).context("usage: pool-create <name> [commission_bps] [capacity_slots] [manager-address] [fee]")?, args.get(2).map(String::as_str).unwrap_or("0"), args.get(3).map(String::as_str).unwrap_or("8"), args.get(4).map(String::as_str), args.get(5).map(String::as_str).unwrap_or("0.00001")),
        "pool-top-up" => cmd_pool_top_up(&settings, args.get(1).context("usage: pool-top-up <pool-id> <extra_capacity_slots> [fee]")?, args.get(2).context("usage: pool-top-up <pool-id> <extra_capacity_slots> [fee]")?, args.get(3).map(String::as_str).unwrap_or("0.00001")),
        "pool-set-commission" => cmd_pool_set_commission(&settings, args.get(1).context("usage: pool-set-commission <pool-id> <new_commission_bps> [fee]")?, args.get(2).context("usage: pool-set-commission <pool-id> <new_commission_bps> [fee]")?, args.get(3).map(String::as_str).unwrap_or("0.00001")),
        "pool-rename" => cmd_pool_rename(&settings, args.get(1).context("usage: pool-rename <pool-id> <new-name> [fee]")?, args.get(2).context("usage: pool-rename <pool-id> <new-name> [fee]")?, args.get(3).map(String::as_str).unwrap_or("0.00001")),
        "pool-join" => cmd_pool_join(&settings, args.get(1).context("usage: pool-join <pool-id> [miner-address]")?, args.get(2).map(String::as_str)),
        "pool-mine" => cmd_pool_mine(&settings, args.get(1).context("usage: pool-mine <pool-id> [blocks] [miner-address]")?, args.get(2).map(String::as_str).unwrap_or("1").parse()?, args.get(3).map(String::as_str)),
        "mine" => cmd_mine(&settings, args.get(1).map(String::as_str).unwrap_or("1").parse()?, args.get(2).map(String::as_str)),
        "explorer-api" => cmd_explorer_api(&settings, args.get(1).map(String::as_str).unwrap_or("127.0.0.1:18765")),
        other => bail!("unknown command {other}"),
    }
}

fn cmd_init(settings: &Settings) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    println!("initialized {}", settings.network.name);
    println!("height: {}", chain.height());
    println!("bestblockhash: {}", chain.tip_hash());
    println!("wallet_keys: {}", wallet.keys.len());
    println!("{}", v1_feature_notice(settings));
    Ok(())
}
fn cmd_info(settings: &Settings) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let spendable = wallet.balance_atoms(&chain, settings, false)?;
    let total = wallet.balance_atoms(&chain, settings, true)?;
    let wallet_total_jin = wallet.jin_balance_units(&chain, settings).unwrap_or(0);
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "chain": settings.network.name,
        "height": chain.height(),
        "bestblockhash": chain.tip_hash().to_string(),
        "mempooltx": chain.mempool.len(),
        "wallet_spendable_qub": Amount::from_atoms(spendable)?.to_string(),
        "wallet_immature_qub": Amount::from_atoms(total.saturating_sub(spendable))?.to_string(),
        "wallet_total_qub": Amount::from_atoms(total)?.to_string(),
        "wallet_total_jin": format_jin_amount(wallet_total_jin),
        "jin_enabled": settings.jin.enabled && settings.features.jin_native_coin_enabled,
        "jin_activation_height": settings.jin.activation_height,
        "jin_conversion_activation_height": settings.jin.conversion_activation_height,
        "jin_protocol_address": settings.jin.protocol_address,
        "feature_notice": v1_feature_notice(settings),
        "qns_enabled": settings.qns.enabled,
        "qns_activation_height": settings.qns.activation_height,
        "qns_miner_split_activation_height": settings.qns.miner_split_activation_height,
        "qns_protocol_name": settings.qns.protocol_name,
        "qns_protocol_address": settings.qns.protocol_address,
        "pools_enabled": settings.features.pooled_mining_enabled && settings.pools.enabled,
        "pools_activation_height": settings.pools.activation_height,
        "pools_protocol_name": settings.pools.protocol_name,
        "pools_protocol_address": settings.pools.protocol_address,
        "pools_count": pools_registry_from_blocks(settings, &chain.blocks).map(|m| m.len()).unwrap_or(0),
        "library_enabled": settings.library.enabled,
        "library_activation_height": library_activation_height(settings),
        "library_posts_count": library_state_from_blocks(settings, &chain.blocks).map(|s| s.posts.iter().filter(|p| !p.deleted).count()).unwrap_or(0),
        "verified_governance_enabled": settings.verified_governance.enabled,
        "verified_governance_activation_height": settings.verified_governance.activation_height,
        "verified_governance_active": verified_governance_active(settings, chain.height() + 1),
        "verified_wallets_count": verified_governance_state_from_blocks(settings, &chain.blocks).map(|s| s.wallets.len()).unwrap_or(0),
        "verified_pools_count": verified_governance_state_from_blocks(settings, &chain.blocks).map(|s| s.pools.len()).unwrap_or(0),
        "report_cases_count": verified_governance_state_from_blocks(settings, &chain.blocks).map(|s| s.reports.len()).unwrap_or(0),
        "moderators_count": verified_governance_state_from_blocks(settings, &chain.blocks).map(|s| s.moderators.values().filter(|m| m.status == VerifiedStatus::Active).count()).unwrap_or(0)
    }))?);
    Ok(())
}
fn cmd_wallet_new(settings: &Settings) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let mut wallet = load_or_init_wallet(settings)?;
    let key = wallet.create_key(settings, "default", chain.height())?;
    save_wallet(settings, &wallet)?;
    println!("address: {}", key.address);
    println!("public_key: {}", key.public_key_hex);
    println!("secret_key_hex: {}", key.secret_key_hex);
    eprintln!("WARNING: v1 local wallet stores plaintext secret_key_hex in wallet.json.");
    Ok(())
}
fn cmd_wallet_list(settings: &Settings) -> Result<()> {
    let wallet = load_or_init_wallet(settings)?;
    for (idx, key) in wallet.keys.iter().enumerate() {
        let mark = if Some(idx) == wallet.default_index { "*" } else { " " };
        println!("{mark} {} label={} created_height={}", key.address, key.label, key.created_height);
    }
    if wallet.keys.is_empty() { println!("wallet empty; run wallet-new"); }
    Ok(())
}
fn cmd_balance(settings: &Settings) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let spendable = wallet.balance_atoms(&chain, settings, false)?;
    let total = wallet.balance_atoms(&chain, settings, true)?;
    println!("spendable: {} QUB", Amount::from_atoms(spendable)?);
    println!("immature:  {} QUB", Amount::from_atoms(total.saturating_sub(spendable))?);
    println!("total:     {} QUB", Amount::from_atoms(total)?);
    if let Some(addr) = wallet.default_address() {
        println!("jin:       {} JIN", format_jin_amount(jin_balance_units_for_address(settings, &chain, addr)?));
    }
    Ok(())
}


fn cmd_jin_sale_list(settings: &Settings) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let listings = jin_sale_listings(settings, &chain)?;
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "active": jin_swap_active(settings, chain.height() + 1),
        "activation_height": jin_swap_activation_height(settings),
        "height": chain.height(),
        "listings": listings.into_iter().map(|l| serde_json::json!({
            "listing_id": l.listing_id,
            "price_qub_per_jin": Amount::from_atoms(l.price_atoms_per_jin).map(|a| a.to_string()).unwrap_or_else(|_| l.price_atoms_per_jin.to_string()),
            "total_jin": format_jin_amount(l.total_units),
            "sold_jin": format_jin_amount(l.sold_units),
            "remaining_jin": format_jin_amount(l.remaining_units),
        })).collect::<Vec<_>>()
    }))?);
    Ok(())
}

fn cmd_buy_jin(settings: &Settings, listing_id_s: &str, amount: &str, fee: &str) -> Result<()> {
    // HF74/v1.5.8 compile fix: use the tiered canonical sync helper that exists
    // in p2p.rs. `sync_chain_once` was an old helper name and breaks the CLI build.
    if settings.p2p.enabled {
        if let Err(err) = p2p::hf82_auto_catchup(settings, 8_000) {
            eprintln!("p2p pre-buy-jin sync warning: {err:#}");
        }
    }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let listing_id = listing_id_s.trim().parse::<u32>()?;
    let units = parse_jin_amount(amount.trim())?;
    let tx = wallet.create_jin_public_sale_buy_transaction(&chain, settings, listing_id, units, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    // HF114/v1.7.2: JIN buys relay the exact tx with short timeouts instead of
    // rebroadcasting the whole mempool, which could slow miner/block activity
    // after a buy attempt on weak links.
    let mut relayed = p2p::broadcast_tx_limited(settings, &tx, 24, 850).unwrap_or(0);
    for _ in 0..2 {
        relayed = relayed.saturating_add(p2p::rebroadcast_txid_limited(settings, &txid, 24, 850).unwrap_or(0));
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "txid": txid.to_string(),
        "listing_id": listing_id,
        "amount_jin": format_jin_amount(units),
        "relayed_to_peers": relayed,
        "local_mempooltx": chain.mempool.len()
    }))?);
    Ok(())
}

fn cmd_jin_balance(settings: &Settings, address: Option<&str>) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let addr = address.map(str::to_string).or_else(|| wallet.default_address().map(str::to_string)).context("usage: jin-balance [address]")?;
    Address::parse_with_prefix(&addr, &settings.network.address_prefix)?;
    let units = jin_balance_units_for_address(settings, &chain, &addr)?;
    println!("address: {addr}");
    println!("jin_units: {units}");
    println!("jin: {} JIN", format_jin_amount(units));
    Ok(())
}

fn cmd_mempool(settings: &Settings) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    println!("mempooltx: {}", chain.mempool.len());
    for tx in &chain.mempool { println!("{}", tx.txid()); }
    Ok(())
}
fn cmd_relay_mempool(settings: &Settings) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let mut txs = 0usize;
    let mut peers_total = 0usize;
    for tx in &chain.mempool {
        txs += 1;
        match p2p::broadcast_tx(settings, tx) {
            Ok(sent) => peers_total = peers_total.saturating_add(sent),
            Err(err) => eprintln!("relay warning for {}: {err:#}", tx.txid()),
        }
    }
    println!("mempooltx: {txs}");
    println!("relayed_to_peers_total: {peers_total}");
    if txs > 0 && peers_total == 0 {
        eprintln!("WARNING: no peers accepted outbound relay; check peers/connectivity or keep this node mining.");
    }
    Ok(())
}
fn cmd_send(settings: &Settings, to: &str, amount: &str, fee: &str) -> Result<()> {
    // Make sure coin selection is based on the current active chain before signing.
    if settings.p2p.enabled {
        if let Err(err) = p2p::hf82_auto_catchup(settings, 8_000) {
            eprintln!("p2p pre-send sync warning: {err:#}");
        }
    }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let to = resolve_address_or_qns(settings, &chain, to)?;
    let tx = wallet.create_signed_transaction(&chain, settings, &to, Amount::from_str(amount)?, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx, settings)?;
    save_chain(settings, &chain)?;
    println!("txid: {txid}");
    println!("local_mempooltx: {}", chain.mempool.len());
    match p2p::broadcast_tx(settings, chain.mempool.last().context("missing just-created mempool tx")?) {
        Ok(sent) => {
            println!("relayed_to_peers: {sent}");
            if sent == 0 {
                eprintln!("WARNING: transaction is only in this local mempool until a peer is reachable or this node mines a block.");
            }
        }
        Err(err) => eprintln!("p2p relay warning: {err:#}"),
    }
    if settings.p2p.enabled {
        if let Err(err) = p2p::hf82_auto_catchup(settings, 6_000) {
            eprintln!("p2p post-send sync warning: {err:#}");
        }
    }
    Ok(())
}
fn cmd_send_jin(settings: &Settings, to: &str, amount: &str, fee: &str, fee_asset: &str) -> Result<()> {
    if settings.p2p.enabled {
        if let Err(err) = p2p::hf82_auto_catchup(settings, 8_000) {
            eprintln!("p2p pre-send sync warning: {err:#}");
        }
    }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let to = resolve_address_or_qns(settings, &chain, to)?;
    let fee_asset = fee_asset.trim().to_ascii_uppercase();
    let amount_units = parse_jin_amount(amount)?;
    let (qub_fee, jin_fee_units) = if fee_asset == "QUB" { (Amount::from_str(fee)?, 0u128) } else { (Amount::from_atoms(0)?, parse_jin_amount(fee)?) };
    let tx = wallet.create_jin_transfer_transaction(&chain, settings, &to, amount_units, qub_fee, jin_fee_units, &fee_asset)?;
    let txid = chain.accept_transaction_to_mempool(tx, settings)?;
    save_chain(settings, &chain)?;
    println!("txid: {txid}");
    println!("asset: JIN");
    println!("amount_jin: {}", format_jin_amount(amount_units));
    println!("fee_asset: {fee_asset}");
    println!("local_mempooltx: {}", chain.mempool.len());
    match p2p::broadcast_tx(settings, chain.mempool.last().context("missing just-created mempool tx")?) {
        Ok(sent) => println!("relayed_to_peers: {sent}"),
        Err(err) => eprintln!("p2p relay warning: {err:#}"),
    }
    Ok(())
}


fn parse_multi_entries(settings: &Settings, chain: &ChainState, entries: &str, asset: &str) -> Result<Vec<(Address, String)>> {
    let mut out = Vec::new();
    for raw in entries.split(',') {
        let raw = raw.trim();
        if raw.is_empty() { continue; }
        let Some((to, amount)) = raw.split_once(':') else { bail!("multi-send entry must be address_or_qns:amount"); };
        let addr = resolve_address_or_qns(settings, chain, to.trim())?;
        out.push((addr, amount.trim().to_string()));
    }
    if out.is_empty() { bail!("multi-send needs at least one entry"); }
    if out.len() > MAX_SEND_ENTRIES_PER_TX { bail!("multi-send supports at most {} entries", MAX_SEND_ENTRIES_PER_TX); }
    if asset != "QUB" && asset != "JIN" { bail!("asset must be QUB or JIN"); }
    Ok(out)
}

fn cmd_send_multi(settings: &Settings, asset: &str, entries: &str, fee: &str, fee_asset: &str) -> Result<()> {
    if settings.p2p.enabled {
        if let Err(err) = p2p::hf82_auto_catchup(settings, 8_000) {
            eprintln!("p2p pre-multi-send sync warning: {err:#}");
        }
    }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let asset = asset.trim().to_ascii_uppercase();
    let parsed = parse_multi_entries(settings, &chain, entries, &asset)?;
    let tx = if asset == "JIN" {
        let payments = parsed.iter().map(|(a, amt)| parse_jin_amount(amt).map(|u| (a.clone(), u))).collect::<Result<Vec<_>>>()?;
        let fee_asset = fee_asset.trim().to_ascii_uppercase();
        let (qub_fee, jin_fee_units) = if fee_asset == "QUB" { (Amount::from_str(fee)?, 0u128) } else { (Amount::from_atoms(0)?, parse_jin_amount(fee)?) };
        wallet.create_jin_multi_transfer_transaction(&chain, settings, &payments, qub_fee, jin_fee_units, &fee_asset)?
    } else {
        let payments = parsed.iter().map(|(a, amt)| Amount::from_str(amt).map(|q| (a.clone(), q))).collect::<Result<Vec<_>>>()?;
        wallet.create_multi_signed_transaction(&chain, settings, &payments, Amount::from_str(fee)?)?
    };
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "txid": txid.to_string(),
        "asset": asset,
        "entries": parsed.len(),
        "relayed_to_peers": relayed,
        "local_mempooltx": chain.mempool.len()
    }))?);
    Ok(())
}

fn generate_blast_code() -> String {
    let secret = generate_secret_key();
    format!("b{}", secret_key_to_hex(&secret))
}

fn cmd_blast_create(settings: &Settings, total: &str, per_claim: &str, max_claims_s: &str, code_opt: Option<&str>, fee: &str) -> Result<()> {
    if settings.p2p.enabled {
        if let Err(err) = p2p::hf82_auto_catchup(settings, 8_000) {
            eprintln!("p2p pre-blast sync warning: {err:#}");
        }
    }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let code = code_opt.map(str::to_string).unwrap_or_else(generate_blast_code);
    let max_claims = max_claims_s.parse::<u32>()?;
    let tx = wallet.create_blast_create_transaction_qub(&chain, settings, Amount::from_str(total)?, Amount::from_str(per_claim)?, max_claims, &code, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    let claim_code = make_blast_code_payload(txid, 0, &code)?;
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "txid": txid.to_string(),
        "blast_vault_outpoint": format!("{}:0", txid),
        "private_code": code,
        "claim_code": claim_code,
        "qr_payload": claim_code,
        "asset": "QUB",
        "total_qub": total,
        "per_claim_qub": per_claim,
        "max_claims": max_claims,
        "warning": "Blast code is private until first on-chain claim. For public campaigns, generate separate one-time codes; a single public code can be copied after first claim.",
        "relayed_to_peers": relayed,
        "local_mempooltx": chain.mempool.len()
    }))?);
    Ok(())
}

fn cmd_blast_claim(settings: &Settings, claim_code: &str, claimant: Option<&str>) -> Result<()> {
    if settings.p2p.enabled {
        if let Err(err) = p2p::hf82_auto_catchup(settings, 8_000) {
            eprintln!("p2p pre-blast-claim sync warning: {err:#}");
        }
    }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let claimant = match claimant {
        Some(s) => Some(resolve_address_or_qns(settings, &chain, s)?),
        None => None,
    };
    let tx = wallet.create_blast_claim_transaction_qub(&chain, settings, claim_code, claimant.as_ref())?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "txid": txid.to_string(),
        "asset": "QUB",
        "relayed_to_peers": relayed,
        "local_mempooltx": chain.mempool.len()
    }))?);
    Ok(())
}


fn cmd_library_list(settings: &Settings) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let state = library_state_from_blocks(settings, &chain.blocks)?;
    let posts = state.posts.into_iter().filter(|p| !p.deleted).collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "activation_height": library_activation_height(settings),
        "active": library_active(settings, chain.height() + 1),
        "posts": posts
    }))?);
    Ok(())
}

fn cmd_library_read(settings: &Settings, post_id: &str) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let state = library_state_from_blocks(settings, &chain.blocks)?;
    let post = state.posts.iter().find(|p| p.id == post_id && !p.deleted).context("post not found")?;
    let comments = state.comments.iter().filter(|c| c.post_id == post.id && !c.deleted).cloned().collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"post": post, "comments": comments}))?);
    Ok(())
}

fn cmd_library_create(settings: &Settings, title: &str, category: &str, body: &str, fee: &str) -> Result<()> {
    if settings.p2p.enabled { let _ = p2p::hf82_auto_catchup(settings, 8_000); }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let price_atoms = library_post_price_atoms(settings, title, category, body)?;
    let tx = wallet.create_library_post_transaction(&chain, settings, title, category, body, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"txid": txid.to_string(), "post_id": txid.to_string(), "protocol_fee_to_miner_atoms": price_atoms, "protocol_fee_to_miner_qub": Amount::from_atoms(price_atoms)?.to_string(), "relayed_to_peers": relayed, "local_mempooltx": chain.mempool.len()}))?);
    Ok(())
}

fn cmd_library_comment(settings: &Settings, post_id: &str, body: &str, parent: Option<&str>, fee: &str) -> Result<()> {
    if settings.p2p.enabled { let _ = p2p::hf82_auto_catchup(settings, 8_000); }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let parent = parent.and_then(|p| if p.trim().is_empty() || p.trim() == "-" { None } else { Some(p.trim()) });
    let tx = wallet.create_library_comment_transaction(&chain, settings, post_id, parent, body, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"txid": txid.to_string(), "comment_id": txid.to_string(), "relayed_to_peers": relayed, "local_mempooltx": chain.mempool.len()}))?);
    Ok(())
}

fn cmd_library_vote(settings: &Settings, kind: &str, target_id: &str, vote: &str, fee: &str) -> Result<()> {
    if settings.p2p.enabled { let _ = p2p::hf82_auto_catchup(settings, 8_000); }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let vote = vote.trim().eq_ignore_ascii_case("up") || vote.trim() == "+";
    let tx = wallet.create_library_vote_transaction(&chain, settings, kind, target_id, vote, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"txid": txid.to_string(), "relayed_to_peers": relayed, "local_mempooltx": chain.mempool.len()}))?);
    Ok(())
}

fn cmd_library_edit(settings: &Settings, kind: &str, target_id: &str, title: &str, category: &str, body: &str, fee: &str) -> Result<()> {
    if settings.p2p.enabled { let _ = p2p::hf82_auto_catchup(settings, 8_000); }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let tx = wallet.create_library_edit_transaction(&chain, settings, kind, target_id, title, category, body, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"txid": txid.to_string(), "relayed_to_peers": relayed, "local_mempooltx": chain.mempool.len()}))?);
    Ok(())
}

fn cmd_library_delete(settings: &Settings, kind: &str, target_id: &str, fee: &str) -> Result<()> {
    if settings.p2p.enabled { let _ = p2p::hf82_auto_catchup(settings, 8_000); }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let tx = wallet.create_library_delete_transaction(&chain, settings, kind, target_id, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"txid": txid.to_string(), "relayed_to_peers": relayed, "local_mempooltx": chain.mempool.len(), "note": "Library delete is a consensus tombstone; historical chain bytes remain immutable."}))?);
    Ok(())
}

fn cmd_convert_jin_token(settings: &Settings, matrix_address: &str, amount: &str, fee: &str, fee_asset: &str) -> Result<()> {
    if settings.p2p.enabled {
        if let Err(err) = p2p::hf82_auto_catchup(settings, 8_000) {
            eprintln!("p2p pre-conversion sync warning: {err:#}");
        }
    }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let fee_asset = fee_asset.trim().to_ascii_uppercase();
    let amount_units = parse_jin_amount(amount)?;
    let (qub_fee, jin_fee_units) = if fee_asset == "QUB" { (Amount::from_str(fee)?, 0u128) } else { (Amount::from_atoms(0)?, parse_jin_amount(fee)?) };
    let tx = wallet.create_jin_token_conversion_transaction(&chain, settings, matrix_address, amount_units, qub_fee, jin_fee_units, &fee_asset)?;
    let txid = chain.accept_transaction_to_mempool(tx, settings)?;
    save_chain(settings, &chain)?;
    println!("txid: {txid}");
    println!("type: JIN Coin -> JIN Token conversion");
    println!("matrix_address: {matrix_address}");
    println!("amount_jin: {}", format_jin_amount(amount_units));
    println!("fee_asset: {fee_asset}");
    println!("local_mempooltx: {}", chain.mempool.len());
    match p2p::broadcast_tx(settings, chain.mempool.last().context("missing just-created mempool tx")?) {
        Ok(sent) => println!("relayed_to_peers: {sent}"),
        Err(err) => eprintln!("p2p relay warning: {err:#}"),
    }
    Ok(())
}

fn resolve_address_or_qns(settings: &Settings, chain: &ChainState, input: &str) -> Result<Address> {
    let trimmed = input.trim();
    if trimmed.to_ascii_lowercase().ends_with(".qub") {
        let rec = qns_resolve(settings, chain, trimmed)?.with_context(|| format!("QNS name not found: {trimmed}"))?;
        return Address::parse_with_prefix(&rec.address, &settings.network.address_prefix);
    }
    Address::parse_with_prefix(trimmed, &settings.network.address_prefix)
}

fn cmd_qns_resolve(settings: &Settings, name: &str) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    match qns_resolve(settings, &chain, name)? {
        Some(rec) => println!("{}", serde_json::to_string_pretty(&serde_json::json!({"found": true, "name": rec.name, "address": rec.address, "height": rec.height, "txid": rec.txid.to_string(), "price_qub": Amount::from_atoms(rec.price_atoms)?.to_string()}))?),
        None => println!("{}", serde_json::to_string_pretty(&serde_json::json!({"found": false, "name": normalize_qns_name(name, settings.qns.max_label_chars)?}))?),
    }
    Ok(())
}

fn cmd_qns_price(settings: &Settings, name: &str) -> Result<()> {
    let name = normalize_qns_name(name, settings.qns.max_label_chars)?;
    let atoms = qns_registration_price_atoms(settings, &name)?;
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"name": name, "price_atoms": atoms, "price_qub": Amount::from_atoms(atoms)?.to_string(), "marker_atoms": settings.qns.marker_output_atoms, "activation_height": settings.qns.activation_height, "protocol_name": settings.qns.protocol_name, "protocol_address": settings.qns.protocol_address}))?);
    Ok(())
}

fn cmd_qns_list(settings: &Settings, address: Option<&str>) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let mut rows = qns_registry_from_blocks(settings, &chain.blocks)?.into_values().collect::<Vec<_>>();
    if let Some(addr) = address {
        Address::parse_with_prefix(addr, &settings.network.address_prefix)?;
        rows.retain(|r| r.address == addr);
    }
    rows.sort_by(|a,b| a.name.cmp(&b.name));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"network": settings.network.name, "height": chain.height(), "count": rows.len(), "names": rows.iter().map(|r| serde_json::json!({"name": r.name, "address": r.address, "height": r.height, "txid": r.txid.to_string(), "price_qub": Amount::from_atoms(r.price_atoms).map(|a| a.to_string()).unwrap_or_else(|_| r.price_atoms.to_string()) })).collect::<Vec<_>>() }))?);
    Ok(())
}

fn cmd_qns_register(settings: &Settings, name: &str, target_address: Option<&str>, fee: &str) -> Result<()> {
    if settings.p2p.enabled { let _ = p2p::hf82_auto_catchup(settings, 8_000); }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let target = match target_address {
        Some(addr) => Address::parse_with_prefix(addr, &settings.network.address_prefix)?,
        None => Address::parse_with_prefix(wallet.default_address().context("wallet empty; pass target-address or create a wallet key")?, &settings.network.address_prefix)?,
    };
    let normalized = normalize_qns_name(name, settings.qns.max_label_chars)?;
    let price = qns_registration_price_atoms(settings, &normalized)?;
    let tx = wallet.create_qns_registration_transaction(&chain, settings, &normalized, &target, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"txid": txid.to_string(), "name": normalized, "target_address": target.to_string(), "price_qub": Amount::from_atoms(price)?.to_string(), "relayed_to_peers": relayed, "local_mempooltx": chain.mempool.len()}))?);
    Ok(())
}


fn cmd_pool_list(settings: &Settings) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let mut rows = pools_registry_from_blocks(settings, &chain.blocks)?.into_values().collect::<Vec<_>>();
    rows.sort_by(|a, b| a.created_height.cmp(&b.created_height).then(a.pool_id.to_string().cmp(&b.pool_id.to_string())));
    let pools = rows.iter().map(|p| pool_create_summary_json(settings, &chain, p)).collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "network": settings.network.name,
        "height": chain.height(),
        "pooled_mining_enabled": settings.features.pooled_mining_enabled && settings.pools.enabled,
        "activation_height": settings.pools.activation_height,
        "protocol_name": settings.pools.protocol_name,
        "protocol_address": settings.pools.protocol_address,
        "share_window_blocks": settings.pools.share_window_blocks,
        "share_target_bits": pool_share_target_as_hex(settings).unwrap_or_else(|_| settings.pools.share_target_bits.clone()),
        "count": pools.len(),
        "pools": pools,
    }))?);
    Ok(())
}

fn cmd_pool_info(settings: &Settings, pool_id_s: &str) -> Result<()> {
    let chain = load_or_init_chain(settings)?;
    let pool_id = Hash256::from_hex(pool_id_s)?;
    let registry = pools_registry_from_blocks(settings, &chain.blocks)?;
    let pool = registry.get(&pool_id).context("pool not found")?;
    let scores = pool_share_scores_from_blocks(settings, &chain.blocks, chain.height() + 1, pool_id);
    let score_rows = {
        let mut rows = scores.into_iter().collect::<Vec<_>>();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        rows.into_iter().map(|(address, score)| serde_json::json!({"address": address, "shares": score})).collect::<Vec<_>>()
    };
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "network": settings.network.name,
        "height": chain.height(),
        "pool": pool_create_summary_json(settings, &chain, pool),
        "scores": score_rows,
        "pplns_window_blocks": settings.pools.share_window_blocks,
        "share_target_bits": pool_share_target_as_hex(settings).unwrap_or_else(|_| settings.pools.share_target_bits.clone()),
    }))?);
    Ok(())
}

fn cmd_pool_create(settings: &Settings, name: &str, commission_bps_s: &str, capacity_slots_s: &str, manager_address: Option<&str>, fee: &str) -> Result<()> {
    if settings.p2p.enabled { let _ = p2p::hf82_auto_catchup(settings, 8_000); }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let commission_bps = commission_bps_s.parse::<u16>()?;
    let capacity_slots = capacity_slots_s.parse::<u32>()?;
    let manager = match manager_address {
        Some(addr) => Address::parse_with_prefix(addr, &settings.network.address_prefix)?,
        None => Address::parse_with_prefix(wallet.default_address().context("wallet empty; pass manager-address or create a wallet key")?, &settings.network.address_prefix)?,
    };
    let normalized = normalize_pool_name(name, settings.pools.max_name_chars, settings.pools.max_name_bytes)?;
    let price = pool_create_price_atoms(settings, capacity_slots)?;
    let tx = wallet.create_pool_create_transaction(&chain, settings, &normalized, &manager, commission_bps, capacity_slots, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "txid": txid.to_string(),
        "pool_id": txid.to_string(),
        "name": normalized,
        "manager_address": manager.to_string(),
        "commission_bps": commission_bps,
        "capacity_slots": capacity_slots,
        "price_atoms": price,
        "price_qub": Amount::from_atoms(price)?.to_string(),
        "protocol_atoms": pool_protocol_share_atoms(price),
        "miner_split_atoms": pool_miner_share_atoms(price),
        "relayed_to_peers": relayed,
        "local_mempooltx": chain.mempool.len(),
    }))?);
    Ok(())
}

fn cmd_pool_top_up(settings: &Settings, pool_id_s: &str, extra_slots_s: &str, fee: &str) -> Result<()> {
    if settings.p2p.enabled { let _ = p2p::hf82_auto_catchup(settings, 8_000); }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let pool_id = Hash256::from_hex(pool_id_s)?;
    let extra_slots = extra_slots_s.parse::<u32>()?;
    let price = pool_topup_price_atoms(settings, extra_slots)?;
    let tx = wallet.create_pool_topup_transaction(&chain, settings, pool_id, extra_slots, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "txid": txid.to_string(),
        "pool_id": pool_id.to_string(),
        "extra_capacity_slots": extra_slots,
        "price_atoms": price,
        "price_qub": Amount::from_atoms(price)?.to_string(),
        "protocol_atoms": pool_protocol_share_atoms(price),
        "miner_split_atoms": pool_miner_share_atoms(price),
        "relayed_to_peers": relayed,
        "local_mempooltx": chain.mempool.len(),
    }))?);
    Ok(())
}

fn cmd_pool_set_commission(settings: &Settings, pool_id_s: &str, new_bps_s: &str, fee: &str) -> Result<()> {
    if settings.p2p.enabled { let _ = p2p::hf82_auto_catchup(settings, 8_000); }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let pool_id = Hash256::from_hex(pool_id_s)?;
    let new_bps = new_bps_s.parse::<u16>()?;
    let tx = wallet.create_pool_set_commission_transaction(&chain, settings, pool_id, new_bps, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "txid": txid.to_string(),
        "pool_id": pool_id.to_string(),
        "new_commission_bps": new_bps,
        "relayed_to_peers": relayed,
        "local_mempooltx": chain.mempool.len(),
    }))?);
    Ok(())
}

fn cmd_pool_rename(settings: &Settings, pool_id_s: &str, new_name: &str, fee: &str) -> Result<()> {
    if settings.p2p.enabled { let _ = p2p::hf82_auto_catchup(settings, 8_000); }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let pool_id = Hash256::from_hex(pool_id_s)?;
    let normalized = normalize_pool_name(new_name, settings.pools.max_name_chars, settings.pools.max_name_bytes)?;
    let tx = wallet.create_pool_rename_transaction(&chain, settings, pool_id, &normalized, Amount::from_str(fee)?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, &chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(settings, 16).unwrap_or(0));
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "txid": txid.to_string(),
        "pool_id": pool_id.to_string(),
        "new_name": normalized,
        "relayed_to_peers": relayed,
        "local_mempooltx": chain.mempool.len(),
    }))?);
    Ok(())
}

fn wallet_key_for_pool(settings: &Settings, wallet: &WalletFile, address: Option<&str>) -> Result<WalletKey> {
    let target = match address {
        Some(addr) => Address::parse_with_prefix(addr, &settings.network.address_prefix)?.to_string(),
        None => wallet.default_address().context("wallet empty; create a key with wallet-new or pass miner-address")?.to_string(),
    };
    wallet.keys.iter().find(|k| k.address == target).cloned().with_context(|| format!("wallet does not contain private key for {target}"))
}

fn find_pool_share_nonce(settings: &Settings, pool_id: Hash256, miner_address: &str, parent_height: u32, parent_hash: Hash256, start_nonce: u64) -> Result<u64> {
    let mut nonce = start_nonce;
    loop {
        if pool_share_meets_target(settings, pool_id, miner_address, parent_height, parent_hash, nonce)? {
            return Ok(nonce);
        }
        nonce = nonce.wrapping_add(1);
        if nonce == start_nonce { bail!("pool share nonce space exhausted"); }
    }
}

fn create_local_pool_share(settings: &Settings, chain: &mut ChainState, pool_id: Hash256, miner_key: &WalletKey, start_nonce: u64) -> Result<(Hash256, u64)> {
    let registry = pools_registry_from_blocks(settings, &chain.blocks)?;
    if !registry.contains_key(&pool_id) { bail!("unknown pool_id; create/confirm pool first"); }
    let parent_height = chain.height();
    let parent_hash = chain.tip_hash();
    let nonce = find_pool_share_nonce(settings, pool_id, &miner_key.address, parent_height, parent_hash, start_nonce)?;
    let tx = create_pool_share_transaction(settings, pool_id, miner_key, parent_height, parent_hash, nonce)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, chain)?;
    let _ = p2p::broadcast_tx(settings, &tx);
    let _ = p2p::rebroadcast_local_mempool(settings, 16);
    Ok((txid, nonce))
}

fn cmd_pool_join(settings: &Settings, pool_id_s: &str, miner_address: Option<&str>) -> Result<()> {
    if settings.p2p.enabled { let _ = p2p::hf82_auto_catchup(settings, 8_000); }
    let mut chain = load_or_init_chain(settings)?;
    let wallet = load_or_init_wallet(settings)?;
    let pool_id = Hash256::from_hex(pool_id_s)?;
    let key = wallet_key_for_pool(settings, &wallet, miner_address)?;
    let (txid, nonce) = create_local_pool_share(settings, &mut chain, pool_id, &key, 0)?;
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "joined_by_share": true,
        "pool_id": pool_id.to_string(),
        "miner_address": key.address.clone(),
        "share_txid": txid.to_string(),
        "share_nonce": nonce,
        "parent_height": chain.height(),
        "note": "First accepted share makes this miner active after the share tx is confirmed in a block.",
        "local_mempooltx": chain.mempool.len(),
    }))?);
    Ok(())
}

fn cmd_pool_mine(settings: &Settings, pool_id_s: &str, blocks: u32, miner_address: Option<&str>) -> Result<()> {
    let pool_id = Hash256::from_hex(pool_id_s)?;
    let mut chain = load_or_init_chain(settings)?;
    let mut wallet = load_or_init_wallet(settings)?;
    if miner_address.is_none() && wallet.default_key().is_none() {
        let key = wallet.create_key(settings, "pool-miner", chain.height())?;
        println!("created pool miner address: {}", key.address);
        save_wallet(settings, &wallet)?;
    }
    let key = wallet_key_for_pool(settings, &wallet, miner_address)?;
    let mut start_nonce = 0u64;
    for _ in 0..blocks {
        p2p::mining_safety_check(settings).with_context(|| "mining safety check failed")?;
        chain = load_or_init_chain(settings)?;
        match create_local_pool_share(settings, &mut chain, pool_id, &key, start_nonce) {
            Ok((share_txid, nonce)) => {
                start_nonce = nonce.wrapping_add(1);
                println!("pool share submitted txid={} nonce={}", share_txid, nonce);
                chain = load_or_init_chain(settings)?;
            }
            Err(err) => eprintln!("pool share warning: {err:#}"),
        }
        if settings.p2p.enabled {
            p2p::mining_parent_guard(settings, chain.height(), chain.tip_hash())
                .with_context(|| "pool mining candidate parent guard failed")?;
        }
        match mine_next_pool_block(&chain, settings, pool_id, MiningOptions::from_settings(settings)) {
            Ok(block) => {
                let candidate_parent_hash = block.header.prev_block_hash;
                let candidate_parent_height = chain.height();
                if settings.p2p.enabled {
                    p2p::mining_parent_submit_guard(settings, candidate_parent_height, candidate_parent_hash)
                        .with_context(|| "pool block submit guard failed")?;
                }
                chain = load_or_init_chain(settings)?;
                if chain.height() != candidate_parent_height || chain.tip_hash() != candidate_parent_hash {
                    bail!("pool block became stale before submit: local tip is #{} {}, candidate parent was #{} {}", chain.height(), chain.tip_hash(), candidate_parent_height, candidate_parent_hash);
                }
                let relay_block = block.clone();
                let hash = chain.connect_block(block, settings)?;
                save_chain(settings, &chain)?;
                match p2p::broadcast_block(settings, &relay_block) {
                    Ok(sent) if sent > 0 => println!("relayed_to_peers: {sent}"),
                    Ok(_) => {},
                    Err(err) => eprintln!("p2p relay warning: {err:#}"),
                }
                println!("pooled mined height={} hash={} pool_id={}", chain.height(), hash, pool_id);
            }
            Err(err) => {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "pool_id": pool_id.to_string(),
                    "miner_address": key.address.clone(),
                    "pool_block_not_mined": err.to_string(),
                    "note": "Pool payout uses confirmed prior-window shares. Confirm at least one pool-share tx first, then pool-mine can build deterministic pool blocks.",
                }))?);
                break;
            }
        }
    }
    Ok(())
}


fn cmd_mine(settings: &Settings, blocks: u32, address: Option<&str>) -> Result<()> {
    let mut chain = load_or_init_chain(settings)?;
    let mut wallet = load_or_init_wallet(settings)?;
    let miner = match address {
        Some(s) => Address::parse_with_prefix(s, &settings.network.address_prefix)?,
        None => {
            if wallet.default_key().is_none() {
                let key = wallet.create_key(settings, "miner", chain.height())?;
                println!("created miner address: {}", key.address);
                save_wallet(settings, &wallet)?;
            }
            Address::parse_with_prefix(wallet.default_address().context("wallet empty")?, &settings.network.address_prefix)?
        }
    };
    for _ in 0..blocks {
        p2p::mining_safety_check(settings).with_context(|| "mining safety check failed")?;
        chain = load_or_init_chain(settings)?;
        if settings.p2p.enabled {
            p2p::mining_parent_guard(settings, chain.height(), chain.tip_hash())
                .with_context(|| "mining candidate parent guard failed")?;
        }
        let block = mine_next_block(&chain, settings, &miner, MiningOptions::from_settings(settings))?;
        let candidate_parent_hash = block.header.prev_block_hash;
        let candidate_parent_height = chain.height();
        if settings.p2p.enabled {
            p2p::mining_parent_submit_guard(settings, candidate_parent_height, candidate_parent_hash)
                .with_context(|| "block submit guard failed")?;
        }
        chain = load_or_init_chain(settings)?;
        if chain.height() != candidate_parent_height || chain.tip_hash() != candidate_parent_hash {
            bail!("block became stale before submit: local tip is #{} {}, candidate parent was #{} {}", chain.height(), chain.tip_hash(), candidate_parent_height, candidate_parent_hash);
        }
        let relay_block = block.clone();
        let hash = chain.connect_block(block, settings)?;
        save_chain(settings, &chain)?;
        match p2p::broadcast_block(settings, &relay_block) {
            Ok(sent) if sent > 0 => println!("relayed_to_peers: {sent}"),
            Ok(_) => {},
            Err(err) => eprintln!("p2p relay warning: {err:#}"),
        }
        println!("mined height={} hash={}", chain.height(), hash);
    }
    Ok(())
}
fn merge_cli_sync_report(into: &mut p2p::P2PSyncReport, other: p2p::P2PSyncReport) {
    into.peers_contacted = into.peers_contacted.saturating_add(other.peers_contacted);
    into.peer_errors = into.peer_errors.saturating_add(other.peer_errors);
    into.best_peer_height = into.best_peer_height.max(other.best_peer_height);
    into.chains_adopted = into.chains_adopted.saturating_add(other.chains_adopted);
    into.blocks_connected = into.blocks_connected.saturating_add(other.blocks_connected);
    into.txs_accepted = into.txs_accepted.saturating_add(other.txs_accepted);
    if other.height > into.height || (other.height == into.height && !other.tip_hash.trim().is_empty()) {
        into.height = other.height;
        into.tip_hash = other.tip_hash;
    }
}

fn cmd_sync(settings: &Settings) -> Result<()> {
    // HF82/v1.6.2: manual Sync uses the same single-flight catch-up path as
    // GUI/mining. It keeps retrying bounded official/direct catch-up before it
    // falls back to heavier snapshot repair.
    let mut report = p2p::hf82_manual_catchup(settings, 45_000)?;
    let local_after_suffix = load_or_init_chain(settings)?;
    let official_tip = p2p::official_http_tip(settings, 3_000).ok().flatten();
    let official_h = official_tip.as_ref().map(|(h, _)| *h).unwrap_or(report.best_peer_height);
    let official_hash = official_tip.as_ref().map(|(_, h)| h.clone()).unwrap_or_default();
    let local_suffix_hash = local_after_suffix.tip_hash().to_string();
    let same_height_wrong_tip = official_h == local_after_suffix.height() && !official_hash.trim().is_empty() && official_hash != local_suffix_hash;
    if official_h > local_after_suffix.height() || same_height_wrong_tip {
        if let Ok(tail) = p2p::sync_official_http_tail(settings, 20_000) {
            merge_cli_sync_report(&mut report, tail);
        }
    }
    let local_after_tail = load_or_init_chain(settings)?;
    if report.best_peer_height == 0 || local_after_tail.height() < report.best_peer_height {
        let followup = p2p::sync_quick(settings, 2, 4_000)?;
        merge_cli_sync_report(&mut report, followup);
    }
    let local_after_quick = load_or_init_chain(settings)?;
    let official_h = official_tip.as_ref().map(|(h, _)| *h).unwrap_or(report.best_peer_height);
    let official_hash = official_tip.as_ref().map(|(_, h)| h.clone()).unwrap_or_default();
    let quick_hash = local_after_quick.tip_hash().to_string();
    let still_wrong_tip = official_h == local_after_quick.height() && !official_hash.trim().is_empty() && official_hash != quick_hash;
    if official_h > local_after_quick.height() || still_wrong_tip {
        if let Ok(full) = p2p::sync_official_http_snapshot(settings, 30_000) {
            merge_cli_sync_report(&mut report, full);
        }
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn cmd_peers(settings: &Settings) -> Result<()> {
    let status = p2p::peer_status(settings)?;
    let peers = status.peers.iter().map(|peer| {
        let public_address = peer.miner_address.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or("Guest");
        serde_json::json!({
            "public_address": public_address,
            "reachable": peer.reachable,
            "height": peer.height,
            "tip_hash": peer.tip_hash,
            "role": peer.role,
            "last_seen_unix": peer.last_seen_unix,
        })
    }).collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "enabled": status.enabled,
        "known_peers": status.known_peers,
        "reachable_peers": status.reachable_peers,
        "peers": peers,
        "privacy": "peer IPs hidden; use peers-raw only for local operator debugging"
    }))?);
    Ok(())
}

fn cmd_peers_raw(settings: &Settings) -> Result<()> {
    let status = p2p::peer_status(settings)?;
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}


fn cmd_preflight(settings: &Settings) -> Result<()> {
    let mut ok_all = true;
    let mut checks = Vec::new();

    let chain = load_or_init_chain(settings)?;
    let genesis_hash = genesis_block(settings)?.block_hash().to_string();
    let chain_valid = chain.validate_all(settings).is_ok();
    add_preflight_check(&mut checks, &mut ok_all, "chain_replay_valid", chain_valid, format!("height={} bestblockhash={}", chain.height(), chain.tip_hash()));

    add_preflight_check(&mut checks, &mut ok_all, "economics_supply_cap_21m", settings.consensus.max_money_atoms == 2_100_000_000_000_000, "max_money_atoms must be 21,000,000 QUB".to_string());
    add_preflight_check(&mut checks, &mut ok_all, "economics_reward_5_qub", settings.consensus.initial_subsidy_atoms == 500_000_000, "initial subsidy must be 5 QUB".to_string());
    add_preflight_check(&mut checks, &mut ok_all, "economics_target_spacing_60s", settings.consensus.target_spacing_secs == 60, "target spacing must be 60 seconds".to_string());
    add_preflight_check(&mut checks, &mut ok_all, "economics_halving_btc_x10", settings.consensus.subsidy_halving_interval == 2_100_000, "halving interval must be 2,100,000 blocks".to_string());
    add_preflight_check(&mut checks, &mut ok_all, "difficulty_adjustment_enabled", settings.consensus.difficulty_adjustment_interval > 0 && settings.consensus.difficulty_max_adjustment_factor >= 2, "DAA window/factor are non-zero".to_string());
    add_preflight_check(&mut checks, &mut ok_all, "qns_enabled", settings.qns.enabled, format!("QNS enabled={} activation_height={}", settings.qns.enabled, settings.qns.activation_height));
    add_preflight_check(&mut checks, &mut ok_all, "qns_max_label_32", settings.qns.max_label_chars == 32, "QNS max label chars should be 32 for v1.5.1".to_string());
    add_preflight_check(&mut checks, &mut ok_all, "qns_protocol_name", normalize_qns_name(&settings.qns.protocol_name, settings.qns.max_label_chars).ok().as_deref() == Some("qns.qub"), "reserved first QNS name must be qns.qub".to_string());
    add_preflight_check(&mut checks, &mut ok_all, "qns_protocol_address_valid", Address::parse_with_prefix(&settings.qns.protocol_address, &settings.network.address_prefix).is_ok(), "QNS protocol treasury address must match network prefix".to_string());
    add_preflight_check(&mut checks, &mut ok_all, "p2p_enabled", settings.p2p.enabled, "p2p.enabled should be true for testnet/mainnet".to_string());
    add_preflight_check(&mut checks, &mut ok_all, "p2p_limits_present", settings.p2p.max_inbound_peers > 0 && settings.p2p.max_outbound_peers > 0 && settings.p2p.max_message_bytes >= 1_048_576, "peer/message limits are configured".to_string());
    add_preflight_check(&mut checks, &mut ok_all, "jin_native_enabled", settings.features.jin_native_coin_enabled && settings.jin.enabled, format!("JIN native coin enabled at #{}", settings.jin.activation_height));
    add_preflight_check(&mut checks, &mut ok_all, "jin_supply_cap_105m", parse_jin_units_raw(&settings.jin.total_supply_units).map(|v| v == JIN_TOTAL_SUPPLY_UNITS).unwrap_or(false), "JIN supply cap must be exactly 105,000,000 JIN".to_string());

    if settings.network.name == "mainnet" {
        let seeds = p2p::release_bootnodes(settings);
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_plaintext_wallet_disabled", !settings.wallet.plaintext_keys_allowed, "mainnet local plaintext key creation must be disabled by default".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_coinbase_maturity", settings.consensus.coinbase_maturity >= 100, "mainnet coinbase maturity should be at least 100 blocks".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_dns_seeds_present", seeds.len() >= 2, format!("{} DNS seed domain(s) configured", seeds.len()));
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_seeds_not_placeholders", !seeds.iter().any(|b| looks_placeholder(b)), "seed domains must be final official domains, not placeholders".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_seeds_are_dns_not_raw_ip", !seeds.iter().any(|b| bootnode_host_is_raw_ip(b)), "publish DNS seed names; do not ship personal/server raw IPs in the public mainnet GUI".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_seed_domains_official", seeds.iter().all(|b| bootnode_host(b).ends_with("qubit-coin.io")), "official mainnet DNS seed domains must be under qubit-coin.io".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_seed_domains_resolve", seeds.iter().all(|b| bootnode_resolves(b)), "all mainnet DNS seed domains must resolve before building a public release".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_rpc_off_or_secret", !settings.rpc.enabled || !looks_placeholder(&settings.rpc.auth_token), "if RPC is enabled, auth_token must be a real secret".to_string());
        let current_height = chain.height();
        let qns_already_active = current_height >= settings.qns.activation_height;
        let qns_safely_ahead = settings.qns.activation_height >= current_height.saturating_add(500);
        let qns_activation_detail = if qns_already_active {
            format!("QNS already active since #{}; current height {}", settings.qns.activation_height, current_height)
        } else {
            format!("QNS activation #{} should be at least 500 blocks ahead of current height {}", settings.qns.activation_height, current_height)
        };
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_qns_activation_safe", qns_already_active || qns_safely_ahead, qns_activation_detail);
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_jin_activation_5555", settings.jin.activation_height == 5555, "JIN mainnet activation must be #5555".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_qns_miner_split_activation_8305", settings.qns.miner_split_activation_height == 8305, "QNS miner split mainnet activation must be #8305".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_jin_conversion_activation_8305", settings.jin.conversion_activation_height == 8305, "JIN conversion mainnet activation must be #8305".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_pools_activation_9999", settings.pools.activation_height == 9999 && settings.features.pooled_mining_enabled && settings.pools.enabled, "Pools mainnet activation must be #9999 and enabled by consensus override".to_string());
        let checkpoint_ok = chain.blocks.get(MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT as usize)
            .map(|b| b.block_hash().to_string() == MAINNET_FORK_SAFETY_CHECKPOINT_HASH)
            .unwrap_or(chain.height() < MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT);
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_fork_safety_checkpoint_10367", checkpoint_ok, format!("Mainnet checkpoint #{} must be {}", MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT, MAINNET_FORK_SAFETY_CHECKPOINT_HASH));
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_daa_v2_activation_10500", MAINNET_DAA_V2_ACTIVATION_HEIGHT == 10500, "DAA v2 activates at #10500".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_blast_activation_10600", MAINNET_BLAST_ACTIVATION_HEIGHT == 10600, "Blast activates at #10600".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_library_activation_10550", MAINNET_LIBRARY_ACTIVATION_HEIGHT == 10550 && settings.library.activation_height == 10550, "Library activates at #10550".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_jin_sale_activation_10720", MAINNET_JIN_SWAP_ACTIVATION_HEIGHT == 10720 && settings.jin_swap.activation_height == 10720, "JIN public sale activates at #10720".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "mainnet_jin_conversion_disabled_10720", MAINNET_JIN_CONVERSION_DISABLE_HEIGHT == 10720, "JIN Coin -> Token conversion disabled from #10720 until bridge is live".to_string());
    } else if settings.network.name == "testnet" {
        let seeds = p2p::release_bootnodes(settings);
        add_preflight_check(&mut checks, &mut ok_all, "testnet_dns_seeds_present", seeds.len() >= 2, format!("{} DNS seed domain(s) configured", seeds.len()));
        add_preflight_check(&mut checks, &mut ok_all, "testnet_seeds_not_placeholders", !seeds.iter().any(|b| looks_placeholder(b)), "seed domains must be final official domains, not placeholders".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "testnet_seeds_are_dns_not_raw_ip", !seeds.iter().any(|b| bootnode_host_is_raw_ip(b)), "publish DNS seed names; do not ship raw IPs in the public testnet GUI".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "testnet_seed_domains_official", seeds.iter().all(|b| bootnode_host(b).ends_with("qubit-coin.io")), "official testnet DNS seed domains must be under qubit-coin.io".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "testnet_seed_domains_resolve", seeds.iter().all(|b| bootnode_resolves(b)), "all testnet DNS seed domains must resolve before building a public testnet release".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "testnet_qns_activation_soon", settings.qns.activation_height >= 1, format!("QNS testnet activation #{}", settings.qns.activation_height));
        add_preflight_check(&mut checks, &mut ok_all, "testnet_jin_activation_3365", settings.jin.activation_height == 3365, "JIN testnet activation must be #3365".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "testnet_pools_activation_dynamic", settings.pools.activation_height >= 1 && settings.features.pooled_mining_enabled && settings.pools.enabled, format!("Pools testnet activation #{}", settings.pools.activation_height));
        add_preflight_check(&mut checks, &mut ok_all, "testnet_daa_v2_activation_3330", TESTNET_DAA_V2_ACTIVATION_HEIGHT == 3330, "DAA v2 testnet activation #3330".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "testnet_blast_activation_3420", TESTNET_BLAST_ACTIVATION_HEIGHT == 3420, "Blast testnet activation #3420".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "testnet_library_activation_3440", TESTNET_LIBRARY_ACTIVATION_HEIGHT == 3440 && settings.library.activation_height == 3440, "Library testnet activation #3440".to_string());
        add_preflight_check(&mut checks, &mut ok_all, "testnet_jin_sale_activation_3520", TESTNET_JIN_SWAP_ACTIVATION_HEIGHT == 3520 && settings.jin_swap.activation_height == 3520, "JIN public sale testnet activation #3520".to_string());
    }

    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "network": settings.network.name,
        "ok": ok_all,
        "height": chain.height(),
        "bestblockhash": chain.tip_hash().to_string(),
        "genesis_hash": genesis_hash,
        "checks": checks
    }))?);

    if !ok_all { bail!("preflight failed; fix failed checks before public launch"); }
    Ok(())
}

fn add_preflight_check(checks: &mut Vec<serde_json::Value>, ok_all: &mut bool, name: &str, ok: bool, detail: String) {
    if !ok { *ok_all = false; }
    checks.push(serde_json::json!({ "name": name, "ok": ok, "detail": detail }));
}

fn bootnode_resolves(value: &str) -> bool {
    value.trim().trim_start_matches("tcp://").to_socket_addrs().map(|mut addrs| addrs.next().is_some()).unwrap_or(false)
}

fn bootnode_host(value: &str) -> String {
    let raw = value.trim().trim_start_matches("tcp://");
    if let Some(rest) = raw.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest).to_ascii_lowercase()
    } else {
        raw.split(':').next().unwrap_or(raw).to_ascii_lowercase()
    }
}

fn bootnode_host_is_raw_ip(value: &str) -> bool {
    bootnode_host(value).parse::<IpAddr>().is_ok()
}

fn looks_placeholder(value: &str) -> bool {
    let v = value.trim().to_ascii_uppercase();
    v.is_empty()
        || v.contains("YOUR_")
        || v.contains("SEED_")
        || v.contains("CHANGE_THIS")
        || v.contains("PASTE_")
        || v.contains("EXAMPLE")
}


fn cmd_explorer_api(settings: &Settings, bind: &str) -> Result<()> {
    let listener = TcpListener::bind(bind).with_context(|| format!("failed to bind explorer API on {bind}"))?;
    println!("QUB Explorer API listening on http://{bind}");
    println!("Network={} data_dir={}", settings.network.name, settings.node.data_dir);
    println!("Read-only mode: every request reloads chain.json; no explorer database is used.");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let s = settings.clone();
                thread::spawn(move || {
                    if let Err(err) = handle_explorer_http(stream, &s) {
                        eprintln!("explorer api request error: {err:#}");
                    }
                });
            }
            Err(err) => eprintln!("explorer api accept error: {err}"),
        }
    }
    Ok(())
}

fn handle_explorer_http(mut stream: TcpStream, settings: &Settings) -> Result<()> {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf)?;
    if n == 0 { return Ok(()); }
    let req = String::from_utf8_lossy(&buf[..n]);
    let line = req.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    if method == "OPTIONS" {
        return write_http(&mut stream, 204, "application/json", "{}", true);
    }
    if method != "GET" {
        return write_json_value(&mut stream, 405, serde_json::json!({"error":"method_not_allowed"}));
    }
    let (path, query) = split_path_query(target);
    let response = explorer_route(settings, &path, &query);
    match response {
        Ok(v) => write_json_value(&mut stream, 200, v),
        Err(err) => write_json_value(&mut stream, 400, serde_json::json!({"error": err.to_string()})),
    }
}

fn explorer_route(settings: &Settings, path: &str, query: &HashMap<String, String>) -> Result<serde_json::Value> {
    let chain = load_or_init_chain(settings)?;
    let path = path.trim_end_matches('/');
    if path.is_empty() || path == "/" || path == "/api" || path == "/api/v1" {
        return Ok(serde_json::json!({
            "service": "Qubit Coin Explorer API",
            "version": "1.5.1",
            "network": settings.network.name,
            "endpoints": [
                "/api/v1/summary",
                "/api/v1/blocks?limit=25&offset=0",
                "/api/v1/block/<height-or-hash>",
                "/api/v1/tx/<txid>",
                "/api/v1/address/<address>?limit=25&offset=0",
                "/api/v1/search?q=<height|hash|txid|address>",
                "/api/v1/mempool",
                "/api/v1/qns?limit=25&offset=0",
                "/api/v1/qns/<name.qub>",
                "/api/v1/resolve/<name.qub>",
                "/api/v1/pools?limit=25&offset=0",
                "/api/v1/pool/<pool-id>"
            ]
        }));
    }
    if path == "/health" || path == "/api/v1/health" {
        return Ok(serde_json::json!({"ok": true, "network": settings.network.name, "height": chain.height(), "bestblockhash": chain.tip_hash().to_string()}));
    }
    if path == "/api/v1/summary" {
        return Ok(explorer_summary(settings, &chain));
    }
    if path == "/api/v1/blocks" {
        let (limit, offset) = explorer_pagination(query, 25, 100);
        return Ok(explorer_blocks(settings, &chain, limit, offset));
    }
    if let Some(id) = path.strip_prefix("/api/v1/block/") {
        return explorer_block(settings, &chain, &url_decode(id));
    }
    if let Some(id) = path.strip_prefix("/api/v1/tx/") {
        return explorer_tx(settings, &chain, &url_decode(id));
    }
    if let Some(addr) = path.strip_prefix("/api/v1/address/") {
        let (limit, offset) = explorer_pagination(query, 25, 100);
        return explorer_address(settings, &chain, &url_decode(addr), limit, offset);
    }
    if path == "/api/v1/search" {
        let q = query.get("q").map(|s| s.trim()).unwrap_or("");
        return explorer_search(settings, &chain, q);
    }
    if path == "/api/v1/mempool" {
        return Ok(explorer_mempool(settings, &chain));
    }
    if path == "/api/v1/pools" {
        let (limit, offset) = explorer_pagination(query, 25, 100);
        return explorer_pool_list(settings, &chain, limit, offset);
    }
    if let Some(pool_id) = path.strip_prefix("/api/v1/pool/") {
        return explorer_pool_info(settings, &chain, &url_decode(pool_id));
    }
    if path == "/api/v1/qns" {
        let (limit, offset) = explorer_pagination(query, 25, 100);
        return explorer_qns_list(settings, &chain, limit, offset);
    }
    if let Some(name) = path.strip_prefix("/api/v1/qns/") {
        return explorer_qns_name(settings, &chain, &url_decode(name));
    }
    if let Some(name) = path.strip_prefix("/api/v1/resolve/") {
        return explorer_qns_name(settings, &chain, &url_decode(name));
    }
    bail!("unknown explorer endpoint")
}

fn explorer_summary(settings: &Settings, chain: &ChainState) -> serde_json::Value {
    let latest = chain.blocks.iter().enumerate().rev().take(10).map(|(height, block)| block_summary_json(settings, block, height as u32)).collect::<Vec<_>>();
    let tx_count: usize = chain.blocks.iter().map(|b| b.transactions.len()).sum();
    let supply_atoms: u64 = chain.utxos.values().map(|c| c.tx_out.value.atoms()).sum();
    let peers = p2p::peer_status(settings).ok();
    serde_json::json!({
        "network": settings.network.name,
        "height": chain.height(),
        "bestblockhash": chain.tip_hash().to_string(),
        "total_work_hex": chain.total_work_hex().unwrap_or_else(|_| "0".to_string()),
        "mempooltx": chain.mempool.len(),
        "confirmed_txs": tx_count,
        "utxo_count": chain.utxos.len(),
        "supply_atoms": supply_atoms,
        "supply_qub": amount_string(supply_atoms),
        "target_spacing_secs": settings.consensus.target_spacing_secs,
        "initial_subsidy_qub": amount_string(settings.consensus.initial_subsidy_atoms),
        "halving_interval": settings.consensus.subsidy_halving_interval,
        "best_blocks": latest,
        "qns_count": qns_registry_from_blocks(settings, &chain.blocks).map(|m| m.len()).unwrap_or(0),
        "qns_activation_height": settings.qns.activation_height,
        "qns_miner_split_activation_height": settings.qns.miner_split_activation_height,
        "qns_protocol_name": settings.qns.protocol_name,
        "qns_protocol_address": settings.qns.protocol_address,
        "pools_enabled": settings.features.pooled_mining_enabled && settings.pools.enabled,
        "pools_activation_height": settings.pools.activation_height,
        "pools_protocol_name": settings.pools.protocol_name,
        "pools_protocol_address": settings.pools.protocol_address,
        "pools_count": pools_registry_from_blocks(settings, &chain.blocks).map(|m| m.len()).unwrap_or(0),
        "peer_snapshot": peers.map(|p| serde_json::json!({
            "known_peers": p.known_peers,
            "reachable_peers": p.reachable_peers,
        }))
    })
}

fn explorer_blocks(settings: &Settings, chain: &ChainState, limit: usize, offset: usize) -> serde_json::Value {
    let total = chain.blocks.len();
    let rows = chain.blocks.iter().enumerate().rev().skip(offset).take(limit).map(|(height, block)| block_summary_json(settings, block, height as u32)).collect::<Vec<_>>();
    serde_json::json!({"network": settings.network.name, "total": total, "limit": limit, "offset": offset, "blocks": rows})
}

fn explorer_block(settings: &Settings, chain: &ChainState, id: &str) -> Result<serde_json::Value> {
    let maybe = if let Ok(height) = id.parse::<usize>() {
        chain.blocks.get(height).map(|b| (height as u32, b))
    } else {
        chain.blocks.iter().enumerate().find(|(_, b)| b.block_hash().to_string() == id).map(|(h,b)| (h as u32,b))
    };
    let (height, block) = maybe.context("block not found")?;
    let txs = block.transactions.iter().map(|tx| tx_json(settings, chain, tx, Some(height))).collect::<Vec<_>>();
    let prev = if height == 0 { None } else { chain.blocks.get(height as usize - 1).map(|b| b.block_hash().to_string()) };
    let next = chain.blocks.get(height as usize + 1).map(|b| b.block_hash().to_string());
    Ok(serde_json::json!({
        "network": settings.network.name,
        "height": height,
        "hash": block.block_hash().to_string(),
        "prev_block_hash": block.header.prev_block_hash.to_string(),
        "prev": prev,
        "next": next,
        "confirmations": chain.height().saturating_sub(height).saturating_add(1),
        "header": {
            "version": block.header.version,
            "merkle_root": block.header.merkle_root.to_string(),
            "time": block.header.time,
            "bits": format!("0x{:08x}", block.header.bits),
            "nonce": block.header.nonce,
        },
        "tx_count": block.transactions.len(),
        "transactions": txs
    }))
}

fn explorer_tx(settings: &Settings, chain: &ChainState, txid: &str) -> Result<serde_json::Value> {
    for (height, block) in chain.blocks.iter().enumerate() {
        for tx in &block.transactions {
            if tx.txid().to_string() == txid {
                return Ok(serde_json::json!({"network": settings.network.name, "status": "confirmed", "height": height, "block_hash": block.block_hash().to_string(), "confirmations": chain.height().saturating_sub(height as u32).saturating_add(1), "tx": tx_json(settings, chain, tx, Some(height as u32))}));
            }
        }
    }
    for tx in &chain.mempool {
        if tx.txid().to_string() == txid {
            return Ok(serde_json::json!({"network": settings.network.name, "status": "mempool", "confirmations": 0, "tx": tx_json(settings, chain, tx, None)}));
        }
    }
    bail!("transaction not found")
}

fn explorer_address(settings: &Settings, chain: &ChainState, address: &str, limit: usize, offset: usize) -> Result<serde_json::Value> {
    let addr = Address::parse_with_prefix(address, &settings.network.address_prefix)?;
    let script = addr.script_pubkey().0;
    let index = build_output_index(settings, chain);
    let mut received_atoms = 0u64;
    let mut spent_atoms = 0u64;
    let mut utxo_atoms = 0u64;
    let mut immature_atoms = 0u64;
    let mut utxos = Vec::new();
    let mut history = Vec::new();

    for entry in index.outputs.values().filter(|e| e.address.as_deref() == Some(address)) {
        received_atoms = received_atoms.saturating_add(entry.value_atoms);
        if let Some(sp) = index.spent.get(&entry.outpoint_key) {
            spent_atoms = spent_atoms.saturating_add(entry.value_atoms);
            history.push(serde_json::json!({
                "kind": "spent",
                "height": sp.height,
                "txid": sp.txid,
                "value_atoms": entry.value_atoms,
                "value_qub": amount_string(entry.value_atoms),
                "spent_outpoint": entry.outpoint_key,
                "block_hash": sp.block_hash,
            }));
        } else {
            utxo_atoms = utxo_atoms.saturating_add(entry.value_atoms);
            if entry.is_coinbase && chain.height().saturating_sub(entry.height) < settings.consensus.coinbase_maturity {
                immature_atoms = immature_atoms.saturating_add(entry.value_atoms);
            }
            utxos.push(serde_json::json!({
                "outpoint": entry.outpoint_key,
                "txid": entry.txid,
                "vout": entry.vout,
                "height": entry.height,
                "value_atoms": entry.value_atoms,
                "value_qub": amount_string(entry.value_atoms),
                "coinbase": entry.is_coinbase,
                "mature": !(entry.is_coinbase && chain.height().saturating_sub(entry.height) < settings.consensus.coinbase_maturity),
            }));
        }
        history.push(serde_json::json!({
            "kind": "received",
            "height": entry.height,
            "txid": entry.txid,
            "vout": entry.vout,
            "value_atoms": entry.value_atoms,
            "value_qub": amount_string(entry.value_atoms),
            "block_hash": entry.block_hash,
            "coinbase": entry.is_coinbase,
        }));
    }

    for tx in &chain.mempool {
        for (vout, out) in tx.outputs.iter().enumerate() {
            if out.script_pubkey.0 == script {
                history.push(serde_json::json!({"kind":"mempool_received", "height": null, "txid": tx.txid().to_string(), "vout": vout, "value_atoms": out.value.atoms(), "value_qub": amount_string(out.value.atoms())}));
            }
        }
    }

    history.sort_by(|a,b| json_height_sort_key(b).cmp(&json_height_sort_key(a)));
    let total_history = history.len();
    let history_page = history.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
    utxos.sort_by(|a,b| json_height_sort_key(b).cmp(&json_height_sort_key(a)));

    Ok(serde_json::json!({
        "network": settings.network.name,
        "address": address,
        "height": chain.height(),
        "received_atoms": received_atoms,
        "received_qub": amount_string(received_atoms),
        "spent_atoms": spent_atoms,
        "spent_qub": amount_string(spent_atoms),
        "balance_atoms": utxo_atoms,
        "balance_qub": amount_string(utxo_atoms),
        "spendable_atoms": utxo_atoms.saturating_sub(immature_atoms),
        "spendable_qub": amount_string(utxo_atoms.saturating_sub(immature_atoms)),
        "immature_atoms": immature_atoms,
        "immature_qub": amount_string(immature_atoms),
        "qns_names": qns_names_for_address(settings, chain, address).unwrap_or_default().into_iter().map(|r| r.name).collect::<Vec<_>>(),
        "utxo_count": utxos.len(),
        "utxos": utxos.into_iter().take(100).collect::<Vec<_>>(),
        "history_total": total_history,
        "limit": limit,
        "offset": offset,
        "history": history_page,
    }))
}

fn explorer_search(settings: &Settings, chain: &ChainState, q: &str) -> Result<serde_json::Value> {
    if q.is_empty() { bail!("empty search query"); }
    if q.to_ascii_lowercase().ends_with(".qub") {
        if let Some(rec) = qns_resolve(settings, chain, q)? {
            return Ok(serde_json::json!({"type":"qns", "name": rec.name, "address": rec.address, "height": rec.height, "txid": rec.txid.to_string()}));
        }
        return Ok(serde_json::json!({"type":"not_found", "query": q}));
    }
    if Address::parse_with_prefix(q, &settings.network.address_prefix).is_ok() {
        return Ok(serde_json::json!({"type":"address", "address": q}));
    }
    if let Ok(height) = q.parse::<usize>() {
        if height < chain.blocks.len() { return Ok(serde_json::json!({"type":"block", "height": height, "hash": chain.blocks[height].block_hash().to_string()})); }
    }
    if q.len() == 64 && q.chars().all(|c| c.is_ascii_hexdigit()) {
        for (height, block) in chain.blocks.iter().enumerate() {
            if block.block_hash().to_string() == q { return Ok(serde_json::json!({"type":"block", "height": height, "hash": q})); }
            for tx in &block.transactions { if tx.txid().to_string() == q { return Ok(serde_json::json!({"type":"tx", "txid": q, "height": height, "block_hash": block.block_hash().to_string()})); } }
        }
        for tx in &chain.mempool { if tx.txid().to_string() == q { return Ok(serde_json::json!({"type":"tx", "txid": q, "status":"mempool"})); } }
    }
    Ok(serde_json::json!({"type":"not_found", "query": q}))
}


fn explorer_pool_list(settings: &Settings, chain: &ChainState, limit: usize, offset: usize) -> Result<serde_json::Value> {
    let mut rows = pools_registry_from_blocks(settings, &chain.blocks)?.into_values().collect::<Vec<_>>();
    rows.sort_by(|a, b| a.created_height.cmp(&b.created_height).then(a.pool_id.to_string().cmp(&b.pool_id.to_string())));
    let total = rows.len();
    let page = rows.into_iter().skip(offset).take(limit).map(|p| pool_create_summary_json(settings, chain, &p)).collect::<Vec<_>>();
    Ok(serde_json::json!({
        "network": settings.network.name,
        "height": chain.height(),
        "activation_height": settings.pools.activation_height,
        "protocol_name": settings.pools.protocol_name,
        "protocol_address": settings.pools.protocol_address,
        "total": total,
        "limit": limit,
        "offset": offset,
        "pools": page,
    }))
}

fn explorer_pool_info(settings: &Settings, chain: &ChainState, pool_id_s: &str) -> Result<serde_json::Value> {
    let pool_id = Hash256::from_hex(pool_id_s)?;
    let registry = pools_registry_from_blocks(settings, &chain.blocks)?;
    let pool = registry.get(&pool_id).context("pool not found")?;
    let mut scores = pool_share_scores_from_blocks(settings, &chain.blocks, chain.height() + 1, pool_id).into_iter().collect::<Vec<_>>();
    scores.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(serde_json::json!({
        "network": settings.network.name,
        "height": chain.height(),
        "pool": pool_create_summary_json(settings, chain, pool),
        "scores": scores.into_iter().map(|(address, shares)| serde_json::json!({"address": address, "shares": shares})).collect::<Vec<_>>(),
        "pplns_window_blocks": settings.pools.share_window_blocks,
    }))
}


fn explorer_qns_list(settings: &Settings, chain: &ChainState, limit: usize, offset: usize) -> Result<serde_json::Value> {
    let mut rows = qns_registry_from_blocks(settings, &chain.blocks)?.into_values().collect::<Vec<_>>();
    rows.sort_by(|a,b| a.name.cmp(&b.name));
    let total = rows.len();
    let page = rows.into_iter().skip(offset).take(limit).map(|r| serde_json::json!({"name": r.name, "address": r.address, "height": r.height, "txid": r.txid.to_string(), "price_atoms": r.price_atoms, "price_qub": amount_string(r.price_atoms)})).collect::<Vec<_>>();
    Ok(serde_json::json!({"network": settings.network.name, "height": chain.height(), "activation_height": settings.qns.activation_height, "total": total, "limit": limit, "offset": offset, "names": page}))
}

fn explorer_qns_name(settings: &Settings, chain: &ChainState, name: &str) -> Result<serde_json::Value> {
    let normalized = normalize_qns_name(name, settings.qns.max_label_chars)?;
    let price_atoms = qns_registration_price_atoms(settings, &normalized)?;
    match qns_resolve(settings, chain, &normalized)? {
        Some(r) => Ok(serde_json::json!({"found": true, "name": r.name, "address": r.address, "height": r.height, "txid": r.txid.to_string(), "price_atoms": r.price_atoms, "price_qub": amount_string(r.price_atoms)})),
        None => Ok(serde_json::json!({"found": false, "name": normalized, "available_after_activation": chain.height() + 1 >= settings.qns.activation_height, "activation_height": settings.qns.activation_height, "price_atoms": price_atoms, "price_qub": amount_string(price_atoms)})),
    }
}

fn explorer_mempool(settings: &Settings, chain: &ChainState) -> serde_json::Value {
    let txs = chain.mempool.iter().map(|tx| tx_json(settings, chain, tx, None)).collect::<Vec<_>>();
    serde_json::json!({"network": settings.network.name, "count": txs.len(), "transactions": txs})
}

fn block_summary_json(settings: &Settings, block: &Block, height: u32) -> serde_json::Value {
    let coinbase_to = block.transactions.first().and_then(|tx| tx.outputs.first()).and_then(|o| address_from_script_pubkey(&settings.network.address_prefix, &o.script_pubkey)).map(|a| a.to_string());
    let reward_atoms = block.transactions.first().map(|tx| tx.outputs.iter().map(|o| o.value.atoms()).sum::<u64>()).unwrap_or(0);
    let pool_id = parse_pool_block_marker(block).map(|id| id.to_string());
    serde_json::json!({
        "height": height,
        "hash": block.block_hash().to_string(),
        "time": block.header.time,
        "tx_count": block.transactions.len(),
        "reward_atoms": reward_atoms,
        "reward_qub": amount_string(reward_atoms),
        "miner_address": coinbase_to.unwrap_or_else(|| "unknown".to_string()),
        "pool_block": pool_id.is_some(),
        "pool_id": pool_id,
        "bits": format!("0x{:08x}", block.header.bits),
        "nonce": block.header.nonce,
        "merkle_root": block.header.merkle_root.to_string(),
    })
}

fn tx_json(settings: &Settings, chain: &ChainState, tx: &Transaction, confirmed_height: Option<u32>) -> serde_json::Value {
    let index = build_output_index(settings, chain);
    let txid = tx.txid().to_string();
    let outputs = tx.outputs.iter().enumerate().map(|(vout, out)| {
        let address = address_from_script_pubkey(&settings.network.address_prefix, &out.script_pubkey).map(|a| a.to_string());
        let key = format!("{}:{}", txid, vout);
        let spent_by = index.spent.get(&key).map(|s| serde_json::json!({"txid": s.txid, "height": s.height, "block_hash": s.block_hash}));
        serde_json::json!({
            "vout": vout,
            "value_atoms": out.value.atoms(),
            "value_qub": amount_string(out.value.atoms()),
            "address": address,
            "script_pubkey_hex": hex::encode(out.script_pubkey.as_bytes()),
            "qns_registration": parse_qns_marker_script(&out.script_pubkey, settings).map(|r| serde_json::json!({"name": r.name, "address": r.address})),
            "pool_create": parse_pool_create_marker_script(&out.script_pubkey, settings).map(|p| serde_json::json!({"name": p.name, "manager_address": p.manager_address, "commission_bps": p.commission_bps, "capacity_slots": p.capacity_slots})),
            "pool_top_up": parse_pool_topup_marker_script(&out.script_pubkey, settings).map(|p| serde_json::json!({"pool_id": p.pool_id.to_string(), "manager_address": p.manager_address, "extra_capacity_slots": p.extra_capacity_slots})),
            "pool_commission": parse_pool_commission_marker_script(&out.script_pubkey, settings).map(|p| serde_json::json!({"pool_id": p.pool_id.to_string(), "manager_address": p.manager_address, "new_commission_bps": p.new_commission_bps})),
            "pool_rename": parse_pool_rename_marker_script(&out.script_pubkey, settings).map(|p| serde_json::json!({"pool_id": p.pool_id.to_string(), "manager_address": p.manager_address, "new_name": p.new_name})),
            "spent_by": spent_by,
        })
    }).collect::<Vec<_>>();
    let inputs = tx.inputs.iter().enumerate().map(|(vin, input)| {
        if is_pool_share_transaction(tx) {
            let share = parse_pool_share_tx(tx);
            serde_json::json!({"vin": vin, "coinbase": false, "pool_share": share.map(|s| serde_json::json!({"pool_id": s.pool_id.to_string(), "miner_address": s.miner_address, "parent_height": s.parent_height, "parent_hash": s.parent_hash.to_string(), "nonce": s.nonce})), "sequence": input.sequence, "signature_script_hex": hex::encode(input.signature_script.as_bytes())})
        } else if input.previous_output == OutPoint::null() {
            serde_json::json!({"vin": vin, "coinbase": true, "sequence": input.sequence, "signature_script_hex": hex::encode(input.signature_script.as_bytes())})
        } else {
            let key = input.previous_output.key();
            let prev = index.outputs.get(&key);
            serde_json::json!({
                "vin": vin,
                "coinbase": false,
                "previous_output": key,
                "prev_txid": input.previous_output.txid.to_string(),
                "prev_vout": input.previous_output.vout,
                "prev_value_atoms": prev.map(|p| p.value_atoms),
                "prev_value_qub": prev.map(|p| amount_string(p.value_atoms)),
                "prev_address": prev.and_then(|p| p.address.clone()),
                "sequence": input.sequence,
                "signature_script_hex": hex::encode(input.signature_script.as_bytes()),
            })
        }
    }).collect::<Vec<_>>();
    let output_sum: u64 = tx.outputs.iter().map(|o| o.value.atoms()).sum();
    let input_sum: Option<u64> = if tx.is_coinbase() { None } else {
        let mut sum = 0u64;
        let mut complete = true;
        for input in &tx.inputs {
            if let Some(prev) = index.outputs.get(&input.previous_output.key()) { sum = sum.saturating_add(prev.value_atoms); } else { complete = false; }
        }
        if complete { Some(sum) } else { None }
    };
    let fee_atoms = input_sum.map(|v| v.saturating_sub(output_sum));
    serde_json::json!({
        "txid": txid,
        "version": tx.version,
        "locktime": tx.locktime,
        "coinbase": tx.is_coinbase(),
        "pool_share": parse_pool_share_tx(tx).map(|s| serde_json::json!({"pool_id": s.pool_id.to_string(), "miner_address": s.miner_address, "parent_height": s.parent_height, "parent_hash": s.parent_hash.to_string(), "nonce": s.nonce})),
        "height": confirmed_height,
        "confirmations": confirmed_height.map(|h| chain.height().saturating_sub(h).saturating_add(1)),
        "input_count": tx.inputs.len(),
        "output_count": tx.outputs.len(),
        "output_sum_atoms": output_sum,
        "output_sum_qub": amount_string(output_sum),
        "fee_atoms": fee_atoms,
        "fee_qub": fee_atoms.map(amount_string),
        "inputs": inputs,
        "outputs": outputs,
        "raw_hex": hex::encode(tx.serialize_base()),
    })
}

#[derive(Clone)]
struct OutputEntry {
    outpoint_key: String,
    txid: String,
    vout: usize,
    value_atoms: u64,
    address: Option<String>,
    height: u32,
    block_hash: String,
    is_coinbase: bool,
}

#[derive(Clone)]
struct SpendEntry { txid: String, height: u32, block_hash: String }
struct ExplorerIndex { outputs: HashMap<String, OutputEntry>, spent: HashMap<String, SpendEntry> }

fn build_output_index(settings: &Settings, chain: &ChainState) -> ExplorerIndex {
    let mut outputs = HashMap::new();
    let mut spent = HashMap::new();
    for (height, block) in chain.blocks.iter().enumerate() {
        let height = height as u32;
        let block_hash = block.block_hash().to_string();
        for (tx_index, tx) in block.transactions.iter().enumerate() {
            let txid = tx.txid().to_string();
            for (vout, out) in tx.outputs.iter().enumerate() {
                outputs.insert(format!("{}:{}", txid, vout), OutputEntry {
                    outpoint_key: format!("{}:{}", txid, vout),
                    txid: txid.clone(),
                    vout,
                    value_atoms: out.value.atoms(),
                    address: address_from_script_pubkey(&settings.network.address_prefix, &out.script_pubkey).map(|a| a.to_string()),
                    height,
                    block_hash: block_hash.clone(),
                    is_coinbase: tx_index == 0,
                });
            }
            for input in &tx.inputs {
                if input.previous_output != OutPoint::null() {
                    spent.insert(input.previous_output.key(), SpendEntry { txid: txid.clone(), height, block_hash: block_hash.clone() });
                }
            }
        }
    }
    ExplorerIndex { outputs, spent }
}

fn amount_string(atoms: u64) -> String { Amount::from_atoms(atoms).map(|a| a.to_string()).unwrap_or_else(|_| atoms.to_string()) }
fn json_height_sort_key(v: &serde_json::Value) -> u64 { v.get("height").and_then(|h| h.as_u64()).unwrap_or(u64::MAX) }

fn explorer_pagination(query: &HashMap<String, String>, default_limit: usize, max_limit: usize) -> (usize, usize) {
    let limit = query.get("limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(default_limit).clamp(1, max_limit);
    let offset = query.get("offset").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    (limit, offset)
}

fn split_path_query(target: &str) -> (String, HashMap<String, String>) {
    let mut parts = target.splitn(2, '?');
    let path = parts.next().unwrap_or("/").to_string();
    let mut query = HashMap::new();
    if let Some(q) = parts.next() {
        for pair in q.split('&') {
            if pair.is_empty() { continue; }
            let mut kv = pair.splitn(2, '=');
            let k = url_decode(kv.next().unwrap_or(""));
            let v = url_decode(kv.next().unwrap_or(""));
            query.insert(k, v);
        }
    }
    (path, query)
}

fn url_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => { out.push(b' '); i += 1; }
            b'%' if i + 2 < bytes.len() => {
                if let (Some(a), Some(b)) = (hex_val(bytes[i+1]), hex_val(bytes[i+2])) { out.push((a << 4) | b); i += 3; } else { out.push(bytes[i]); i += 1; }
            }
            b => { out.push(b); i += 1; }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}
fn hex_val(b: u8) -> Option<u8> { match b { b'0'..=b'9' => Some(b-b'0'), b'a'..=b'f' => Some(10+b-b'a'), b'A'..=b'F' => Some(10+b-b'A'), _ => None } }

fn write_json_value(stream: &mut TcpStream, status: u16, value: serde_json::Value) -> Result<()> {
    let body = serde_json::to_string_pretty(&value)?;
    write_http(stream, status, "application/json; charset=utf-8", &body, true)
}

fn write_http(stream: &mut TcpStream, status: u16, content_type: &str, body: &str, cors: bool) -> Result<()> {
    let reason = match status { 200 => "OK", 204 => "No Content", 400 => "Bad Request", 404 => "Not Found", 405 => "Method Not Allowed", 500 => "Internal Server Error", _ => "OK" };
    let cors_headers = if cors {
        "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\n"
    } else { "" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n{cors_headers}Connection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    stream.write_all(response.as_bytes())?;
    Ok(())
}
fn take_flag(args: &mut Vec<String>, flag: &str) -> Option<String> { let pos = args.iter().position(|a| a == flag)?; args.remove(pos); if pos >= args.len() { None } else { Some(args.remove(pos)) } }
fn help(config: &str) { println!("QUB Core v1.7.2\nUsage: qubd --config {config} <command>\nCommands: init, info, validate, node, sync, peers, peers-raw, preflight, wallet-new, wallet-address, wallet-list, balance, mempool, relay-mempool, send <address> <amount> [fee], send-jin <address> <amount_jin> [fee] [JIN|QUB], send-multi <QUB|JIN> <addr:amount,...> [fee] [JIN|QUB], blast-create <total_qub> <per_claim_qub> <max_claims> [private_code] [fee], blast-claim <QUBBLAST1|txid|vout|code> [claimant-address], convert-jin-token <matrix-address> <amount_jin> [fee] [JIN|QUB], jin-balance [address], jin-sale-list, buy-jin <listing-id> <amount_jin> [fee], mine [blocks] [address], pool-list, pool-info <pool-id>, pool-create <name> [commission_bps] [capacity_slots] [manager-address] [fee], pool-top-up <pool-id> <extra_capacity_slots> [fee], pool-set-commission <pool-id> <new_commission_bps> [fee], pool-rename <pool-id> <new-name> [fee], pool-join <pool-id> [miner-address], pool-mine <pool-id> [blocks] [miner-address], qns-resolve <name.qub>, qns-price <name.qub>, qns-list [address], qns-register <name.qub> [target-address] [fee], explorer-api [bind]"); }
