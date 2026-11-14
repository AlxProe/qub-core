//! Small, transactional UTXO update for an already validated coinbase.
//! Every contextual/header/feature check remains in ChainState::connect_block.
use crate::{CoinRecord, OutPoint, Transaction};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(crate) fn connect(
    tx: &Transaction,
    utxos: &mut Arc<HashMap<OutPoint, CoinRecord>>,
    height: u32,
) -> Result<()> {
    if !tx.is_coinbase() {
        bail!("coinbase-only UTXO update requires a coinbase");
    }
    let txid = tx.txid();
    let mut additions = Vec::new();
    let mut seen = HashSet::new();
    for (vout, output) in tx.outputs.iter().enumerate() {
        let outpoint = OutPoint { txid, vout: vout as u32 };
        let coin = CoinRecord { tx_out: output.clone(), height, is_coinbase: true };
        if utxos.contains_key(&outpoint) || !seen.insert(outpoint.clone()) {
            bail!("duplicate created outpoint");
        }
        additions.push((outpoint, coin));
    }
    // All fallible checks finished before touching shared or private state.
    // A retained reader triggers one COW clone; a private replay reuses its map.
    // Staging is proportional to this coinbase's outputs, not the entire UTXO set.
    if !additions.is_empty() {
        let target = Arc::make_mut(utxos);
        // Retain the legacy vout insertion order as well as identical contents.
        for (outpoint, coin) in additions {
            target.insert(outpoint, coin);
        }
    }
    Ok(())
}
