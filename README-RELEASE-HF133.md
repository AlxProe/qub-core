# QUB Core 1.8.6 / HF133 source release

The final mainnet activation height is **100000**. Read
`release/HF133/RELEASE-METADATA.json` and
`release/HF133/RELEASE_NOTES-v1.8.6-HF133.md` for the final release decision.

All 226 files from the tested source archive are preserved byte-for-byte.
The 201 external Windows build assets are the exact inputs recorded by the
verified native build. Only this README and files in `release/HF133/` are
added as final release documentation; no Rust, Cargo, configuration, installer
template or historical test input is edited. `SHA256SUMS.txt` covers the
resulting public source tree. `HF133-SOURCE-MANIFEST.json` remains the original
inventory of the tested source archive, before these documented additions.

Historical preparation documents in the source root still describe the
candidate and earlier gates. Their candidate label is historical; the final
activation decision above supersedes it. The old HF132 publisher remains
disabled because its release metadata does not describe HF133.

`release/HF133/BUILD-PROVENANCE.json` links the release to the verified source
archive, executable hashes, native receipts, mainnet replay and canary report.
Its original source archive SHA256 differs from the checksum of this public
archive because this archive also includes the verified assets and final
release documents. It does not claim that compiler output is reproducible
across arbitrary toolchains.

For a Linux mainnet node build, from this source directory:

```sh
QUB_BUILD_CONFIG=mainnet cargo build --locked --release --bin qubd
```

For Windows x64 MSVC, use the established static CRT configuration:

```powershell
$env:QUB_BUILD_CONFIG = 'mainnet'
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --locked --release --target x86_64-pc-windows-msvc --bin qubd --bin qub-core --bin qub-rpc-miner
```

These commands compile the source; they do not publish or install a release,
start a network or mine. This release procedure does not execute testnet or
regtest. Use the official binaries when their reviewed hashes are required.
