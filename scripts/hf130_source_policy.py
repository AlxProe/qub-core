"""HF130 source inventory policy. Integrity is anchored by the external ZIP SHA256.

Workspace exclusions are never copied by the exporter. Archive mode has no
workspace exclusions. Local runtime assets are checked separately and never
added to the signed-off source inventory.
"""

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat

MANIFEST = "HF130-SOURCE-MANIFEST.json"
MODES = ("archive", "local-build", "checkout")
WORKSPACE_EXCLUSIONS = {".git", "target", "dist"}
CHECKOUT_GOVERNANCE = {
    "LICENSE", "LICENSE.md", "LICENSE.txt", "NOTICE", "NOTICE.md", "NOTICE.txt",
    "SECURITY.md", "CONTRIBUTING.md", "CODE_OF_CONDUCT.md",
}
FORBIDDEN_DIRS = WORKSPACE_EXCLUSIONS | {"data", "chain-v2", "__pycache__"}
ASSET_READMES = {"assets/README.md", "explorer/public/assets/README.md"}
FORBIDDEN_NAMES = {
    "wallet.json", "node_id", "peers.json", "chain.json", "current.json",
    "previous.json", "legacy-export-status.json", ".env",
}
PRIVATE_EXTENSIONS = {".pem", ".key", ".pfx", ".p12", ".keystore"}
BINARY_EXTENSIONS = {".exe", ".dll", ".so", ".dylib", ".msi", ".pyc"}
ACTIVE_ASSET_EXTENSIONS = {
    ".7z", ".appx", ".bat", ".bash", ".bz2", ".cab", ".cjs", ".cmd",
    ".com", ".cpl", ".fish", ".gz", ".hta", ".html", ".inf", ".iso",
    ".jar", ".js", ".jsx", ".jse", ".lnk", ".mjs", ".msix", ".php",
    ".pl", ".ps1", ".psm1", ".py", ".rar", ".rb", ".reg", ".scr",
    ".sh", ".sys", ".tar", ".tgz", ".ts", ".tsx", ".url", ".vbs",
    ".wasm", ".wsf", ".xz", ".zsh", ".zip",
}
MEDIA_EXTENSIONS = {
    ".bmp", ".flac", ".gif", ".ico", ".jpeg", ".jpg", ".m4a", ".mp3",
    ".mp4", ".ogg", ".otf", ".png", ".svg", ".ttf", ".wav", ".webm",
    ".webp", ".woff", ".woff2",
}
FROZEN_SHA256 = {
    "src/pools.rs": "066d5135831e7701903433140c47c388024648f55d07dfcbeac597ba2add66a3",
    "src/gpu_miner.rs": "d915a70ec9235886f120f5c17e1be3a393e19e65185c70fbe0b974184e4bde86",
    "config/headless-mainnet.toml": "9c419030df84b4cf97252c916270134efe2acadc2c22cb2f1a407866e89f5300",
    "config/mainnet-launch-template.toml": "7d3ef6d29e77a57ad348fc79364fecae6c0012b6b7fe35f97929c9dd54aad6ed",
    "config/mainnet.toml": "d5542bc3619650e1c946435c8433ad876fa776fb0044e1259d9ca198db2659ed",
    "config/regtest-lan.toml": "5b81c366825eb5d2e1b988d0013bddfcd23415fe1b3a385bd2c7ed497b31386b",
    "config/regtest-seed-template.toml": "fa087c4387118a993928066eef06ddee16926390bdc78162354f128d062cb198",
    "config/regtest.toml": "bfaba9dc5ed2c04cc747f9cca42d698ef0dba29e657b86bf6b76ebd04b7b6dfa",
    "config/testnet.toml": "dddb662581ba451af762e1ba066948a34a63d99ba307f0c1e9e568415643b845",
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def safe_relative(value):
    require(isinstance(value, str) and value and "\\" not in value, "invalid manifest path")
    path = PurePosixPath(value)
    require(not path.is_absolute() and path.as_posix() == value, "non-canonical manifest path")
    require(all(part not in (".", "..") and ":" not in part for part in path.parts), "unsafe manifest path")
    return path


def check_file(path, relative, local_asset):
    name = path.name.lower()
    suffix = path.suffix.lower()
    require(name not in FORBIDDEN_NAMES, f"runtime/private file: {relative}")
    require(not re.fullmatch(r"blocks-.*\.jsonl|state-.*\.json|pending-.*", name), f"runtime state: {relative}")
    require(suffix not in PRIVATE_EXTENSIONS | BINARY_EXTENSIONS, f"private/binary file: {relative}")
    if re.search(r"secret|token|credential", name):
        require(relative == "deploy/digitalocean/generate-rpc-token.sh" or
                (local_asset and suffix in MEDIA_EXTENSIONS), f"sensitive filename: {relative}")
    with path.open("rb") as stream:
        prefix = stream.read(8192)
    require(not re.search(rb"-----BEGIN (?:RSA |EC |OPENSSH |ENCRYPTED )?PRIVATE KEY-----", prefix),
            f"private key material: {relative}")
    if local_asset:
        require(suffix not in ACTIVE_ASSET_EXTENSIONS, f"active/archive asset: {relative}")
        require(not prefix.startswith((b"MZ", b"\x7fELF", b"PK\x03\x04", b"#!", b"\x00asm",
                                       b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe",
                                       b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe")),
                f"executable/archive content in asset: {relative}")


def source_files(root, mode):
    """Return reviewed files, excluding only explicit top-level workspace roots."""
    require(mode in MODES, "unknown source policy mode")
    require(root.is_dir() and not root.is_symlink(), "source root is not a regular directory")
    found = {}
    folded = set()

    def visit(directory):
        for entry in sorted(os.scandir(directory), key=lambda item: item.name):
            path = Path(entry.path)
            relative = path.relative_to(root).as_posix()
            parts = PurePosixPath(relative).parts
            # A git worktree's .git file is metadata too. Excluded entries are
            # not traversed, inspected, trusted, or copied into an archive.
            if mode != "archive" and len(parts) == 1 and entry.name.lower() in WORKSPACE_EXCLUSIONS:
                continue
            safe_relative(relative)
            require(relative.casefold() not in folded, f"Windows case collision: {relative}")
            folded.add(relative.casefold())
            require(not entry.is_symlink(), f"symlink is forbidden: {relative}")
            metadata = entry.stat(follow_symlinks=False)
            is_dir = stat.S_ISDIR(metadata.st_mode)
            is_file = stat.S_ISREG(metadata.st_mode)
            require(is_dir or is_file, f"special file is forbidden: {relative}")
            if is_dir:
                require(entry.name.lower() not in FORBIDDEN_DIRS, f"runtime/build directory: {relative}")
                visit(path)
                continue
            in_assets = any(part.lower() == "assets" for part in parts[:-1])
            local_asset = mode == "local-build" and parts[0] == "assets" and relative not in ASSET_READMES
            require(not in_assets or local_asset or relative in ASSET_READMES, f"runtime asset: {relative}")
            check_file(path, relative, local_asset)
            if not local_asset:
                found[relative] = path

    visit(root)
    return found


def verify_inventory(root, mode):
    files = source_files(root, mode)
    require(MANIFEST in files, f"missing {MANIFEST}")
    manifest = json.loads(files[MANIFEST].read_text(encoding="utf-8"))
    require(manifest.get("schema") == "QUB-HF130-SOURCE-1" and manifest.get("release") == "HF130-v185",
            "wrong source manifest identity")
    entries = manifest.get("files")
    require(isinstance(entries, dict) and entries, "empty/invalid source manifest")
    for relative in entries:
        safe_relative(relative)
    expected = set(entries) | {MANIFEST}
    if mode != "archive":
        # Preserve the existing repository's license, notices and workflows.
        # They were inspected by source_files (including symlink/private-file
        # checks), but are not release source and must never enter an export.
        # If a future manifest owns one of these paths, its exact bytes apply.
        files = {name: path for name, path in files.items()
                 if name in expected or not
                 (name in CHECKOUT_GOVERNANCE or name.startswith(".github/"))}
    require(set(files) == expected,
            f"source inventory mismatch: missing={sorted(expected - set(files))}, extra={sorted(set(files) - expected)}")
    for relative, details in entries.items():
        require(isinstance(details, dict), f"invalid manifest entry: {relative}")
        require(files[relative].stat().st_size == details.get("bytes"), f"source size changed: {relative}")
        require(sha256(files[relative]) == details.get("sha256"), f"source hash changed: {relative}")
    return files


def verify_release(root):
    for relative, expected in FROZEN_SHA256.items():
        require(sha256(root / relative) == expected, f"HF128 compatibility bytes changed: {relative}")
    identities = {
        "Cargo.toml": ['version = "1.8.5"', 'HF130'],
        "Cargo.lock": ['name = "qubd"\nversion = "1.8.5"'],
        "src/bin/qub_core.rs": ['const APP_VERSION: &str = "v1.8.5"'],
        "src/p2p.rs": ['const USER_AGENT: &str = "/QUB Core:1.8.5/"'],
        "src/rpc.rs": ['const RPC_VERSION: &str = "1.8.5"'],
        "src/bin/qub_rpc_miner.rs": ['const APP_VERSION: &str = "v1.8.5"'],
        "installer/QUB-Core.iss": ['#define MyAppVersion "1.8.5"'],
        "scripts/build-windows-release.ps1": ["$Version = '1.8.5'", "hotfix = 'HF130'"],
        "src/fast_storage.rs": ['pub const HF123_FAST_STORAGE_MAGIC: &str = "QUB-FCE-1";'],
        "src/lib.rs": [
            'pub const MAINNET_QNS_ACTIVATION_HEIGHT: u32 = 1000;',
            'pub const MAINNET_JIN_ACTIVATION_HEIGHT: u32 = 5555;',
            'pub const MAINNET_QNS_MINER_SPLIT_ACTIVATION_HEIGHT: u32 = 8305;',
            'pub const MAINNET_JIN_CONVERSION_ACTIVATION_HEIGHT: u32 = 8305;',
            'pub const MAINNET_POOLS_ACTIVATION_HEIGHT: u32 = 9999;',
            'pub const MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT: u32 = 10367;',
            '"21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000";',
            'pub const MAINNET_DAA_V2_ACTIVATION_HEIGHT: u32 = 10500;',
            'pub const MAINNET_BLAST_ACTIVATION_HEIGHT: u32 = 10600;',
            'pub const MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT: u32 = 24000;',
            'pub const PROTOCOL_EPOCH_2_BLOCK_VERSION: u32 = 2;',
        ],
    }
    for relative, required in identities.items():
        text = (root / relative).read_text(encoding="utf-8")
        for marker in required:
            require(marker in text, f"release identity/compatibility missing in {relative}: {marker}")
    runbook = (root / "HF130-v185-DEPLOY-RUNBOOK.md").read_text(encoding="utf-8")
    require(runbook.isascii(), "deployment runbook must be Windows-console ASCII safe")
    for filename in ("HF130-v185-source-no-assets-r7.zip", "HF130-v185-r7-SHA256SUMS.txt"):
        require(filename in runbook, f"wrong deployment artifact naming: {filename}")
    fences = re.findall(r"^```powershell\n(.*?)^```\s*$", runbook, re.M | re.S)
    require(bool(fences), "missing PowerShell runbook blocks")
    for index, fence in enumerate(fences, 1):
        require(fence.strip().startswith(("& {", ". {")) and fence.strip().endswith("}"),
                f"PowerShell fence {index} must be one fail-stop block")
        require("$ErrorActionPreference = 'Stop'" in fence, f"PowerShell fence {index} missing Stop preference")
