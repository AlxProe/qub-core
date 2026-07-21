# QUB HF123 RPC and Reference Miner

## Production model

```text
qubd node
  ├─ QUB P2P
  ├─ Fast Chain Engine canonical state owner
  └─ authenticated loopback RPC
       ├─ qub-rpc-miner
       ├─ future official QUB pool service
       └─ future hardware/Stratum adapter
```

Do not expose raw QUB RPC directly to the public Internet. It has token authentication and strict limits but no built-in TLS. Keep it on loopback, an authenticated private network, or behind a separately reviewed TLS gateway.

## Headless config

`config/headless-mainnet.toml` deliberately uses a data directory and P2P port separate from the public seeds:

```text
P2P:   0.0.0.0:17446
RPC:   127.0.0.1:17445
Data:  /opt/qub/headless/data/mainnet
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

Duplicate sensitive headers are rejected.

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

Mining statistics are aggregate-only. They do not infer address ownership, operator identity or coordination.

## Mining templates

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

A template includes the tracked job ID, height, parent hash, block version, timestamp, compact target, QUB header bytes, nonce offset, coinbase bytes, extra nonce and expiry.

## Submit a tracked candidate

```bash
curl -sS \
  -H "Content-Type: application/json" \
  -H "X-QUB-RPC-Token: $TOKEN" \
  -d '{"job_id":"<job-id>","nonce":123456}' \
  http://127.0.0.1:17445/rpc/v1/mining/submit-block | jq
```

Submission requires a known, unexpired job tied to the current canonical parent and expected block version. Arbitrary untracked block submission is unavailable.

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

The worker is a CPU protocol reference and smoke tool. It is not intended to compete with ASIC/GPU infrastructure.

## Bitaxe/AxeOS boundary

Bitaxe Gamma performs SHA-256 ASIC work, but stock AxeOS speaks Bitcoin-oriented Stratum and Bitcoin job semantics. HF123 RPC is QUB-native and tracked-job based. A separately reviewed adapter must translate QUB templates and submissions while preserving QUB header, version and coinbase semantics. Hardware compatibility must be verified on the actual device before release.
