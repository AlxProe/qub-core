"""HF132 source inventory policy. Integrity is anchored by the external ZIP SHA256.

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

MANIFEST = "HF132-SOURCE-MANIFEST.json"
MANIFEST_SCHEMA = "QUB-HF132-SOURCE-1"
RELEASE = "HF132-v185"
PRODUCT_VERSION = "1.8.5"
PREPARATION_REVISION = "canary-memory-r1"
OBSOLETE_MANIFESTS = {"HF130-SOURCE-MANIFEST.json", "HF131-SOURCE-MANIFEST.json"}
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
    "config/headless-mainnet.toml": "9c419030df84b4cf97252c916270134efe2acadc2c22cb2f1a407866e89f5300",
    "config/mainnet-launch-template.toml": "7d3ef6d29e77a57ad348fc79364fecae6c0012b6b7fe35f97929c9dd54aad6ed",
    "config/mainnet.toml": "d5542bc3619650e1c946435c8433ad876fa776fb0044e1259d9ca198db2659ed",
    "config/regtest-lan.toml": "5b81c366825eb5d2e1b988d0013bddfcd23415fe1b3a385bd2c7ed497b31386b",
    "config/regtest-seed-template.toml": "fa087c4387118a993928066eef06ddee16926390bdc78162354f128d062cb198",
    "config/regtest.toml": "bfaba9dc5ed2c04cc747f9cca42d698ef0dba29e657b86bf6b76ebd04b7b6dfa",
    "config/testnet.toml": "dddb662581ba451af762e1ba066948a34a63d99ba307f0c1e9e568415643b845",
}

# HF132 changes sync scheduling and publication, retaining HF131 resource fixes.
# Freeze the actual consensus/work blocks from the deployed HF128 source.
# Slices include both delimiters; the start must occur exactly once. The first
# closing unindented Rust brace ends the scanner impl / wire enum, whose nested
# braces are indented in the exact pinned source. Changing any pinned bytes,
# adding an earlier terminator, or duplicating the entry signature fails closed.
FROZEN_SOURCE_SLICES = (
    ("GPU kernel", "src/gpu_miner.rs", b'const OPENCL_KERNEL: &str = r#"', b'"#;',
     "82f81612d1d5c1689200411420582d89b5852f031af354ef25061afd0c1a21e2"),
    ("GPU nonce scanner", "src/gpu_miner.rs", b'    pub fn scan_nonce_range(&mut self, prefix:', b'\n}\n',
     "882246b504c5e22cc6e47847448224fa2804910df50adaf01d32d4e831249bb7"),
    ("GPU work item policy", "src/gpu_miner.rs", b'pub fn recommended_work_items',
     b'#[cfg(not(target_os = "windows"))]',
     "2b64de166366ae7b2c3a9148e405b25ad25c602f14b60e97f40b1dd89d18dfaa"),
    ("GPU CPU midstate", "src/gpu_miner.rs", b'const SHA256_K_CPU',
     b'#[cfg(target_os = "windows")]\nconst OPENCL_KERNEL',
     "fa86d9e3783ca87890293b9704fa2e83945987d19c0d6a03034457f08a997b69"),
    ("P2P wire enum and serde attributes", "src/p2p.rs",
     b'#[derive(Debug, Clone, Serialize, Deserialize)]\n'
     b'#[serde(tag = "type", rename_all = "snake_case")]\nenum WireMessage {', b'\n}\n',
     "cdcc3433808a0a6dc5141c75acecbe49e60a598c2de138158104587250d38377"),
)


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
    for part in path.parts:
        require(not any(ord(char) < 32 or char in '<>"|?*' for char in part), "invalid Windows path")
        require(part == part.rstrip(" ."), "Windows path has trailing space/dot")
        stem = part.split(".", 1)[0].upper()
        require(stem not in {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"} and
                not re.fullmatch(r"(?:COM|LPT)[1-9\u00b9\u00b2\u00b3]", stem), "Windows device path")
    return path


def regular_metadata(path):
    """lstat catches Windows directory junctions as well as ordinary symlinks."""
    metadata = path.lstat()
    require(not stat.S_ISLNK(metadata.st_mode) and
            not (getattr(metadata, "st_file_attributes", 0) &
                 getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400)),
            f"symlink/reparse point is forbidden: {path.name}")
    return metadata


def require_outside_source(source, destination):
    source_real = source.resolve()
    destination_real = destination.resolve()
    require(destination_real != source_real and source_real not in destination_real.parents,
            "destination must be outside source (including resolved paths)")


def source_slice(data, start, end, label):
    require(data.count(start) == 1, f"missing/ambiguous compatibility slice: {label}")
    first = data.index(start)
    last = data.find(end, first + len(start))
    require(last >= 0, f"unterminated compatibility slice: {label}")
    return data[first:last + len(end)]


def verify_frozen_sources(root):
    for relative, expected in FROZEN_SHA256.items():
        require(sha256(root / relative) == expected, f"HF128 compatibility bytes changed: {relative}")
    cached = {}
    for label, relative, start, end, expected in FROZEN_SOURCE_SLICES:
        if relative not in cached:
            cached[relative] = (root / relative).read_bytes()
        actual = hashlib.sha256(source_slice(cached[relative], start, end, label)).hexdigest()
        require(actual == expected, f"HF128 compatibility bytes changed: {label}")


def check_file(path, relative, local_asset):
    require(stat.S_ISREG(regular_metadata(path).st_mode), f"not a regular file: {relative}")
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
    require(stat.S_ISDIR(regular_metadata(root).st_mode), "source root is not a regular directory")
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
            metadata = regular_metadata(path)
            is_dir = stat.S_ISDIR(metadata.st_mode)
            is_file = stat.S_ISREG(metadata.st_mode)
            require(is_dir or is_file, f"special file is forbidden: {relative}")
            if is_dir:
                require(entry.name.lower() not in FORBIDDEN_DIRS, f"runtime/build directory: {relative}")
                visit(path)
                continue
            require(relative not in OBSOLETE_MANIFESTS, f"obsolete source manifest: {relative}")
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
    require(isinstance(manifest, dict), "invalid source manifest")
    require(manifest.get("schema") == MANIFEST_SCHEMA and manifest.get("release") == RELEASE and
            manifest.get("product_version") == PRODUCT_VERSION and
            manifest.get("preparation_revision") == PREPARATION_REVISION,
            "wrong source manifest identity")
    entries = manifest.get("files")
    require(isinstance(entries, dict) and entries, "empty/invalid source manifest")
    for relative in entries:
        safe_relative(relative)
        require(relative != MANIFEST, "manifest cannot inventory itself")
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
        require(type(details.get("bytes")) is int and details["bytes"] >= 0 and
                isinstance(details.get("sha256"), str) and
                re.fullmatch(r"[0-9a-f]{64}", details["sha256"]),
                f"invalid manifest size/hash: {relative}")
        require(files[relative].stat().st_size == details.get("bytes"), f"source size changed: {relative}")
        require(sha256(files[relative]) == details.get("sha256"), f"source hash changed: {relative}")
    return files


def verify_release(root):
    verify_frozen_sources(root)
    identities = {
        "Cargo.toml": ['version = "1.8.5"', 'HF132'],
        "Cargo.lock": ['name = "qubd"\nversion = "1.8.5"'],
        "src/bin/qub_core.rs": ['const APP_VERSION: &str = "v1.8.5"'],
        "src/p2p.rs": ['const USER_AGENT: &str = "/QUB Core:1.8.5/"'],
        "src/rpc.rs": ['const RPC_VERSION: &str = "1.8.5"'],
        "src/bin/qub_rpc_miner.rs": ['const APP_VERSION: &str = "v1.8.5"'],
        "installer/QUB-Core.iss": ['#define MyAppVersion "1.8.5"'],
        "scripts/build-windows-release.ps1": ["$Version = '1.8.5'", "hotfix = 'HF132'"],
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
    verify_runbook_text((root / "HF132-v185-DEPLOY-RUNBOOK.md").read_text(encoding="utf-8"))


def verify_runbook_text(runbook):
    require(runbook.isascii(), "deployment runbook must be Windows-console ASCII safe")
    for filename in ("HF132-v185-source-no-assets.zip", "HF132-v185-SHA256SUMS.txt"):
        require(filename in runbook, f"wrong deployment artifact naming: {filename}")
    fences = re.findall(r"^```powershell[ \t]*\n(.*?)^```\s*$", runbook, re.M | re.S | re.I)
    require(bool(fences), "missing PowerShell runbook blocks")
    for index, fence in enumerate(fences, 1):
        require(fence.strip().startswith(("& {", ". {")) and fence.strip().endswith("}"),
                f"PowerShell fence {index} must be one fail-stop block")
        require("$ErrorActionPreference = 'Stop'" in fence, f"PowerShell fence {index} missing Stop preference")
