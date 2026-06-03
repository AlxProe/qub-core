const CFG = window.QUB_EXPLORER_CONFIG || {};
const API = (CFG.apiBase || '/api/v1').replace(/\/$/, '');
const PAGE = Number(CFG.pageSize || 25);
const REFRESH_MS = Number(CFG.refreshMs || 5000);
const view = document.getElementById('view');
const statusEl = document.getElementById('status');
const networkLabel = document.getElementById('networkLabel');
const apiStatus = document.getElementById('apiStatus');
let lastRoute = '';

const qs = (o) => new URLSearchParams(o).toString();
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const short = (s, a=10, b=8) => !s ? '—' : String(s).length <= a+b+3 ? esc(s) : `${esc(String(s).slice(0,a))}…${esc(String(s).slice(-b))}`;
const linkBlock = (height, hash) => `<a class="hash" href="#/block/${esc(hash || height)}">#${esc(height)}</a>`;
const linkHash = (hash, kind='block') => hash ? `<a class="hash" href="#/${kind}/${esc(hash)}">${short(hash,12,10)}</a>` : '—';
const linkAddress = (addr) => addr && addr !== 'unknown' ? `<a class="hash" href="#/address/${esc(addr)}">${short(addr,12,10)}</a>` : esc(addr || '—');
const fmtQ = (v) => `${esc(v ?? '0')} QUB`;
const age = (unix) => { if (!unix) return '—'; const d=Math.max(0, Math.floor(Date.now()/1000 - unix)); if (d<90) return `${d}s`; if (d<5400) return `${Math.floor(d/60)}m`; if (d<172800) return `${Math.floor(d/3600)}h`; return `${Math.floor(d/86400)}d`; };

async function api(path) {
  const r = await fetch(`${API}${path}`, {cache:'no-store'});
  if (!r.ok) {
    let msg = `${r.status} ${r.statusText}`;
    try { const j = await r.json(); if (j.error) msg = j.error; } catch {}
    throw new Error(msg);
  }
  return await r.json();
}

async function refreshStatus() {
  try {
    const s = await api('/summary');
    networkLabel.textContent = `${s.network} · height #${s.height}`;
    apiStatus.textContent = `API: live · ${s.bestblockhash?.slice(0,12)}…`;
    statusEl.innerHTML = [
      metric('Height', `#${s.height}`),
      metric('Best block', short(s.bestblockhash,12,10)),
      metric('Mempool', s.mempooltx),
      metric('Supply', fmtQ(s.supply_qub)),
      metric('Confirmed txs', s.confirmed_txs),
      metric('Peers', `${s.peer_snapshot?.reachable_peers ?? '—'} reachable / ${s.peer_snapshot?.known_peers ?? '—'} known`),
      metric('QNS', `${s.qns_count ?? 0} names · activates #${s.qns_activation_height ?? '—'}`),
    ].join('');
  } catch (e) {
    apiStatus.textContent = `API: ${e.message}`;
    statusEl.innerHTML = `<div class="notice error">Explorer API is not reachable: ${esc(e.message)}</div>`;
  }
}
function metric(k,v){ return `<div class="metric"><small>${esc(k)}</small><b>${v}</b></div>`; }

async function route() {
  const hash = location.hash || '#/';
  lastRoute = hash;
  try {
    if (hash === '#/' || hash === '#') return renderHome();
    const parts = hash.slice(2).split('/').map(decodeURIComponent);
    if (parts[0] === 'blocks') return renderBlocks(Number(parts[1]||0));
    if (parts[0] === 'block') return renderBlock(parts[1]);
    if (parts[0] === 'tx') return renderTx(parts[1]);
    if (parts[0] === 'address') return renderAddress(parts[1], Number(parts[2]||0));
    if (parts[0] === 'mempool') return renderMempool();
    if (parts[0] === 'qns') return renderQns(Number(parts[1]||0));
    if (parts[0] === 'name') return renderQnsName(parts[1]);
    view.innerHTML = `<div class="notice error">Unknown route.</div>`;
  } catch(e) {
    view.innerHTML = `<div class="view-head"><h1>Error</h1></div><div class="notice error">${esc(e.message)}</div>`;
  }
}

async function renderHome(){
  const s = await api('/summary');
  view.innerHTML = `
    <div class="view-head"><div><h1>Live chain</h1><p class="subtle">Latest blocks and current chain state loaded directly from the QUB node.</p></div><a class="pill ok" href="#/blocks/0">All blocks</a></div>
    <div class="grid-2">
      <section><h2>Latest blocks</h2>${blocksTable(s.best_blocks || [])}</section>
      <section><h2>Protocol</h2><div class="card kv">
        <div>Network</div><div>${esc(s.network)}</div>
        <div>Target spacing</div><div>${esc(s.target_spacing_secs)} seconds</div>
        <div>Initial subsidy</div><div>${fmtQ(s.initial_subsidy_qub)}</div>
        <div>Halving interval</div><div>${esc(s.halving_interval)} blocks</div>
        <div>Total work</div><div class="hash">${short(s.total_work_hex,18,18)}</div><div>QNS protocol</div><div>${esc(s.qns_protocol_name||"qns.qub")} → ${linkAddress(s.qns_protocol_address)}</div>
      </div><p><a class="pill" href="#/mempool">Mempool: ${esc(s.mempooltx)} tx</a> <a class="pill" href="#/qns/0">QNS names</a></p></section>
    </div>`;
}
async function renderBlocks(offset){
  const d = await api(`/blocks?${qs({limit:PAGE, offset})}`);
  view.innerHTML = `<div class="view-head"><h1>Blocks</h1><span class="pill">${d.total} total</span></div>${blocksTable(d.blocks)}${pager('#/blocks', offset, d.limit, d.total)}`;
}
function blocksTable(rows){
  return `<div class="table-wrap"><table><thead><tr><th>Height</th><th>Age</th><th>Txs</th><th>Reward</th><th>Mined by</th><th>Hash</th></tr></thead><tbody>${(rows||[]).map(b=>`<tr><td>${linkBlock(b.height,b.hash)}</td><td>${age(b.time)}</td><td>${esc(b.tx_count)}</td><td>${fmtQ(b.reward_qub)}</td><td>${linkAddress(b.miner_address)}</td><td>${linkHash(b.hash,'block')}</td></tr>`).join('')}</tbody></table></div>`;
}
async function renderBlock(id){
  const b = await api(`/block/${encodeURIComponent(id)}`);
  view.innerHTML = `<div class="view-head"><div><h1>Block #${esc(b.height)}</h1><p class="hash">${esc(b.hash)}</p></div><span class="pill ok">${esc(b.confirmations)} confirmations</span></div>
  <div class="card kv">
    <div>Previous</div><div>${b.prev ? linkHash(b.prev,'block') : 'genesis'}</div><div>Next</div><div>${b.next ? linkHash(b.next,'block') : '—'}</div>
    <div>Time</div><div>${new Date((b.header.time||0)*1000).toLocaleString()}</div><div>Bits</div><div>${esc(b.header.bits)}</div>
    <div>Nonce</div><div>${esc(b.header.nonce)}</div><div>Merkle root</div><div class="hash">${esc(b.header.merkle_root)}</div>
  </div><h2>Transactions</h2>${txTable(b.transactions || [])}`;
}
async function renderTx(txid){
  const d = await api(`/tx/${encodeURIComponent(txid)}`); const tx=d.tx;
  view.innerHTML = `<div class="view-head"><div><h1>Transaction</h1><p class="hash">${esc(tx.txid)}</p></div><span class="pill ${d.status==='confirmed'?'ok':''}">${esc(d.status)} · ${esc(d.confirmations || 0)} confirmations</span></div>
  <div class="card kv"><div>Coinbase</div><div>${esc(tx.coinbase)}</div><div>Height</div><div>${tx.height ?? '—'}</div><div>Outputs</div><div>${fmtQ(tx.output_sum_qub)}</div><div>Fee</div><div>${tx.fee_qub ? fmtQ(tx.fee_qub) : '—'}</div></div>
  <div class="grid-2"><section><h2>Inputs</h2>${inputsTable(tx.inputs || [])}</section><section><h2>Outputs</h2>${outputsTable(tx.outputs || [])}</section></div>`;
}
function txTable(rows){ return `<div class="table-wrap"><table><thead><tr><th>Txid</th><th>Type</th><th>Outputs</th><th>Fee</th></tr></thead><tbody>${rows.map(t=>`<tr><td>${linkHash(t.txid,'tx')}</td><td>${t.coinbase?'coinbase':'transfer'}</td><td>${fmtQ(t.output_sum_qub)}</td><td>${t.fee_qub ? fmtQ(t.fee_qub) : '—'}</td></tr>`).join('')}</tbody></table></div>`; }
function inputsTable(rows){ return `<div class="table-wrap"><table><thead><tr><th>Input</th><th>Address</th><th>Value</th></tr></thead><tbody>${rows.map(i=>`<tr><td>${i.coinbase?'coinbase':esc(i.previous_output)}</td><td>${i.prev_address?linkAddress(i.prev_address):'—'}</td><td>${i.prev_value_qub?fmtQ(i.prev_value_qub):'—'}</td></tr>`).join('')}</tbody></table></div>`; }
function outputsTable(rows){ return `<div class="table-wrap"><table><thead><tr><th>Vout</th><th>Address / QNS</th><th>Value</th><th>Spent</th></tr></thead><tbody>${rows.map(o=>`<tr><td>${esc(o.vout)}</td><td>${o.qns_registration?`🏷 <a class="hash" href="#/name/${esc(o.qns_registration.name)}">${esc(o.qns_registration.name)}</a> → ${linkAddress(o.qns_registration.address)}`:linkAddress(o.address)}</td><td>${fmtQ(o.value_qub)}</td><td>${o.spent_by?linkHash(o.spent_by.txid,'tx'):'no'}</td></tr>`).join('')}</tbody></table></div>`; }
async function renderAddress(addr, offset){
  const a = await api(`/address/${encodeURIComponent(addr)}?${qs({limit:PAGE, offset})}`);
  view.innerHTML = `<div class="view-head"><div><h1>Address</h1><p class="hash">${esc(a.address)}</p></div><span class="pill ok">Balance ${fmtQ(a.balance_qub)}</span></div>
  <div class="cards">${metric('Spendable',fmtQ(a.spendable_qub))}${metric('Immature',fmtQ(a.immature_qub))}${metric('Received',fmtQ(a.received_qub))}${metric('Spent',fmtQ(a.spent_qub))}${metric('UTXOs',a.utxo_count)}${metric('QNS', (a.qns_names||[]).map(n=>`<a href="#/name/${esc(n)}">${esc(n)}</a>`).join(', ') || '—')}</div>
  <h2>History</h2>${historyTable(a.history||[])}${pager(`#/address/${encodeURIComponent(addr)}`, offset, a.limit, a.history_total)}`;
}
function historyTable(rows){ return `<div class="table-wrap"><table><thead><tr><th>Kind</th><th>Height</th><th>Txid</th><th>Value</th></tr></thead><tbody>${rows.map(h=>`<tr><td>${esc(h.kind)}</td><td>${h.height==null?'mempool':linkBlock(h.height,h.block_hash)}</td><td>${linkHash(h.txid,'tx')}</td><td>${fmtQ(h.value_qub)}</td></tr>`).join('')}</tbody></table></div>`; }

async function renderQns(offset){
  const d = await api(`/qns?${qs({limit:PAGE, offset})}`);
  view.innerHTML = `<div class="view-head"><div><h1>QNS names</h1><p class="subtle">Permanent on-chain .qub names loaded directly from chain state.</p></div><span class="pill">${d.total} total</span></div>
  <div class="table-wrap"><table><thead><tr><th>Name</th><th>Address</th><th>Height</th><th>Price</th><th>Tx</th></tr></thead><tbody>${(d.names||[]).map(n=>`<tr><td><a class="hash" href="#/name/${esc(n.name)}">${esc(n.name)}</a></td><td>${linkAddress(n.address)}</td><td>${n.height?linkBlock(n.height):'reserved'}</td><td>${fmtQ(n.price_qub)}</td><td>${n.txid?linkHash(n.txid,'tx'):'—'}</td></tr>`).join('')}</tbody></table></div>${pager('#/qns', offset, d.limit, d.total)}`;
}
async function renderQnsName(name){
  const n = await api(`/qns/${encodeURIComponent(name)}`);
  view.innerHTML = n.found
    ? `<div class="view-head"><div><h1>${esc(n.name)}</h1><p class="subtle">QNS permanent name</p></div><span class="pill ok">registered</span></div><div class="card kv"><div>Address</div><div>${linkAddress(n.address)}</div><div>Height</div><div>${n.height?linkBlock(n.height):'reserved'}</div><div>Txid</div><div>${n.txid?linkHash(n.txid,'tx'):'—'}</div><div>Paid</div><div>${fmtQ(n.price_qub)}</div></div>`
    : `<div class="view-head"><div><h1>${esc(n.name)}</h1><p class="subtle">QNS permanent name</p></div><span class="pill">available after activation</span></div><div class="card kv"><div>Activation height</div><div>#${esc(n.activation_height)}</div><div>Registration price</div><div>${fmtQ(n.price_qub)}</div></div>`;
}

async function renderMempool(){ const m=await api('/mempool'); view.innerHTML = `<div class="view-head"><h1>Mempool</h1><span class="pill">${m.count} tx</span></div>${txTable(m.transactions||[])}`; }
function pager(base, offset, limit, total){ const prev=Math.max(0, offset-limit), next=offset+limit; return `<div class="pager">${offset>0?`<a class="pill" href="${base}/${prev}">← Previous</a>`:''}${next<total?`<a class="pill" href="${base}/${next}">Next →</a>`:''}</div>`; }

document.getElementById('searchForm').addEventListener('submit', async (e)=>{
  e.preventDefault();
  const q = document.getElementById('searchInput').value.trim(); if(!q) return;
  try { const r = await api(`/search?${qs({q})}`); if(r.type==='block') location.hash=`#/block/${r.hash}`; else if(r.type==='tx') location.hash=`#/tx/${r.txid}`; else if(r.type==='address') location.hash=`#/address/${r.address}`; else if(r.type==='qns') location.hash=`#/name/${r.name}`; else view.innerHTML=`<div class="notice">Nothing found for <code>${esc(q)}</code>.</div>`; }
  catch(err){ view.innerHTML=`<div class="notice error">${esc(err.message)}</div>`; }
});

document.getElementById('themeToggle').addEventListener('click', ()=>{
  document.documentElement.classList.toggle('light');
  localStorage.setItem('qubExplorerTheme', document.documentElement.classList.contains('light')?'light':'dark');
  document.getElementById('themeToggle').textContent = document.documentElement.classList.contains('light') ? 'Light' : 'Dark';
});
if(localStorage.getItem('qubExplorerTheme')==='light'){ document.documentElement.classList.add('light'); document.getElementById('themeToggle').textContent='Light'; }
window.addEventListener('hashchange', route);
refreshStatus(); route(); setInterval(()=>{ refreshStatus(); if(location.hash===lastRoute && ['#/','#'].includes(location.hash||'#/')) route(); }, REFRESH_MS);
