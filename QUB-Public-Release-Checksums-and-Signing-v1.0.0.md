# QUB Core Public Release Checksums + Signing Policy

This is the minimum policy before distributing a public `QUB Core v1.0.0` Windows package.

## Build reproducibility discipline

1. Freeze the exact source commit.
2. Freeze the exact `Cargo.lock` produced by the release build.
3. Freeze every consensus config value before the first public block.
4. Build from a clean checkout.
5. Do not rebuild a published version number with different binaries.

## Release artifacts

Each public release folder should contain:

```text
qub-core.exe
qubd.exe
Start-Qubit-Coin-Core.cmd
Start-QUB-Seed-Node.cmd
Sync-QUB-Once.cmd
qubd-cli.cmd
config\
assets\README.md
README-MINER-WINDOWS.md
SHA256SUMS.txt
RELEASE-NOTES.txt
```

## Checksums

The build script creates `SHA256SUMS.txt` for the release folder.

Verify locally:

```powershell
Get-ChildItem . -Recurse -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | ForEach-Object {
  $hash = Get-FileHash -Algorithm SHA256 $_.FullName
  "$($hash.Hash.ToLower())  $($_.FullName.Substring((Get-Location).Path.Length + 1))"
}
```

Publish the SHA256 of the final zip separately from the zip download page.

## Signing policy

Recommended release flow:

1. Generate `SHA256SUMS.txt` inside the release folder.
2. Zip the folder.
3. Generate SHA256 for the zip.
4. Sign the checksum manifest with an offline signing key.
5. Publish:
   - zip file
   - `SHA256SUMS.txt`
   - detached signature for `SHA256SUMS.txt`
   - public signing key fingerprint
   - release notes

Do not keep the signing key on a mining machine, seed node, build machine used for browsing, or Windows machine used for daily work.

## Consensus freeze checklist

Before public mainnet mining starts, confirm these are final and identical for everyone:

```text
network.name
network.magic
network.default_port
network.address_prefix
consensus.version
consensus.max_money_atoms
consensus.subsidy_halving_interval
consensus.initial_subsidy_atoms
consensus.coinbase_maturity
consensus.target_spacing_secs
consensus.difficulty_adjustment_interval
consensus.difficulty_max_adjustment_factor
consensus.pow_bits
consensus.genesis_time
consensus.genesis_bits
consensus.genesis_nonce
features.pooled_mining_enabled
features.jin_native_coin_enabled
```

After public launch, changing any of those values is a hard fork or a new chain.
