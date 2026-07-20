# QUB HF122 RPC and Reference Miner

## Production model

The recommended HF122 layout is:

```text
qubd node
  ├─ public/private QUB P2P
  └─ authenticated loopback RPC
       ├─ qub-rpc-miner (reference CPU worker)
       ├─ future official QUB pool service
       └─ future hardware/Stratum adapter
```

Do not expose raw HF122 RPC directly to the public Internet. It has token authentication but no built-in TLS. Keep it on loopback, an authenticated private network, or behind a separately reviewed TLS gateway.

## Headless config

Use `config/headless-mainnet.toml` as the starting template. It deliberately uses a data directory and P2P port separate from the public seeds:

```text
P2P: 0.0.0.0:17446
RPC: 127.0.0.1:17445
Data: /opt/qub/headless/data/mainnet
Token: /opt/qub/headless/config/rpc.token
```

Generate the token:

```bash
sudo install -d -m 0750 -o deploy -g deploy /opt/qub/headless/config
sudo -u deploy bash deploy/digitalocean/generate-rpc-token.sh \
  /opt/qub/headless/config/rpc.token
chmod 600 /opt/qub/headless/config/rpc.token
```

## Authentication

Every request must contain exactly one of:

```http
X-QUB-RPC-Token: <token>
```

or:

```http
Authorization: Bearer <token>
```

Duplicate sensitive authentication headers are rejected.

Example:

```bash
TOKEN="$(tr -d '\r\n' < /opt/qub/headless/config/rpc.token)"

curl -sS \
  -H "X-QUB-RPC-Token: $TOKEN" \
  http://127.0.0.1:17445/rpc/v1/status | jq
```

## Read endpoints

```text
GET /rpc/v1/status
GET /rpc/v1/chain/tip
GET /rpc/v1/chain/block/<height-or-hash>
GET /rpc/v1/chain/tx/<txid>
GET /rpc/v1/mempool?limit=250
GET /rpc/v1/mining/status
GET /rpc/v1/mining/stats?window=256
GET /rpc/v1/events/tip?after=<tip-hash>&wait_ms=30000
```

## Mining template endpoints

Solo:

```bash
curl -sS \
  -H "X-QUB-RPC-Token: $TOKEN" \
  "http://127.0.0.1:17445/rpc/v1/mining/template?address=qub1..." | jq
```

Batch:

```bash
curl -sS \
  -H "X-QUB-RPC-Token: $TOKEN" \
  "http://127.0.0.1:17445/rpc/v1/mining/template-batch?address=qub1...&count=8" | jq
```

Existing on-chain pool:

```bash
curl -sS \
  -H "X-QUB-RPC-Token: $TOKEN" \
  "http://127.0.0.1:17445/rpc/v1/mining/template?pool_id=<64-hex-pool-id>" | jq
```

A template includes:

```text
job_id
height / parent_hash / version / time / bits
target_hex
80-byte header and 76-byte nonce prefix
nonce_offset = 76
coinbase bytes and txid
extra_nonce
expiry
```

HF122 proof-of-work is double SHA-256. The response explicitly documents the internal-byte-order comparison required by the QUB implementation.

## Submit a tracked candidate

```bash
curl -sS \
  -H "Content-Type: application/json" \
  -H "X-QUB-RPC-Token: $TOKEN" \
  -d '{"job_id":"<job-id>","nonce":123456}' \
  http://127.0.0.1:17445/rpc/v1/mining/submit-block | jq
```

A submit can be rejected because the job is unknown/expired, proof of work is insufficient, the canonical parent moved, the job is stale, or the expected block version changed. Arbitrary full-block submission without a tracked job is intentionally unavailable.

## Reference miner

Solo:

```bash
/opt/qub/bin/qub-rpc-miner \
  --rpc http://127.0.0.1:17445 \
  --token-file /opt/qub/headless/config/rpc.token \
  --address qub1... \
  --workers 4 \
  --batch 4 \
  --refresh-secs 20
```

Existing on-chain pool:

```bash
/opt/qub/bin/qub-rpc-miner \
  --rpc http://127.0.0.1:17445 \
  --token-file /opt/qub/headless/config/rpc.token \
  --pool-id <64-hex-pool-id> \
  --workers 4 \
  --batch 4
```

The worker is a protocol reference and CPU smoke tool. It is not intended to compete with ASIC/GPU infrastructure.

## Bitaxe/AxeOS boundary

Bitaxe Gamma performs SHA-256 ASIC work, but stock AxeOS speaks Bitcoin-oriented Stratum and Bitcoin job semantics. HF122 RPC is QUB-native and tracked-job based. A separate adapter must translate QUB templates and submits, preserve QUB header/version/coinbase semantics, and be hardware-tested before any Dedicated QUB Miner claim.
