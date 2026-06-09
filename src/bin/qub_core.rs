#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use anyhow::{Context as AnyhowContext, Result};
use eframe::egui;
use qubd::*;
use image::AnimationDecoder;
use qrcode::{QrCode, types::Color as QrColor};
use num_bigint::BigUint;
use num_traits::{One, Zero, ToPrimitive, Num};
use secp256k1::{Secp256k1, SecretKey, PublicKey, Message};
use secp256k1::ecdsa::RecoverableSignature;
use tiny_keccak::{Hasher, Keccak};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::str::FromStr;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[path = "../gpu_miner.rs"]
mod gpu_miner;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use std::process::Command;
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(target_os = "windows")]
#[link(name = "winmm")]
unsafe extern "system" {
    fn mciSendStringW(
        command: *const u16,
        return_string: *mut u16,
        return_length: u32,
        callback: *mut std::ffi::c_void,
    ) -> u32;
}

const APP_TITLE: &str = "Qubit Coin Core";
const APP_TITLE_TESTNET: &str = "Qubit Coin Core Testnet";
const APP_VERSION: &str = "v1.7.1";
const BUILD_CONFIG: &str = env!("QUB_BUILD_CONFIG");
const LOGO_PATH: &str = "assets/qubit-coin-logo.png";
const OPENING_BANNER_PATH: &str = "assets/opening-banner.png";
const JIN_LOGO_PATH: &str = "assets/jin-coin-logo.png";
const MINED_SOUND_PATH: &str = "assets/mined.mp3";
const NETWORK_MINED_SOUND_PATH: &str = "assets/network-mined.mp3";
const MINING_OFF_ICON_PATH: &str = "assets/mining-off.png";
const MINING_OFF_ICON_WHITE_PATH: &str = "assets/mining-off-white.png";
const MINING_PREP_GIF_PATH: &str = "assets/mining-prep.gif";
const MINING_PREP_GIF_WHITE_PATH: &str = "assets/mining-prep-white.gif";
const MINING_ON_GIF_PATH: &str = "assets/mining-on.gif";
const MINING_ON_GIF_WHITE_PATH: &str = "assets/mining-on-white.gif";
const MINING_ON_SOUND_PATH: &str = "assets/mining-on.mp3";
const ONLINE_ICON_PATH: &str = "assets/online.png";
const OFFLINE_ICON_PATH: &str = "assets/offline.png";
const MINING_ON_SOUND_LOOP_SECS: u64 = 4;
const FONT_PATH: &str = "assets/fonts/Ubuntu-BoldItalic.ttf";
const PREFS_FILE: &str = "data/qub-core-gui-settings.json";
const HF105_POST_UPDATE_RESTART_MARKER: &str = "data/updater/hf99-post-update-restart.flag";
const BLOCK_HISTORY_LIMIT: usize = 32;
const LOCAL_ACTIVE_DOT_SECS: u64 = 14;
const LOCAL_BLOCK_ROLLBACK_WATCH_SECS: u64 = 20 * 60;
const LEGACY_DEFAULT_UPDATE_URL: &str = "https://download.qubit-coin.io/QUB-Core-Latest.exe";
const LEGACY_MAINNET_UPDATE_URL: &str = "https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe";
const LEGACY_TESTNET_UPDATE_URL: &str = "https://download.qubit-coin.io/testnet/QUB-Core-Latest.exe";
const DEFAULT_MAINNET_UPDATE_URL: &str = "https://download.qubit-coin.io/mainnet/windows-x64/manifest.json";
const DEFAULT_TESTNET_UPDATE_URL: &str = "https://download.qubit-coin.io/testnet/windows-x64/manifest.json";
const DEFAULT_UPDATE_URL: &str = DEFAULT_MAINNET_UPDATE_URL;
const ALLOW_UNSIGNED_MAINNET_UPDATES_PRIVATE_BUILD: bool = true;
const UPDATE_CHECK_INTERVAL_SECS: u64 = 60;
const UPDATE_INSTALL_COUNTDOWN_SECS: u64 = 15;
const UPDATE_DOWNLOAD_PATH: &str = "data/updater/QUB-Core-Latest.exe";
const DEFAULT_WALLET_SYNC_INTERVAL_SECS: u64 = 15;
const ENJIN_MATRIX_METRICS_REFRESH_SECS: u64 = 300;
const ENJIN_MATRIX_JIN_INITIAL_MAX_SUPPLY: u128 = 105_000_000;
const ENJIN_MATRIX_RPC_URLS: &[&str] = &[
    "https://rpc.matrix.blockchain.enjin.io",
    "https://matrix-rpc.enjin.io",
];
const ENJIN_MATRIX_JIN_TOKEN_STORAGE_KEY: &str = "0xfa7484c926e764ee2a64df96876c814599971b5749ac43e0235e41b0d378691884fc0f7cf200fdee9785e65b2cef05e3471100000000000000000000000000003ba80a3778f04ebf45e806d19a05202501000000000000000000000000000000";
const JIN_TOKEN_SUBSCAN_URL: &str = "https://matrix.subscan.io/multitoken_item/4423-1";
const JIN_TOKEN_NFT_IO_URL: &str = "https://nft.io/asset/4423-1";
const USDJ_ETH_BACKING_NOTE: &str = "USDJ is designed as Jinex USD on QUB Chain, backed by USDT + USDC smart contracts on Ethereum through the future bridge.";
const USDJ_BRIDGE_DISABLED_NOTE: &str = "Bridge contracts are not live yet. QUB-chain USDJ/EURJ mint/burn is shown as roadmap UI only.";
const EURJ_ETH_BACKING_NOTE: &str = "EURJ is designed as Jinex EUR, backed by EURC + EURS smart contracts on Ethereum through the same pooled-reserve model as USDJ.";
const XAUJ_ETH_BACKING_NOTE: &str = "XAUJ is designed as Jinex Gold, backed by PAXG + XAUt smart contracts on Ethereum through the same pooled-reserve model as USDJ/EURJ.";
const ETHEREUM_WALLETS_FILE: &str = "data/ethereum-wallets.json";
const ETHEREUM_CHAIN_ID: u64 = 1;
const ETHEREUM_USDC_ADDRESS: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
const ETHEREUM_USDT_ADDRESS: &str = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
const ETHEREUM_EURC_ADDRESS: &str = "0x1aBaEA1f7C830bD89Acc67eC4Af516284b1bC33c";
const ETHEREUM_EURS_ADDRESS: &str = "0xdB25f211AB05b1c97D595516F45794528a807ad8";
const ETHEREUM_PAXG_ADDRESS: &str = "0x45804880De22913dAFE09f4980848ECE6EcbAf78";
const ETHEREUM_XAUT_ADDRESS: &str = "0x68749665FF8D2d112Fa859AA293F07A622782F38";
const ETHEREUM_USDJ_ADDRESS_DEFAULT: &str = "0x458E9D99a1B79EB23819023BBEd39c59098FFE66"; // Official USDJ mainnet token.
const ETHEREUM_USDJ_VAULT_ADDRESS_DEFAULT: &str = "0xf9d43BF71d7bc86baeB0fD3bd09e0151e19460CF"; // Official USDJ ReserveVault.
const ETHEREUM_EURJ_ADDRESS_DEFAULT: &str = "0x8AF433799acc1452eF5582d9423f8162306Fa091"; // Official EURJ mainnet token.
const ETHEREUM_EURJ_VAULT_ADDRESS_DEFAULT: &str = "0x81ca538FF5BfB24b1B228C9039b1EC56643fE5B2"; // Official EURJ ReserveVault.
const ETHEREUM_XAUJ_ADDRESS_DEFAULT: &str = "0xe4F7fd0cC31F215d8Ac1ccb75AA1b166EA49aC6b"; // Official XAUJ mainnet token.
const ETHEREUM_XAUJ_VAULT_ADDRESS_DEFAULT: &str = "0x0761dd9969007dC7A1A9b4F79c9FA3d0FaA84Beb"; // Official XAUJ ReserveVault.
const ETHEREUM_USDJ_BRIDGE_ADDRESS_DEFAULT: &str = ""; // Filled after official USDJ bridge gateway deployment.
const USDJ_BRIDGE_TOLL_BPS: u32 = 100; // 1% QUB-side protocol toll.
const QUB_USDJ_BRIDGE_PROTOCOL_ADDRESS: &str = "qub1a229a209ca3fc2b3066f6f31d4b27c9d663c46959346d1";
const ETHEREUM_DEFAULT_RPC_URLS: &[&str] = &[
    "https://ethereum-rpc.publicnode.com",
    "https://eth.llamarpc.com",
    "https://cloudflare-eth.com",
];
const ETHEREUM_ERC20_TRANSFER_SELECTOR: &str = "a9059cbb";
const ETHEREUM_ERC20_BALANCE_OF_SELECTOR: &str = "70a08231";
const FIATJ_DECIMALS: u32 = 6;
const GOLDJ_DECIMALS: u32 = 18;
const FIATJ_INFUSE_GAS_LIMIT: u64 = 220_000;
const FIATJ_MELT_GAS_LIMIT: u64 = 190_000;
const FIATJ_APPROVE_GAS_LIMIT: u64 = 70_000;



static HF105_CATCHUP_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static HF105_CATCHUP_EPOCH: AtomicU64 = AtomicU64::new(0);
static HF105_CATCHUP_STARTED_MS: AtomicU64 = AtomicU64::new(0);
static HF105_SNAPSHOT_WORKER_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static HF105_SNAPSHOT_WORKER_STARTED_MS: AtomicU64 = AtomicU64::new(0);

fn hf85_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn hf85_catchup_epoch() -> u64 {
    HF105_CATCHUP_EPOCH.load(Ordering::SeqCst)
}

fn hf85_catchup_running() -> bool {
    HF105_CATCHUP_IN_FLIGHT.load(Ordering::SeqCst)
}

fn hf85_catchup_elapsed_secs() -> u64 {
    let started = HF105_CATCHUP_STARTED_MS.load(Ordering::SeqCst);
    if started == 0 { return 0; }
    hf85_now_ms().saturating_sub(started) / 1000
}

fn spawn_hf85_catchup_pulse(config_path: String, force: bool) {
    // HF106/v1.6.9: chain catch-up is a detached writer, never a GUI reader gate.
    // It is allowed to run longer than a UI snapshot, but the wallet/blocks view
    // must remain visible and refresh through quick local snapshots.
    let now_ms = hf85_now_ms();
    if HF105_CATCHUP_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        let started = HF105_CATCHUP_STARTED_MS.load(Ordering::SeqCst);
        if now_ms.saturating_sub(started) < 260_000 {
            return;
        }
        // HF106: release a stale detached pulse gate. Any old worker that later
        // finishes still writes only consensus-validated chain state; fresh pulses
        // must not be suppressed forever.
        HF105_CATCHUP_IN_FLIGHT.store(false, Ordering::SeqCst);
        if HF105_CATCHUP_IN_FLIGHT
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
    }
    HF105_CATCHUP_STARTED_MS.store(now_ms, Ordering::SeqCst);
    thread::spawn(move || {
        let _ = std::panic::catch_unwind(|| {
            let _ = (|| -> Result<()> {
                let settings = load_gui_settings(&config_path)?;
                if settings.p2p.enabled && matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
                    if force {
                        // HF111/v1.7.1: force/manual/auto-heal repairs use a
                        // deeper official-only chain repair path. This replaces
                        // the user workaround of deleting local data while keeping
                        // wallet.json safe.
                        let _ = p2p::hf110_deep_official_repair(&settings, 420_000);
                    } else {
                        let _ = p2p::hf90_auto_catchup(&settings, 240_000);
                    }
                }
                Ok(())
            })();
        });
        HF105_CATCHUP_EPOCH.fetch_add(1, Ordering::SeqCst);
        HF105_CATCHUP_IN_FLIGHT.store(false, Ordering::SeqCst);
    });
}


fn hf88_try_begin_snapshot_worker() -> bool {
    let now_ms = hf85_now_ms();
    if HF105_SNAPSHOT_WORKER_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        HF105_SNAPSHOT_WORKER_STARTED_MS.store(now_ms, Ordering::SeqCst);
        return true;
    }

    // HF106: an abandoned UI snapshot worker may still be waiting on disk/JSON.
    // Do not queue another reader behind it; that was the HF84-HF87 loop. Only
    // allow a replacement after a hard timeout so the UI can recover from a
    // genuinely wedged worker while still avoiding reader stampedes.
    let started = HF105_SNAPSHOT_WORKER_STARTED_MS.load(Ordering::SeqCst);
    if now_ms.saturating_sub(started) >= 180_000 {
        HF105_SNAPSHOT_WORKER_IN_FLIGHT.store(false, Ordering::SeqCst);
        if HF105_SNAPSHOT_WORKER_IN_FLIGHT
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            HF105_SNAPSHOT_WORKER_STARTED_MS.store(now_ms, Ordering::SeqCst);
            return true;
        }
    }
    false
}

fn hf88_finish_snapshot_worker() {
    HF105_SNAPSHOT_WORKER_IN_FLIGHT.store(false, Ordering::SeqCst);
}

fn hf88_snapshot_worker_running() -> bool {
    HF105_SNAPSHOT_WORKER_IN_FLIGHT.load(Ordering::SeqCst)
}

const UI_ICON_NAMES: &[&str] = &[
    "mining-controls", "start-mining", "stop-mining", "cpu", "gpu",
    "wallet-address", "send", "success", "failed", "qub", "jin", "register-qub",
    "danger-zone", "delete-local-private-keys", "benchmark", "settings",
    "dark", "light", "sync", "live-chain-data", "network", "height", "best-block",
    "mempool-tx", "spendable", "immature", "total-wallet", "wallet-keys",
    "coinbase-maturity", "block-target", "next-reward", "halving-interval",
    "pow-bits", "p2p-peers", "qns", "data-dir", "miner-telemetry", "hashrate",
    "total-hashes", "cpu-workers", "duty", "target-block", "peers-block-stream",
    "web-map", "list", "peers-other-miners", "recent-global-blocks",
    "your-pending-mined-block", "your-confirmed-mined-block", "updates-available", "check-now", "install-and-restart",
    "online", "offline", "verified", "qr", "qr-scan", "i",
    "address-balances", "crypto", "stablecoins", "import-private-key", "buy", "sell", "offer",
    "melt", "infuse", "convert", "jin-token", "usd", "eur", "gold", "usdj", "usdt", "usdc", "eurj", "eurc", "eurs", "xauj", "paxg", "xaut", "eth", "enj", "subscan-logo", "nft-io-logo", "auction", "list-asset",
    "pools", "create-pool", "join-pool", "pool-capacity", "pool-commission",
    "to-right", "to-left", "full-to-right", "full-to-left",
];


fn build_channel() -> &'static str {
    if BUILD_CONFIG.eq_ignore_ascii_case("testnet") { "testnet" } else { "mainnet" }
}

fn app_title() -> &'static str {
    if build_channel() == "testnet" { APP_TITLE_TESTNET } else { APP_TITLE }
}

fn default_config_path() -> String {
    cli_config_path().unwrap_or_else(|| {
        if build_channel() == "testnet" { "config/testnet.toml".to_string() } else { "config/mainnet.toml".to_string() }
    })
}

fn default_update_url() -> &'static str {
    if build_channel() == "testnet" { DEFAULT_TESTNET_UPDATE_URL } else { DEFAULT_MAINNET_UPDATE_URL }
}

fn default_setup_profile() -> SetupProfile {
    if build_channel() == "testnet" { SetupProfile::Testnet } else { SetupProfile::Mainnet }
}

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("{} {}", app_title(), APP_VERSION))
        .with_inner_size([1220.0, 820.0])
        .with_min_inner_size([980.0, 680.0]);
    if let Some(icon) = load_window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        app_title(),
        options,
        Box::new(|cc| Ok(Box::new(QubCoreApp::new(cc)))),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum ThemeChoice {
    System,
    Dark,
    Light,
}

impl Default for ThemeChoice {
    fn default() -> Self { Self::Dark }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum UiLanguage {
    EnUs,
    ElGr,
    FrFr,
    DeDe,
    EsEs,
    ZhCn,
    JaJp,
    KoKr,
}

impl UiLanguage {
    const ALL: [UiLanguage; 8] = [Self::EnUs, Self::ElGr, Self::FrFr, Self::DeDe, Self::EsEs, Self::ZhCn, Self::JaJp, Self::KoKr];
    fn code(&self) -> &'static str {
        match self {
            Self::EnUs => "en-US",
            Self::ElGr => "el-GR",
            Self::FrFr => "fr-FR",
            Self::DeDe => "de-DE",
            Self::EsEs => "es-ES",
            Self::ZhCn => "zh-CN",
            Self::JaJp => "ja-JP",
            Self::KoKr => "ko-KR",
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Self::EnUs => "English",
            Self::ElGr => "Greek",
            Self::FrFr => "French",
            Self::DeDe => "German",
            Self::EsEs => "Spanish",
            Self::ZhCn => "Chinese",
            Self::JaJp => "Japanese",
            Self::KoKr => "Korean",
        }
    }
    fn flag_icon(&self) -> &'static str {
        match self {
            Self::EnUs => "flag-en-US",
            Self::ElGr => "flag-el-GR",
            Self::FrFr => "flag-fr-FR",
            Self::DeDe => "flag-de-DE",
            Self::EsEs => "flag-es-ES",
            Self::ZhCn => "flag-zh-CN",
            Self::JaJp => "flag-ja-JP",
            Self::KoKr => "flag-ko-KR",
        }
    }
    fn from_system_hint(raw: &str) -> Self {
        let raw = raw.to_ascii_lowercase().replace('_', "-");
        if raw.starts_with("el") { Self::ElGr }
        else if raw.starts_with("fr") { Self::FrFr }
        else if raw.starts_with("de") { Self::DeDe }
        else if raw.starts_with("es") { Self::EsEs }
        else if raw.starts_with("zh") || raw.contains("chinese") { Self::ZhCn }
        else if raw.starts_with("ja") || raw.contains("japanese") { Self::JaJp }
        else if raw.starts_with("ko") || raw.contains("korean") { Self::KoKr }
        else { Self::EnUs }
    }
}

impl Default for UiLanguage {
    fn default() -> Self { default_ui_language() }
}

fn default_ui_language() -> UiLanguage {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("powershell.exe");
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "[System.Globalization.CultureInfo]::CurrentUICulture.Name"]);
        if let Ok(out) = cmd.output() {
            if out.status.success() {
                let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !raw.is_empty() { return UiLanguage::from_system_hint(&raw); }
            }
        }
    }
    let raw = std::env::var("LANG")
        .or_else(|_| std::env::var("LANGUAGE"))
        .or_else(|_| std::env::var("LC_ALL"))
        .unwrap_or_default();
    UiLanguage::from_system_hint(&raw)
}


#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum PeerViewMode {
    Web,
    List,
}

impl Default for PeerViewMode {
    fn default() -> Self { Self::Web }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum AutoMiningMode {
    Solo,
    Pool,
}

impl Default for AutoMiningMode {
    fn default() -> Self { Self::Solo }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MiningPhase {
    Off,
    Preparing,
    Mining,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum SetupProfile {
    RegtestLan,
    Testnet,
    Mainnet,
}

impl Default for SetupProfile {
    fn default() -> Self { Self::Mainnet }
}

impl SetupProfile {
    fn label(&self) -> &'static str {
        match self {
            Self::RegtestLan => "Regtest-LAN rehearsal",
            Self::Testnet => "Public testnet",
            Self::Mainnet => "Mainnet production",
        }
    }
    fn template_config(&self) -> &'static str {
        match self {
            Self::RegtestLan => "config/regtest-lan.toml",
            Self::Testnet => "config/testnet.toml",
            Self::Mainnet => "config/mainnet.toml",
        }
    }
    fn generated_config(&self) -> &'static str {
        match self {
            Self::RegtestLan => "config/qub-core-regtest-lan.toml",
            Self::Testnet => "config/qub-core-testnet.toml",
            Self::Mainnet => "config/qub-core-mainnet.toml",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PeerUiStatus {
    addr: String,
    reachable: bool,
    global_live: bool,
    height: Option<u32>,
    tip_hash: String,
    user_agent: String,
    error: String,
    node_id: String,
    observed_addr: String,
    listen_addr: String,
    role: String,
    miner_address: String,
    last_seen_unix: Option<u64>,
    seen_age_secs: Option<u64>,
    qns_names: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct SnapshotBlock {
    height: u32,
    hash: String,
    txs: usize,
    reward: String,
    time: u32,
    first_seen_unix: u32,
    canonical: bool,
    local: bool,
    miner_address: String,
    miner_qns: Vec<String>,
    pool_block: bool,
    pool_id: String,
    pool_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum ActivityTypeFilter { All, Transfer, Mining, QnsRegistration, Library, Infusion, Melt, Conversion }
impl Default for ActivityTypeFilter { fn default() -> Self { Self::All } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum ActivityStatusFilter { All, Mempool, Confirmed, Immature, PendingDecision, Matured }
impl Default for ActivityStatusFilter { fn default() -> Self { Self::All } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum ActivityDirectionFilter { All, Incoming, Outgoing }
impl Default for ActivityDirectionFilter { fn default() -> Self { Self::All } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum ActivityAssetFilter { All, Qub, Jin }
impl Default for ActivityAssetFilter { fn default() -> Self { Self::All } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum BalanceTab { Crypto, Qns }
impl Default for BalanceTab { fn default() -> Self { Self::Crypto } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum JinBalanceSubTab { Coin, Token }
impl Default for JinBalanceSubTab { fn default() -> Self { Self::Coin } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum UsdjBalanceSubTab { Usdj, Usdt, Usdc }
impl Default for UsdjBalanceSubTab { fn default() -> Self { Self::Usdj } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum EurjBalanceSubTab { Eurj, Eurc, Eurs }
impl Default for EurjBalanceSubTab { fn default() -> Self { Self::Eurj } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum XaujBalanceSubTab { Xauj, Paxg, Xaut }
impl Default for XaujBalanceSubTab { fn default() -> Self { Self::Xauj } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum StablecoinFamily { Usd, Eur, Gold }
impl StablecoinFamily {
    fn icon(&self) -> &'static str { match self { Self::Usd => "usd", Self::Eur => "eur", Self::Gold => "gold" } }
    fn label(&self) -> &'static str { match self { Self::Usd => "USD", Self::Eur => "EUR", Self::Gold => "Gold" } }
    fn token_symbol(&self) -> &'static str { match self { Self::Usd => "USDJ", Self::Eur => "EURJ", Self::Gold => "XAUJ" } }
    fn token_icon(&self) -> &'static str { match self { Self::Usd => "usdj", Self::Eur => "eurj", Self::Gold => "xauj" } }
    fn token_decimals(&self) -> u32 { match self { Self::Gold => GOLDJ_DECIMALS, Self::Usd | Self::Eur => FIATJ_DECIMALS } }
}
fn default_stablecoin_family() -> StablecoinFamily {
    match default_ui_language() {
        UiLanguage::ElGr | UiLanguage::FrFr | UiLanguage::DeDe | UiLanguage::EsEs => StablecoinFamily::Eur,
        _ => StablecoinFamily::Usd,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum EthereumAsset { Eth, Usdt, Usdc, Usdj, Eurc, Eurs, Eurj, Paxg, Xaut, Xauj }
impl EthereumAsset {
    fn symbol(&self) -> &'static str { match self { Self::Eth => "ETH", Self::Usdt => "USDT", Self::Usdc => "USDC", Self::Usdj => "USDJ", Self::Eurc => "EURC", Self::Eurs => "EURS", Self::Eurj => "EURJ", Self::Paxg => "PAXG", Self::Xaut => "XAUt", Self::Xauj => "XAUJ" } }
    fn icon(&self) -> &'static str { match self { Self::Eth => "eth", Self::Usdt => "usdt", Self::Usdc => "usdc", Self::Usdj => "usdj", Self::Eurc => "eurc", Self::Eurs => "eurs", Self::Eurj => "eurj", Self::Paxg => "paxg", Self::Xaut => "xaut", Self::Xauj => "xauj" } }
    fn decimals(&self) -> u32 { match self { Self::Eth => 18, Self::Eurs => 2, Self::Paxg | Self::Xauj => 18, Self::Usdt | Self::Usdc | Self::Usdj | Self::Eurc | Self::Eurj | Self::Xaut => 6 } }
    fn gas_limit(&self) -> u64 { match self { Self::Eth => 21_000, _ => 75_000 } }
    fn family(&self) -> Option<StablecoinFamily> { match self { Self::Usdt | Self::Usdc | Self::Usdj => Some(StablecoinFamily::Usd), Self::Eurc | Self::Eurs | Self::Eurj => Some(StablecoinFamily::Eur), Self::Paxg | Self::Xaut | Self::Xauj => Some(StablecoinFamily::Gold), Self::Eth => None } }
    fn reserve_asset_id(&self) -> u8 { match self { Self::Usdc | Self::Eurs | Self::Xaut => 1, _ => 0 } }
}
impl Default for EthereumAsset { fn default() -> Self { Self::Eth } }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EthereumSendMode { Single, Multi }

#[derive(Debug, Clone)]
struct EthereumSendRow { recipient: String, amount: String }
impl Default for EthereumSendRow { fn default() -> Self { Self { recipient: String::new(), amount: String::new() } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EthereumWalletEntry {
    address: String,
    private_key_hex: String,
    label: String,
    created_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct EthereumWalletBook {
    wallets: Vec<EthereumWalletEntry>,
    selected_index: usize,
    rpc_url: String,
    chain_id: u64,
}
impl Default for EthereumWalletBook {
    fn default() -> Self { Self { wallets: Vec::new(), selected_index: 0, rpc_url: ETHEREUM_DEFAULT_RPC_URLS[0].to_string(), chain_id: ETHEREUM_CHAIN_ID } }
}

#[derive(Debug, Clone)]
struct EthereumBalanceState {
    eth: String,
    usdt: String,
    usdc: String,
    usdj_eth: String,
    usdt_reserve: String,
    usdc_reserve: String,
    eurc: String,
    eurs: String,
    eurj_eth: String,
    eurc_reserve: String,
    eurs_reserve: String,
    paxg: String,
    xaut: String,
    xauj_eth: String,
    paxg_reserve: String,
    xaut_reserve: String,
    reserve_status: String,
    eur_reserve_status: String,
    gold_reserve_status: String,
    status: String,
    updated_at: Option<Instant>,
}
impl Default for EthereumBalanceState {
    fn default() -> Self {
        Self {
            eth: "-".to_string(),
            usdt: "-".to_string(),
            usdc: "-".to_string(),
            usdj_eth: "-".to_string(),
            usdt_reserve: "-".to_string(),
            usdc_reserve: "-".to_string(),
            eurc: "-".to_string(),
            eurs: "-".to_string(),
            eurj_eth: "-".to_string(),
            eurc_reserve: "-".to_string(),
            eurs_reserve: "-".to_string(),
            paxg: "-".to_string(),
            xaut: "-".to_string(),
            xauj_eth: "-".to_string(),
            paxg_reserve: "-".to_string(),
            xaut_reserve: "-".to_string(),
            reserve_status: "USDJ contracts not configured".to_string(),
            eur_reserve_status: "EURJ contracts not configured".to_string(),
            gold_reserve_status: "XAUJ contracts not configured".to_string(),
            status: "Not fetched yet".to_string(),
            updated_at: None,
        }
    }
}

#[derive(Debug, Clone)]
struct EthereumWalletDialog {
    import_private_key: String,
    import_label: String,
    message: String,
}
impl Default for EthereumWalletDialog {
    fn default() -> Self { Self { import_private_key: String::new(), import_label: "Ethereum wallet".to_string(), message: String::new() } }
}

#[derive(Debug, Clone)]
struct EthereumSendDialog {
    open: bool,
    asset: EthereumAsset,
    mode: EthereumSendMode,
    recipient: String,
    amount: String,
    multi_entries: String,
    multi_rows: Vec<EthereumSendRow>,
    gas_price_gwei: String,
    status: SendDialogStatus,
    txid: String,
    message: String,
}
impl Default for EthereumSendDialog {
    fn default() -> Self {
        Self { open: false, asset: EthereumAsset::Eth, mode: EthereumSendMode::Single, recipient: String::new(), amount: "0".to_string(), multi_entries: String::new(), multi_rows: vec![EthereumSendRow::default()], gas_price_gwei: String::new(), status: SendDialogStatus::Editing, txid: String::new(), message: String::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsdjVaultMode { Infuse, Melt }

#[derive(Debug, Clone)]
struct UsdjVaultDialog {
    open: bool,
    mode: UsdjVaultMode,
    asset: EthereumAsset,
    amount: String,
    receiver: String,
    gas_price_gwei: String,
    status: SendDialogStatus,
    txid: String,
    message: String,
}
impl Default for UsdjVaultDialog {
    fn default() -> Self {
        Self {
            open: false,
            mode: UsdjVaultMode::Infuse,
            asset: EthereumAsset::Usdt,
            amount: "0".to_string(),
            receiver: String::new(),
            gas_price_gwei: String::new(),
            status: SendDialogStatus::Editing,
            txid: String::new(),
            message: String::new(),
        }
    }
}


#[derive(Debug)]
enum EthereumWalletEvent {
    Balance { eth: String, usdt: String, usdc: String, usdj_eth: String, usdt_reserve: String, usdc_reserve: String, eurc: String, eurs: String, eurj_eth: String, eurc_reserve: String, eurs_reserve: String, paxg: String, xaut: String, xauj_eth: String, paxg_reserve: String, xaut_reserve: String, reserve_status: String, eur_reserve_status: String, gold_reserve_status: String, status: String },
    SendCreated { txids: Vec<String>, message: String },
    UsdjActionCreated { txids: Vec<String>, message: String },
    Failed(String),
}

#[derive(Debug, Clone, Default)]
struct AddressActivityEntry {
    txid: String,
    activity_type: String,
    status: String,
    direction: String,
    amount: String,
    fee: String,
    height: Option<u32>,
    confirmations: u32,
    time: u32,
    counterparty: String,
    details: String,
    qns_name: String,
}

impl AddressActivityEntry {
    fn matches_type(&self, filter: ActivityTypeFilter) -> bool {
        match filter {
            ActivityTypeFilter::All => true,
            ActivityTypeFilter::Transfer => self.activity_type == "Transfer",
            ActivityTypeFilter::Mining => self.activity_type == "Mining",
            ActivityTypeFilter::QnsRegistration => self.activity_type == "QNS Registration",
            ActivityTypeFilter::Library => self.activity_type == "Library",
            ActivityTypeFilter::Infusion => self.activity_type == "Infusion",
            ActivityTypeFilter::Melt => self.activity_type == "Melt",
            ActivityTypeFilter::Conversion => self.activity_type == "Conversion",
        }
    }
    fn matches_status(&self, filter: ActivityStatusFilter) -> bool {
        match filter {
            ActivityStatusFilter::All => true,
            ActivityStatusFilter::Mempool => self.status == "Mempool",
            ActivityStatusFilter::Confirmed => self.status == "Confirmed",
            ActivityStatusFilter::Immature => self.status == "Immature",
            ActivityStatusFilter::PendingDecision => self.status == "Pending Decision",
            ActivityStatusFilter::Matured => self.status == "Matured",
        }
    }
    fn matches_direction(&self, filter: ActivityDirectionFilter) -> bool {
        match filter {
            ActivityDirectionFilter::All => true,
            ActivityDirectionFilter::Incoming => self.direction == "Incoming",
            ActivityDirectionFilter::Outgoing => self.direction == "Outgoing",
        }
    }
    fn matches_asset(&self, filter: ActivityAssetFilter) -> bool {
        let amount = self.amount.to_ascii_uppercase();
        let fee = self.fee.to_ascii_uppercase();
        match filter {
            ActivityAssetFilter::All => true,
            ActivityAssetFilter::Qub => amount.contains("QUB") || fee.contains("QUB"),
            ActivityAssetFilter::Jin => amount.contains("JIN") || fee.contains("JIN"),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct GuiPrefs {
    config_path: String,
    payout_address: String,
    cpu_percent: u8,
    gpu_percent: u8,
    gpu_device_selector: String,
    theme: ThemeChoice,
    language: UiLanguage,
    auto_start_windows: bool,
    sound_enabled: bool,
    network_sound_enabled: bool,
    mining_loop_sound_enabled: bool,
    visual_enabled: bool,
    auto_sync_wallet_balances: bool,
    auto_sync_wallet_interval_secs: u64,
    start_mining_on_open: bool,
    start_mining_on_open_mode: AutoMiningMode,
    start_mining_after_update_restart: bool,
    start_mining_after_update_restart_mode: AutoMiningMode,
    hf105_auto_restart_default_applied: bool,
    mining_peak_hash_rate_hps: f64,
    mining_peak_cpu_hash_rate_hps: f64,
    mining_peak_gpu_hash_rate_hps: f64,
    confirm_plaintext_wallet_risk: bool,
    benchmark_seconds: u64,
    pace_to_target_spacing: bool,
    peer_view_mode: PeerViewMode,
    peer_zoom: f32,
    left_mining_controls_expanded: bool,
    central_live_expanded: bool,
    central_miner_expanded: bool,
    central_peers_expanded: bool,
    central_recent_blocks_expanded: bool,
    activity_type_filter: ActivityTypeFilter,
    activity_status_filter: ActivityStatusFilter,
    activity_direction_filter: ActivityDirectionFilter,
    activity_asset_filter: ActivityAssetFilter,
    activity_page: usize,
    balance_tab: BalanceTab,
    jin_balance_tab: JinBalanceSubTab,
    usdj_balance_tab: UsdjBalanceSubTab,
    eurj_balance_tab: EurjBalanceSubTab,
    xauj_balance_tab: XaujBalanceSubTab,
    stablecoin_family: StablecoinFamily,
    last_pool_id: String,
    setup_complete: bool,
    setup_profile: SetupProfile,
    setup_bootnodes: String,
    setup_advertise_addr: String,
    setup_listen_for_peers: bool,
    setup_seed_node_mode: bool,
    allow_isolated_regtest_mining: bool,
    auto_check_updates: bool,
    auto_download_updates: bool,
    auto_install_updates: bool,
    stop_miner_on_update_available: bool,
    update_url: String,
    eth_usdt_contract_override: String,
    eth_usdc_contract_override: String,
    eth_usdj_token_contract: String,
    eth_usdj_vault_contract: String,
    eth_eurc_contract_override: String,
    eth_eurs_contract_override: String,
    eth_eurj_token_contract: String,
    eth_eurj_vault_contract: String,
    eth_paxg_contract_override: String,
    eth_xaut_contract_override: String,
    eth_xauj_token_contract: String,
    eth_xauj_vault_contract: String,
}


impl Default for GuiPrefs {
    fn default() -> Self {
        Self {
            config_path: default_config_path(),
            payout_address: String::new(),
            cpu_percent: 50,
            gpu_percent: 0,
            gpu_device_selector: gpu_miner::GPU_DEVICE_ALL.to_string(),
            theme: ThemeChoice::Dark,
            language: default_ui_language(),
            auto_start_windows: false,
            sound_enabled: true,
            network_sound_enabled: true,
            mining_loop_sound_enabled: true,
            visual_enabled: true,
            auto_sync_wallet_balances: false,
            auto_sync_wallet_interval_secs: DEFAULT_WALLET_SYNC_INTERVAL_SECS,
            start_mining_on_open: false,
            start_mining_on_open_mode: AutoMiningMode::Solo,
            start_mining_after_update_restart: true,
            start_mining_after_update_restart_mode: AutoMiningMode::Solo,
            hf105_auto_restart_default_applied: false,
            mining_peak_hash_rate_hps: 0.0,
            mining_peak_cpu_hash_rate_hps: 0.0,
            mining_peak_gpu_hash_rate_hps: 0.0,
            confirm_plaintext_wallet_risk: false,
            benchmark_seconds: 3,
            pace_to_target_spacing: false,
            peer_view_mode: PeerViewMode::Web,
            peer_zoom: 1.0,
            left_mining_controls_expanded: true,
            central_live_expanded: false,
            central_miner_expanded: false,
            central_peers_expanded: false,
            central_recent_blocks_expanded: true,
            activity_type_filter: ActivityTypeFilter::All,
            activity_status_filter: ActivityStatusFilter::All,
            activity_direction_filter: ActivityDirectionFilter::All,
            activity_asset_filter: ActivityAssetFilter::All,
            activity_page: 0,
            balance_tab: BalanceTab::Crypto,
            jin_balance_tab: JinBalanceSubTab::Coin,
            usdj_balance_tab: UsdjBalanceSubTab::Usdj,
            eurj_balance_tab: EurjBalanceSubTab::Eurj,
            xauj_balance_tab: XaujBalanceSubTab::Xauj,
            stablecoin_family: default_stablecoin_family(),
            last_pool_id: String::new(),
            setup_complete: false,
            setup_profile: default_setup_profile(),
            setup_bootnodes: String::new(),
            setup_advertise_addr: String::new(),
            setup_listen_for_peers: true,
            setup_seed_node_mode: false,
            allow_isolated_regtest_mining: false,
            auto_check_updates: true,
            auto_download_updates: true,
            auto_install_updates: true,
            stop_miner_on_update_available: true,
            update_url: default_update_url().to_string(),
            eth_usdt_contract_override: String::new(),
            eth_usdc_contract_override: String::new(),
            eth_usdj_token_contract: String::new(),
            eth_usdj_vault_contract: String::new(),
            eth_eurc_contract_override: String::new(),
            eth_eurs_contract_override: String::new(),
            eth_eurj_token_contract: String::new(),
            eth_eurj_vault_contract: String::new(),
            eth_paxg_contract_override: String::new(),
            eth_xaut_contract_override: String::new(),
            eth_xauj_token_contract: String::new(),
            eth_xauj_vault_contract: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateStatus {
    Idle,
    Checking,
    Ready,
    Installing,
    Failed,
    UpToDate,
}

impl Default for UpdateStatus {
    fn default() -> Self { Self::Idle }
}

#[derive(Debug, Clone, Default)]
struct UpdateDialog {
    open: bool,
    status: UpdateStatus,
    latest_version: String,
    installer_path: String,
    remote_signature: String,
    message: String,
    auto_install_at: Option<Instant>,
}

#[derive(Debug)]
enum UpdateEvent {
    UpToDate { signature: String, message: String },
    Ready { version: String, installer_path: String, signature: String, message: String },
    Failed(String),
}

#[derive(Debug, Clone)]
struct ChainSnapshot {
    network: String,
    height: u32,
    best_hash: String,
    known_network_height: u32,
    known_network_hash: String,
    direct_network_height: u32,
    recent_avg_10_secs: u32,
    recent_avg_20_secs: u32,
    daa_observation: String,
    mempool_txs: usize,
    spendable: String,
    immature: String,
    total: String,
    jin_total: String,
    wallet_keys: usize,
    default_address: String,
    coinbase_maturity: u32,
    target_spacing_secs: u32,
    block_reward: String,
    mined_qub_supply: String,
    halving_interval: u64,
    pow_bits: String,
    data_dir: String,
    qns_count: usize,
    pools_count: usize,
    verified_wallets_count: usize,
    verified_pools_count: usize,
    report_cases_count: usize,
    active_moderators_count: usize,
    verified_governance_activation_height: u32,
    pools_activation_height: u32,
    pools_protocol_address: String,
    pools: Vec<PoolUiSummary>,
    qns_activation_height: u32,
    qns_protocol_name: String,
    qns_protocol_address: String,
    owned_qns: Vec<String>,
    features: String,
    p2p_enabled: bool,
    known_peers: usize,
    reachable_peers: usize,
    direct_reachable_peers: usize,
    global_live_peers: usize,
    relay_capable: bool,
    nat_private: bool,
    stale_warning: String,
    peers: Vec<PeerUiStatus>,
    recent_blocks: Vec<SnapshotBlock>,
    activity: Vec<AddressActivityEntry>,
}

impl Default for ChainSnapshot {
    fn default() -> Self {
        Self {
            network: "not loaded".to_string(),
            height: 0,
            best_hash: "-".to_string(),
            known_network_height: 0,
            known_network_hash: String::new(),
            direct_network_height: 0,
            recent_avg_10_secs: 0,
            recent_avg_20_secs: 0,
            daa_observation: String::new(),
            mempool_txs: 0,
            spendable: "0".to_string(),
            immature: "0".to_string(),
            total: "0".to_string(),
            jin_total: "0".to_string(),
            wallet_keys: 0,
            default_address: String::new(),
            coinbase_maturity: 0,
            target_spacing_secs: 0,
            block_reward: "0".to_string(),
            mined_qub_supply: "0".to_string(),
            halving_interval: 0,
            pow_bits: String::new(),
            data_dir: String::new(),
            qns_count: 0,
            pools_count: 0,
            verified_wallets_count: 0,
            verified_pools_count: 0,
            report_cases_count: 0,
            active_moderators_count: 0,
            verified_governance_activation_height: 0,
            pools_activation_height: 0,
            pools_protocol_address: String::new(),
            pools: Vec::new(),
            qns_activation_height: 0,
            qns_protocol_name: String::new(),
            qns_protocol_address: String::new(),
            owned_qns: Vec::new(),
            features: String::new(),
            p2p_enabled: false,
            known_peers: 0,
            reachable_peers: 0,
            direct_reachable_peers: 0,
            global_live_peers: 0,
            relay_capable: false,
            nat_private: true,
            stale_warning: String::new(),
            peers: Vec::new(),
            recent_blocks: Vec::new(),
            activity: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PoolUiSummary {
    pool_id: String,
    name: String,
    manager_address: String,
    commission_bps: u16,
    capacity_slots: u32,
    active_miners: usize,
    open_slots: i64,
    recent_shares: u128,
    your_shares: u128,
    your_active: bool,
    is_manager: bool,
    created_height: u32,
    total_paid_qub: String,
}

#[derive(Debug, Clone)]
struct PoolDialog {
    open: bool,
    create_open: bool,
    manage_open: bool,
    search: String,
    open_only: bool,
    selected_pool_id: String,
    create_name: String,
    create_commission_bps: String,
    create_capacity_slots: u32,
    create_fee: String,
    create_price_capacity_slots: u32,
    create_price_preview: String,
    manage_pool_id: String,
    rename_name: String,
    new_commission_bps: String,
    extra_capacity_slots: u32,
    manage_fee: String,
    join_pool_id: String,
    miner_address: String,
    status: SendDialogStatus,
    txid: String,
    action: String,
    message: String,
    relayed_to_peers: usize,
    local_mempooltx: usize,
    last_checked_height: u32,
}

impl Default for PoolDialog {
    fn default() -> Self {
        Self {
            open: false,
            create_open: false,
            manage_open: false,
            search: String::new(),
            open_only: false,
            selected_pool_id: String::new(),
            create_name: "My QUB Pool".to_string(),
            create_commission_bps: "500".to_string(),
            create_capacity_slots: 8,
            create_fee: "0.00001".to_string(),
            create_price_capacity_slots: 0,
            create_price_preview: String::new(),
            manage_pool_id: String::new(),
            rename_name: String::new(),
            new_commission_bps: "0".to_string(),
            extra_capacity_slots: 8,
            manage_fee: "0.00001".to_string(),
            join_pool_id: String::new(),
            miner_address: String::new(),
            status: SendDialogStatus::Editing,
            txid: String::new(),
            action: String::new(),
            message: String::new(),
            relayed_to_peers: 0,
            local_mempooltx: 0,
            last_checked_height: 0,
        }
    }
}

#[derive(Debug)]
enum PoolGuiEvent {
    Created { action: String, txid: String, pool_id: String, relayed_to_peers: usize, local_mempooltx: usize, message: String },
    Failed(String),
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LibraryTab { Browse, Read, Create }

#[derive(Debug, Clone)]
struct LibraryDialog {
    open: bool,
    tab: LibraryTab,
    selected_post_id: String,
    create_title: String,
    create_category: String,
    create_body: String,
    edit_target_kind: String,
    edit_target_id: String,
    edit_title: String,
    edit_category: String,
    edit_body: String,
    comment_body: String,
    comment_parent_id: String,
    fee: String,
    message: String,
    pending_txid: String,
    create_price_preview_key: String,
    create_price_preview: String,
}
impl Default for LibraryDialog {
    fn default() -> Self {
        Self {
            open: false,
            tab: LibraryTab::Browse,
            selected_post_id: String::new(),
            create_title: String::new(),
            create_category: "general".to_string(),
            create_body: String::new(),
            edit_target_kind: "post".to_string(),
            edit_target_id: String::new(),
            edit_title: String::new(),
            edit_category: String::new(),
            edit_body: String::new(),
            comment_body: String::new(),
            comment_parent_id: String::new(),
            fee: "0.00001".to_string(),
            message: String::new(),
            pending_txid: String::new(),
            create_price_preview_key: String::new(),
            create_price_preview: String::new(),
        }
    }
}


#[derive(Debug, Clone)]
enum LibraryGuiAction {
    Create,
    Comment { post_id: String, parent: Option<String> },
    Vote { kind: String, id: String, up: bool },
    Edit { kind: String, id: String },
    Delete { kind: String, id: String },
}

#[derive(Debug)]
enum LibraryActionEvent {
    Created { txid: String, relayed_to_peers: usize, local_mempooltx: usize },
    Failed(String),
}

#[derive(Debug, Clone)]
struct EnjinMatrixMetrics {
    melted_jin_supply: String,
    true_max_jin_supply: String,
    total_infused_enj: String,
    per_jin_infusion: String,
    last_status: String,
    updated_at: Option<Instant>,
}

impl Default for EnjinMatrixMetrics {
    fn default() -> Self {
        Self {
            melted_jin_supply: "0 JIN".to_string(),
            true_max_jin_supply: "105,000,000 JIN".to_string(),
            total_infused_enj: "0 ENJ".to_string(),
            per_jin_infusion: "0 ENJ/JIN".to_string(),
            last_status: "Matrixchain RPC not fetched yet".to_string(),
            updated_at: None,
        }
    }
}

#[derive(Debug)]
enum MinerEvent {
    Started { threads: usize, duty: u8, target_height: u32 },
    Hashrate { hps: f64, total_hashes: u64, threads: usize, duty: u8, target_height: u32 },
    GpuStarted { device: String, workers: usize, power: u8, target_height: u32 },
    GpuHashrate { hps: f64, total_hashes: u64, workers: usize, device: String, target_height: u32 },
    GpuStatus(String),
    BlockFound { height: u32, hash: String, txs: usize, reward: String },
    BlockStale { height: u32, hash: String, winner_hash: String },
    Status(String),
    Error(String),
    Stopped,
}

#[derive(Debug)]
enum BenchmarkEvent {
    Done { hps: f64, seconds: f64 },
    Error(String),
}

struct MinerHandle {
    stop: Arc<AtomicBool>,
    rx: mpsc::Receiver<MinerEvent>,
    join: Option<thread::JoinHandle<()>>,
}

impl MinerHandle {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for MinerHandle {
    fn drop(&mut self) {
        // HF53/v1.5.2: never join worker threads from the GUI thread.
        // The stop flag is enough; dropping JoinHandle detaches the worker so
        // Stop mining, auto-update, and window close cannot freeze the UI.
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.join.take();
    }
}

struct AnimatedAsset {
    frames: Vec<egui::TextureHandle>,
    frame_ms: Vec<u64>,
    total_ms: u64,
}

impl AnimatedAsset {
    fn texture_at(&self, elapsed: Duration) -> Option<&egui::TextureHandle> {
        if self.frames.is_empty() { return None; }
        if self.frames.len() == 1 || self.total_ms == 0 { return self.frames.first(); }
        let mut t = (elapsed.as_millis() as u64) % self.total_ms;
        for (idx, ms) in self.frame_ms.iter().enumerate() {
            if t < *ms { return self.frames.get(idx); }
            t = t.saturating_sub(*ms);
        }
        self.frames.last()
    }
}

struct IconSet {
    textures: HashMap<String, egui::TextureHandle>,
}

impl IconSet {
    fn load(ctx: &egui::Context) -> Self {
        let mut textures = HashMap::new();
        for base in UI_ICON_NAMES {
            for candidate in [base.to_string(), format!("{base}-white")] {
                let path = format!("assets/{candidate}.png");
                if let Some(tex) = load_asset_texture(ctx, &path, &format!("ui-icon-{candidate}")) {
                    textures.insert(candidate, tex);
                }
            }
        }
        for (key, path) in [("flag-en-US", "assets/flags/en-US.png"), ("flag-el-GR", "assets/flags/el-GR.png"), ("flag-fr-FR", "assets/flags/fr-FR.png"), ("flag-de-DE", "assets/flags/de-DE.png"), ("flag-es-ES", "assets/flags/es-ES.png"), ("flag-zh-CN", "assets/flags/zh-CN.png"), ("flag-ja-JP", "assets/flags/ja-JP.png"), ("flag-ko-KR", "assets/flags/ko-KR.png")] {
            if let Some(tex) = load_asset_texture(ctx, path, &format!("ui-icon-{key}")) {
                textures.insert(key.to_string(), tex);
            }
        }
        Self { textures }
    }

    fn get(&self, base: &str, dark: bool) -> Option<&egui::TextureHandle> {
        if dark {
            self.textures.get(&format!("{base}-white")).or_else(|| self.textures.get(base))
        } else {
            self.textures.get(base).or_else(|| self.textures.get(&format!("{base}-white")))
        }
    }
}

struct QubCoreApp {
    prefs: GuiPrefs,
    snapshot: ChainSnapshot,
    status_line: String,
    last_error: Option<String>,
    last_success: Option<String>,
    last_block_card: Option<BlockCard>,
    miner: Option<MinerHandle>,
    benchmark_rx: Option<mpsc::Receiver<BenchmarkEvent>>,
    benchmark_running: bool,
    benchmark_result: Option<String>,
    delete_private_key_confirm: bool,
    import_key_dialog: ImportKeyDialog,
    send_dialog: SendDialog,
    send_rx: Option<mpsc::Receiver<SendEvent>>,
    eth_wallets: EthereumWalletBook,
    eth_wallet_dialog: EthereumWalletDialog,
    eth_balances: EthereumBalanceState,
    eth_balance_rx: Option<mpsc::Receiver<EthereumWalletEvent>>,
    eth_balance_in_flight: bool,
    eth_send_dialog: EthereumSendDialog,
    eth_send_rx: Option<mpsc::Receiver<EthereumWalletEvent>>,
    usdj_vault_dialog: UsdjVaultDialog,
    usdj_vault_rx: Option<mpsc::Receiver<EthereumWalletEvent>>,
    conversion_dialog: ConversionDialog,
    conversion_rx: Option<mpsc::Receiver<SendEvent>>,
    buy_jin_dialog: BuyJinDialog,
    buy_jin_rx: Option<mpsc::Receiver<BuyJinEvent>>,
    qns_dialog: QnsDialog,
    qns_rx: Option<mpsc::Receiver<QnsEvent>>,
    pool_dialog: PoolDialog,
    pool_rx: Option<mpsc::Receiver<PoolGuiEvent>>,
    library_dialog: LibraryDialog,
    library_state_cache: Option<LibraryState>,
    library_state_rx: Option<mpsc::Receiver<std::result::Result<LibraryState, String>>>,
    library_state_in_flight: bool,
    library_state_last_started: Instant,
    library_state_last_loaded: Option<Instant>,
    library_state_error: Option<String>,
    library_action_rx: Option<mpsc::Receiver<LibraryActionEvent>>,
    library_action_in_flight: bool,
    pool_mining_pool_id: String,
    right_balances_collapsed: bool,
    right_activity_collapsed: bool,
    last_send_status_poll: Instant,
    snapshot_rx: Option<mpsc::Receiver<std::result::Result<ChainSnapshot, String>>>,
    snapshot_in_flight: bool,
    last_theme_applied: ThemeChoice,
    logo: Option<egui::TextureHandle>,
    jin_logo: Option<egui::TextureHandle>,
    mining_off_icon: Option<egui::TextureHandle>,
    mining_off_icon_white: Option<egui::TextureHandle>,
    mining_prep_anim: Option<AnimatedAsset>,
    mining_prep_anim_white: Option<AnimatedAsset>,
    mining_on_anim: Option<AnimatedAsset>,
    mining_on_anim_white: Option<AnimatedAsset>,
    online_icon: Option<egui::TextureHandle>,
    offline_icon: Option<egui::TextureHandle>,
    icons: IconSet,
    wallet_sync_rx: Option<mpsc::Receiver<std::result::Result<ChainSnapshot, String>>>,
    wallet_sync_in_flight: bool,
    wallet_create_rx: Option<mpsc::Receiver<std::result::Result<String, String>>>,
    wallet_create_in_flight: bool,
    send_qns_resolve_rx: Option<mpsc::Receiver<std::result::Result<(String, String), String>>>,
    send_qns_resolve_in_flight: bool,
    multi_qns_resolve_rx: Option<mpsc::Receiver<(usize, String, std::result::Result<String, String>)>>,
    last_wallet_sync: Instant,
    app_started: Instant,
    last_refresh: Instant,
    hash_rate_hps: f64,
    total_hashes: u64,
    gpu_hash_rate_hps: f64,
    gpu_total_hashes: u64,
    gpu_workers: usize,
    gpu_device: String,
    gpu_device_options: Vec<String>,
    gpu_device_last_scan: Instant,
    gpu_device_rates: HashMap<String, (f64, u64, usize)>,
    miner_threads: usize,
    miner_duty: u8,
    target_height: u32,
    block_history: VecDeque<BlockCard>,
    last_local_mined_at: Option<Instant>,
    mining_phase: MiningPhase,
    mining_phase_started_at: Option<Instant>,
    mining_on_next_sound_at: Option<Instant>,
    last_observed_tip_hash: String,
    last_observed_height: u32,
    sync_progress_last_height: u32,
    sync_progress_last_sample: Instant,
    sync_progress_rate_bps: f32,
    prefs_dirty: bool,
    p2p_node_started: bool,
    p2p_node_rx: Option<mpsc::Receiver<String>>,
    update_dialog: UpdateDialog,
    update_rx: Option<mpsc::Receiver<UpdateEvent>>,
    update_check_in_flight: bool,
    update_check_forced: bool,
    last_update_check: Instant,
    qr_cache_address: String,
    qr_cache_texture: Option<egui::TextureHandle>,
    qr_hover_until: Option<Instant>,
    qr_hover_address: String,
    qr_scan_dialog_open: bool,
    qr_scan_message: String,
    qr_camera_devices: Vec<String>,
    qr_camera_selected: usize,
    tx_status_rx: Option<mpsc::Receiver<std::result::Result<(String, TxUiStatus), String>>>,
    tx_status_in_flight: bool,
    enjin_metrics: EnjinMatrixMetrics,
    enjin_metrics_rx: Option<mpsc::Receiver<std::result::Result<EnjinMatrixMetrics, String>>>,
    enjin_metrics_in_flight: bool,
    last_enjin_metrics_refresh: Instant,
    opening_banner: Option<egui::TextureHandle>,
    initial_loading: bool,
    initial_loading_started_at: Instant,
    pending_start_mining_on_open: bool,
    pending_start_mining_mode: AutoMiningMode,
    hf86_force_quick_snapshot: bool,
    manual_stop_requested: bool,
    desired_pool_mining_pool_id: String,
    auto_restart_mining_at: Option<Instant>,
    hf110_autoheal_paused_mining: bool,
    hf110_last_mining_auto_pause: Instant,
    hf85_last_auto_catchup_sync: Instant,
    hf85_auto_catchup_last_height: u32,
    hf85_auto_catchup_stale_since: Instant,
    hf85_seen_catchup_epoch: u64,
    hf88_last_peer_probe: Instant,
    hf88_last_snapshot_height: u32,
    hf88_snapshot_stale_since: Instant,
    hf88_snapshot_backoff_until: Option<Instant>,
    hf88_snapshot_timeout_count: u32,
    hf88_last_snapshot_success: Instant,
}


#[derive(Debug, Clone)]
struct BlockCard {
    height: u32,
    hash: String,
    txs: usize,
    reward: String,
    at: Instant,
    confirmed: bool,
    confirmations: u32,
}


#[derive(Debug, Clone, Default)]
struct ImportKeyDialog {
    open: bool,
    secret_key_hex: String,
    label: String,
    message: String,
    success: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendDialogStatus {
    Editing,
    Sending,
    Pending,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendMode { Single, Multi, Blast }

#[derive(Debug, Clone, Default)]
struct MultiSendRow {
    recipient: String,
    amount: String,
    resolved_address: String,
    resolve_message: String,
    resolving: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BlastCodeRecord {
    txid: String,
    private_code: String,
    claim_payload: String,
    saved_unix: u64,
}

#[derive(Debug, Clone)]
struct SendDialog {
    open: bool,
    send_mode: SendMode,
    multi_entries: String,
    multi_rows: Vec<MultiSendRow>,
    blast_create_mode: bool,
    blast_total: String,
    blast_per_claim: String,
    blast_max_claims: String,
    blast_private_code: String,
    blast_claim_code: String,
    blast_claimant_address: String,
    blast_last_claim_payload: String,
    blast_show_qr: bool,
    recipient: String,
    resolved_address: String,
    amount: String,
    fee: String,
    asset: String,
    fee_asset: String,
    status: SendDialogStatus,
    txid: String,
    message: String,
    relayed_to_peers: usize,
    last_checked_height: u32,
    last_relay_at: Option<Instant>,
}

#[derive(Debug, Clone)]
struct ConversionDialog {
    open: bool,
    matrix_address: String,
    amount: String,
    fee: String,
    fee_asset: String,
    status: SendDialogStatus,
    txid: String,
    message: String,
    relayed_to_peers: usize,
    last_checked_height: u32,
}

impl Default for ConversionDialog {
    fn default() -> Self {
        Self {
            open: false,
            matrix_address: String::new(),
            amount: "1".to_string(),
            fee: "0.001".to_string(),
            fee_asset: "JIN".to_string(),
            status: SendDialogStatus::Editing,
            txid: String::new(),
            message: String::new(),
            relayed_to_peers: 0,
            last_checked_height: 0,
        }
    }
}


#[derive(Debug, Clone)]
struct JinSaleUiListing {
    listing_id: u32,
    price_qub_per_jin: String,
    total_jin: String,
    sold_jin: String,
    remaining_jin: String,
    remaining_units: u128,
}

#[derive(Debug, Clone)]
struct BuyJinDialog {
    open: bool,
    listings: Vec<JinSaleUiListing>,
    page: usize,
    selected_listing: u32,
    amount_jin: String,
    fee: String,
    status: SendDialogStatus,
    txid: String,
    message: String,
    relayed_to_peers: usize,
    loading: bool,
    preview_key: String,
    preview_lines: Vec<String>,
}

impl Default for BuyJinDialog {
    fn default() -> Self {
        Self {
            open: false,
            listings: Vec::new(),
            page: 0,
            selected_listing: 0,
            amount_jin: "100".to_string(),
            fee: "0.00001".to_string(),
            status: SendDialogStatus::Editing,
            txid: String::new(),
            message: String::new(),
            relayed_to_peers: 0,
            loading: false,
            preview_key: String::new(),
            preview_lines: Vec::new(),
        }
    }
}

#[derive(Debug)]
enum BuyJinEvent {
    Listings(std::result::Result<Vec<JinSaleUiListing>, String>),
    Created { txid: String, relayed_to_peers: usize, local_mempooltx: usize },
    Failed(String),
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QnsDialogStatus { Editing, Sending, Pending, Confirmed, Failed }

#[derive(Debug, Clone)]
struct QnsDialog {
    open: bool,
    name: String,
    target_address: String,
    fee: String,
    price: String,
    status: QnsDialogStatus,
    txid: String,
    message: String,
    relayed_to_peers: usize,
}

impl Default for QnsDialog {
    fn default() -> Self {
        Self { open: false, name: String::new(), target_address: String::new(), fee: "0.00001".to_string(), price: "-".to_string(), status: QnsDialogStatus::Editing, txid: String::new(), message: String::new(), relayed_to_peers: 0 }
    }
}

#[derive(Debug)]
enum QnsEvent { Created { txid: String, relayed_to_peers: usize, local_mempooltx: usize }, Failed(String) }

impl Default for SendDialog {
    fn default() -> Self {
        Self {
            open: false,
            send_mode: SendMode::Single,
            multi_entries: String::new(),
            multi_rows: vec![MultiSendRow::default()],
            blast_create_mode: true,
            blast_total: "100".to_string(),
            blast_per_claim: "1".to_string(),
            blast_max_claims: "100".to_string(),
            blast_private_code: String::new(),
            blast_claim_code: String::new(),
            blast_claimant_address: String::new(),
            blast_last_claim_payload: String::new(),
            blast_show_qr: false,
            recipient: String::new(),
            resolved_address: String::new(),
            amount: "0".to_string(),
            fee: "0.00001".to_string(),
            asset: "QUB".to_string(),
            fee_asset: "QUB".to_string(),
            status: SendDialogStatus::Editing,
            txid: String::new(),
            message: String::new(),
            relayed_to_peers: 0,
            last_checked_height: 0,
            last_relay_at: None,
        }
    }
}

#[derive(Debug)]
enum SendWork {
    Single { recipient: String, amount: String, fee: String, asset: String, fee_asset: String },
    Multi { entries: String, fee: String, asset: String, fee_asset: String },
    BlastCreate { total: String, per_claim: String, max_claims: String, private_code: String, fee: String },
    BlastClaim { claim_code: String, claimant: String },
}

#[derive(Debug)]
enum SendEvent {
    Created { txid: String, relayed_to_peers: usize, local_mempooltx: usize },
    Failed(String),
}

#[derive(Debug, Clone)]
enum TxUiStatus {
    PendingMempool,
    Confirmed { height: u32, confirmations: u32 },
    NotFound,
}

fn tr_fr(en: &'static str) -> &'static str { en }
fn tr_de(en: &'static str) -> &'static str { en }
fn tr_es(en: &'static str) -> &'static str { en }
fn tr_zh(en: &'static str) -> &'static str { en }
fn tr_ja(en: &'static str) -> &'static str { en }
fn tr_ko(en: &'static str) -> &'static str { en }


impl QubCoreApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut prefs = load_gui_prefs().unwrap_or_default();
        prefs.language = UiLanguage::EnUs; // v1.5.2 safety: temporarily disable broken localized rendering.
        if let Some(config_path) = cli_config_path() {
            prefs.config_path = config_path;
        }
        let initial_theme = prefs.theme.clone();
        apply_theme(&cc.egui_ctx, &prefs.theme);
        apply_visual_tuning(&cc.egui_ctx);
        install_optional_font(&cc.egui_ctx);
        preload_core_audio();

        let mut app = Self {
            prefs,
            snapshot: ChainSnapshot::default(),
            status_line: "Ready.".to_string(),
            last_error: None,
            last_success: None,
            last_block_card: None,
            miner: None,
            benchmark_rx: None,
            benchmark_running: false,
            benchmark_result: None,
            delete_private_key_confirm: false,
            import_key_dialog: ImportKeyDialog::default(),
            send_dialog: SendDialog::default(),
            send_rx: None,
            eth_wallets: load_ethereum_wallet_book().unwrap_or_default(),
            eth_wallet_dialog: EthereumWalletDialog::default(),
            eth_balances: EthereumBalanceState::default(),
            eth_balance_rx: None,
            eth_balance_in_flight: false,
            eth_send_dialog: EthereumSendDialog::default(),
            eth_send_rx: None,
            usdj_vault_dialog: UsdjVaultDialog::default(),
            usdj_vault_rx: None,
            conversion_dialog: ConversionDialog::default(),
            conversion_rx: None,
            buy_jin_dialog: BuyJinDialog::default(),
            buy_jin_rx: None,
            qns_dialog: QnsDialog::default(),
            qns_rx: None,
            pool_dialog: PoolDialog::default(),
            pool_rx: None,
            library_dialog: LibraryDialog::default(),
            library_state_cache: None,
            library_state_rx: None,
            library_state_in_flight: false,
            library_state_last_started: Instant::now() - Duration::from_secs(60),
            library_state_last_loaded: None,
            library_state_error: None,
            library_action_rx: None,
            library_action_in_flight: false,
            pool_mining_pool_id: String::new(),
            right_balances_collapsed: false,
            right_activity_collapsed: false,
            last_send_status_poll: Instant::now() - Duration::from_secs(10),
            snapshot_rx: None,
            snapshot_in_flight: false,
            last_theme_applied: initial_theme,
            logo: load_logo_texture(&cc.egui_ctx),
            jin_logo: load_asset_texture(&cc.egui_ctx, JIN_LOGO_PATH, "jin-coin-logo"),
            mining_off_icon: load_asset_texture(&cc.egui_ctx, MINING_OFF_ICON_PATH, "mining-off"),
            mining_off_icon_white: load_asset_texture(&cc.egui_ctx, MINING_OFF_ICON_WHITE_PATH, "mining-off-white"),
            // v1.2.0: GIF frames are decoded once at startup into egui textures.
            // This restores v1.0.6 mining visuals without disk reads during frames.
            mining_prep_anim: load_gif_animation(&cc.egui_ctx, MINING_PREP_GIF_PATH, "mining-prep"),
            mining_prep_anim_white: load_gif_animation(&cc.egui_ctx, MINING_PREP_GIF_WHITE_PATH, "mining-prep-white"),
            mining_on_anim: load_gif_animation(&cc.egui_ctx, MINING_ON_GIF_PATH, "mining-on"),
            mining_on_anim_white: load_gif_animation(&cc.egui_ctx, MINING_ON_GIF_WHITE_PATH, "mining-on-white"),
            online_icon: load_asset_texture(&cc.egui_ctx, ONLINE_ICON_PATH, "online-icon"),
            offline_icon: load_asset_texture(&cc.egui_ctx, OFFLINE_ICON_PATH, "offline-icon"),
            icons: IconSet::load(&cc.egui_ctx),
            wallet_sync_rx: None,
            wallet_sync_in_flight: false,
            wallet_create_rx: None,
            wallet_create_in_flight: false,
            send_qns_resolve_rx: None,
            send_qns_resolve_in_flight: false,
            multi_qns_resolve_rx: None,
            last_wallet_sync: Instant::now() - Duration::from_secs(DEFAULT_WALLET_SYNC_INTERVAL_SECS),

            app_started: Instant::now(),
            last_refresh: Instant::now() - Duration::from_secs(10),
            hash_rate_hps: 0.0,
            total_hashes: 0,
            gpu_hash_rate_hps: 0.0,
            gpu_total_hashes: 0,
            gpu_workers: 0,
            gpu_device: String::new(),
            gpu_device_options: gpu_miner::available_gpu_device_labels().unwrap_or_default(),
            gpu_device_last_scan: Instant::now(),
            gpu_device_rates: HashMap::new(),
            miner_threads: 0,
            miner_duty: 0,
            target_height: 0,
            block_history: VecDeque::new(),
            last_local_mined_at: None,
            mining_phase: MiningPhase::Off,
            mining_phase_started_at: None,
            mining_on_next_sound_at: None,
            last_observed_tip_hash: String::new(),
            last_observed_height: 0,
            sync_progress_last_height: 0,
            sync_progress_last_sample: Instant::now(),
            sync_progress_rate_bps: 0.0,
            prefs_dirty: false,
            p2p_node_started: false,
            p2p_node_rx: None,
            update_dialog: UpdateDialog::default(),
            update_rx: None,
            update_check_in_flight: false,
            update_check_forced: false,
            last_update_check: Instant::now() - Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS),
            qr_cache_address: String::new(),
            qr_cache_texture: None,
            qr_hover_until: None,
            qr_hover_address: String::new(),
            qr_scan_dialog_open: false,
            qr_scan_message: String::new(),
            qr_camera_devices: detect_camera_devices(),
            qr_camera_selected: 0,
            tx_status_rx: None,
            tx_status_in_flight: false,
            enjin_metrics: EnjinMatrixMetrics::default(),
            enjin_metrics_rx: None,
            enjin_metrics_in_flight: false,
            last_enjin_metrics_refresh: Instant::now() - Duration::from_secs(ENJIN_MATRIX_METRICS_REFRESH_SECS),
            opening_banner: load_asset_texture(&cc.egui_ctx, OPENING_BANNER_PATH, "opening-banner"),
            initial_loading: true,
            initial_loading_started_at: Instant::now(),
            pending_start_mining_on_open: false,
            pending_start_mining_mode: AutoMiningMode::Solo,
            hf86_force_quick_snapshot: false,
            manual_stop_requested: false,
            desired_pool_mining_pool_id: String::new(),
            auto_restart_mining_at: None,
            hf110_autoheal_paused_mining: false,
            hf110_last_mining_auto_pause: Instant::now() - Duration::from_secs(600),
            hf85_last_auto_catchup_sync: Instant::now() - Duration::from_secs(60),
            hf85_auto_catchup_last_height: 0,
            hf85_auto_catchup_stale_since: Instant::now(),
            hf85_seen_catchup_epoch: hf85_catchup_epoch(),
            hf88_last_peer_probe: Instant::now() - Duration::from_secs(60),
            hf88_last_snapshot_height: 0,
            hf88_snapshot_stale_since: Instant::now(),
            hf88_snapshot_backoff_until: None,
            hf88_snapshot_timeout_count: 0,
            hf88_last_snapshot_success: Instant::now(),
        };
        app.prefs.pace_to_target_spacing = false;
        if app.prefs.gpu_device_selector.trim().is_empty() {
            app.prefs.gpu_device_selector = gpu_miner::GPU_DEVICE_ALL.to_string();
        }
        app.normalize_update_prefs_for_network();
        app.apply_hf105_first_run_auto_restart_default();
        app.update_runtime_identity();
        if app.prefs.setup_complete {
            let after_update_restart = consume_hf86_post_update_restart_marker();
            if after_update_restart {
                app.pending_start_mining_on_open = app.prefs.start_mining_after_update_restart && !app.prefs.payout_address.trim().is_empty();
                app.pending_start_mining_mode = app.prefs.start_mining_after_update_restart_mode;
                if app.pending_start_mining_on_open {
                    app.status_line = "Startup: post-update restart detected; local wallet/blocks load first, then QUB Core auto-mining policy applies.".to_string();
                } else {
                    app.status_line = "Startup: post-update restart detected; auto-mining after update is disabled.".to_string();
                }
            } else {
                app.pending_start_mining_on_open = app.prefs.start_mining_on_open && !app.prefs.payout_address.trim().is_empty();
                app.pending_start_mining_mode = app.prefs.start_mining_on_open_mode;
                app.status_line = "Startup: loading local wallet/blocks first; network workers start after the first local view is visible...".to_string();
            }
            app.start_background_snapshot_refresh();
        } else {
            app.initial_loading = false;
            app.status_line = "Setup wizard ready.".to_string();
        }
        if !app.eth_wallets.wallets.is_empty() {
            app.start_ethereum_balance_refresh();
        }
        app
    }

    fn poll_enjin_matrix_metrics(&mut self) {
        let polled = self.enjin_metrics_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(Ok(metrics))) => {
                self.enjin_metrics = metrics;
                self.enjin_metrics_in_flight = false;
                self.enjin_metrics_rx = None;
            }
            Some(Ok(Err(err))) => {
                self.enjin_metrics.last_status = format!("Matrixchain RPC fetch failed: {err}");
                self.enjin_metrics.updated_at = Some(Instant::now());
                self.enjin_metrics_in_flight = false;
                self.enjin_metrics_rx = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.enjin_metrics_in_flight = false;
                self.enjin_metrics_rx = None;
            }
        }
    }

    fn start_enjin_matrix_metrics_fetch(&mut self) {
        if self.enjin_metrics_in_flight { return; }
        if !self.snapshot.network.eq_ignore_ascii_case("mainnet") { return; }
        let (tx, rx) = mpsc::channel();
        self.enjin_metrics_rx = Some(rx);
        self.enjin_metrics_in_flight = true;
        self.last_enjin_metrics_refresh = Instant::now();
        thread::spawn(move || {
            let result = fetch_enjin_matrix_metrics_direct().map_err(|err| format!("{err:#}"));
            let _ = tx.send(result);
        });
    }

    fn normalize_update_prefs_for_network(&mut self) {
        // Updates are tied to the installed build channel, not to the currently selected config.
        // This prevents a testnet canary update from replacing the mainnet install.
        let channel = build_channel();
        let desired_url = if channel == "testnet" { DEFAULT_TESTNET_UPDATE_URL } else { DEFAULT_MAINNET_UPDATE_URL };
        let current = self.prefs.update_url.trim();
        let should_migrate_url = current.is_empty()
            || current.eq_ignore_ascii_case(LEGACY_DEFAULT_UPDATE_URL)
            || current.eq_ignore_ascii_case(LEGACY_MAINNET_UPDATE_URL)
            || current.eq_ignore_ascii_case(LEGACY_TESTNET_UPDATE_URL)
            || current.ends_with("/QUB-Core-Latest.exe")
            || (channel == "mainnet" && current.contains("/testnet/"))
            || (channel == "testnet" && current.contains("/mainnet/"));
        if should_migrate_url {
            self.prefs.update_url = desired_url.to_string();
            self.prefs_dirty = true;
        }
        if channel == "mainnet" && ALLOW_UNSIGNED_MAINNET_UPDATES_PRIVATE_BUILD {
            if !self.prefs.auto_check_updates || !self.prefs.auto_download_updates || !self.prefs.auto_install_updates {
                self.prefs.auto_check_updates = true;
                self.prefs.auto_download_updates = true;
                self.prefs.auto_install_updates = true;
                self.prefs_dirty = true;
            }
        }
        self.prefs.pace_to_target_spacing = false;
    }

    fn is_mainnet_network(&self) -> bool {
        self.snapshot.network.trim().eq_ignore_ascii_case("mainnet")
    }

    fn version_network_label(&self) -> String {
        if self.snapshot.network.eq_ignore_ascii_case("testnet") {
            format!("{} (Testnet)", APP_VERSION)
        } else if self.snapshot.network.eq_ignore_ascii_case("regtest") || self.snapshot.network.eq_ignore_ascii_case("regtest-lan") {
            format!("{} ({})", APP_VERSION, self.snapshot.network)
        } else {
            APP_VERSION.to_string()
        }
    }


    fn normalized_gpu_device_selector(&self) -> String {
        let selected = self.prefs.gpu_device_selector.trim();
        if selected.is_empty() { gpu_miner::GPU_DEVICE_ALL.to_string() } else { selected.to_string() }
    }

    fn gpu_selector_status_label(&self) -> String {
        let selected = self.normalized_gpu_device_selector();
        if selected.eq_ignore_ascii_case(gpu_miner::GPU_DEVICE_ALL) {
            let count = self.gpu_device_options.len();
            if count == 0 {
                "All high-performance GPUs (auto-detect at mining start)".to_string()
            } else {
                format!("All high-performance GPUs by default ({count} detected; integrated GPUs can be selected manually)")
            }
        } else if selected.eq_ignore_ascii_case(gpu_miner::GPU_DEVICE_ALL_DETECTED) {
            let count = self.gpu_device_options.len();
            if count == 0 {
                "All detected GPUs (experimental; auto-detect at mining start)".to_string()
            } else {
                format!("All detected GPUs (experimental, {count} device(s))")
            }
        } else {
            selected
        }
    }

    fn refresh_gpu_devices(&mut self) {
        self.gpu_device_options = gpu_miner::available_gpu_device_labels().unwrap_or_default();
        self.gpu_device_last_scan = Instant::now();
        if self.prefs.gpu_device_selector.trim().is_empty() {
            self.prefs.gpu_device_selector = gpu_miner::GPU_DEVICE_ALL.to_string();
            self.prefs_dirty = true;
        }
        let selected = self.prefs.gpu_device_selector.trim().to_string();
        if !selected.eq_ignore_ascii_case(gpu_miner::GPU_DEVICE_ALL)
            && !selected.eq_ignore_ascii_case(gpu_miner::GPU_DEVICE_ALL_DETECTED)
            && !self.gpu_device_options.iter().any(|d| d == &selected)
        {
            self.prefs.gpu_device_selector = gpu_miner::GPU_DEVICE_ALL.to_string();
            self.prefs_dirty = true;
        }
    }

    fn recompute_gpu_aggregate(&mut self) {
        if self.gpu_device_rates.is_empty() { return; }
        let mut entries = self.gpu_device_rates.iter().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        self.gpu_hash_rate_hps = entries.iter().map(|(_, (hps, _, _))| *hps).sum::<f64>();
        self.gpu_total_hashes = entries.iter().map(|(_, (_, total, _))| *total).sum::<u64>();
        self.gpu_workers = entries.iter().map(|(_, (_, _, workers))| *workers).sum::<usize>();
        self.gpu_device = if entries.len() == 1 {
            entries[0].0.to_string()
        } else {
            format!("All ({} GPUs)", entries.len())
        };
    }

    fn total_hash_rate_hps(&self) -> f64 {
        self.hash_rate_hps.max(0.0) + self.gpu_hash_rate_hps.max(0.0)
    }

    fn cpu_hashing_active(&self) -> bool {
        self.miner.is_some() && self.hash_rate_hps > 0.5
    }

    fn gpu_hashing_active(&self) -> bool {
        self.miner.is_some() && self.gpu_hash_rate_hps > 0.5
    }

    fn truly_mining_active(&self) -> bool {
        self.miner.is_some() && self.total_hash_rate_hps() > 0.5
    }

    fn mining_mode_short(&self) -> &'static str {
        if self.miner.is_some() && !self.pool_mining_pool_id.trim().is_empty() { "P" } else { "S" }
    }

    fn mining_status_dot_text(&self, active: bool) -> &'static str {
        if active { "●" } else { "○" }
    }

    fn mining_controls_header_text(&self) -> String {
        "Mining controls".to_string()
    }

    fn miner_telemetry_header_text(&self) -> String {
        "Miner telemetry".to_string()
    }

    fn mining_controls_header_suffix(&self) -> &'static str {
        if self.miner.is_some() { self.mining_mode_short() } else { "-" }
    }

    fn hashrate_text_meter_hf96(&self, segments: usize) -> String {
        let total = self.total_hash_rate_hps();
        let peak = self.prefs.mining_peak_hash_rate_hps.max(total).max(1.0);
        let fraction = (total / peak).clamp(0.0, 1.0);
        let filled = ((segments as f64) * fraction).round().clamp(0.0, segments as f64) as usize;
        let mut out = String::with_capacity(segments + 2);
        out.push('[');
        for idx in 0..segments {
            out.push(if idx < filled { '=' } else { '-' });
        }
        out.push(']');
        out
    }

    fn update_hashrate_records(&mut self) {
        let total = self.total_hash_rate_hps();
        if total <= 0.5 { return; }
        let mut changed = false;
        if total > self.prefs.mining_peak_hash_rate_hps {
            self.prefs.mining_peak_hash_rate_hps = total;
            changed = true;
        }
        if self.hash_rate_hps > self.prefs.mining_peak_cpu_hash_rate_hps {
            self.prefs.mining_peak_cpu_hash_rate_hps = self.hash_rate_hps;
            changed = true;
        }
        if self.gpu_hash_rate_hps > self.prefs.mining_peak_gpu_hash_rate_hps {
            self.prefs.mining_peak_gpu_hash_rate_hps = self.gpu_hash_rate_hps;
            changed = true;
        }
        if changed { self.prefs_dirty = true; }
    }

    fn ui_status_dot_png(&self, ui: &mut egui::Ui, active: bool, size: f32) {
        let texture = if active { self.online_icon.as_ref() } else { self.offline_icon.as_ref() };
        if let Some(texture) = texture {
            let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(size, size));
            ui.add(egui::Image::from_texture(sized));
        } else {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            let color = if active { egui::Color32::from_rgb(60, 220, 120) } else { egui::Color32::from_gray(110) };
            ui.painter().circle_filled(rect.center(), size * 0.38, color);
        }
    }

    fn ui_disclosure_glyph(&self, ui: &mut egui::Ui, open: bool) {
        let glyph = if open { "v" } else { ">" };
        ui.label(egui::RichText::new(glyph).strong().weak());
    }

    fn ui_mining_mode_badge(&self, ui: &mut egui::Ui) {
        let active = self.truly_mining_active();
        ui.horizontal(|ui| {
            let text = self.mining_controls_header_suffix();
            self.ui_status_dot_png(ui, active, 12.0);
            ui.add_space(2.0);
            let color = if active { egui::Color32::from_rgb(80, 230, 135) } else { ui.visuals().weak_text_color() };
            ui.label(egui::RichText::new(text).strong().color(color));
        }).response.on_hover_text("Mining status: green = actively hashing. S = solo mining, P = pool mining.");
    }


    fn ui_mining_controls_header_hf99(&self, ui: &mut egui::Ui) {
        self.ui_icon(ui, "mining-controls", 16.0);
        ui.label(egui::RichText::new(self.mining_controls_header_text()).strong());
        ui.add_space(4.0);
        self.ui_mining_mode_badge(ui);
    }

    fn ui_miner_telemetry_header_hf99(&self, ui: &mut egui::Ui) {
        self.ui_icon(ui, "miner-telemetry", 16.0);
        ui.label(egui::RichText::new(self.miner_telemetry_header_text()).strong());
        ui.add_space(8.0);
        self.ui_status_dot_png(ui, self.cpu_hashing_active(), 12.0);
        ui.label(egui::RichText::new(format!("CPU {}", format_hps(self.hash_rate_hps))).strong());
        ui.add_space(8.0);
        self.ui_status_dot_png(ui, self.gpu_hashing_active(), 12.0);
        let gpu_text = if self.gpu_workers == 0 && self.gpu_hash_rate_hps <= 0.5 { "off".to_string() } else { format_hps(self.gpu_hash_rate_hps) };
        ui.label(egui::RichText::new(format!("GPU {}", gpu_text)).strong());
        ui.add_space(10.0);
        let meter_width = ui.available_width().clamp(140.0, 360.0);
        self.ui_hashrate_horizontal_meter_sized(ui, meter_width, 12.0);
    }

    fn ui_hashrate_horizontal_meter(&self, ui: &mut egui::Ui) {
        self.ui_hashrate_horizontal_meter_sized(ui, ui.available_width().clamp(180.0, 520.0), 14.0);
    }

    fn ui_hashrate_horizontal_meter_sized(&self, ui: &mut egui::Ui, width: f32, height: f32) {
        let total = self.total_hash_rate_hps();
        let peak = self.prefs.mining_peak_hash_rate_hps.max(total).max(1.0);
        let fraction = (total / peak).clamp(0.0, 1.0) as f32;
        let active = self.truly_mining_active();
        let width = width.clamp(120.0, 520.0);
        let height = height.clamp(10.0, 18.0);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let bg = if ui.visuals().dark_mode { egui::Color32::from_gray(34) } else { egui::Color32::from_gray(210) };
        let outline = if active { egui::Color32::from_rgb(72, 210, 120) } else { egui::Color32::from_gray(110) };
        let rounding = height * 0.5;
        painter.rect_filled(rect, rounding, bg);
        painter.rect_stroke(rect, rounding, egui::Stroke::new(1.0, outline), egui::StrokeKind::Inside);
        let fill_w = (rect.width() * fraction).clamp(0.0, rect.width());
        if fill_w > 1.0 {
            let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
            let fill = if active { egui::Color32::from_rgb(54, 204, 116) } else { egui::Color32::from_gray(120) };
            painter.rect_filled(fill_rect.shrink(2.0), rounding.max(4.0) - 1.0, fill);
            if active {
                let t = (self.app_started.elapsed().as_secs_f32() * 0.55).fract();
                let x = rect.left() + fill_w * t;
                painter.line_segment(
                    [egui::pos2(x, rect.top() + 2.0), egui::pos2(x, rect.bottom() - 2.0)],
                    egui::Stroke::new(1.8, egui::Color32::from_rgba_unmultiplied(235, 255, 235, 185)),
                );
            }
        }
        response.on_hover_text(format!(
            "Live total hashrate: {}
Personal GUI record: {}
The bar uses your local record as the dynamic max.",
            format_hps(total),
            format_hps(self.prefs.mining_peak_hash_rate_hps.max(total)),
        ));
    }

    fn ui_hashrate_vertical_meter(&self, ui: &mut egui::Ui, label: &str, current: f64, peak: f64, active: bool) {
        // HF106/v1.6.9: fixed-size meter cell. Do not use vertical_centered(),
        // because on wide panels it can consume the whole remaining row width and
        // push CPU/GPU meters to the far right.
        let cell_width = 56.0;
        let meter_width = 38.0;
        let meter_height = 108.0;
        ui.allocate_ui_with_layout(
            egui::vec2(cell_width, meter_height + 42.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let peak = peak.max(current).max(1.0);
                let fraction = (current / peak).clamp(0.0, 1.0) as f32;
                let (rect, response) = ui.allocate_exact_size(egui::vec2(meter_width, meter_height), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                let bg = if ui.visuals().dark_mode { egui::Color32::from_gray(30) } else { egui::Color32::from_gray(220) };
                let outline = if active { egui::Color32::from_rgb(72, 210, 120) } else { egui::Color32::from_gray(115) };
                painter.rect_filled(rect, 8, bg);
                painter.rect_stroke(rect, 8, egui::Stroke::new(1.0, outline), egui::StrokeKind::Inside);
                let fill_h = (rect.height() * fraction).clamp(0.0, rect.height());
                if fill_h > 1.0 {
                    let fill_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.left() + 3.0, rect.bottom() - 3.0 - fill_h),
                        egui::pos2(rect.right() - 3.0, rect.bottom() - 3.0),
                    );
                    let fill = if active { egui::Color32::from_rgb(54, 204, 116) } else { egui::Color32::from_gray(120) };
                    painter.rect_filled(fill_rect, 6, fill);
                    if active {
                        let t = (self.app_started.elapsed().as_secs_f32() * 0.65).fract();
                        let y = fill_rect.bottom() - fill_rect.height() * t;
                        painter.line_segment(
                            [egui::pos2(fill_rect.left() + 3.0, y), egui::pos2(fill_rect.right() - 3.0, y)],
                            egui::Stroke::new(1.6, egui::Color32::from_rgba_unmultiplied(235, 255, 235, 180)),
                        );
                    }
                }
                response.on_hover_text(format!("{label}
Current: {}
Personal record: {}", format_hps(current), format_hps(peak)));
                ui.label(egui::RichText::new(label).strong());
                ui.small(format_hps(current));
            },
        );
    }

    fn ui_hashrate_vertical_meters(&self, ui: &mut egui::Ui) {
        // HF106/v1.6.9: compact left-anchored personal meters.
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Personal hashrate meters").strong());
            ui.small("Dynamic max = your local GUI record");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                self.ui_hashrate_vertical_meter(ui, "Total", self.total_hash_rate_hps(), self.prefs.mining_peak_hash_rate_hps, self.truly_mining_active());
                ui.add_space(22.0);
                self.ui_hashrate_vertical_meter(ui, "CPU", self.hash_rate_hps, self.prefs.mining_peak_cpu_hash_rate_hps, self.cpu_hashing_active());
                ui.add_space(10.0);
                self.ui_hashrate_vertical_meter(ui, "GPU", self.gpu_hash_rate_hps, self.prefs.mining_peak_gpu_hash_rate_hps, self.gpu_hashing_active());
            });
        });
    }

    fn best_known_network_height(&self) -> u32 {
        // HF106/v1.6.9: canonical progress/mining UI must not be driven by
        // arbitrary reachable peers or cached blockstream rows. A random peer can
        // advertise a future/local branch and should remain telemetry only. Use
        // the locally validated height plus official/direct seed-derived heights
        // only; recent block rows are rendered from this value, not vice versa.
        let official_known = self.snapshot.known_network_height.max(self.snapshot.direct_network_height);
        self.snapshot.height.max(official_known)
    }

    fn sync_progress_fraction(&self) -> f32 {
        let local = self.snapshot.height;
        let known = self.best_known_network_height();
        if known > 0 && known > local {
            return ((local as f32) / (known as f32)).clamp(0.05, 0.98);
        }
        if known > 0 && local >= known && (self.wallet_sync_in_flight || self.snapshot_in_flight || self.initial_loading) {
            return 0.96;
        }
        if self.wallet_sync_in_flight {
            let elapsed = self.last_wallet_sync.elapsed().as_secs_f32();
            return (0.12 + elapsed / 240.0 * 0.82).clamp(0.12, 0.94);
        }
        if self.snapshot_in_flight {
            let elapsed = self.last_refresh.elapsed().as_secs_f32();
            return (0.10 + elapsed / 360.0 * 0.84).clamp(0.10, 0.94);
        }
        if self.initial_loading {
            let elapsed = self.initial_loading_started_at.elapsed().as_secs_f32();
            return (0.08 + elapsed / 120.0 * 0.86).clamp(0.08, 0.94);
        }
        1.0
    }

    fn sync_progress_rows(&self, label: &str) -> (String, String, String) {
        let elapsed = if self.wallet_sync_in_flight {
            self.last_wallet_sync.elapsed().as_secs()
        } else if self.snapshot_in_flight {
            self.last_refresh.elapsed().as_secs()
        } else {
            self.initial_loading_started_at.elapsed().as_secs()
        };
        let local = self.snapshot.height;
        let known = self.best_known_network_height();
        let detail = if known > local {
            let remaining = known.saturating_sub(local);
            let source = if self.snapshot.known_network_height >= known { "official seeds" }
                else if self.snapshot.direct_network_height >= known { "direct peers" }
                else { "local block stream" };
            let rate_extra = if self.sync_progress_rate_bps > 0.01 {
                let bpm = self.sync_progress_rate_bps * 60.0;
                let eta_secs = (remaining as f32 / self.sync_progress_rate_bps).max(1.0);
                let eta = if eta_secs < 90.0 { format!("{:.0}s", eta_secs) } else { format!("{:.1}m", eta_secs / 60.0) };
                format!(" | live rate {:.1} block(s)/min | ETA {eta}", bpm)
            } else {
                " | measuring live catch-up rate".to_string()
            };
            format!("local #{local} -> {source} #{known} | {remaining} block(s) remaining{rate_extra}")
        } else if known > 0 {
            format!("local #{local} is at known tip #{known} | verifying balances/activity")
        } else {
            "discovering official/peer tip | waiting for first height sample".to_string()
        };
        let meta = format!("{elapsed}s elapsed | QUB Core strong catch-up runs detached; UI stays live");
        (label.to_string(), detail, meta)
    }

    fn ui_sync_progress_bar_rows(&self, ui: &mut egui::Ui, label: &str, rows: usize) {
        let (headline, detail, meta) = self.sync_progress_rows(label);
        ui.add(egui::ProgressBar::new(self.sync_progress_fraction()).show_percentage().text(headline));
        if rows >= 3 {
            ui.small(detail);
            ui.small(meta);
        } else {
            ui.small(format!("{} | {}", detail, meta));
        }
    }

    fn ui_sync_progress_bar(&self, ui: &mut egui::Ui, label: &str) {
        self.ui_sync_progress_bar_rows(ui, label, 2);
    }

    fn refresh_snapshot(&mut self) {
        match read_snapshot_for_payout(&self.prefs.config_path, &self.prefs.payout_address) {
            Ok(snapshot) => self.apply_snapshot(snapshot),
            Err(err) => {
                self.last_error = Some(format!("Refresh failed: {err:#}"));
            }
        }
        self.last_refresh = Instant::now();
        self.update_runtime_identity();
    }

    fn apply_snapshot(&mut self, snapshot: ChainSnapshot) {
        let now = Instant::now();
        let sample_secs = now.duration_since(self.sync_progress_last_sample).as_secs_f32();
        if sample_secs >= 1.0 {
            if self.sync_progress_last_height == 0 && snapshot.height > 1 {
                // HF79/v1.6.0: the first real snapshot can jump from default 0
                // to current mainnet height. Treat that as the baseline, not a
                // fake 10k+ blocks/sec progress sample.
                self.sync_progress_rate_bps = 0.0;
            } else if snapshot.height > self.sync_progress_last_height {
                let delta = snapshot.height.saturating_sub(self.sync_progress_last_height) as f32;
                let instant_rate = delta / sample_secs.max(0.001);
                self.sync_progress_rate_bps = if self.sync_progress_rate_bps <= 0.0 {
                    instant_rate
                } else {
                    (self.sync_progress_rate_bps * 0.65) + (instant_rate * 0.35)
                };
            } else if self.wallet_sync_in_flight || self.snapshot_in_flight || self.initial_loading {
                self.sync_progress_rate_bps *= 0.92;
                if self.sync_progress_rate_bps < 0.005 { self.sync_progress_rate_bps = 0.0; }
            }
            self.sync_progress_last_height = snapshot.height;
            self.sync_progress_last_sample = now;
        }
        if self.prefs.payout_address.trim().is_empty() && !snapshot.default_address.is_empty() {
            self.prefs.payout_address = snapshot.default_address.clone();
        }
        self.handle_snapshot_tip_change(&snapshot);
        self.last_observed_tip_hash = snapshot.best_hash.clone();
        self.last_observed_height = snapshot.height;
        self.snapshot = snapshot;
        self.ensure_pool_selection_from_last();
        self.last_error = None;
        if self.last_success.as_deref().map(|s| s.contains("Startup sync is still running")).unwrap_or(false) {
            self.last_success = None;
        }
        let was_initial_loading = self.initial_loading;
        if self.initial_loading {
            self.initial_loading = false;
            self.hf85_last_auto_catchup_sync = Instant::now() - Duration::from_secs(60);
        }
        if self.prefs.setup_complete && !self.p2p_node_started && matches!(self.snapshot.network.as_str(), "mainnet" | "testnet") {
            self.ensure_p2p_node_started();
            self.hf85_last_auto_catchup_sync = Instant::now();
            spawn_hf85_catchup_pulse(self.prefs.config_path.clone(), false);
            self.status_line = if was_initial_loading {
                "QUB Core ready: local wallet/blocks are visible; QUB Core network catch-up is detached in the background.".to_string()
            } else {
                "QUB Core local snapshot is live; starting delayed network catch-up in the background.".to_string()
            };
        } else if was_initial_loading {
            self.status_line = "QUB Core ready: local wallet/blocks are visible.".to_string();
        }
        self.update_runtime_identity();
    }

    fn maybe_start_hf85_auto_catchup_sync(&mut self) {
        if !self.prefs.setup_complete || self.initial_loading { return; }
        // HF106: snapshot refresh is a UI read and must not suppress catch-up.
        // Otherwise a slow balance/activity snapshot can freeze network progress.
        if self.wallet_sync_in_flight { return; }
        if !matches!(self.snapshot.network.as_str(), "mainnet" | "testnet") { return; }

        let local = self.snapshot.height;
        let known = self.best_known_network_height();

        // HF106: if no network height has been sampled yet, still send a bounded
        // detached catch-up/probe pulse. HF82 could sit forever at #0/unknown
        // because the first sync worker owned both discovery and wallet loading.
        if known == 0 {
            if self.hf85_last_auto_catchup_sync.elapsed() >= Duration::from_secs(20) {
                self.hf85_last_auto_catchup_sync = Instant::now();
                self.status_line = "QUB Core background catch-up pulse: discovering official/peer tip after local wallet loaded.".to_string();
                spawn_hf85_catchup_pulse(self.prefs.config_path.clone(), false);
            }
            return;
        }

        if known <= local {
            self.hf85_auto_catchup_last_height = local;
            self.hf85_auto_catchup_stale_since = Instant::now();
            // Even when the last fast snapshot only knows the local height, keep
            // a low-frequency official/peer pulse alive. This is the safety net
            // for users who leave QUB Core open overnight without pressing Sync.
            if self.hf85_last_auto_catchup_sync.elapsed() >= Duration::from_secs(18) {
                self.hf85_last_auto_catchup_sync = Instant::now();
                self.status_line = format!("QUB Core heartbeat catch-up: local #{} is reported at known tip; probing seeds/peers for a newer tip without blocking the UI.", local);
                spawn_hf85_catchup_pulse(self.prefs.config_path.clone(), false);
            }
            return;
        }

        if local != self.hf85_auto_catchup_last_height {
            self.hf85_auto_catchup_last_height = local;
            self.hf85_auto_catchup_stale_since = Instant::now();
        }

        let gap = known.saturating_sub(local);
        let stalled_for = self.hf85_auto_catchup_stale_since.elapsed();
        let mining_or_waiting = self.miner.is_some() || matches!(self.mining_phase, MiningPhase::Preparing | MiningPhase::Mining);

        // HF111/v1.7.1: if mining/preparing is present while official/direct
        // chain stays ahead, pause hashing entirely and let a chain-only repair
        // own disk/network resources. Users were fixing this manually by Stop ->
        // restart -> Sync/Repair -> Start; do it automatically and resume after
        // canonical catch-up. This is safety-first and prevents local-only forks.
        if mining_or_waiting && gap > 0 && (stalled_for >= Duration::from_secs(60) || gap >= 4) {
            self.hf110_pause_mining_for_canonical_catchup(gap, stalled_for.as_secs());
            return;
        }

        let force_strong = stalled_for >= Duration::from_secs(45) || gap >= 4;
        let interval = if force_strong { Duration::from_secs(5) }
            else if mining_or_waiting || gap >= 2 { Duration::from_secs(9) }
            else { Duration::from_secs(14) };

        if self.hf85_last_auto_catchup_sync.elapsed() >= interval {
            self.hf85_last_auto_catchup_sync = Instant::now();
            self.status_line = format!(
                "QUB Core strong detached catch-up: local #{} -> live #{} ({} block(s) behind, stalled {}s). Wallet stays responsive; mining waits for green-light.",
                local,
                known,
                gap,
                stalled_for.as_secs()
            );
            spawn_hf85_catchup_pulse(self.prefs.config_path.clone(), force_strong);
        }
    }


    fn poll_hf85_catchup_completion(&mut self) {
        let epoch = hf85_catchup_epoch();
        if epoch == self.hf85_seen_catchup_epoch {
            return;
        }
        self.hf85_seen_catchup_epoch = epoch;
        self.hf85_auto_catchup_stale_since = Instant::now();
        self.hf86_force_quick_snapshot = true;
        self.hf88_snapshot_backoff_until = None;
        self.status_line = "QUB Core catch-up pulse completed; refreshing local wallet/blocks/peers with a quick local snapshot.".to_string();
        if self.snapshot_in_flight || hf88_snapshot_worker_running() {
            // HF106: do not abandon an existing snapshot worker. Abandoning it
            // created the HF87 repeated timeout loop and queued extra readers
            // behind the storage mutex. The next normal tick will refresh once
            // the active reader finishes.
            self.hf86_force_quick_snapshot = true;
            return;
        }
        self.start_background_snapshot_refresh();
    }

    fn poll_snapshot_refresh(&mut self) {
        let polled = self.snapshot_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(Ok(snapshot))) => {
                self.snapshot_in_flight = false;
                self.snapshot_rx = None;
                self.hf88_snapshot_backoff_until = None;
                self.hf88_snapshot_timeout_count = 0;
                self.hf88_last_snapshot_success = Instant::now();
                self.apply_snapshot(snapshot);
            }
            Some(Ok(Err(err))) => {
                self.snapshot_in_flight = false;
                self.snapshot_rx = None;
                self.initial_loading = false;
                self.last_error = Some(format!("Refresh failed: {err}"));
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {
                if self.snapshot_in_flight {
                    let elapsed = self.last_refresh.elapsed().as_secs();
                    if !self.initial_loading && elapsed > 90 {
                        self.snapshot_in_flight = false;
                        self.snapshot_rx = None;
                        self.hf88_snapshot_timeout_count = self.hf88_snapshot_timeout_count.saturating_add(1);
                        let backoff_secs = match self.hf88_snapshot_timeout_count {
                            0 | 1 => 20,
                            2 => 45,
                            _ => 90,
                        };
                        self.hf88_snapshot_backoff_until = Some(Instant::now() + Duration::from_secs(backoff_secs));
                        self.last_error = Some(format!("QUB Core snapshot worker exceeded 90s and was released. Local UI stays live; next quick local refresh is backed off for {backoff_secs}s while detached catch-up continues."));
                        self.hf86_force_quick_snapshot = true;
                        self.last_refresh = Instant::now();
                    } else if self.initial_loading && elapsed > 20 {
                        // HF72/v1.5.8: do not keep non-mining users trapped on the
                        // splash screen. The verified startup worker continues in the
                        // background and will apply the fresh snapshot when it returns.
                        self.initial_loading = false;
                        self.status_line = "QUB Core opened while the local snapshot is still loading. Network catch-up is paused until wallet/blocks are visible.".to_string();
                        self.last_success = Some("QUB Core is keeping the UI open. If wallet/blocks are not visible within a few minutes, restart once and capture the bottom status.".to_string());
                        self.last_error = None;
                    } else if self.miner.is_none() {
                        let local = self.snapshot.height;
                        let known = self.best_known_network_height();
                        if hf85_catchup_running() && known > local {
                            self.status_line = format!("QUB Core catch-up writer active: local #{local} -> network #{known} ({} block(s) remaining). Snapshot waits; UI stays live... {}s elapsed", known.saturating_sub(local), hf85_catchup_elapsed_secs());
                        } else if known > local {
                            self.status_line = format!("Startup/background chain refresh: local #{local} -> network #{known} ({} block(s) remaining)... {elapsed}s elapsed", known.saturating_sub(local));
                        } else {
                            self.status_line = format!("Startup/background chain refresh: verifying latest tip and wallet state... {elapsed}s elapsed");
                        }
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.snapshot_in_flight = false;
                self.snapshot_rx = None;
                self.initial_loading = false;
            }
        }
    }

    fn start_background_snapshot_refresh(&mut self) {
        if self.snapshot_in_flight { return; }
        if let Some(until) = self.hf88_snapshot_backoff_until {
            if Instant::now() < until { return; }
            self.hf88_snapshot_backoff_until = None;
        }
        // HF106: do not start a UI snapshot reader while a detached catch-up
        // writer owns storage. HF88/HF89 made the UI visible but could still
        // show a long "snapshot refresh" spinner because the reader was waiting
        // behind the writer. Keep the current local view and refresh immediately
        // when the catch-up epoch completes.
        if self.snapshot.height > 0 && hf85_catchup_running() && !self.hf86_force_quick_snapshot {
            let local = self.snapshot.height;
            let known = self.best_known_network_height();
            let elapsed = hf85_catchup_elapsed_secs();
            if known > local {
                self.status_line = format!(
                    "QUB Core catch-up writer active: local #{local} -> network #{known} ({} block(s) remaining). UI stays live; next snapshot waits for the writer. {elapsed}s elapsed",
                    known.saturating_sub(local)
                );
            } else {
                self.status_line = format!(
                    "QUB Core catch-up writer active: probing official/direct tips while local UI stays live. {elapsed}s elapsed"
                );
            }
            self.last_refresh = Instant::now();
            return;
        }
        if !hf88_try_begin_snapshot_worker() {
            // HF106: never queue repeated snapshot readers behind a still-running
            // reader. Keep the existing local UI and let the next completed
            // catch-up/snapshot epoch trigger a fresh render.
            self.last_refresh = Instant::now();
            if self.status_line.to_lowercase().contains("snapshot") || self.status_line.to_lowercase().contains("refresh") {
                // Avoid noisy loops: do not overwrite an already useful status.
            } else {
                self.status_line = "QUB Core snapshot worker is already finishing; local UI remains live and no extra reader will be queued.".to_string();
            }
            return;
        }
        // HF106: snapshots are UI reads only. They never run network catch-up and
        // never wait behind a previous abandoned read. The worker gate above
        // prevents the repeated 45s release loop seen in HF87.
        let config_path = self.prefs.config_path.clone();
        let payout_address = self.prefs.payout_address.clone();
        let startup_sync = self.initial_loading;
        let startup_snapshot = startup_sync || self.snapshot.height == 0;
        let force_quick_snapshot = self.hf86_force_quick_snapshot;
        self.hf86_force_quick_snapshot = false;
        let _ = force_quick_snapshot;
        let (tx, rx) = mpsc::channel();
        self.snapshot_rx = Some(rx);
        self.snapshot_in_flight = true;
        self.last_refresh = Instant::now();
        if startup_sync {
            self.status_line = "Startup: loading local wallet/blocks first; network catch-up starts only after the local view is visible...".to_string();
        }
        thread::spawn(move || {
            let result = std::panic::catch_unwind(|| -> Result<ChainSnapshot> {
                if startup_snapshot {
                    read_snapshot_for_payout_startup(&config_path, &payout_address)
                } else {
                    read_snapshot_for_payout_quick(&config_path, &payout_address)
                }
            })
            .unwrap_or_else(|_| Err(anyhow::anyhow!("QUB Core snapshot worker panicked")))
            .map_err(|err| format!("{err:#}"));
            hf88_finish_snapshot_worker();
            let _ = tx.send(result);
        });
    }

    fn start_tx_status_check(&mut self, txid: String) {
        if self.tx_status_in_flight || txid.trim().is_empty() { return; }
        let config_path = self.prefs.config_path.clone();
        let (tx, rx) = mpsc::channel();
        self.tx_status_rx = Some(rx);
        self.tx_status_in_flight = true;
        thread::spawn(move || {
            let id_for_result = txid.clone();
            let result = query_gui_tx_status(&config_path, &txid)
                .map(|status| (id_for_result, status))
                .map_err(|err| format!("{err:#}"));
            let _ = tx.send(result);
        });
    }

    fn poll_tx_status(&mut self) {
        let polled = self.tx_status_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(Ok((txid, status)))) => {
                self.tx_status_in_flight = false;
                self.tx_status_rx = None;
                self.apply_tx_status_result(&txid, status);
            }
            Some(Ok(Err(err))) => {
                self.tx_status_in_flight = false;
                self.tx_status_rx = None;
                self.apply_tx_status_error(err);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.tx_status_in_flight = false;
                self.tx_status_rx = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    fn apply_tx_status_error(&mut self, err: String) {
        let msg = format!("Status check warning: {err}");
        if self.send_dialog.status == SendDialogStatus::Pending { self.send_dialog.message = msg.clone(); }
        if self.conversion_dialog.status == SendDialogStatus::Pending { self.conversion_dialog.message = msg.clone(); }
        if self.buy_jin_dialog.status == SendDialogStatus::Pending { self.buy_jin_dialog.message = msg.clone(); }
        if self.pool_dialog.status == SendDialogStatus::Pending { self.pool_dialog.message = msg.clone(); }
        if !self.library_dialog.pending_txid.is_empty() { self.library_dialog.message = msg.clone(); }
        if self.qns_dialog.status == QnsDialogStatus::Pending { self.qns_dialog.message = msg; }
    }

    fn apply_tx_status_result(&mut self, txid: &str, status: TxUiStatus) {
        if self.send_dialog.status == SendDialogStatus::Pending && self.send_dialog.txid == txid {
            match &status {
                TxUiStatus::Confirmed { height, confirmations } => {
                    self.send_dialog.status = SendDialogStatus::Confirmed;
                    self.send_dialog.last_checked_height = *height;
                    self.send_dialog.message = format!("Confirmed in block #{} with {} confirmation(s). Receiver balance should update after sync.", height, confirmations);
                    self.last_success = Some(format!("Transaction confirmed in block #{}.", height));
                    self.start_background_snapshot_refresh();
                }
                TxUiStatus::PendingMempool => {
                    self.send_dialog.message = format!("Pending in local mempool. relayed_to_peers={}. Waiting for a miner to include it in a block.", self.send_dialog.relayed_to_peers);
                }
                TxUiStatus::NotFound => {
                    self.send_dialog.message = "Transaction is not visible in the local active chain or mempool yet. QUB Core will keep checking in the background.".to_string();
                }
            }
        }

        if self.conversion_dialog.status == SendDialogStatus::Pending && self.conversion_dialog.txid == txid {
            match &status {
                TxUiStatus::Confirmed { height, confirmations } => {
                    self.conversion_dialog.status = SendDialogStatus::Confirmed;
                    self.conversion_dialog.last_checked_height = *height;
                    self.conversion_dialog.message = format!("Conversion request included in block #{} with {} confirmation(s). The Enjin Matrixchain token claim flow is handled by the bridge service after finality.", height, confirmations);
                    self.last_success = Some(format!("JIN conversion request confirmed in block #{}.", height));
                    self.start_background_snapshot_refresh();
                }
                TxUiStatus::PendingMempool => {
                    self.conversion_dialog.message = format!("Pending in local mempool. relayed_to_peers={}. Waiting for a miner to include it in a block.", self.conversion_dialog.relayed_to_peers);
                }
                TxUiStatus::NotFound => {
                    self.conversion_dialog.message = "Conversion request is not visible in the local active chain or mempool yet. QUB Core will keep checking in the background.".to_string();
                }
            }
        }


        if self.buy_jin_dialog.status == SendDialogStatus::Pending && self.buy_jin_dialog.txid == txid {
            match &status {
                TxUiStatus::Confirmed { height, confirmations } => {
                    self.buy_jin_dialog.status = SendDialogStatus::Confirmed;
                    self.buy_jin_dialog.message = format!("JIN purchase confirmed in block #{} with {} confirmation(s). Balance updates after sync.", height, confirmations);
                    self.last_success = Some(format!("JIN purchase confirmed in block #{}.", height));
                    self.buy_jin_dialog.loading = false;
                    self.buy_jin_dialog.listings.clear();
                    self.start_background_snapshot_refresh();
                }
                TxUiStatus::PendingMempool => {
                    self.buy_jin_dialog.message = format!("JIN purchase pending in local mempool. Relayed to {} peer(s). Waiting for confirmation.", self.buy_jin_dialog.relayed_to_peers);
                }
                TxUiStatus::NotFound => {
                    self.buy_jin_dialog.message = "JIN purchase is not visible locally yet. QUB Core will keep checking in the background.".to_string();
                }
            }
        }

        if self.qns_dialog.status == QnsDialogStatus::Pending && self.qns_dialog.txid == txid {
            match &status {
                TxUiStatus::Confirmed { height, confirmations } => {
                    self.qns_dialog.status = QnsDialogStatus::Confirmed;
                    self.qns_dialog.message = format!("Confirmed in block #{} with {} confirmation(s). The name resolves after sync.", height, confirmations);
                    self.start_background_snapshot_refresh();
                }
                TxUiStatus::PendingMempool => {
                    self.qns_dialog.message = format!("Pending in local mempool. relayed_to_peers={}. Waiting for a miner to include it in a block.", self.qns_dialog.relayed_to_peers);
                }
                TxUiStatus::NotFound => {
                    self.qns_dialog.message = "Registration tx is not visible in the local active chain or mempool yet. QUB Core will keep checking in the background.".to_string();
                }
            }
        }

        if self.pool_dialog.status == SendDialogStatus::Pending && self.pool_dialog.txid == txid {
            match &status {
                TxUiStatus::Confirmed { height, confirmations } => {
                    self.pool_dialog.status = SendDialogStatus::Confirmed;
                    self.pool_dialog.last_checked_height = *height;
                    self.pool_dialog.message = format!("Pool {} confirmed in block #{} with {} confirmation(s).", self.pool_dialog.action, height, confirmations);
                    self.last_success = Some(self.pool_dialog.message.clone());
                    self.start_background_snapshot_refresh();
                }
                TxUiStatus::PendingMempool => {
                    self.pool_dialog.message = format!("Pool {} pending in local mempool. Relayed to {} peer(s). Waiting for confirmation.", self.pool_dialog.action, self.pool_dialog.relayed_to_peers);
                }
                TxUiStatus::NotFound => {
                    self.pool_dialog.message = "Pool transaction/share is not visible in the active chain or local mempool yet. QUB Core will keep checking.".to_string();
                }
            }
        }

        if !self.library_dialog.pending_txid.is_empty() && self.library_dialog.pending_txid == txid {
            match &status {
                TxUiStatus::Confirmed { height, confirmations } => {
                    self.library_dialog.message = format!("Library transaction confirmed in block #{} with {} confirmation(s).", height, confirmations);
                    self.library_dialog.pending_txid.clear();
                    self.library_state_cache = None;
                    self.library_state_last_loaded = None;
                    self.last_success = Some(format!("Library action confirmed in block #{}.", height));
                    self.start_background_snapshot_refresh();
                }
                TxUiStatus::PendingMempool => {
                    self.library_dialog.message = format!("Library pending tx {} is in local mempool. Waiting for a miner to include it in a block.", shorten_hash(txid));
                }
                TxUiStatus::NotFound => {
                    self.library_dialog.message = format!("Library tx {} is not visible locally yet. QUB Core will keep checking in the background.", shorten_hash(txid));
                }
            }
        }
    }


    fn start_wallet_sync(&mut self, force: bool) {
        if self.wallet_sync_in_flight {
            if force && self.last_wallet_sync.elapsed() > Duration::from_secs(12) {
                // HF106: release stale UI worker; catch-up pulses are detached and
                // single-flight at the GUI layer, so Sync cannot freeze the wallet.
                self.wallet_sync_rx = None;
                self.wallet_sync_in_flight = false;
            } else {
                return;
            }
        }
        if !force {
            if !self.prefs.auto_sync_wallet_balances { return; }
            let interval = self.prefs.auto_sync_wallet_interval_secs.clamp(5, 600);
            if self.last_wallet_sync.elapsed() < Duration::from_secs(interval) { return; }
        }

        let config_path = self.prefs.config_path.clone();
        let payout_address = self.prefs.payout_address.clone();
        let (tx, rx) = mpsc::channel();
        self.wallet_sync_rx = Some(rx);
        self.wallet_sync_in_flight = true;
        self.last_wallet_sync = Instant::now();
        if force {
            self.status_line = "Starting HF110 deep official Sync/Repair pulse; wallet UI stays live and mining will wait for canonical green-light...".to_string();
        }
        spawn_hf85_catchup_pulse(config_path.clone(), force);

        thread::spawn(move || {
            let result = (|| -> Result<ChainSnapshot> {
                // Return a local snapshot immediately. The detached HF105 catch-up
                // pulse updates chain.json; this or the next snapshot refresh will
                // show the new height without blocking balances/activity.
                read_snapshot_for_payout_quick(&config_path, &payout_address)
            })().map_err(|err| format!("{err:#}"));
            let _ = tx.send(result);
        });
    }

    fn poll_wallet_sync(&mut self) {
        let polled = self.wallet_sync_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(Ok(snapshot))) => {
                self.wallet_sync_in_flight = false;
                self.wallet_sync_rx = None;
                self.apply_snapshot(snapshot);
                self.status_line = "Wallet balances synced.".to_string();
            }
            Some(Ok(Err(err))) => {
                self.wallet_sync_in_flight = false;
                self.wallet_sync_rx = None;
                self.last_error = Some(format!("Wallet sync failed: {err}"));
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.wallet_sync_in_flight = false;
                self.wallet_sync_rx = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {
                if self.wallet_sync_in_flight {
                    let elapsed = self.last_wallet_sync.elapsed().as_secs();
                    if elapsed > 45 {
                        // HF106: do not let a stale worker make the Sync button spin forever.
                        self.wallet_sync_in_flight = false;
                        self.wallet_sync_rx = None;
                        self.last_error = Some("QUB Core Sync/Repair is taking longer than expected. The UI was released; HF110 deep official repair keeps retrying in the background.".to_string());
                    } else {
                        self.status_line = format!("QUB Core Sync pulse: local wallet view is live; fetching missing blocks... {elapsed}s elapsed");
                    }
                }
            }
        }
    }

    fn handle_snapshot_tip_change(&mut self, snapshot: &ChainSnapshot) {
        if self.last_observed_tip_hash.is_empty() || snapshot.best_hash == self.last_observed_tip_hash {
            return;
        }

        let mut local_tip_is_ours = false;
        let mut play_local_confirm_sound = false;
        let mut clear_last_block_card = false;
        if let Some(card) = &mut self.last_block_card {
            if card.at.elapsed() < Duration::from_secs(LOCAL_BLOCK_ROLLBACK_WATCH_SECS) {
                if let Some(active_at_height) = snapshot.recent_blocks.iter().find(|block| block.height == card.height) {
                    if active_at_height.hash != card.hash {
                        self.last_error = Some(format!(
                            "Local candidate #{} was rolled back by the active chain. This is a normal orphan/stale-block race; no reward was credited.",
                            card.height
                        ));
                        self.status_line = "Local candidate became stale; following the network-selected chain.".to_string();
                        clear_last_block_card = true;
                    } else {
                        local_tip_is_ours = true;
                        let confirmations = snapshot.height.saturating_sub(card.height).saturating_add(1).max(1);
                        card.confirmations = confirmations;
                        // Mark confirmed only after at least one later block builds on top of our candidate.
                        // This avoids playing the mined sound at local acceptance time and reduces stale-race false positives.
                        let known_network_ahead = snapshot.known_network_height > snapshot.height.saturating_add(1);
                        if confirmations >= 2 && !card.confirmed && !known_network_ahead {
                            card.confirmed = true;
                            play_local_confirm_sound = true;
                            self.last_success = Some(format!(
                                "Your mined block #{} is confirmed on the active chain with {} confirmation(s). Reward remains immature until coinbase maturity.",
                                card.height, confirmations
                            ));
                            self.status_line = format!("Your mined block #{} is confirmed on the active chain.", card.height);
                        } else if confirmations >= 2 && known_network_ahead {
                            self.status_line = format!(
                                "QUB Core finality guard: local mined block #{} has local confirmations, but known network tip is ahead (#{} vs local #{}). Keeping it Pending Decision until catch-up confirms it on the active chain.",
                                card.height, snapshot.known_network_height, snapshot.height
                            );
                        }
                    }
                }
            }
        }
        if clear_last_block_card {
            self.last_block_card = None;
        }
        if play_local_confirm_sound && self.prefs.sound_enabled {
            play_block_sound();
        }

        let Some(tip) = snapshot.recent_blocks.first() else { return; };
        let local_tip = local_tip_is_ours
            || is_local_block(tip, &self.prefs.payout_address)
            || self.last_local_mined_at.map(|t| t.elapsed() < Duration::from_secs(8)).unwrap_or(false);
        if !local_tip && self.prefs.network_sound_enabled {
            play_network_mined_sound();
        }
    }

    fn set_mining_phase(&mut self, phase: MiningPhase) {
        if self.mining_phase == phase { return; }
        self.mining_phase = phase;
        self.mining_phase_started_at = Some(Instant::now());
        if phase == MiningPhase::Mining {
            self.mining_on_next_sound_at = Some(Instant::now());
        } else {
            self.mining_on_next_sound_at = None;
        }
    }

    fn poll_mining_loop_sound(&mut self) {
        if self.mining_phase != MiningPhase::Mining || !self.prefs.mining_loop_sound_enabled {
            return;
        }
        let now = Instant::now();
        let Some(next) = self.mining_on_next_sound_at else {
            self.mining_on_next_sound_at = Some(now);
            return;
        };
        if now >= next {
            play_mining_loop_tick_sound();
            self.mining_on_next_sound_at = Some(now + Duration::from_secs(MINING_ON_SOUND_LOOP_SECS));
        }
    }

    fn update_runtime_identity(&mut self) {
        if let Ok(settings) = load_gui_settings(&self.prefs.config_path) {
            let _ = p2p::set_runtime_miner_address(&settings, self.prefs.payout_address.trim());
        }
    }

    fn complete_setup_wizard(&mut self) {
        match write_wizard_config(
            &self.prefs.setup_profile,
            &self.prefs.setup_bootnodes,
            &self.prefs.setup_advertise_addr,
            self.prefs.setup_listen_for_peers,
            self.prefs.setup_seed_node_mode,
        ) {
            Ok(path) => {
                self.prefs.config_path = path;
                self.prefs.setup_complete = true;
                self.prefs_dirty = true;
                self.status_line = format!("Setup complete: {}", self.prefs.setup_profile.label());
                self.last_success = Some("QUB Core is configured. Create/paste a payout address and start mining when ready.".to_string());
                self.start_background_snapshot_refresh();
                self.update_runtime_identity();
                // HF106: delay embedded P2P/catch-up until the first local snapshot
                // has rendered. Fresh installs must never show #0/no wallet because
                // a network worker grabbed storage before the UI reader.

            }
            Err(err) => self.last_error = Some(format!("Setup failed: {err:#}")),
        }
    }

    fn save_prefs_if_needed(&mut self) {
        if self.prefs_dirty {
            if let Err(err) = save_gui_prefs(&self.prefs) {
                self.last_error = Some(format!("Could not save GUI settings: {err:#}"));
            }
            self.prefs_dirty = false;
        }
    }

    fn ensure_p2p_node_started(&mut self) {
        if self.p2p_node_started { return; }
        match load_gui_settings(&self.prefs.config_path) {
            Ok(settings) if settings.p2p.enabled => {
                let (tx, rx) = mpsc::channel();
                thread::spawn(move || {
                    if let Err(err) = p2p::run_node(settings) {
                        let _ = tx.send(format!("Embedded P2P listener stopped: {err:#}"));
                    }
                });
                self.p2p_node_rx = Some(rx);
                self.p2p_node_started = true;
                self.status_line = "Embedded P2P node starting...".to_string();
            }
            Ok(_) => {}
            Err(err) => { self.last_error = Some(format!("Could not start embedded P2P node: {err:#}")); }
        }
    }

    fn poll_p2p_node(&mut self) {
        let polled = self.p2p_node_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(msg)) => {
                self.last_error = Some(msg);
                self.p2p_node_rx = None;
                self.p2p_node_started = false;
                self.status_line = "QUB Core P2P listener/outbound worker stopped; QUB Core will retry after the next local snapshot.".to_string();
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {
                if self.wallet_sync_in_flight {
                    let elapsed = self.last_wallet_sync.elapsed().as_secs();
                    self.status_line = format!("Syncing wallet balances and chain state... {elapsed}s elapsed");
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => { self.p2p_node_rx = None; }
        }
    }

    fn start_update_check(&mut self, force: bool) {
        if self.update_check_in_flight { return; }
        self.normalize_update_prefs_for_network();
        if !force {
            if !self.prefs.setup_complete || !self.prefs.auto_check_updates || !self.prefs.auto_download_updates {
                return;
            }
        }
        let url = self.prefs.update_url.trim().to_string();
        if url.is_empty() { return; }
        self.update_check_in_flight = true;
        self.update_check_forced = force;
        self.update_dialog.status = UpdateStatus::Checking;
        self.update_dialog.message = format!("Checking {} for QUB Core updates...", url);
        let last_sig = self.update_dialog.remote_signature.clone();
        let current_version = APP_VERSION.to_string();
        let allow_unsigned_update = self.snapshot.network.eq_ignore_ascii_case("testnet")
            || (self.is_mainnet_network() && ALLOW_UNSIGNED_MAINNET_UPDATES_PRIVATE_BUILD);
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let event = windows_check_and_stage_update(&url, &current_version, &last_sig, allow_unsigned_update)
                .unwrap_or_else(|err| UpdateEvent::Failed(format!("{err:#}")));
            let _ = tx.send(event);
        });
        self.update_rx = Some(rx);
        self.last_update_check = Instant::now();
    }

    fn poll_updates(&mut self) {
        let polled = self.update_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(UpdateEvent::UpToDate { signature, message })) => {
                self.update_check_in_flight = false;
                self.update_check_forced = false;
                self.update_dialog.remote_signature = signature;
                if self.update_dialog.status != UpdateStatus::Ready {
                    self.update_dialog.status = UpdateStatus::UpToDate;
                    self.update_dialog.message = message;
                }
                self.update_rx = None;
            }
            Some(Ok(UpdateEvent::Ready { version, installer_path, signature, message })) => {
                self.update_check_in_flight = false;
                self.update_check_forced = false;
                self.update_dialog.status = UpdateStatus::Ready;
                self.update_dialog.latest_version = version.clone();
                self.update_dialog.installer_path = installer_path;
                self.update_dialog.remote_signature = signature;
                self.update_dialog.message = message;
                self.update_dialog.open = true;
                if self.prefs.stop_miner_on_update_available && self.miner.is_some() {
                    self.stop_mining();
                    self.last_error = Some(format!("QUB Core {} is ready. Mining was stopped until you install the update.", version));
                }
                self.status_line = format!("Update {} ready. Install and restart from the Updates window.", version);
                if self.prefs.auto_install_updates {
                    self.update_dialog.auto_install_at = Some(Instant::now() + Duration::from_secs(UPDATE_INSTALL_COUNTDOWN_SECS));
                    self.update_dialog.message = format!("{} Installing automatically in {} seconds unless you click Later.", self.update_dialog.message, UPDATE_INSTALL_COUNTDOWN_SECS);
                }
                self.update_rx = None;
            }
            Some(Ok(UpdateEvent::Failed(err))) => {
                let was_forced = self.update_check_forced;
                self.update_check_in_flight = false;
                self.update_check_forced = false;
                self.update_dialog.status = UpdateStatus::Failed;
                self.update_dialog.message = err.clone();
                self.last_error = Some(format!("Update check failed: {err}"));
                if was_forced { self.update_dialog.open = true; }
                self.update_rx = None;
            }

            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.update_check_in_flight = false;
                self.update_check_forced = false;
                self.update_rx = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {
                if self.wallet_sync_in_flight {
                    let elapsed = self.last_wallet_sync.elapsed().as_secs();
                    self.status_line = format!("Syncing wallet balances and chain state... {elapsed}s elapsed");
                }
            }
        }

        if self.prefs.setup_complete && !self.update_check_in_flight && self.last_update_check.elapsed() >= Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS) {
            self.start_update_check(false);
        }

        if self.update_dialog.status == UpdateStatus::Ready {
            if let Some(at) = self.update_dialog.auto_install_at {
                if Instant::now() >= at {
                    let _ = self.install_downloaded_update();
                }
            }
        }
    }

    fn install_downloaded_update(&mut self) -> Result<()> {
        #[cfg(not(target_os = "windows"))]
        {
            anyhow::bail!("Windows self-update is only available on Windows builds")
        }
        #[cfg(target_os = "windows")]
        {
            let installer = PathBuf::from(self.update_dialog.installer_path.trim());
            if !installer.exists() {
                anyhow::bail!("downloaded installer not found: {}", installer.display());
            }
            self.update_dialog.status = UpdateStatus::Installing;
            self.update_dialog.message = format!("Installing QUB Core {} and restarting...", self.update_dialog.latest_version);
            self.stop_mining();
            // HF106: always mark this relaunch as an update restart so the
            // dedicated post-update mining policy overrides normal app-open auto mining.
            let _ = write_hf86_post_update_restart_marker();
            self.save_prefs_if_needed();
            let current_exe = std::env::current_exe().context("current exe unavailable")?;
            let app_dir = current_exe.parent().context("missing current exe dir")?;
            let config_path = resolve_app_read_path(&self.prefs.config_path);
            let launcher = app_write_path("data/updater/install-and-relaunch.cmd");
            if let Some(parent) = launcher.parent() { std::fs::create_dir_all(parent)?; }
            let script = format!(r#"@echo off
setlocal
ping 127.0.0.1 -n 3 >nul
start /wait "" "{}" /SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /FORCECLOSEAPPLICATIONS /DIR="{}"
start "" "{}" --config "{}"
del "%~f0"
"#,
                installer.display(),
                app_dir.display(),
                current_exe.display(),
                config_path.display(),
            );
            std::fs::write(&launcher, script)?;
            let mut command = Command::new("cmd.exe");
            command.args(["/C", launcher.to_string_lossy().as_ref()]);
            hide_command_window(&mut command);
            command.spawn().context("spawn updater launcher")?;
            std::process::exit(0);
        }
    }

    fn create_wallet_address(&mut self) {
        if self.wallet_create_in_flight { return; }
        let config_path = self.prefs.config_path.clone();
        let accepted = self.prefs.confirm_plaintext_wallet_risk;
        let (tx, rx) = mpsc::channel();
        self.wallet_create_rx = Some(rx);
        self.wallet_create_in_flight = true;
        self.status_line = "Creating local wallet address in background...".to_string();
        thread::spawn(move || {
            let result = create_local_wallet_address(&config_path, accepted)
                .map_err(|err| format!("{err:#}"));
            let _ = tx.send(result);
        });
    }

    fn poll_wallet_create(&mut self) {
        let polled = self.wallet_create_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(Ok(address))) => {
                self.wallet_create_in_flight = false;
                self.wallet_create_rx = None;
                self.prefs.payout_address = address.clone();
                self.prefs_dirty = true;
                self.update_runtime_identity();
                self.last_success = Some(format!("Created local mining address: {address}"));
                self.status_line = "Local v1 mining address created.".to_string();
                self.start_background_snapshot_refresh();
            }
            Some(Ok(Err(err))) => {
                self.wallet_create_in_flight = false;
                self.wallet_create_rx = None;
                self.last_error = Some(format!("Could not create address: {err}"));
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.wallet_create_in_flight = false;
                self.wallet_create_rx = None;
                self.last_error = Some("Address worker disconnected.".to_string());
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    fn start_send_qns_resolve(&mut self, recipient: String) {
        if self.send_qns_resolve_in_flight { return; }
        let config_path = self.prefs.config_path.clone();
        let (tx, rx) = mpsc::channel();
        self.send_qns_resolve_rx = Some(rx);
        self.send_qns_resolve_in_flight = true;
        self.send_dialog.message = format!("Resolving {recipient} in background...");
        thread::spawn(move || {
            let result = resolve_recipient_for_gui(&config_path, &recipient)
                .map(|address| (recipient, address));
            let _ = tx.send(result);
        });
    }

    fn poll_send_qns_resolve(&mut self) {
        let polled = self.send_qns_resolve_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(Ok((recipient, address)))) => {
                self.send_qns_resolve_in_flight = false;
                self.send_qns_resolve_rx = None;
                if self.send_dialog.recipient.trim().eq_ignore_ascii_case(recipient.trim()) {
                    self.send_dialog.resolved_address = address;
                    self.send_dialog.message.clear();
                } else {
                    self.send_dialog.message = format!("Resolved {recipient}, but the recipient field changed. Click Resolve QNS again if needed.");
                }
            }
            Some(Ok(Err(err))) => {
                self.send_qns_resolve_in_flight = false;
                self.send_qns_resolve_rx = None;
                self.send_dialog.message = format!("QNS resolve failed: {err}");
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.send_qns_resolve_in_flight = false;
                self.send_qns_resolve_rx = None;
                self.send_dialog.message = "QNS resolve worker disconnected.".to_string();
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    fn start_multi_qns_resolve(&mut self, row_idx: usize, recipient: String) {
        if self.multi_qns_resolve_rx.is_some() { return; }
        if row_idx >= self.send_dialog.multi_rows.len() { return; }
        let config_path = self.prefs.config_path.clone();
        let (tx, rx) = mpsc::channel();
        self.multi_qns_resolve_rx = Some(rx);
        if let Some(row) = self.send_dialog.multi_rows.get_mut(row_idx) {
            row.resolving = true;
            row.resolve_message = format!("Resolving {}...", recipient.trim());
            row.resolved_address.clear();
        }
        thread::spawn(move || {
            let result = resolve_recipient_for_gui(&config_path, &recipient);
            let _ = tx.send((row_idx, recipient, result));
        });
    }

    fn poll_multi_qns_resolve(&mut self) {
        let polled = self.multi_qns_resolve_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok((idx, original, Ok(address)))) => {
                self.multi_qns_resolve_rx = None;
                if let Some(row) = self.send_dialog.multi_rows.get_mut(idx) {
                    row.resolving = false;
                    if row.recipient.trim().eq_ignore_ascii_case(original.trim()) {
                        row.resolved_address = address.clone();
                        row.recipient = address;
                        row.resolve_message = "Resolved".to_string();
                    } else {
                        row.resolve_message = format!("Resolved {}, but row changed.", original.trim());
                    }
                }
            }
            Some(Ok((idx, _original, Err(err)))) => {
                self.multi_qns_resolve_rx = None;
                if let Some(row) = self.send_dialog.multi_rows.get_mut(idx) {
                    row.resolving = false;
                    row.resolve_message = format!("Resolve failed: {err}");
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.multi_qns_resolve_rx = None;
                for row in &mut self.send_dialog.multi_rows {
                    if row.resolving {
                        row.resolving = false;
                        row.resolve_message = "Resolve worker disconnected.".to_string();
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    fn open_import_key_dialog(&mut self) {
        self.import_key_dialog.open = true;
        self.import_key_dialog.message.clear();
        self.import_key_dialog.success = false;
        if self.import_key_dialog.label.trim().is_empty() { self.import_key_dialog.label = "imported-gui-key".to_string(); }
    }

    fn import_private_key_from_dialog(&mut self) {
        if !self.prefs.confirm_plaintext_wallet_risk {
            self.import_key_dialog.message = "Confirm the plaintext wallet risk checkbox first.".to_string();
            self.import_key_dialog.success = false;
            return;
        }
        match import_private_key_hex(&self.prefs.config_path, &self.import_key_dialog.secret_key_hex, &self.import_key_dialog.label) {
            Ok(address) => {
                self.import_key_dialog.success = true;
                self.import_key_dialog.message = format!("Imported private key for address {address}");
                self.prefs.payout_address = address.clone();
                self.prefs_dirty = true;
                self.update_runtime_identity();
                self.start_background_snapshot_refresh();
            }
            Err(err) => {
                self.import_key_dialog.success = false;
                self.import_key_dialog.message = format!("Import failed: {err:#}");
            }
        }
    }

    fn delete_local_private_keys(&mut self) {
        if self.miner.is_some() {
            self.last_error = Some("Stop mining before deleting local private keys.".to_string());
            return;
        }
        if !self.delete_private_key_confirm {
            self.last_error = Some("Confirm the private-key deletion warning first.".to_string());
            return;
        }
        match delete_local_wallet_private_keys(&self.prefs.config_path) {
            Ok(deleted) => {
                self.delete_private_key_confirm = false;
                self.last_success = Some(format!("Deleted {deleted} local private key(s) from wallet.json. Payout address text was left unchanged."));
                self.status_line = "Local private keys deleted.".to_string();
                self.start_background_snapshot_refresh();
            }
            Err(err) => self.last_error = Some(format!("Could not delete local private keys: {err:#}")),
        }
    }

    fn start_mining(&mut self) {
        if self.update_dialog.status == UpdateStatus::Ready || self.update_dialog.status == UpdateStatus::Installing {
            self.last_error = Some("A newer QUB Core version is pending. Install the update before mining again.".to_string());
            self.status_line = "Update required before mining resumes.".to_string();
            return;
        }
        if self.miner.is_some() {
            return;
        }
        self.pool_mining_pool_id.clear();
        self.desired_pool_mining_pool_id.clear();
        self.manual_stop_requested = false;
        self.auto_restart_mining_at = None;
        let config_path = self.prefs.config_path.clone();
        let payout = self.prefs.payout_address.trim().to_string();
        if payout.is_empty() {
            self.last_error = Some("Set a payout address first.".to_string());
            return;
        }
        self.update_runtime_identity();
        self.set_mining_phase(MiningPhase::Preparing);
        if let Err(message) = self.ensure_network_ready_to_mine() {
            self.last_error = Some(message);
            self.status_line = "Mining blocked until P2P is connected.".to_string();
            self.set_mining_phase(MiningPhase::Off);
            return;
        }
        let cpu_percent = self.prefs.cpu_percent.clamp(1, 100);
        let gpu_percent = self.prefs.gpu_percent.clamp(0, 100);
        let gpu_device_selector = self.normalized_gpu_device_selector();
        if gpu_percent > 0 {
            self.status_line = format!("OpenCL GPU mining enabled at {}% on {}. CPU mining remains active and every GPU-found block is CPU-verified before submit.", gpu_percent, self.gpu_selector_status_label());
        }
        self.gpu_hash_rate_hps = 0.0;
        self.gpu_total_hashes = 0;
        self.gpu_workers = 0;
        self.gpu_device.clear();
        self.gpu_device_rates.clear();
        self.prefs.pace_to_target_spacing = false;
        let pace_to_target_spacing = false;
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let join = thread::spawn(move || run_miner(config_path, payout, cpu_percent, gpu_percent, gpu_device_selector, pace_to_target_spacing, tx, thread_stop));
        self.miner = Some(MinerHandle { stop, rx, join: Some(join) });
        self.status_line = "Starting solo miner in background: running QUB Core bounded green-light catch-up, then preparing the first candidate...".to_string();
        self.last_success = None;
        self.last_error = None;
    }

    fn ensure_network_ready_to_mine(&mut self) -> std::result::Result<(), String> {
        let settings = load_gui_settings(&self.prefs.config_path)
            .map_err(|err| format!("Could not load config before mining: {err:#}"))?;
        if !settings.p2p.enabled { return Ok(()); }

        // HF51: never run slow P2P convergence on the UI thread. Start/Stop
        // buttons must stay responsive; the miner worker performs the strict
        // fork-safety guard and auto-retry in the background. This method only
        // performs cheap local checks and sets helpful status text.
        if self.prefs.allow_isolated_regtest_mining && settings.network.name.contains("regtest") {
            return Ok(());
        }

        let peers = p2p::known_peer_addrs(&settings).unwrap_or_default();
        if peers.is_empty() {
            self.status_line = "Mining queued: discovering peers. The background guard will sync before hashing.".to_string();
        } else {
            self.status_line = format!("Mining queued: {} peer candidate(s). Running fork-safety sync in background...", peers.len());
        }
        Ok(())
    }

    fn hf110_pause_mining_for_canonical_catchup(&mut self, gap: u32, stalled_secs: u64) {
        if self.miner.is_none() { return; }
        if self.hf110_last_mining_auto_pause.elapsed() < Duration::from_secs(45) { return; }

        let wanted_pool = if !self.desired_pool_mining_pool_id.trim().is_empty() {
            self.desired_pool_mining_pool_id.clone()
        } else if !self.pool_mining_pool_id.trim().is_empty() {
            self.pool_mining_pool_id.clone()
        } else {
            String::new()
        };

        if let Some(mut miner) = self.miner.take() {
            miner.stop();
            drop(miner);
        }

        self.hash_rate_hps = 0.0;
        self.gpu_hash_rate_hps = 0.0;
        self.gpu_total_hashes = 0;
        self.gpu_workers = 0;
        self.gpu_device_rates.clear();
        self.miner_threads = 0;
        self.miner_duty = 0;
        self.target_height = self.snapshot.height.saturating_add(1);
        self.set_mining_phase(MiningPhase::Off);
        self.manual_stop_requested = false;
        self.hf110_autoheal_paused_mining = true;
        self.hf110_last_mining_auto_pause = Instant::now();
        self.desired_pool_mining_pool_id = wanted_pool.clone();
        self.pool_mining_pool_id.clear();
        self.auto_restart_mining_at = Some(Instant::now() + Duration::from_secs(12));
        self.hf85_last_auto_catchup_sync = Instant::now() - Duration::from_secs(60);
        self.hf86_force_quick_snapshot = true;

        self.status_line = format!(
            "HF110 auto-heal paused mining for canonical catch-up: local #{} is {} block(s) behind official/direct tip (stalled {}s). Mining will resume automatically after catch-up.",
            self.snapshot.height, gap, stalled_secs
        );
        self.last_error = Some("Mining paused by HF110 auto-heal because QUB Core was behind the official/direct chain. This prevents local-only forks; no reinstall is needed.".to_string());
        spawn_hf85_catchup_pulse(self.prefs.config_path.clone(), true);
        self.start_wallet_sync(true);
    }

    fn stop_mining(&mut self) {
        self.manual_stop_requested = true;
        self.auto_restart_mining_at = None;
        self.desired_pool_mining_pool_id.clear();
        self.pool_mining_pool_id.clear();
        if let Some(mut miner) = self.miner.take() {
            // HF72/v1.5.8: Stop must be immediate from the user's perspective.
            // Signal all workers and drop/detach the handle now; do not wait for
            // the worker to finish a guard/snapshot/GPU round before releasing UI.
            miner.stop();
            drop(miner);
        }
        self.hash_rate_hps = 0.0;
        self.gpu_hash_rate_hps = 0.0;
        self.gpu_workers = 0;
        self.gpu_device_rates.clear();
        self.miner_threads = 0;
        self.miner_duty = 0;
        self.target_height = self.snapshot.height.saturating_add(1);
        self.set_mining_phase(MiningPhase::Off);
        self.status_line = "Mining stopped locally. Worker threads were signalled and will exit in the background.".to_string();
    }

    fn poll_miner_events(&mut self) {
        let mut should_drop_miner = false;
        let mut should_refresh_snapshot = false;
        let mut phase_change: Option<MiningPhase> = None;

        // HF78/v1.6.0 fixed: drain miner events while borrowing only the
        // receiver, then process the events after that borrow has ended. This
        // keeps GPU aggregate/status updates free to borrow `self` normally.
        let mut pending_events = Vec::new();
        if let Some(miner) = &mut self.miner {
            while let Ok(event) = miner.rx.try_recv() {
                pending_events.push(event);
            }
        }
        for event in pending_events {
            match event {
                    MinerEvent::Started { threads, duty, target_height } => {
                        self.miner_threads = threads;
                        self.miner_duty = duty;
                        self.target_height = target_height;
                        self.status_line = format!("Preparing block #{target_height} with {threads} CPU worker(s), duty {duty}%.");
                    }
                    MinerEvent::Hashrate { hps, total_hashes, threads, duty, target_height } => {
                        self.hash_rate_hps = hps;
                        self.total_hashes = total_hashes;
                        self.miner_threads = threads;
                        self.miner_duty = duty;
                        self.target_height = target_height;
                        if self.mining_phase == MiningPhase::Preparing { phase_change = Some(MiningPhase::Mining); }
                        let combined = hps + self.gpu_hash_rate_hps;
                        self.status_line = if self.gpu_workers > 0 {
                            format!("Mining block #{} at {} total (CPU {} + GPU {}) with {} CPU worker(s) + {} GPU lane(s).", target_height, format_hps(combined), format_hps(hps), format_hps(self.gpu_hash_rate_hps), threads, self.gpu_workers)
                        } else {
                            format!("Mining block #{} at {} with {} CPU worker(s).", target_height, format_hps(hps), threads)
                        };
                    }
                    MinerEvent::GpuStarted { device, workers, power, target_height } => {
                        if !device.is_empty() {
                            self.gpu_device_rates.entry(device.clone()).or_insert((0.0, 0, workers));
                            self.recompute_gpu_aggregate();
                        } else {
                            self.gpu_device = self.gpu_selector_status_label();
                            self.gpu_workers = workers;
                        }
                        self.target_height = target_height;
                        let active_lanes = if self.gpu_workers > 0 { self.gpu_workers } else { workers };
                        self.status_line = format!("OpenCL GPU active on {} at {}% power ({} total work item batch, high-performance full-SHA adaptive GPU auto-tune mode). CPU verification is enforced.", self.gpu_device, power, active_lanes);
                    }
                    MinerEvent::GpuHashrate { hps, total_hashes, workers, device, target_height } => {
                        if !device.is_empty() {
                            self.gpu_device_rates.insert(device, (hps, total_hashes, workers));
                            self.recompute_gpu_aggregate();
                        } else {
                            self.gpu_hash_rate_hps = hps;
                            self.gpu_total_hashes = total_hashes;
                            self.gpu_workers = workers;
                        }
                        self.target_height = target_height;
                    }
                    MinerEvent::GpuStatus(status) => {
                        self.status_line = status;
                    }
                    MinerEvent::BlockFound { height, hash, txs, reward } => {
                        if self.pool_mining_pool_id.trim().is_empty() {
                            let card = BlockCard { height, hash: hash.clone(), txs, reward: reward.clone(), at: Instant::now(), confirmed: false, confirmations: 0 };
                            self.last_block_card = Some(card.clone());
                            self.block_history.push_front(card);
                            while self.block_history.len() > BLOCK_HISTORY_LIMIT { self.block_history.pop_back(); }
                            self.last_local_mined_at = Some(Instant::now());
                            self.last_success = Some(format!("Your block #{height} was accepted locally and relayed. Waiting for active-chain confirmation. Reward candidate: {reward}."));
                            self.status_line = "Your block was accepted locally and relayed. Waiting for confirmation...".to_string();
                            // mined.mp3 is intentionally played only when the solo block is confirmed on the active chain.
                        } else {
                            self.last_block_card = None;
                            self.last_local_mined_at = None;
                            self.last_success = Some(format!("Pool block #{} was accepted locally and relayed. Rewards are split by confirmed pool shares; check Address Activity for your payout.", height));
                            self.status_line = "Pool block accepted locally and relayed. Pool rewards stay pending-decision until active-chain confirmation.".to_string();
                            // HF106: no success sound at pool-candidate local acceptance. The pool/payout reward
                            // may still lose the peer race, so sounds are reserved for confirmed active-chain state.
                        }
                        should_refresh_snapshot = true;
                    }
                    MinerEvent::BlockStale { height, hash, winner_hash } => {
                        self.last_error = Some(format!("Block candidate #{height} became stale immediately after sync. Local hash {} lost to active hash {}. No reward was credited.", shorten_hash(&hash), shorten_hash(&winner_hash)));
                        self.status_line = "Stale block race detected; following the network-selected chain.".to_string();
                        should_refresh_snapshot = true;
                    }
                    MinerEvent::Status(status) => self.status_line = status,
                    MinerEvent::Error(err) => {
                        // v1.5.2: worker/guard errors are not always fatal. Keep the
                        // last error visible but let the miner thread decide whether it
                        // actually stops. This prevents transient pool-candidate or
                        // peer-safety warnings from silently killing all miners.
                        self.last_error = Some(err.clone());
                        self.status_line = err;
                    }
                    MinerEvent::Stopped => {
                        self.status_line = "Miner stopped.".to_string();
                        should_drop_miner = true;
                    }
                }
        }
        self.update_hashrate_records();
        if let Some(phase) = phase_change {
            self.set_mining_phase(phase);
        }
        if should_drop_miner {
            let wanted_pool = self.desired_pool_mining_pool_id.clone();
            let should_auto_restart = !self.manual_stop_requested
                && self.update_dialog.status != UpdateStatus::Ready
                && self.update_dialog.status != UpdateStatus::Installing
                && !self.prefs.payout_address.trim().is_empty();
            self.miner = None;
            self.hash_rate_hps = 0.0;
            self.gpu_hash_rate_hps = 0.0;
            self.miner_threads = 0;
            self.gpu_workers = 0;
            self.miner_duty = 0;
            self.set_mining_phase(MiningPhase::Off);
            if should_auto_restart {
                if !wanted_pool.trim().is_empty() {
                    self.pool_mining_pool_id = wanted_pool.clone();
                    self.status_line = format!("Pool miner stopped unexpectedly; auto-restarting {} after sync...", shorten_hash(&wanted_pool));
                } else {
                    self.pool_mining_pool_id.clear();
                    self.status_line = "Solo miner stopped unexpectedly; auto-restarting after sync...".to_string();
                }
                self.auto_restart_mining_at = Some(Instant::now() + Duration::from_secs(3));
            } else {
                self.pool_mining_pool_id.clear();
                self.desired_pool_mining_pool_id.clear();
                self.manual_stop_requested = false;
                self.auto_restart_mining_at = None;
            }
        }
        if should_refresh_snapshot {
            self.start_background_snapshot_refresh();
        }
    }

    fn start_benchmark(&mut self) {
        if self.benchmark_running {
            return;
        }
        let config_path = self.prefs.config_path.clone();
        let payout = self.prefs.payout_address.trim().to_string();
        if payout.is_empty() {
            self.last_error = Some("Set a payout address before benchmarking.".to_string());
            return;
        }
        let seconds = self.prefs.benchmark_seconds.clamp(1, 30);
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = benchmark_hashing(&config_path, &payout, seconds);
            let _ = match result {
                Ok((hps, elapsed)) => tx.send(BenchmarkEvent::Done { hps, seconds: elapsed }),
                Err(err) => tx.send(BenchmarkEvent::Error(format!("Benchmark failed: {err:#}"))),
            };
        });
        self.benchmark_rx = Some(rx);
        self.benchmark_running = true;
        self.benchmark_result = None;
        self.status_line = "Benchmark running...".to_string();
    }

    fn poll_benchmark(&mut self) {
        let polled = self.benchmark_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(BenchmarkEvent::Done { hps, seconds })) => {
                self.benchmark_running = false;
                self.benchmark_rx = None;
                self.benchmark_result = Some(format!("{} over {:.1}s", format_hps(hps), seconds));
                self.status_line = "Benchmark complete.".to_string();
            }
            Some(Ok(BenchmarkEvent::Error(err))) => {
                self.benchmark_running = false;
                self.benchmark_rx = None;
                self.last_error = Some(err);
                self.status_line = "Benchmark failed.".to_string();
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {
                if self.wallet_sync_in_flight {
                    let elapsed = self.last_wallet_sync.elapsed().as_secs();
                    self.status_line = format!("Syncing wallet balances and chain state... {elapsed}s elapsed");
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.benchmark_running = false;
                self.benchmark_rx = None;
                self.last_error = Some("Benchmark worker disconnected.".to_string());
            }
        }
    }

    fn open_send_dialog(&mut self) {
        self.send_dialog.open = true;
        self.send_dialog.status = SendDialogStatus::Editing;
        self.send_dialog.message.clear();
        self.send_dialog.txid.clear();
        self.send_dialog.resolved_address.clear();
        self.send_dialog.relayed_to_peers = 0;
        self.send_dialog.last_checked_height = self.snapshot.height;
        self.send_dialog.last_relay_at = None;
        if self.send_dialog.fee.trim().is_empty() || self.send_dialog.fee.trim() == "0" {
            self.send_dialog.fee = default_fee_for_config(&self.prefs.config_path).unwrap_or_else(|| "0.00001".to_string());
        }
        if self.send_dialog.amount.trim().is_empty() {
            self.send_dialog.amount = "0".to_string();
        }
        if self.send_dialog.multi_rows.is_empty() {
            self.send_dialog.multi_rows.push(MultiSendRow::default());
        }
    }

    fn open_conversion_dialog(&mut self) {
        self.conversion_dialog.open = true;
        self.conversion_dialog.status = SendDialogStatus::Editing;
        self.conversion_dialog.message.clear();
        self.conversion_dialog.txid.clear();
        self.conversion_dialog.relayed_to_peers = 0;
        if self.conversion_dialog.fee.trim().is_empty() || self.conversion_dialog.fee.trim() == "0" {
            self.conversion_dialog.fee = "0.001".to_string();
        }
        if self.conversion_dialog.fee_asset.trim().is_empty() {
            self.conversion_dialog.fee_asset = "JIN".to_string();
        }
    }

    fn start_conversion_dialog(&mut self) {
        if self.snapshot.wallet_keys == 0 {
            self.conversion_dialog.status = SendDialogStatus::Failed;
            self.conversion_dialog.message = "No local wallet key is available. Create/import a local wallet first, then retry.".to_string();
            return;
        }
        let config_path = self.prefs.config_path.clone();
        let matrix_address = self.conversion_dialog.matrix_address.trim().to_string();
        let amount = self.conversion_dialog.amount.trim().to_string();
        let fee = self.conversion_dialog.fee.trim().to_string();
        let fee_asset = self.conversion_dialog.fee_asset.trim().to_ascii_uppercase();
        if matrix_address.is_empty() || amount.is_empty() || fee.is_empty() {
            self.conversion_dialog.status = SendDialogStatus::Failed;
            self.conversion_dialog.message = "Matrix address, amount, and fee are required.".to_string();
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.conversion_rx = Some(rx);
        self.conversion_dialog.status = SendDialogStatus::Sending;
        self.conversion_dialog.message = "Creating JIN Coin -> JIN Token conversion request...".to_string();
        self.conversion_dialog.txid.clear();
        self.conversion_dialog.relayed_to_peers = 0;
        thread::spawn(move || {
            match execute_gui_jin_token_conversion(&config_path, &matrix_address, &amount, &fee, &fee_asset) {
                Ok((txid, relayed_to_peers, local_mempooltx)) => {
                    let _ = tx.send(SendEvent::Created { txid, relayed_to_peers, local_mempooltx });
                }
                Err(err) => { let _ = tx.send(SendEvent::Failed(format!("{err:#}"))); }
            }
        });
    }

    fn poll_conversion_dialog(&mut self) {
        let polled = self.conversion_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(SendEvent::Created { txid, relayed_to_peers, local_mempooltx })) => {
                self.conversion_dialog.status = SendDialogStatus::Pending;
                self.conversion_dialog.txid = txid.clone();
                self.conversion_dialog.relayed_to_peers = relayed_to_peers;
                self.conversion_dialog.message = format!("Conversion request created, added to local mempool, and relayed to {relayed_to_peers} peer(s). Local mempool tx: {local_mempooltx}. Waiting for block confirmation.");
                self.status_line = "JIN Coin -> Token conversion request is pending confirmation.".to_string();
                self.last_success = Some(format!("Conversion request pending: {}", shorten_hash(&txid)));
                self.conversion_rx = None;
                self.start_background_snapshot_refresh();
            }
            Some(Ok(SendEvent::Failed(err))) => {
                self.conversion_dialog.status = SendDialogStatus::Failed;
                self.conversion_dialog.message = err.clone();
                self.last_error = Some(format!("JIN conversion failed: {err}"));
                self.conversion_rx = None;
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.conversion_dialog.status = SendDialogStatus::Failed;
                self.conversion_dialog.message = "Conversion worker disconnected.".to_string();
                self.conversion_rx = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }

        if self.conversion_dialog.status == SendDialogStatus::Pending
            && !self.conversion_dialog.txid.is_empty()
            && self.last_send_status_poll.elapsed() >= Duration::from_secs(5)
        {
            self.last_send_status_poll = Instant::now();
            self.start_tx_status_check(self.conversion_dialog.txid.clone());
        }
    }

    fn open_send_dialog_for(&mut self, asset: &str) {
        self.send_dialog.asset = asset.to_ascii_uppercase();
        if self.send_dialog.asset == "JIN" {
            self.send_dialog.fee_asset = "JIN".to_string();
            if self.send_dialog.fee.trim().is_empty() || self.send_dialog.fee.trim() == "0" {
                self.send_dialog.fee = "0.001".to_string();
            }
        } else {
            self.send_dialog.fee_asset = "QUB".to_string();
            if self.send_dialog.fee.trim().is_empty() || self.send_dialog.fee.trim() == "0" {
                self.send_dialog.fee = default_fee_for_config(&self.prefs.config_path).unwrap_or_else(|| "0.00001".to_string());
            }
        }
        self.open_send_dialog();
    }

    fn start_send_dialog(&mut self) {
        if self.snapshot.wallet_keys == 0 {
            self.send_dialog.status = SendDialogStatus::Failed;
            self.send_dialog.message = "No local wallet key is available. Create/import a local wallet first, then retry.".to_string();
            return;
        }
        let mode = self.send_dialog.send_mode;
        let config_path = self.prefs.config_path.clone();
        let fee = self.send_dialog.fee.trim().to_string();
        let asset = self.send_dialog.asset.trim().to_ascii_uppercase();
        let fee_asset = self.send_dialog.fee_asset.trim().to_ascii_uppercase();

        let work = match mode {
            SendMode::Single => {
                let recipient = self.send_dialog.recipient.trim().to_string();
                let amount = self.send_dialog.amount.trim().to_string();
                if recipient.is_empty() || amount.is_empty() || fee.is_empty() {
                    self.send_dialog.status = SendDialogStatus::Failed;
                    self.send_dialog.message = "Recipient, amount, and fee are required.".to_string();
                    return;
                }
                SendWork::Single { recipient, amount, fee, asset, fee_asset }
            }
            SendMode::Multi => {
                let entries = self.send_dialog.multi_rows.iter()
                    .map(|row| (row.recipient.trim(), row.amount.trim()))
                    .filter(|(recipient, amount)| !recipient.is_empty() || !amount.is_empty())
                    .map(|(recipient, amount)| format!("{recipient},{amount}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.send_dialog.multi_entries = entries.clone();
                if entries.trim().is_empty() || fee.is_empty() {
                    self.send_dialog.status = SendDialogStatus::Failed;
                    self.send_dialog.message = "Add at least one multi-send row with recipient and amount, and set a fee.".to_string();
                    return;
                }
                if self.send_dialog.multi_rows.iter().filter(|row| !row.recipient.trim().is_empty() || !row.amount.trim().is_empty()).count() > MAX_SEND_ENTRIES_PER_TX {
                    self.send_dialog.status = SendDialogStatus::Failed;
                    self.send_dialog.message = format!("Multi-send supports at most {} entries.", MAX_SEND_ENTRIES_PER_TX);
                    return;
                }
                SendWork::Multi { entries, fee, asset, fee_asset }
            }
            SendMode::Blast => {
                if self.send_dialog.blast_create_mode {
                    let total = self.send_dialog.blast_total.trim().to_string();
                    let per_claim = self.send_dialog.blast_per_claim.trim().to_string();
                    let mut private_code = self.send_dialog.blast_private_code.trim().to_string();
                    if total.is_empty() || per_claim.is_empty() || fee.is_empty() {
                        self.send_dialog.status = SendDialogStatus::Failed;
                        self.send_dialog.message = "Blast total, per-claim amount, and fee are required.".to_string();
                        return;
                    }
                    let max_claims = match compute_blast_max_claims_text(&total, &per_claim) {
                        Ok(v) => v,
                        Err(err) => {
                            self.send_dialog.status = SendDialogStatus::Failed;
                            self.send_dialog.message = format!("Invalid Blast amounts: {err:#}");
                            return;
                        }
                    };
                    private_code = generate_gui_blast_code();
                    self.send_dialog.blast_private_code = private_code.clone();
                    self.send_dialog.blast_last_claim_payload.clear();
                    self.send_dialog.blast_show_qr = false;
                    SendWork::BlastCreate { total, per_claim, max_claims, private_code, fee }
                } else {
                    let claim_code = self.send_dialog.blast_claim_code.trim().to_string();
                    let claimant = self.send_dialog.blast_claimant_address.trim().to_string();
                    if claim_code.is_empty() {
                        self.send_dialog.status = SendDialogStatus::Failed;
                        self.send_dialog.message = "Blast claim code is required.".to_string();
                        return;
                    }
                    SendWork::BlastClaim { claim_code, claimant }
                }
            }
        };

        self.send_dialog.resolved_address.clear();
        let (tx, rx) = mpsc::channel();
        self.send_rx = Some(rx);
        self.send_dialog.status = SendDialogStatus::Sending;
        self.send_dialog.message = match mode {
            SendMode::Single => "Creating, signing, adding to local mempool, and relaying transaction...".to_string(),
            SendMode::Multi => "Creating multi-send transaction, signing locally, and relaying...".to_string(),
            SendMode::Blast => if self.send_dialog.blast_create_mode { "Creating Blast vault and private claim code...".to_string() } else { "Claiming Blast with the local wallet...".to_string() },
        };
        self.send_dialog.txid.clear();
        self.send_dialog.relayed_to_peers = 0;
        thread::spawn(move || {
            let result = match work {
                SendWork::Single { recipient, amount, fee, asset, fee_asset } => execute_gui_send_transaction(&config_path, &recipient, &amount, &fee, &asset, &fee_asset),
                SendWork::Multi { entries, fee, asset, fee_asset } => execute_gui_multi_send_transaction(&config_path, &entries, &fee, &asset, &fee_asset),
                SendWork::BlastCreate { total, per_claim, max_claims, private_code, fee } => execute_gui_blast_create(&config_path, &total, &per_claim, &max_claims, &private_code, &fee),
                SendWork::BlastClaim { claim_code, claimant } => execute_gui_blast_claim(&config_path, &claim_code, &claimant),
            };
            match result {
                Ok((txid, relayed_to_peers, local_mempooltx)) => { let _ = tx.send(SendEvent::Created { txid, relayed_to_peers, local_mempooltx }); }
                Err(err) => { let _ = tx.send(SendEvent::Failed(format!("{err:#}"))); }
            }
        });
    }

    fn poll_send_dialog(&mut self) {
        let mut event = None;
        if let Some(rx) = &self.send_rx {
            match rx.try_recv() {
                Ok(ev) => event = Some(ev),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => event = Some(SendEvent::Failed("Send worker disconnected.".to_string())),
            }
        }
        if let Some(ev) = event {
            self.send_rx = None;
            match ev {
                SendEvent::Created { txid, relayed_to_peers, local_mempooltx } => {
                    self.send_dialog.status = SendDialogStatus::Pending;
                    self.send_dialog.txid = txid.clone();
                    self.send_dialog.relayed_to_peers = relayed_to_peers;
                    self.send_dialog.message = if self.send_dialog.send_mode == SendMode::Blast && self.send_dialog.blast_create_mode {
                        let claim_payload = Hash256::from_hex(&txid)
                            .ok()
                            .and_then(|h| make_blast_code_payload(h, 0, &self.send_dialog.blast_private_code).ok())
                            .unwrap_or_default();
                        if !claim_payload.is_empty() {
                            self.send_dialog.blast_last_claim_payload = claim_payload.clone();
                            self.send_dialog.blast_show_qr = false;
                            if let Err(err) = save_blast_code_record_for_gui(&self.prefs.config_path, &txid, &self.send_dialog.blast_private_code, &claim_payload) {
                                self.last_error = Some(format!("Blast code save warning: {err:#}"));
                            }
                        }
                        let claim_note = if claim_payload.is_empty() { String::new() } else { format!("\nPRIVATE CLAIM CODE / QR PAYLOAD: {claim_payload}") };
                        format!(
                            "Blast vault pending confirmation. txid={}; local mempool={}, relayed_to_peers={}. Keep this private code visible only to the Blast creator.{}",
                            shorten_hash(&txid), local_mempooltx, relayed_to_peers, claim_note
                        )
                    } else {
                        format!(
                            "Pending confirmation. txid={}; local mempool={}, relayed_to_peers={}. Keep QUB Core open until a block includes it.",
                            shorten_hash(&txid), local_mempooltx, relayed_to_peers
                        )
                    };
                    self.last_success = Some(format!("Transaction created: {}", shorten_hash(&txid)));
                    self.last_send_status_poll = Instant::now() - Duration::from_secs(10);
                    self.start_background_snapshot_refresh();
                }
                SendEvent::Failed(err) => {
                    self.send_dialog.status = SendDialogStatus::Failed;
                    self.send_dialog.message = err;
                }
            }
        }

        if self.send_dialog.status == SendDialogStatus::Pending
            && !self.send_dialog.txid.is_empty()
            && self.last_send_status_poll.elapsed() >= Duration::from_secs(5)
        {
            self.last_send_status_poll = Instant::now();
            self.start_tx_status_check(self.send_dialog.txid.clone());
        }
    }



    fn open_buy_jin_dialog(&mut self, ctx: &egui::Context) {
        self.buy_jin_dialog.open = true;
        self.buy_jin_dialog.message.clear();
        if self.buy_jin_dialog.fee.trim().is_empty() || self.buy_jin_dialog.fee.trim() == "0" {
            self.buy_jin_dialog.fee = default_fee_for_config(&self.prefs.config_path).unwrap_or_else(|| "0.00001".to_string());
        }
        self.request_buy_jin_listings(ctx, false);
    }

    fn request_buy_jin_listings(&mut self, ctx: &egui::Context, force: bool) {
        if self.buy_jin_dialog.loading && !force { return; }
        if !force && !self.buy_jin_dialog.listings.is_empty() { return; }
        let config_path = self.prefs.config_path.clone();
        let repaint = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.buy_jin_rx = Some(rx);
        self.buy_jin_dialog.loading = true;
        self.buy_jin_dialog.message = "Loading JIN public sale listings in background...".to_string();
        thread::spawn(move || {
            let result = jin_sale_listings_for_gui(&config_path).map_err(|err| format!("{err:#}"));
            let _ = tx.send(BuyJinEvent::Listings(result));
            repaint.request_repaint();
        });
    }

    fn start_buy_jin_purchase(&mut self) {
        if self.snapshot.wallet_keys == 0 {
            self.buy_jin_dialog.status = SendDialogStatus::Failed;
            self.buy_jin_dialog.message = "No local wallet key is available. Create/import a local wallet first, then retry.".to_string();
            return;
        }
        let listing_id = self.buy_jin_dialog.selected_listing.to_string();
        let amount = self.buy_jin_dialog.amount_jin.trim().to_string();
        let fee = self.buy_jin_dialog.fee.trim().to_string();
        if amount.is_empty() || fee.is_empty() {
            self.buy_jin_dialog.status = SendDialogStatus::Failed;
            self.buy_jin_dialog.message = "Amount and fee are required.".to_string();
            return;
        }
        let config_path = self.prefs.config_path.clone();
        let (tx, rx) = mpsc::channel();
        self.buy_jin_rx = Some(rx);
        self.buy_jin_dialog.status = SendDialogStatus::Sending;
        self.buy_jin_dialog.message = "Creating JIN public sale purchase in background...".to_string();
        self.buy_jin_dialog.txid.clear();
        thread::spawn(move || {
            match execute_gui_buy_jin(&config_path, &listing_id, &amount, &fee) {
                Ok((txid, relayed_to_peers, local_mempooltx)) => { let _ = tx.send(BuyJinEvent::Created { txid, relayed_to_peers, local_mempooltx }); }
                Err(err) => { let _ = tx.send(BuyJinEvent::Failed(format!("{err:#}"))); }
            }
        });
    }

    fn poll_buy_jin_dialog(&mut self, ctx: &egui::Context) {
        let polled = self.buy_jin_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(BuyJinEvent::Listings(result))) => {
                self.buy_jin_rx = None;
                self.buy_jin_dialog.loading = false;
                match result {
                    Ok(listings) => {
                        self.buy_jin_dialog.listings = listings;
                        if let Some(first) = self.buy_jin_dialog.listings.iter().find(|l| l.remaining_units > 0) {
                            self.buy_jin_dialog.selected_listing = first.listing_id;
                        }
                        self.buy_jin_dialog.message = format!("Loaded {} JIN public sale listing(s).", self.buy_jin_dialog.listings.len());
                    }
                    Err(err) => self.buy_jin_dialog.message = format!("Could not load JIN sale listings: {err}"),
                }
            }
            Some(Ok(BuyJinEvent::Created { txid, relayed_to_peers, local_mempooltx })) => {
                self.buy_jin_rx = None;
                self.buy_jin_dialog.status = SendDialogStatus::Pending;
                self.buy_jin_dialog.txid = txid.clone();
                self.buy_jin_dialog.relayed_to_peers = relayed_to_peers;
                self.buy_jin_dialog.message = format!("JIN purchase pending. txid={} local mempool={} relayed_to_peers={}. Waiting for confirmation.", shorten_hash(&txid), local_mempooltx, relayed_to_peers);
                self.last_success = Some(format!("JIN purchase created: {}", shorten_hash(&txid)));
                self.last_send_status_poll = Instant::now() - Duration::from_secs(10);
                self.start_tx_status_check(txid);
                self.start_background_snapshot_refresh();
                self.request_buy_jin_listings(ctx, true);
            }
            Some(Ok(BuyJinEvent::Failed(err))) => {
                self.buy_jin_rx = None;
                self.buy_jin_dialog.status = SendDialogStatus::Failed;
                self.buy_jin_dialog.message = err;
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.buy_jin_rx = None;
                self.buy_jin_dialog.loading = false;
                self.buy_jin_dialog.message = "JIN sale worker disconnected.".to_string();
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
        if self.buy_jin_dialog.status == SendDialogStatus::Pending
            && !self.buy_jin_dialog.txid.is_empty()
            && self.last_send_status_poll.elapsed() >= Duration::from_secs(5)
        {
            self.last_send_status_poll = Instant::now();
            self.start_tx_status_check(self.buy_jin_dialog.txid.clone());
        }
    }

    fn open_qns_dialog(&mut self) {
        self.qns_dialog.open = true;
        self.qns_dialog.status = QnsDialogStatus::Editing;
        self.qns_dialog.message.clear();
        self.qns_dialog.txid.clear();
        self.qns_dialog.relayed_to_peers = 0;
        if self.qns_dialog.target_address.trim().is_empty() {
            self.qns_dialog.target_address = if !self.snapshot.default_address.is_empty() { self.snapshot.default_address.clone() } else { self.prefs.payout_address.clone() };
        }
        if self.qns_dialog.fee.trim().is_empty() || self.qns_dialog.fee.trim() == "0" {
            self.qns_dialog.fee = default_fee_for_config(&self.prefs.config_path).unwrap_or_else(|| "0.00001".to_string());
        }
        if self.qns_dialog.price.trim().is_empty() || self.qns_dialog.price == "-" {
            self.qns_dialog.price = "Click Calculate cost.".to_string();
        }
    }

    fn update_qns_price_preview(&mut self) {
        if self.qns_dialog.name.trim().is_empty() { self.qns_dialog.price = "-".to_string(); return; }
        match qns_price_for_gui(&self.prefs.config_path, &self.qns_dialog.name) {
            Ok(price) => self.qns_dialog.price = price,
            Err(err) => self.qns_dialog.price = format!("error: {err}"),
        }
    }

    fn start_qns_dialog(&mut self) {
        if self.snapshot.wallet_keys == 0 {
            self.qns_dialog.status = QnsDialogStatus::Failed;
            self.qns_dialog.message = "No local wallet key is available for paying the QNS registration.".to_string();
            return;
        }
        let config_path = self.prefs.config_path.clone();
        let name = self.qns_dialog.name.trim().to_string();
        let target = self.qns_dialog.target_address.trim().to_string();
        let fee = self.qns_dialog.fee.trim().to_string();
        let (tx, rx) = mpsc::channel();
        self.qns_rx = Some(rx);
        self.qns_dialog.status = QnsDialogStatus::Sending;
        self.qns_dialog.message = "Creating, signing, adding to local mempool, and relaying QNS registration...".to_string();
        self.qns_dialog.txid.clear();
        thread::spawn(move || {
            match execute_gui_qns_register(&config_path, &name, &target, &fee) {
                Ok((txid, relayed_to_peers, local_mempooltx)) => {
                    let _ = tx.send(QnsEvent::Created { txid, relayed_to_peers, local_mempooltx });
                }
                Err(err) => { let _ = tx.send(QnsEvent::Failed(format!("{err:#}"))); }
            }
        });
    }

    fn poll_qns_dialog(&mut self) {
        let mut event = None;
        if let Some(rx) = &self.qns_rx {
            match rx.try_recv() {
                Ok(ev) => event = Some(ev),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => event = Some(QnsEvent::Failed("QNS worker disconnected.".to_string())),
            }
        }
        if let Some(ev) = event {
            self.qns_rx = None;
            match ev {
                QnsEvent::Created { txid, relayed_to_peers, local_mempooltx } => {
                    self.qns_dialog.status = QnsDialogStatus::Pending;
                    self.qns_dialog.txid = txid.clone();
                    self.qns_dialog.relayed_to_peers = relayed_to_peers;
                    self.qns_dialog.message = format!("QNS registration pending. txid={}; local mempool={}, relayed_to_peers={}.", shorten_hash(&txid), local_mempooltx, relayed_to_peers);
                    self.start_background_snapshot_refresh();
                }
                QnsEvent::Failed(err) => {
                    self.qns_dialog.status = QnsDialogStatus::Failed;
                    self.qns_dialog.message = err;
                }
            }
        }
        if self.qns_dialog.status == QnsDialogStatus::Pending
            && !self.qns_dialog.txid.is_empty()
            && self.last_send_status_poll.elapsed() >= Duration::from_secs(5)
        {
            self.last_send_status_poll = Instant::now();
            self.start_tx_status_check(self.qns_dialog.txid.clone());
        }
    }


    fn open_pools_window(&mut self) {
        self.pool_dialog.open = true;
        if self.pool_dialog.miner_address.trim().is_empty() {
            self.pool_dialog.miner_address = if !self.prefs.payout_address.trim().is_empty() { self.prefs.payout_address.clone() } else { self.snapshot.default_address.clone() };
        }
        self.ensure_pool_selection_from_last();
        if self.pool_dialog.create_fee.trim().is_empty() { self.pool_dialog.create_fee = "0.00001".to_string(); }
        if self.pool_dialog.manage_fee.trim().is_empty() { self.pool_dialog.manage_fee = "0.00001".to_string(); }
    }

    fn pool_by_id(&self, id: &str) -> Option<&PoolUiSummary> {
        let id = id.trim();
        if id.is_empty() { return None; }
        self.snapshot.pools.iter().find(|p| p.pool_id == id)
    }

    fn selected_pool(&self) -> Option<&PoolUiSummary> {
        self.pool_by_id(&self.pool_dialog.selected_pool_id)
    }

    fn ensure_pool_selection_from_last(&mut self) {
        if !self.pool_dialog.selected_pool_id.trim().is_empty() && self.pool_by_id(&self.pool_dialog.selected_pool_id).is_some() {
            return;
        }
        if !self.prefs.last_pool_id.trim().is_empty() {
            let last = self.prefs.last_pool_id.clone();
            if let Some(pool) = self.pool_by_id(&last).cloned() {
                self.select_pool_for_gui(&pool);
                return;
            }
        }
        self.pool_dialog.selected_pool_id.clear();
        self.pool_dialog.join_pool_id.clear();
    }

    fn remember_pool_for_user(&mut self, pool_id: &str) {
        let pool_id = pool_id.trim();
        if pool_id.is_empty() { return; }
        self.prefs.last_pool_id = pool_id.to_string();
        self.prefs_dirty = true;
        if let Some(pool) = self.pool_by_id(pool_id).cloned() {
            self.select_pool_for_gui(&pool);
        } else {
            self.pool_dialog.selected_pool_id = pool_id.to_string();
            self.pool_dialog.join_pool_id = pool_id.to_string();
        }
    }

    fn pool_activation_ready(&self) -> bool {
        self.snapshot.height.saturating_add(1) >= self.snapshot.pools_activation_height
    }

    fn apply_hf105_first_run_auto_restart_default(&mut self) {
        if self.prefs.hf105_auto_restart_default_applied { return; }
        self.prefs.start_mining_after_update_restart = true;
        self.prefs.start_mining_after_update_restart_mode = if !self.prefs.last_pool_id.trim().is_empty() {
            AutoMiningMode::Pool
        } else {
            AutoMiningMode::Solo
        };
        self.prefs.hf105_auto_restart_default_applied = true;
        self.prefs_dirty = true;
    }

    fn auto_mining_mode_label(&self, mode: AutoMiningMode) -> String {
        match mode {
            AutoMiningMode::Solo => "Solo".to_string(),
            AutoMiningMode::Pool => {
                if let Some(pool) = self.pool_by_id(&self.prefs.last_pool_id) {
                    format!("Pool ({})", pool.name)
                } else if !self.prefs.last_pool_id.trim().is_empty() {
                    format!("Pool ({})", shorten_hash(&self.prefs.last_pool_id))
                } else {
                    "Pool (select/join a pool first)".to_string()
                }
            }
        }
    }

    fn start_mining_from_auto_mode(&mut self, mode: AutoMiningMode, reason: &str) {
        match mode {
            AutoMiningMode::Solo => {
                self.pool_mining_pool_id.clear();
                self.desired_pool_mining_pool_id.clear();
                self.status_line = format!("QUB Core {}: starting Solo mining after local wallet/chain view loaded.", reason);
                self.start_mining();
            }
            AutoMiningMode::Pool => {
                self.ensure_pool_selection_from_last();
                let pool_id = self.prefs.last_pool_id.trim().to_string();
                if pool_id.is_empty() {
                    self.status_line = format!("QUB Core {}: Pool mode is selected, but no pool is selected yet. Open Pools and click Mine once.", reason);
                    return;
                }
                if !self.pool_activation_ready() {
                    self.status_line = format!("QUB Core {}: Pool mode is selected, but pool mining is not active at the current height yet.", reason);
                    return;
                }
                let label = self.auto_mining_mode_label(AutoMiningMode::Pool);
                self.status_line = format!("QUB Core {}: starting {} after local wallet/chain view loaded.", reason, label);
                self.start_pool_mining(pool_id);
            }
        }
    }

    fn select_pool_for_gui(&mut self, pool: &PoolUiSummary) {
        self.pool_dialog.selected_pool_id = pool.pool_id.clone();
        self.pool_dialog.join_pool_id = pool.pool_id.clone();
        self.pool_dialog.manage_pool_id = pool.pool_id.clone();
        self.pool_dialog.rename_name = pool.name.clone();
        self.pool_dialog.new_commission_bps = pool.commission_bps.to_string();
        if self.pool_dialog.miner_address.trim().is_empty() {
            self.pool_dialog.miner_address = if !self.prefs.payout_address.trim().is_empty() { self.prefs.payout_address.clone() } else { self.snapshot.default_address.clone() };
        }
    }

    fn start_pool_create_dialog(&mut self) {
        if self.snapshot.wallet_keys == 0 { self.pool_action_failed("No local wallet key is available for pool creation.".to_string()); return; }
        let config_path = self.prefs.config_path.clone();
        let name = self.pool_dialog.create_name.trim().to_string();
        let commission_bps = self.pool_dialog.create_commission_bps.trim().to_string();
        let capacity_slots = self.pool_dialog.create_capacity_slots;
        let manager_address = if !self.prefs.payout_address.trim().is_empty() { self.prefs.payout_address.trim().to_string() } else { self.snapshot.default_address.clone() };
        let fee = self.pool_dialog.create_fee.trim().to_string();
        let (tx, rx) = mpsc::channel();
        self.pool_rx = Some(rx);
        self.pool_dialog.status = SendDialogStatus::Sending;
        self.pool_dialog.action = "create".to_string();
        self.pool_dialog.message = "Creating pool transaction and relaying it...".to_string();
        self.pool_dialog.txid.clear();
        thread::spawn(move || {
            match execute_gui_pool_create(&config_path, &name, &commission_bps, capacity_slots, &manager_address, &fee) {
                Ok(ev) => { let _ = tx.send(ev); }
                Err(err) => { let _ = tx.send(PoolGuiEvent::Failed(format!("{err:#}"))); }
            }
        });
    }

    fn start_pool_join_dialog(&mut self, start_mining_after: bool) {
        if self.snapshot.wallet_keys == 0 { self.pool_action_failed("No local wallet private key is available for signing pool shares.".to_string()); return; }
        let pool_id = self.pool_dialog.join_pool_id.trim().to_string();
        if pool_id.is_empty() { self.pool_action_failed("Select a pool first.".to_string()); return; }
        self.remember_pool_for_user(&pool_id);
        if start_mining_after {
            self.desired_pool_mining_pool_id = pool_id.trim().to_string();
            self.manual_stop_requested = false;
            self.auto_restart_mining_at = None;
        }
        let miner_address = if !self.pool_dialog.miner_address.trim().is_empty() { self.pool_dialog.miner_address.trim().to_string() } else if !self.prefs.payout_address.trim().is_empty() { self.prefs.payout_address.trim().to_string() } else { self.snapshot.default_address.clone() };
        let config_path = self.prefs.config_path.clone();
        let pool_id_for_start = pool_id.clone();
        let (tx, rx) = mpsc::channel();
        self.pool_rx = Some(rx);
        self.pool_dialog.status = SendDialogStatus::Sending;
        self.pool_dialog.action = if start_mining_after { "join+mine".to_string() } else { "join".to_string() };
        self.pool_dialog.message = "Creating PoW share transaction for pool join...".to_string();
        self.pool_dialog.txid.clear();
        thread::spawn(move || {
            match execute_gui_pool_join(&config_path, &pool_id, &miner_address) {
                Ok(mut ev) => {
                    if start_mining_after {
                        if let PoolGuiEvent::Created { message, .. } = &mut ev {
                            message.push_str(" Start pool mining after this share confirms, or keep QUB Core open while the network confirms it.");
                        }
                    }
                    let _ = tx.send(ev);
                }
                Err(err) => { let _ = tx.send(PoolGuiEvent::Failed(format!("{err:#}"))); }
            }
        });
        if start_mining_after {
            self.pool_dialog.selected_pool_id = pool_id_for_start;
        }
    }

    fn start_pool_topup_dialog(&mut self) {
        let pool_id = self.pool_dialog.manage_pool_id.trim().to_string();
        if pool_id.is_empty() { self.pool_action_failed("Select a managed pool first.".to_string()); return; }
        let config_path = self.prefs.config_path.clone();
        let extra_slots = self.pool_dialog.extra_capacity_slots;
        let fee = self.pool_dialog.manage_fee.trim().to_string();
        let (tx, rx) = mpsc::channel();
        self.pool_rx = Some(rx);
        self.pool_dialog.status = SendDialogStatus::Sending;
        self.pool_dialog.action = "capacity top-up".to_string();
        self.pool_dialog.message = "Creating pool capacity top-up transaction...".to_string();
        self.pool_dialog.txid.clear();
        thread::spawn(move || {
            match execute_gui_pool_topup(&config_path, &pool_id, extra_slots, &fee) {
                Ok(ev) => { let _ = tx.send(ev); }
                Err(err) => { let _ = tx.send(PoolGuiEvent::Failed(format!("{err:#}"))); }
            }
        });
    }

    fn start_pool_commission_dialog(&mut self) {
        let pool_id = self.pool_dialog.manage_pool_id.trim().to_string();
        if pool_id.is_empty() { self.pool_action_failed("Select a managed pool first.".to_string()); return; }
        let config_path = self.prefs.config_path.clone();
        let new_bps = self.pool_dialog.new_commission_bps.trim().to_string();
        let fee = self.pool_dialog.manage_fee.trim().to_string();
        let (tx, rx) = mpsc::channel();
        self.pool_rx = Some(rx);
        self.pool_dialog.status = SendDialogStatus::Sending;
        self.pool_dialog.action = "commission decrease".to_string();
        self.pool_dialog.message = "Creating pool commission decrease transaction...".to_string();
        self.pool_dialog.txid.clear();
        thread::spawn(move || {
            match execute_gui_pool_set_commission(&config_path, &pool_id, &new_bps, &fee) {
                Ok(ev) => { let _ = tx.send(ev); }
                Err(err) => { let _ = tx.send(PoolGuiEvent::Failed(format!("{err:#}"))); }
            }
        });
    }

    fn start_pool_rename_dialog(&mut self) {
        let pool_id = self.pool_dialog.manage_pool_id.trim().to_string();
        if pool_id.is_empty() { self.pool_action_failed("Select a managed pool first.".to_string()); return; }
        let config_path = self.prefs.config_path.clone();
        let new_name = self.pool_dialog.rename_name.trim().to_string();
        let fee = self.pool_dialog.manage_fee.trim().to_string();
        let (tx, rx) = mpsc::channel();
        self.pool_rx = Some(rx);
        self.pool_dialog.status = SendDialogStatus::Sending;
        self.pool_dialog.action = "rename".to_string();
        self.pool_dialog.message = "Creating pool rename transaction...".to_string();
        self.pool_dialog.txid.clear();
        thread::spawn(move || {
            match execute_gui_pool_rename(&config_path, &pool_id, &new_name, &fee) {
                Ok(ev) => { let _ = tx.send(ev); }
                Err(err) => { let _ = tx.send(PoolGuiEvent::Failed(format!("{err:#}"))); }
            }
        });
    }

    fn pool_action_failed(&mut self, message: String) {
        self.pool_dialog.status = SendDialogStatus::Failed;
        self.pool_dialog.message = message.clone();
        self.last_error = Some(message);
    }

    fn poll_pool_dialog(&mut self) {
        let mut event = None;
        if let Some(rx) = &self.pool_rx {
            match rx.try_recv() {
                Ok(ev) => event = Some(ev),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => event = Some(PoolGuiEvent::Failed("Pool worker disconnected.".to_string())),
            }
        }
        if let Some(ev) = event {
            self.pool_rx = None;
            match ev {
                PoolGuiEvent::Created { action, txid, pool_id, relayed_to_peers, local_mempooltx, message } => {
                    self.pool_dialog.status = SendDialogStatus::Pending;
                    self.pool_dialog.action = action;
                    self.pool_dialog.txid = txid.clone();
                    self.pool_dialog.selected_pool_id = pool_id.clone();
                    self.pool_dialog.join_pool_id = pool_id.clone();
                    self.pool_dialog.manage_pool_id = pool_id;
                    self.pool_dialog.relayed_to_peers = relayed_to_peers;
                    self.pool_dialog.local_mempooltx = local_mempooltx;
                    self.pool_dialog.message = message;
                    self.status_line = "Pool transaction/share pending confirmation.".to_string();
                    self.start_background_snapshot_refresh();
                }
                PoolGuiEvent::Failed(err) => {
                    self.pool_dialog.status = SendDialogStatus::Failed;
                    self.pool_dialog.message = err.clone();
                    self.last_error = Some(format!("Pool action failed: {err}"));
                }
            }
        }
        if self.pool_dialog.status == SendDialogStatus::Pending
            && !self.pool_dialog.txid.is_empty()
            && self.last_send_status_poll.elapsed() >= Duration::from_secs(5)
        {
            self.last_send_status_poll = Instant::now();
            self.start_tx_status_check(self.pool_dialog.txid.clone());
        }
    }


    fn poll_library_state(&mut self) {
        let polled = self.library_state_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(Ok(state))) => {
                self.library_state_in_flight = false;
                self.library_state_rx = None;
                self.library_state_cache = Some(state);
                self.library_state_last_loaded = Some(Instant::now());
                self.library_state_error = None;
            }
            Some(Ok(Err(err))) => {
                self.library_state_in_flight = false;
                self.library_state_rx = None;
                self.library_state_error = Some(err);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.library_state_in_flight = false;
                self.library_state_rx = None;
                self.library_state_error = Some("Library loader disconnected.".to_string());
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    fn request_library_state_refresh(&mut self, ctx: &egui::Context, force: bool) {
        if self.library_state_in_flight { return; }
        if !force {
            if let Some(loaded) = self.library_state_last_loaded {
                if loaded.elapsed() < Duration::from_secs(20) { return; }
            }
            if self.library_state_cache.is_some() && self.library_state_error.is_none() { return; }
        }
        let config_path = self.prefs.config_path.clone();
        let repaint = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.library_state_rx = Some(rx);
        self.library_state_in_flight = true;
        self.library_state_last_started = Instant::now();
        self.library_state_error = None;
        thread::spawn(move || {
            let result = library_state_for_gui(&config_path).map_err(|err| format!("{err:#}"));
            let _ = tx.send(result);
            repaint.request_repaint();
        });
    }

    fn start_pool_mining(&mut self, pool_id: String) {
        if self.update_dialog.status == UpdateStatus::Ready || self.update_dialog.status == UpdateStatus::Installing {
            self.last_error = Some("A newer QUB Core version is pending. Install the update before mining again.".to_string());
            return;
        }
        if self.miner.is_some() { return; }
        if pool_id.trim().is_empty() { self.last_error = Some("Select a pool before starting pool mining.".to_string()); return; }
        self.remember_pool_for_user(&pool_id);
        self.desired_pool_mining_pool_id = pool_id.trim().to_string();
        self.manual_stop_requested = false;
        self.auto_restart_mining_at = None;
        let miner_address = if !self.pool_dialog.miner_address.trim().is_empty() { self.pool_dialog.miner_address.trim().to_string() } else if !self.prefs.payout_address.trim().is_empty() { self.prefs.payout_address.trim().to_string() } else { self.snapshot.default_address.clone() };
        if miner_address.trim().is_empty() { self.last_error = Some("Create/import a local wallet key or set a miner address before pool mining.".to_string()); return; }
        self.update_runtime_identity();
        self.set_mining_phase(MiningPhase::Preparing);
        if let Err(message) = self.ensure_network_ready_to_mine() {
            self.last_error = Some(message);
            self.status_line = "Pool mining blocked until P2P is connected.".to_string();
            self.set_mining_phase(MiningPhase::Off);
            return;
        }
        let config_path = self.prefs.config_path.clone();
        let cpu_percent = self.prefs.cpu_percent.clamp(1, 100);
        let gpu_percent = self.prefs.gpu_percent.clamp(0, 100);
        let gpu_device_selector = self.normalized_gpu_device_selector();
        self.gpu_hash_rate_hps = 0.0;
        self.gpu_total_hashes = 0;
        self.gpu_workers = 0;
        self.gpu_device.clear();
        self.gpu_device_rates.clear();
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let pool_for_thread = pool_id.trim().to_string();
        let pool_for_status = pool_for_thread.clone();
        let join = thread::spawn(move || run_pool_miner(config_path, pool_for_thread, miner_address, cpu_percent, gpu_percent, gpu_device_selector, tx, thread_stop));
        self.pool_mining_pool_id = pool_for_status.clone();
        self.miner = Some(MinerHandle { stop, rx, join: Some(join) });
        self.status_line = format!("Pool miner starting for {}: running fork-safety sync, submitting/confirming share, then preparing candidates...", shorten_hash(&pool_for_status));
        self.last_success = None;
        self.last_error = None;
    }


    fn ui_icon(&self, ui: &mut egui::Ui, name: &str, size: f32) {
        if let Some(texture) = self.icons.get(name, ui.visuals().dark_mode) {
            let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(size, size));
            ui.add(egui::Image::from_texture(sized));
        }
    }

    fn ui_heading_icon(&self, ui: &mut egui::Ui, icon: &str, text: &str) {
        ui.horizontal(|ui| {
            self.ui_icon(ui, icon, 22.0);
            ui.heading(text);
        });
    }

    fn ui_icon_label(&self, ui: &mut egui::Ui, icon: &str, text: impl Into<String>) {
        ui.horizontal(|ui| {
            self.ui_icon(ui, icon, 16.0);
            ui.label(text.into());
        });
    }

    fn ui_weak_icon_label(&self, ui: &mut egui::Ui, icon: &str, text: impl Into<String>) {
        ui.horizontal(|ui| {
            self.ui_icon(ui, icon, 15.0);
            ui.label(egui::RichText::new(text.into()).weak());
        });
    }

    fn ui_icon_button(&self, ui: &mut egui::Ui, icon: &str, text: &str) -> egui::Response {
        if let Some(texture) = self.icons.get(icon, ui.visuals().dark_mode) {
            let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(16.0, 16.0));
            ui.add(egui::Button::image_and_text(egui::Image::from_texture(sized), text))
        } else {
            ui.button(text)
        }
    }

    fn ui_icon_button_sized(&self, ui: &mut egui::Ui, icon: &str, text: &str, size: f32) -> egui::Response {
        if let Some(texture) = self.icons.get(icon, ui.visuals().dark_mode) {
            let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(size, size));
            ui.add(egui::Button::image_and_text(egui::Image::from_texture(sized), text))
        } else {
            ui.button(text)
        }
    }

    fn ui_info_tip(&self, ui: &mut egui::Ui, text: &'static str) {
        if let Some(texture) = self.icons.get("i", ui.visuals().dark_mode) {
            let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(13.0, 13.0));
            ui.add(egui::Image::from_texture(sized)).on_hover_text(text);
        } else {
            ui.label(egui::RichText::new("i").weak()).on_hover_text(text);
        }
    }

    fn ui_icon_button_enabled(&self, ui: &mut egui::Ui, enabled: bool, icon: &str, text: &str) -> egui::Response {
        if let Some(texture) = self.icons.get(icon, ui.visuals().dark_mode) {
            let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(16.0, 16.0));
            ui.add_enabled(enabled, egui::Button::image_and_text(egui::Image::from_texture(sized), text))
        } else {
            ui.add_enabled(enabled, egui::Button::new(text))
        }
    }

    fn ui_icon_only_button_enabled(&self, ui: &mut egui::Ui, enabled: bool, icon: &str, tooltip: &str) -> egui::Response {
        let response = if let Some(texture) = self.icons.get(icon, ui.visuals().dark_mode) {
            let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(18.0, 18.0));
            ui.add_enabled(enabled, egui::Button::image_and_text(egui::Image::from_texture(sized), "").min_size(egui::vec2(34.0, 30.0)))
        } else {
            let fallback = tooltip.chars().next().unwrap_or('?').to_string();
            ui.add_enabled(enabled, egui::Button::new(fallback).min_size(egui::vec2(34.0, 30.0)))
        };
        response.on_hover_text(tooltip)
    }

    fn ui_tall_icon_button(&self, ui: &mut egui::Ui, icon: &str, fallback: &str, tooltip: &str, height: f32) -> egui::Response {
        let response = if let Some(texture) = self.icons.get(icon, ui.visuals().dark_mode) {
            let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(20.0, height.min(54.0)));
            ui.add_sized([28.0, height.min(62.0)], egui::Button::image_and_text(egui::Image::from_texture(sized), ""))
        } else {
            ui.add_sized([28.0, height.min(62.0)], egui::Button::new(fallback))
        };
        response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tooltip)
    }

    fn ui_icon_selectable_button(&self, ui: &mut egui::Ui, selected: bool, icon: &str, text: &str) -> egui::Response {
        if let Some(texture) = self.icons.get(icon, ui.visuals().dark_mode) {
            let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(16.0, 16.0));
            ui.add(egui::Button::image_and_text(egui::Image::from_texture(sized), text).selected(selected))
        } else {
            ui.add(egui::Button::new(text).selected(selected))
        }
    }

    fn metric_icon(&self, ui: &mut egui::Ui, icon: &str, label: &str, value: impl Into<String>) {
        self.ui_weak_icon_label(ui, icon, label);
        ui.label(egui::RichText::new(value.into()).strong());
        ui.end_row();
    }

    fn metric_info(&self, ui: &mut egui::Ui, icon: &str, label: &'static str, value: impl Into<String>, tip: &'static str) {
        ui.horizontal(|ui| {
            self.ui_icon(ui, icon, 15.0);
            ui.label(egui::RichText::new(self.tr(label, label)).weak());
            self.ui_info_tip(ui, tip);
        });
        ui.label(egui::RichText::new(value.into()).strong());
        ui.end_row();
    }

    fn ui_central_section_header(ui: &mut egui::Ui, title: &str, summary: impl Into<String>, expanded: &mut bool) {
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let arrow = if *expanded { "v" } else { ">" };
                if ui.button(arrow).on_hover_text("Collapse / expand section").clicked() { *expanded = !*expanded; }
                ui.label(egui::RichText::new(title).size(18.0).strong());
                ui.separator();
                ui.label(egui::RichText::new(summary.into()).weak());
            });
        });
    }

    fn metric_qub_jin(&self, ui: &mut egui::Ui, icon: &str, label: &str, qub_value: &str) {
        self.ui_weak_icon_label(ui, icon, label);
        ui.horizontal(|ui| {
            self.ui_icon(ui, "qub", 16.0);
            ui.label(egui::RichText::new(format!("{qub_value} QUB")).strong());
            ui.separator();
            self.ui_icon(ui, "jin", 16.0);
            ui.label(egui::RichText::new(format!("{} JIN", self.snapshot.jin_total)).strong());
        });
        ui.end_row();
    }


    fn qr_texture_for_address(&mut self, ctx: &egui::Context, address: &str) -> Option<egui::TextureHandle> {
        let address = address.trim();
        if address.is_empty() { return None; }
        if self.qr_cache_address == address {
            return self.qr_cache_texture.clone();
        }
        let Ok(code) = QrCode::new(address.as_bytes()) else { return None; };
        let modules = code.to_colors();
        let width = code.width();
        let border = 4usize;
        let scale = 5usize;
        let pixels_w = (width + border * 2) * scale;
        let mut image = egui::ColorImage::new([pixels_w, pixels_w], vec![egui::Color32::WHITE; pixels_w * pixels_w]);
        for y in 0..width {
            for x in 0..width {
                let idx = y * width + x;
                let color = if modules.get(idx) == Some(&QrColor::Dark) { egui::Color32::BLACK } else { egui::Color32::WHITE };
                for yy in 0..scale {
                    for xx in 0..scale {
                        let px = (x + border) * scale + xx;
                        let py = (y + border) * scale + yy;
                        image.pixels[py * pixels_w + px] = color;
                    }
                }
            }
        }
        let tex = ctx.load_texture(format!("qr-address-{}", stable_hash64(address)), image, egui::TextureOptions::NEAREST);
        self.qr_cache_address = address.to_string();
        self.qr_cache_texture = Some(tex.clone());
        Some(tex)
    }

    fn ui_address_qr_hover(&mut self, ctx: &egui::Context, address: &str) {
        let Some(until) = self.qr_hover_until else { return; };
        if Instant::now() > until {
            self.qr_hover_until = None;
            self.qr_hover_address.clear();
            return;
        }
        let target = if !self.qr_hover_address.trim().is_empty() { self.qr_hover_address.trim().to_string() } else { address.trim().to_string() };
        if target.is_empty() { return; }
        if let Some(tex) = self.qr_texture_for_address(ctx, &target) {
            egui::Window::new("Address QR")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::LEFT_TOP, egui::vec2(390.0, 145.0))
                .show(ctx, |ui| {
                    let sized = egui::load::SizedTexture::new(tex.id(), egui::vec2(220.0, 220.0));
                    ui.add(egui::Image::from_texture(sized));
                    ui.monospace(shorten_hash(&target));
                    ui.small("QR encodes this public address only. Never share private keys.");
                });
        }
    }

    fn ui_qr_scan_window(&mut self, ctx: &egui::Context) {
        if !self.qr_scan_dialog_open { return; }
        let mut open = self.qr_scan_dialog_open;
        egui::Window::new("Scan recipient QR")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("QR camera scanner").strong());
                ui.small("Camera device selection is prepared here. Full live QR decoding is not enabled in this build; paste the scanned address into Recipient address if your camera app decodes it.");
                if self.qr_camera_devices.is_empty() {
                    ui.colored_label(egui::Color32::from_rgb(255, 170, 90), "No camera devices detected by Windows.");
                    if ui.button("Refresh cameras").clicked() { self.qr_camera_devices = detect_camera_devices(); self.qr_camera_selected = 0; }
                } else {
                    let current = self.qr_camera_devices.get(self.qr_camera_selected).cloned().unwrap_or_else(|| "Camera".to_string());
                    egui::ComboBox::from_label("Camera")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (idx, name) in self.qr_camera_devices.iter().enumerate() {
                                ui.selectable_value(&mut self.qr_camera_selected, idx, name);
                            }
                        });
                    ui.horizontal(|ui| {
                        if self.ui_icon_button(ui, "qr-scan", "Open camera app").clicked() {
                            open_windows_camera_app();
                            self.qr_scan_message = "Camera app opened. Paste the decoded address into Recipient address.".to_string();
                        }
                        if ui.button("Refresh cameras").clicked() { self.qr_camera_devices = detect_camera_devices(); self.qr_camera_selected = 0; }
                    });
                }
                if !self.qr_scan_message.is_empty() { ui.small(&self.qr_scan_message); }
            });
        self.qr_scan_dialog_open = open;
    }

    fn ui_import_key_window(&mut self, ctx: &egui::Context) {
        if !self.import_key_dialog.open { return; }
        let mut open = self.import_key_dialog.open;
        let mut want_close = false;
        egui::Window::new("Import private key")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                self.ui_icon_label(ui, "import-private-key", "Import private key");
                ui.small("Paste a 32-byte hex private key. This stores plaintext secret_key_hex in wallet.json, exactly like locally-created v1 keys.");
                ui.add_space(8.0);
                ui.label("Label");
                ui.text_edit_singleline(&mut self.import_key_dialog.label);
                ui.label("secret_key_hex");
                ui.add(egui::TextEdit::singleline(&mut self.import_key_dialog.secret_key_hex).password(true).desired_width(500.0));
                ui.small("Do not import keys you use anywhere else. Back up wallet.json before relying on this key.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if self.ui_icon_button(ui, "import-private-key", "Import").clicked() { self.import_private_key_from_dialog(); }
                    if ui.button("Close").clicked() { want_close = true; }
                });
                if !self.import_key_dialog.message.is_empty() {
                    if self.import_key_dialog.success { ui.colored_label(egui::Color32::from_rgb(66, 220, 120), &self.import_key_dialog.message); }
                    else { ui.colored_label(egui::Color32::from_rgb(255, 120, 120), &self.import_key_dialog.message); }
                }
            });
        if want_close { open = false; }
        self.import_key_dialog.open = open;
    }

    fn ui_pools_window(&mut self, ctx: &egui::Context) {
        if !self.pool_dialog.open { return; }
        let mut open = self.pool_dialog.open;
        let mut want_close = false;
        egui::Window::new("Pools")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(1180.0)
            .default_height(620.0)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    self.ui_icon(ui, "pools", 20.0);
                    ui.label(egui::RichText::new("Browse / Join pools").strong());
                    ui.separator();
                    let pool_state = if self.pool_activation_ready() { "active".to_string() } else { format!("activates at #{}", self.snapshot.pools_activation_height) };
                    ui.small(format!("Network: {} | height #{} | pools {} | {} pool(s)", self.snapshot.network, self.snapshot.height, pool_state, self.snapshot.pools_count));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.ui_icon_button(ui, "create-pool", "Create pool").clicked() {
                            self.pool_dialog.create_open = true;
                        }
                    });
                });
                if !self.pool_activation_ready() {
                    ui.colored_label(egui::Color32::from_rgb(255, 170, 90), format!("Pools are not active yet on this chain. Create/join/manage actions unlock at block #{}.", self.snapshot.pools_activation_height));
                }
                ui.small(format!("Protocol: pools.qub -> {}. Pool share window is fixed at 360 blocks; after switching pools, old confirmed shares age out naturally before new rewards are fully weighted.", self.snapshot.pools_protocol_address));
                ui.add_space(8.0);
                self.ui_pools_browse_section(ui);
                ui.add_space(10.0);
                self.ui_pool_action_status(ui);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Refresh").clicked() { self.start_wallet_sync(true); }
                    if ui.button("Close").clicked() { want_close = true; }
                });
            });
        if want_close { open = false; }
        self.pool_dialog.open = open;
    }

    fn ui_pools_browse_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Search");
            ui.text_edit_singleline(&mut self.pool_dialog.search);
            ui.checkbox(&mut self.pool_dialog.open_only, "Open only");
        });
        ui.add_space(6.0);
        let search = self.pool_dialog.search.trim().to_ascii_lowercase();
        let mut pools = self.snapshot.pools.clone();
        pools.sort_by(|a, b| a.commission_bps.cmp(&b.commission_bps).then(b.open_slots.cmp(&a.open_slots)).then(b.recent_shares.cmp(&a.recent_shares)));
        pools.retain(|p| {
            let matches_search = search.is_empty() || p.name.to_ascii_lowercase().contains(&search) || p.pool_id.to_ascii_lowercase().contains(&search) || p.manager_address.to_ascii_lowercase().contains(&search);
            let matches_open = !self.pool_dialog.open_only || p.open_slots > 0 || p.your_active;
            matches_search && matches_open
        });
        if pools.is_empty() {
            ui.small("No pools match the current filters yet.");
            return;
        }

        const POOL_NAME_W: f32 = 210.0;
        const POOL_ID_W: f32 = 132.0;
        const COMMISSION_W: f32 = 92.0;
        const MINERS_W: f32 = 78.0;
        const SHARES_W: f32 = 78.0;
        const YOU_W: f32 = 116.0;
        const MANAGER_W: f32 = 152.0;
        const PAID_W: f32 = 92.0;
        const ACTIONS_W: f32 = 154.0;
        const ROW_H: f32 = 30.0;

        let header_cell = |ui: &mut egui::Ui, w: f32, text: &str| {
            ui.add_sized([w, 22.0], egui::Label::new(egui::RichText::new(text).weak()));
        };

        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                header_cell(ui, POOL_NAME_W, "Pool");
                header_cell(ui, POOL_ID_W, "ID");
                header_cell(ui, COMMISSION_W, "Commission");
                header_cell(ui, MINERS_W, "Miners");
                header_cell(ui, SHARES_W, "Shares");
                header_cell(ui, YOU_W, "You");
                header_cell(ui, MANAGER_W, "Manager");
                header_cell(ui, PAID_W, "Paid");
                header_cell(ui, ACTIONS_W, "Actions");
            });
            ui.add_space(4.0);

            for pool in pools {
                let is_last = self.prefs.last_pool_id.trim() == pool.pool_id;
                let is_pool_mining = self.miner.is_some() && self.pool_mining_pool_id == pool.pool_id;
                let fill = if is_pool_mining {
                    egui::Color32::from_rgba_unmultiplied(40, 92, 62, 150)
                } else if is_last {
                    egui::Color32::from_rgba_unmultiplied(54, 70, 92, 115)
                } else {
                    ui.visuals().faint_bg_color
                };
                egui::Frame::group(ui.style()).fill(fill).inner_margin(egui::Margin::same(7)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_sized([POOL_NAME_W, ROW_H], egui::Label::new(egui::RichText::new(&pool.name).strong()));
                        ui.add_sized([POOL_ID_W, ROW_H], egui::Label::new(egui::RichText::new(shorten_hash(&pool.pool_id)).monospace()));
                        ui.add_sized([COMMISSION_W, ROW_H], egui::Label::new(format!("{:.2}%", pool.commission_bps as f64 / 100.0)));
                        ui.add_sized([MINERS_W, ROW_H], egui::Label::new(format!("{} / {}", pool.active_miners, pool.capacity_slots)));
                        ui.add_sized([SHARES_W, ROW_H], egui::Label::new(pool.recent_shares.to_string()));
                        let you = if pool.your_active { format!("active: {}", pool.your_shares) } else if is_last { "selected".to_string() } else { "-".to_string() };
                        ui.add_sized([YOU_W, ROW_H], egui::Label::new(you));
                        ui.add_sized([MANAGER_W, ROW_H], egui::Label::new(egui::RichText::new(shorten_hash(&pool.manager_address)).monospace()));
                        ui.add_sized([PAID_W, ROW_H], egui::Label::new(format!("{} QUB", pool.total_paid_qub)));
                        ui.allocate_ui_with_layout(egui::vec2(ACTIONS_W, ROW_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if is_pool_mining {
                                if self.ui_icon_button_enabled(ui, true, "stop-mining", "Stop").clicked() { self.stop_mining(); }
                            } else {
                                let can_mine = self.pool_activation_ready() && self.snapshot.wallet_keys > 0 && self.miner.is_none() && (pool.open_slots > 0 || pool.your_active || is_last);
                                let icon = if is_last { "start-mining" } else { "join-pool" };
                                if self.ui_icon_button_enabled(ui, can_mine, icon, "Mine").clicked() {
                                    self.select_pool_for_gui(&pool);
                                    self.start_pool_mining(pool.pool_id.clone());
                                }
                            }
                            ui.menu_button("More", |ui| {
                                let can_join = self.pool_activation_ready() && self.snapshot.wallet_keys > 0 && (pool.open_slots > 0 || pool.your_active || is_last);
                                if ui.add_enabled(can_join, egui::Button::new("Join")).clicked() {
                                    self.select_pool_for_gui(&pool);
                                    self.pool_dialog.join_pool_id = pool.pool_id.clone();
                                    self.start_pool_join_dialog(false);
                                    ui.close();
                                }
                                if ui.add_enabled(pool.is_manager, egui::Button::new("Manage")).clicked() {
                                    self.select_pool_for_gui(&pool);
                                    self.pool_dialog.manage_open = true;
                                    ui.close();
                                }
                            });
                        });
                    });
                });
                ui.add_space(5.0);
            }
        });
    }

    fn ui_pool_create_window(&mut self, ctx: &egui::Context) {
        if !self.pool_dialog.create_open { return; }
        let mut open = self.pool_dialog.create_open;
        let mut want_close = false;
        egui::Window::new("Create pool")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| { self.ui_icon(ui, "create-pool", 20.0); ui.label(egui::RichText::new("Create deterministic non-custodial pool").strong()); });
                ui.small("Pool creation is a permanent on-chain action. The payment is non-refundable and split 50/50 between pools.qub and the miner who confirms the transaction.");
                ui.small("Pool names are not unique. Users should verify the Pool ID. Commission can only decrease later; capacity can only increase.");
                ui.add_space(8.0);
                self.ui_pools_create_section(ui);
                ui.add_space(8.0);
                self.ui_pool_action_status(ui);
                ui.horizontal(|ui| { if ui.button("Close").clicked() { want_close = true; } });
            });
        if want_close { open = false; }
        self.pool_dialog.create_open = open;
    }

    fn ui_pool_manage_window(&mut self, ctx: &egui::Context) {
        if !self.pool_dialog.manage_open { return; }
        let mut open = self.pool_dialog.manage_open;
        let mut want_close = false;
        egui::Window::new("Manage pool")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(660.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| { self.ui_icon(ui, "pool-capacity", 20.0); ui.label(egui::RichText::new("Manage your pool").strong()); });
                ui.small("Manager actions are signed locally and confirmed on-chain. Rename is visible after confirmation. Commission can only decrease; capacity top-up can only increase capacity.");
                ui.small("The PPLNS share window is fixed at 360 blocks. Miners who switch pools do not instantly transfer old share weight; it ages out deterministically.");
                ui.add_space(8.0);
                self.ui_pools_manage_section(ui);
                ui.add_space(8.0);
                self.ui_pool_action_status(ui);
                ui.horizontal(|ui| { if ui.button("Close").clicked() { want_close = true; } });
            });
        if want_close { open = false; }
        self.pool_dialog.manage_open = open;
    }

    fn ui_pools_create_section(&mut self, ui: &mut egui::Ui) {
        let locked = matches!(self.pool_dialog.status, SendDialogStatus::Sending | SendDialogStatus::Pending);
        ui.small("Pool names are not unique. The pool ID is the real identity. Emoji are allowed; invisible/control/bidi characters are rejected by the protocol.");
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label("Name");
                ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.pool_dialog.create_name).desired_width(260.0));
            });
            ui.vertical(|ui| {
                ui.label("Commission bps");
                ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.pool_dialog.create_commission_bps).desired_width(110.0));
                ui.small("500 = 5.00%, max 2000");
            });
            ui.vertical(|ui| {
                ui.label("Capacity slots");
                ui.add_enabled_ui(!locked, |ui| { ui.add(egui::DragValue::new(&mut self.pool_dialog.create_capacity_slots).range(8..=128).speed(8)); });
                ui.small("8-slot steps");
            });
            ui.vertical(|ui| {
                ui.label("Fee QUB");
                ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.pool_dialog.create_fee).desired_width(110.0));
            });
        });
        if self.pool_dialog.create_price_preview.is_empty() || self.pool_dialog.create_price_capacity_slots != self.pool_dialog.create_capacity_slots {
            self.pool_dialog.create_price_capacity_slots = self.pool_dialog.create_capacity_slots;
            self.pool_dialog.create_price_preview = pool_create_price_preview(&self.prefs.config_path, self.pool_dialog.create_capacity_slots);
        }
        ui.small(&self.pool_dialog.create_price_preview);
        ui.small("Non-refundable split: 50% to pools.qub protocol address, 50% to the miner as block fee. Commission can only decrease later.");
        let can_create = self.pool_activation_ready() && self.snapshot.wallet_keys > 0 && !locked;
        if self.ui_icon_button_enabled(ui, can_create, "create-pool", "Create pool").clicked() {
            self.start_pool_create_dialog();
        }
        if self.snapshot.wallet_keys == 0 { ui.colored_label(egui::Color32::from_rgb(255, 170, 90), "Create pool requires a local wallet key with spendable QUB."); }
    }

    fn ui_pools_manage_section(&mut self, ui: &mut egui::Ui) {
        let managed = self.snapshot.pools.iter().filter(|p| p.is_manager).cloned().collect::<Vec<_>>();
        if managed.is_empty() {
            ui.small("No pool managed by a local wallet key was found. Import the manager key or create a pool first.");
            return;
        }
        let selected = managed.iter().find(|p| p.pool_id == self.pool_dialog.manage_pool_id).cloned().or_else(|| managed.first().cloned());
        let Some(pool) = selected else { return; };
        if self.pool_dialog.manage_pool_id.is_empty() { self.select_pool_for_gui(&pool); }
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Attached pool:").weak());
            ui.label(egui::RichText::new(&pool.name).strong());
            ui.monospace(shorten_hash(&pool.pool_id));
        });
        ui.small(format!("Current: commission {:.2}% | capacity {} | active {} | open {}", pool.commission_bps as f64 / 100.0, pool.capacity_slots, pool.active_miners, pool.open_slots));
        let locked = matches!(self.pool_dialog.status, SendDialogStatus::Sending | SendDialogStatus::Pending);
        ui.separator();
        ui.label("Edit pool name");
        ui.horizontal(|ui| {
            ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.pool_dialog.rename_name).desired_width(320.0));
            let can_rename = self.pool_activation_ready() && !locked && !self.pool_dialog.rename_name.trim().is_empty();
            if ui.add_enabled(can_rename, egui::Button::new("Submit rename")).clicked() { self.start_pool_rename_dialog(); }
        });
        ui.label("Decrease commission");
        ui.horizontal(|ui| {
            ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.pool_dialog.new_commission_bps).desired_width(110.0));
            let parsed = self.pool_dialog.new_commission_bps.trim().parse::<u16>().ok();
            let can_decrease = self.pool_activation_ready() && !locked && parsed.map(|bps| bps <= pool.commission_bps).unwrap_or(false);
            if ui.add_enabled(can_decrease, egui::Button::new("Decrease commission")).clicked() { self.start_pool_commission_dialog(); }
        });
        ui.small("Commission cannot increase. The button is disabled unless the new bps is <= current bps.");
        ui.label("Pay extra for capacity increase");
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!locked, |ui| { ui.add(egui::DragValue::new(&mut self.pool_dialog.extra_capacity_slots).range(8..=128).speed(8)); });
            ui.label(pool_topup_price_preview(&self.prefs.config_path, self.pool_dialog.extra_capacity_slots));
            let can_topup = self.pool_activation_ready() && !locked && pool.capacity_slots.saturating_add(self.pool_dialog.extra_capacity_slots) <= 128;
            if ui.add_enabled(can_topup, egui::Button::new("Pay capacity top-up")).clicked() { self.start_pool_topup_dialog(); }
        });
        ui.horizontal(|ui| {
            ui.label("Action fee QUB");
            ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.pool_dialog.manage_fee).desired_width(120.0));
        });
    }

    fn ui_pools_mining_section(&mut self, ui: &mut egui::Ui) {
        let selected = self.selected_pool().cloned();
        if selected.is_none() {
            ui.small("Select a pool in Browse first.");
        }
        if let Some(pool) = selected {
            ui.small(format!("Selected pool: {} ({})", pool.name, shorten_hash(&pool.pool_id)));
            ui.small(format!("Your confirmed shares in window: {} | active: {}", pool.your_shares, if pool.your_active { "yes" } else { "no" }));
        }
        ui.horizontal(|ui| {
            ui.label("Miner address");
            ui.text_edit_singleline(&mut self.pool_dialog.miner_address);
        });
        ui.small("Join submits a zero-fee PoW-gated share. The first confirmed share makes this address active in the pool window. A network block must confirm the share before a pool block can pay it.");
        ui.horizontal_wrapped(|ui| {
            let can_join = self.pool_activation_ready() && self.snapshot.wallet_keys > 0 && !self.pool_dialog.selected_pool_id.trim().is_empty();
            if self.ui_icon_button_enabled(ui, can_join, "join-pool", "Join / submit share").clicked() {
                self.pool_dialog.join_pool_id = self.pool_dialog.selected_pool_id.clone();
                self.start_pool_join_dialog(false);
            }
            let can_start = can_join && self.miner.is_none();
            if self.ui_icon_button_enabled(ui, can_start, "start-mining", "Start pool mining").clicked() {
                self.start_pool_mining(self.pool_dialog.selected_pool_id.clone());
            }
            if self.miner.is_some() {
                if self.ui_icon_button(ui, "stop-mining", "Stop mining").clicked() { self.stop_mining(); }
            }
        });
        if self.miner.is_some() && !self.pool_mining_pool_id.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(66, 220, 120), format!("Currently pool mining {}", shorten_hash(&self.pool_mining_pool_id)));
        }
        if self.snapshot.wallet_keys == 0 {
            ui.colored_label(egui::Color32::from_rgb(255, 170, 90), "Pool join/mining is disabled because this Core has no local private key.");
        }
    }

    fn ui_pool_action_status(&mut self, ui: &mut egui::Ui) {
        match self.pool_dialog.status {
            SendDialogStatus::Editing => {
                if !self.pool_dialog.message.is_empty() { ui.label(&self.pool_dialog.message); }
            }
            SendDialogStatus::Sending => { ui.spinner(); ui.label(&self.pool_dialog.message); }
            SendDialogStatus::Pending => {
                ui.spinner();
                ui.label(egui::RichText::new(format!("Pool {} pending", self.pool_dialog.action)).strong());
                ui.monospace(&self.pool_dialog.txid);
                ui.label(&self.pool_dialog.message);
            }
            SendDialogStatus::Failed => {
                ui.horizontal(|ui| { self.ui_icon(ui, "failed", 18.0); ui.colored_label(egui::Color32::from_rgb(255,105,105), "Pool action failed"); });
                ui.label(&self.pool_dialog.message);
                if ui.button("Reset pool action").clicked() { self.pool_dialog.status = SendDialogStatus::Editing; self.pool_dialog.message.clear(); self.pool_dialog.txid.clear(); }
            }
            SendDialogStatus::Confirmed => {
                ui.horizontal(|ui| { self.ui_icon(ui, "success", 18.0); ui.colored_label(egui::Color32::from_rgb(66,220,120), "Confirmed"); });
                ui.monospace(&self.pool_dialog.txid);
                ui.label(&self.pool_dialog.message);
                if ui.button("New pool action").clicked() { self.pool_dialog.status = SendDialogStatus::Editing; self.pool_dialog.message.clear(); self.pool_dialog.txid.clear(); }
            }
        }
    }


    fn ui_buy_jin_window(&mut self, ctx: &egui::Context) {
        if !self.buy_jin_dialog.open { return; }
        let mut open = self.buy_jin_dialog.open;
        let mut want_close = false;
        egui::Window::new("Buy JIN")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(980.0)
            .default_height(720.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    self.ui_icon(ui, "jin", 26.0);
                    ui.label(egui::RichText::new("JIN Public Protocol Sale").size(22.0).strong());
                    if ui.button("Refresh listings").clicked() { self.request_buy_jin_listings(ctx, true); }
                });
                ui.small("85,000,000 JIN Coin public sale from the JIN protocol reserve. Payments are in QUB only. Each purchase pays a deterministic 0.1% protocol fee split 50/50: 0.05% as miner fee and 0.05% to the JIN protocol address.");
                ui.small("JIN Token bridge conversion is disabled until the Enjin Matrixchain bridge is live. Do not manually send funds to bridge addresses.");
                ui.separator();
                if self.buy_jin_dialog.loading { ui.small("Loading sale listings in the background..."); }
                if !self.buy_jin_dialog.message.is_empty() { ui.label(&self.buy_jin_dialog.message); }
                let listings = self.buy_jin_dialog.listings.clone();
                if listings.is_empty() {
                    ui.add_space(8.0);
                    ui.small("No local listing index loaded yet. Click Refresh listings.");
                    return;
                }

                let total_remaining: u128 = listings.iter().map(|l| l.remaining_units).sum();
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(format!("{} listing(s)", listings.len())).strong());
                    ui.separator();
                    ui.label(format!("Remaining: {} JIN", format_jin_amount(total_remaining)));
                    ui.separator();
                    if let Some(first) = listings.iter().find(|l| l.remaining_units > 0) {
                        ui.label(format!("Current floor: {} QUB / JIN", first.price_qub_per_jin));
                    }
                });

                ui.add_space(8.0);
                ui.label(egui::RichText::new("Price curve / remaining volume").strong());
                let max_remaining = listings.iter().map(|l| l.remaining_units).max().unwrap_or(1).max(1);
                let chart_height = 110.0;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(200.0), chart_height), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, egui::Color32::from_gray(70)), egui::StrokeKind::Inside);
                let visible = listings.iter().take(85).collect::<Vec<_>>();
                let bar_w = (rect.width() / visible.len().max(1) as f32).max(2.0);
                for (idx, listing) in visible.iter().enumerate() {
                    let frac = (listing.remaining_units as f64 / max_remaining as f64).clamp(0.0, 1.0) as f32;
                    let h = (rect.height() - 14.0) * frac;
                    let x0 = rect.left() + idx as f32 * bar_w;
                    let r = egui::Rect::from_min_max(
                        egui::pos2(x0, rect.bottom() - h - 4.0),
                        egui::pos2((x0 + bar_w - 1.0).min(rect.right()), rect.bottom() - 4.0)
                    );
                    let color = if listing.listing_id == self.buy_jin_dialog.selected_listing { egui::Color32::from_rgb(0, 170, 255) } else { egui::Color32::from_rgb(46, 130, 190) };
                    painter.rect_filled(r, 1.0, color);
                }

                ui.add_space(8.0);
                ui.columns(2, |cols| {
                    cols[0].vertical(|ui| {
                        ui.label(egui::RichText::new("Listings").strong());
                        let per_page = 12usize;
                        let pages = (listings.len() + per_page - 1) / per_page;
                        self.buy_jin_dialog.page = self.buy_jin_dialog.page.min(pages.saturating_sub(1));
                        ui.horizontal(|ui| {
                            if ui.button("< Prev").clicked() { self.buy_jin_dialog.page = self.buy_jin_dialog.page.saturating_sub(1); }
                            ui.label(format!("Page {} / {}", self.buy_jin_dialog.page + 1, pages.max(1)));
                            if ui.button("Next >").clicked() && self.buy_jin_dialog.page + 1 < pages { self.buy_jin_dialog.page += 1; }
                        });
                        ui.separator();
                        let start = self.buy_jin_dialog.page * per_page;
                        let end = (start + per_page).min(listings.len());
                        egui::ScrollArea::vertical().max_height(330.0).show(ui, |ui| {
                            for listing in &listings[start..end] {
                                let selected = listing.listing_id == self.buy_jin_dialog.selected_listing;
                                egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.selectable_label(selected, format!("Batch #{}", listing.listing_id + 1)).clicked() {
                                            self.buy_jin_dialog.selected_listing = listing.listing_id;
                                        }
                                        ui.label(format!("{} QUB/JIN", listing.price_qub_per_jin));
                                    });
                                    ui.small(format!("Remaining {} / total {}", listing.remaining_jin, listing.total_jin));
                                    ui.small(format!("Sold {}", listing.sold_jin));
                                });
                                ui.add_space(4.0);
                            }
                        });
                    });
                    cols[1].vertical(|ui| {
                        ui.label(egui::RichText::new("Buy selected listing").strong());
                        let selected = listings.iter().find(|l| l.listing_id == self.buy_jin_dialog.selected_listing).cloned();
                        if let Some(listing) = selected {
                            ui.small(format!("Selected batch #{} at {} QUB per JIN", listing.listing_id + 1, listing.price_qub_per_jin));
                            ui.small(format!("Remaining: {}", listing.remaining_jin));
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| { ui.label("Amount JIN"); ui.text_edit_singleline(&mut self.buy_jin_dialog.amount_jin); });
                        ui.horizontal(|ui| { ui.label("Extra QUB fee"); ui.text_edit_singleline(&mut self.buy_jin_dialog.fee); });
                        if let Some(listing) = listings.iter().find(|l| l.listing_id == self.buy_jin_dialog.selected_listing) {
                            let preview_key = format!("{}|{}|{}", listing.listing_id, self.buy_jin_dialog.amount_jin.trim(), self.prefs.config_path);
                            if preview_key != self.buy_jin_dialog.preview_key {
                                self.buy_jin_dialog.preview_key = preview_key;
                                self.buy_jin_dialog.preview_lines.clear();
                                match parse_jin_amount(self.buy_jin_dialog.amount_jin.trim()) {
                                    Ok(units) => match jin_swap_sale_price_atoms_for_ui(&self.prefs.config_path, listing.listing_id, units) {
                                        Ok(price_atoms) => {
                                            let price = Amount::from_atoms(price_atoms).map(|a| a.to_string()).unwrap_or_else(|_| price_atoms.to_string());
                                            self.buy_jin_dialog.preview_lines.push(format!("Base price: {price} QUB"));
                                            if let Ok((protocol_fee, miner_fee)) = jin_swap_fee_split_for_ui(&self.prefs.config_path, price_atoms) {
                                                self.buy_jin_dialog.preview_lines.push(format!("0.1% fee split: {} QUB to JIN protocol + {} QUB to miner fee", Amount::from_atoms(protocol_fee).map(|a| a.to_string()).unwrap_or_default(), Amount::from_atoms(miner_fee).map(|a| a.to_string()).unwrap_or_default()));
                                            }
                                        }
                                        Err(err) => self.buy_jin_dialog.preview_lines.push(format!("Preview unavailable: {err:#}")),
                                    },
                                    Err(err) => self.buy_jin_dialog.preview_lines.push(format!("Invalid JIN amount: {err:#}")),
                                }
                            }
                            for line in &self.buy_jin_dialog.preview_lines { ui.small(line); }
                        }
                        ui.add_space(8.0);
                        let can_buy = self.snapshot.wallet_keys > 0 && self.buy_jin_dialog.status != SendDialogStatus::Sending;
                        if ui.add_enabled(can_buy, egui::Button::new(egui::RichText::new("Buy JIN").strong().color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(0, 150, 220))).clicked() {
                            self.start_buy_jin_purchase();
                        }
                        if ui.button("Close").clicked() { want_close = true; }
                    });
                });
            });
        if want_close { open = false; }
        self.buy_jin_dialog.open = open;
    }

    fn ui_qns_window(&mut self, ctx: &egui::Context) {
        if !self.qns_dialog.open { return; }
        let mut open = self.qns_dialog.open;
        let mut want_close = false;
        egui::Window::new("Register QNS name")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Permanent .qub registration").strong());
                ui.small(format!("Network: {} - Registered names: {} - Pools: {}", self.snapshot.network, self.snapshot.qns_count, self.snapshot.pools_count));
                ui.add_space(8.0);
                let locked = matches!(self.qns_dialog.status, QnsDialogStatus::Sending | QnsDialogStatus::Pending | QnsDialogStatus::Confirmed);
                ui.label("Name");
                if ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.qns_dialog.name).hint_text("example.qub")).changed() {
                    self.qns_dialog.price = "Click Calculate cost.".to_string();
                }
                ui.label("Target address");
                ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.qns_dialog.target_address).hint_text("qub1..."));
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| { self.ui_icon(ui, "qub", 16.0); ui.label("Fee QUB"); });
                        ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.qns_dialog.fee).desired_width(150.0));
                    });
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| { self.ui_icon(ui, "qub", 16.0); ui.label("Deterministic QNS price"); });
                        let price_ready = self.qns_dialog.price.contains(" QUB") || self.qns_dialog.price.starts_with("error:");
                        if price_ready {
                            ui.monospace(&self.qns_dialog.price);
                            if !locked && ui.small_button("Recalculate cost").clicked() {
                                self.update_qns_price_preview();
                            }
                        } else {
                            let can_calculate = !locked && !self.qns_dialog.name.trim().is_empty();
                            if self.ui_icon_button_enabled(ui, can_calculate, "qub", "Calculate cost").clicked() {
                                self.update_qns_price_preview();
                            }
                        }
                    });
                });
                ui.small(format!("Protocol: {} -> {}", self.snapshot.qns_protocol_name, self.snapshot.qns_protocol_address));
                ui.add_space(10.0);
                match self.qns_dialog.status {
                    QnsDialogStatus::Editing => {
                        ui.small("Names are permanent on the target address. Only latin letters a-z and digits 0-9 are accepted; max 32 label chars. QUB Core does not block on a full sync before registration; the network rejects invalid/stale attempts.");
                        if self.miner.is_some() { ui.colored_label(egui::Color32::from_rgb(255, 170, 90), "Mining is running. Registration can briefly make the GUI feel heavier while the local chain and mempool update."); }
                        ui.horizontal(|ui| {
                            if self.ui_icon_button_enabled(ui, self.snapshot.wallet_keys > 0, "register-qub", "Register").clicked() { self.start_qns_dialog(); }
                            if ui.button("Cancel").clicked() { want_close = true; }
                        });
                    }
                    QnsDialogStatus::Sending => { ui.spinner(); ui.label(&self.qns_dialog.message); }
                    QnsDialogStatus::Pending => { ui.spinner(); ui.label("Pending confirmation"); ui.monospace(&self.qns_dialog.txid); ui.label(&self.qns_dialog.message); if ui.button("Close").clicked() { want_close = true; } }
                    QnsDialogStatus::Failed => { ui.horizontal(|ui| { self.ui_icon(ui, "failed", 18.0); ui.colored_label(egui::Color32::from_rgb(255,105,105), "Failed"); }); ui.label(&self.qns_dialog.message); ui.horizontal(|ui| { if ui.button("Close").clicked() { want_close = true; } if ui.button("Retry").clicked() { self.qns_dialog.status = QnsDialogStatus::Editing; self.qns_dialog.message.clear(); self.qns_dialog.txid.clear(); } }); }
                    QnsDialogStatus::Confirmed => { ui.horizontal(|ui| { self.ui_icon(ui, "success", 18.0); ui.colored_label(egui::Color32::from_rgb(66,220,120), "Success"); }); ui.monospace(&self.qns_dialog.txid); ui.label(&self.qns_dialog.message); if ui.button("Close").clicked() { want_close = true; } }
                }
            });
        if want_close { open = false; }
        self.qns_dialog.open = open;
    }

    fn ui_conversion_window(&mut self, ctx: &egui::Context) {
        if !self.conversion_dialog.open { return; }
        let mut open = self.conversion_dialog.open;
        let mut want_close = false;
        egui::Window::new("Convert JIN Coin to JIN Token")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| { self.ui_icon(ui, "convert", 18.0); ui.label(egui::RichText::new("JIN Coin -> JIN Token conversion request").strong()); });
                ui.small("This burns/locks native JIN Coin on Qubit Chain and creates an on-chain bridge request for 1:1 JIN Token payout on Enjin Matrixchain. JIN Token is integer-only, so the amount must be whole JIN.");
                ui.add_space(8.0);
                let locked = matches!(self.conversion_dialog.status, SendDialogStatus::Sending | SendDialogStatus::Pending | SendDialogStatus::Confirmed);
                ui.label("Enjin Matrixchain address / account id");
                ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.conversion_dialog.matrix_address).desired_width(500.0));
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| { self.ui_icon(ui, "jin", 16.0); ui.label("Amount JIN Coin"); });
                        ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.conversion_dialog.amount).desired_width(160.0));
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.label("Fee asset");
                        egui::ComboBox::from_id_salt("conversion_fee_asset_combo")
                            .selected_text(&self.conversion_dialog.fee_asset)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.conversion_dialog.fee_asset, "JIN".to_string(), "JIN");
                                ui.selectable_value(&mut self.conversion_dialog.fee_asset, "QUB".to_string(), "QUB");
                            });
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        let fee_is_jin = self.conversion_dialog.fee_asset.eq_ignore_ascii_case("JIN");
                        ui.horizontal(|ui| { self.ui_icon(ui, if fee_is_jin { "jin" } else { "qub" }, 16.0); ui.label(format!("Fee {}", if fee_is_jin { "JIN" } else { "QUB" })); });
                        ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.conversion_dialog.fee).desired_width(160.0));
                    });
                });
                ui.add_space(10.0);
                match self.conversion_dialog.status {
                    SendDialogStatus::Editing => {
                        ui.small("The request is signed locally and relayed like any other QUB transaction. Do not use this until the Enjin-side claim flow is live unless you are testing.");
                        ui.horizontal(|ui| {
                            if self.ui_icon_button_enabled(ui, self.snapshot.wallet_keys > 0, "convert", "Create conversion request").clicked() { self.start_conversion_dialog(); }
                            if ui.button("Cancel").clicked() { want_close = true; }
                        });
                    }
                    SendDialogStatus::Sending => { ui.spinner(); ui.label(&self.conversion_dialog.message); }
                    SendDialogStatus::Pending => { ui.spinner(); ui.label("Pending confirmation"); ui.monospace(&self.conversion_dialog.txid); ui.label(&self.conversion_dialog.message); if ui.button("Close").clicked() { want_close = true; } }
                    SendDialogStatus::Failed => { ui.horizontal(|ui| { self.ui_icon(ui, "failed", 18.0); ui.colored_label(egui::Color32::from_rgb(255,105,105), "Failed"); }); ui.label(&self.conversion_dialog.message); ui.horizontal(|ui| { if ui.button("Close").clicked() { want_close = true; } if ui.button("Retry").clicked() { self.conversion_dialog.status = SendDialogStatus::Editing; self.conversion_dialog.message.clear(); self.conversion_dialog.txid.clear(); } }); }
                    SendDialogStatus::Confirmed => { ui.horizontal(|ui| { self.ui_icon(ui, "success", 18.0); ui.colored_label(egui::Color32::from_rgb(66,220,120), "Confirmed"); }); ui.monospace(&self.conversion_dialog.txid); ui.label(&self.conversion_dialog.message); if ui.button("Close").clicked() { want_close = true; } }
                }
            });
        if want_close { open = false; }
        self.conversion_dialog.open = open;
    }

    fn ui_send_window(&mut self, ctx: &egui::Context) {
        if !self.send_dialog.open { return; }
        let mut open = self.send_dialog.open;
        let mut want_close = false;
        egui::Window::new("Send QUB")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                self.ui_send_dialog_contents(ui, &mut want_close);
            });
        if want_close { open = false; }
        self.send_dialog.open = open;
    }

    fn ui_send_status_and_actions(&mut self, ui: &mut egui::Ui, want_close: &mut bool) {
        if matches!(self.send_dialog.status, SendDialogStatus::Editing) && !self.send_dialog.message.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(255, 170, 90), &self.send_dialog.message);
        }
        ui.add_space(10.0);
        match self.send_dialog.status {
            SendDialogStatus::Editing => {
                if self.miner.is_some() {
                    ui.colored_label(egui::Color32::from_rgb(255, 170, 90), "Mining is running. Send/Blast can briefly make the GUI feel heavier while local chain and mempool update.");
                }
                ui.horizontal(|ui| {
                    let label = match self.send_dialog.send_mode { SendMode::Single => "Send", SendMode::Multi => "Create multi-send", SendMode::Blast => if self.send_dialog.blast_create_mode { "Create Blast" } else { "Claim Blast" } };
                    if self.ui_icon_button_enabled(ui, self.snapshot.wallet_keys > 0, "send", label).clicked() { self.start_send_dialog(); }
                    if ui.button("Cancel").clicked() { *want_close = true; }
                });
                if self.snapshot.wallet_keys == 0 { ui.colored_label(egui::Color32::from_rgb(255, 170, 90), "Send is disabled because this QUB Core has no local private key wallet."); }
            }
            SendDialogStatus::Sending => { ui.spinner(); ui.label(&self.send_dialog.message); if ui.button("Cancel view").clicked() { *want_close = true; } }
            SendDialogStatus::Pending => { ui.spinner(); ui.label(egui::RichText::new("Pending confirmation").strong()); ui.monospace(&self.send_dialog.txid); ui.label(&self.send_dialog.message); if ui.button("Close").clicked() { *want_close = true; } }
            SendDialogStatus::Failed => { ui.horizontal(|ui| { self.ui_icon(ui, "failed", 18.0); ui.colored_label(egui::Color32::from_rgb(255, 105, 105), "Failed"); }); ui.label(&self.send_dialog.message); ui.horizontal(|ui| { if ui.button("Close").clicked() { *want_close = true; } if ui.button("Retry").clicked() { self.send_dialog.status = SendDialogStatus::Editing; self.send_dialog.message.clear(); self.send_dialog.txid.clear(); } }); }
            SendDialogStatus::Confirmed => { ui.horizontal(|ui| { self.ui_icon(ui, "success", 18.0); ui.colored_label(egui::Color32::from_rgb(66, 220, 120), "Success"); }); ui.monospace(&self.send_dialog.txid); ui.label(&self.send_dialog.message); if ui.button("Close").clicked() { *want_close = true; } }
        }
    }

    fn ui_send_dialog_contents(&mut self, ui: &mut egui::Ui, want_close: &mut bool) {
        ui.label(egui::RichText::new("Wallet transfer").strong());
        ui.small(format!("Network: {} - Spendable: {} QUB | {} JIN", self.snapshot.network, self.snapshot.spendable, self.snapshot.jin_total));
        ui.add_space(8.0);

        let locked = matches!(self.send_dialog.status, SendDialogStatus::Sending | SendDialogStatus::Pending | SendDialogStatus::Confirmed);

        ui.horizontal(|ui| {
            let single = ui.selectable_label(self.send_dialog.send_mode == SendMode::Single, egui::RichText::new("Single").strong());
            let multi = ui.selectable_label(self.send_dialog.send_mode == SendMode::Multi, egui::RichText::new("Multi").strong());
            let blast = ui.selectable_label(self.send_dialog.send_mode == SendMode::Blast, egui::RichText::new("Blast").strong());
            if !locked && single.clicked() { self.send_dialog.send_mode = SendMode::Single; self.send_dialog.message.clear(); }
            if !locked && multi.clicked() { self.send_dialog.send_mode = SendMode::Multi; self.send_dialog.message.clear(); }
            if !locked && blast.clicked() { self.send_dialog.send_mode = SendMode::Blast; self.send_dialog.message.clear(); }
        });
        ui.small(match self.send_dialog.send_mode {
            SendMode::Single => "Single: one asset to one recipient.",
            SendMode::Multi => "Multi: up to 256 recipients, one entry per line: address_or_qns,amount.",
            SendMode::Blast => "Blast: creator locks a QUB vault and receives a private claim code / QR payload. Claimants can claim without QUB fee.",
        });
        ui.add_space(8.0);

        if self.send_dialog.send_mode == SendMode::Multi {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label("Asset");
                    egui::ComboBox::from_id_salt("send_multi_asset_combo").selected_text(&self.send_dialog.asset).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.send_dialog.asset, "QUB".to_string(), "QUB");
                        ui.selectable_value(&mut self.send_dialog.asset, "JIN".to_string(), "JIN");
                    });
                });
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label("Fee");
                    ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.send_dialog.fee).desired_width(120.0));
                });
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label("Fee asset");
                    ui.add_enabled_ui(!locked && self.send_dialog.asset.eq_ignore_ascii_case("JIN"), |ui| {
                        egui::ComboBox::from_id_salt("send_multi_fee_asset_combo").selected_text(&self.send_dialog.fee_asset).show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.send_dialog.fee_asset, "JIN".to_string(), "JIN");
                            ui.selectable_value(&mut self.send_dialog.fee_asset, "QUB".to_string(), "QUB");
                        });
                    });
                    if !self.send_dialog.asset.eq_ignore_ascii_case("JIN") { self.send_dialog.fee_asset = "QUB".to_string(); }
                });
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Recipients").strong());
                ui.small(format!("{} / {} entries", self.send_dialog.multi_rows.iter().filter(|r| !r.recipient.trim().is_empty() || !r.amount.trim().is_empty()).count(), MAX_SEND_ENTRIES_PER_TX));
                if ui.add_enabled(!locked && self.send_dialog.multi_rows.len() < MAX_SEND_ENTRIES_PER_TX, egui::Button::new("+ Add row")).clicked() {
                    self.send_dialog.multi_rows.push(MultiSendRow::default());
                }
            });

            if self.send_dialog.multi_rows.is_empty() { self.send_dialog.multi_rows.push(MultiSendRow::default()); }
            let mut remove_row: Option<usize> = None;
            let mut resolve_row: Option<(usize, String)> = None;
            let multi_resolve_idle = self.multi_qns_resolve_rx.is_none();
            egui::Grid::new("multi_send_rows_grid")
                .num_columns(5)
                .striped(true)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.small("#");
                    ui.small("Recipient address or .qub name");
                    ui.small(format!("Amount ({})", self.send_dialog.asset));
                    ui.small("Resolve");
                    ui.small("");
                    ui.end_row();

                    let can_remove_rows = self.send_dialog.multi_rows.len() > 1;
                    let amount_hint = if self.send_dialog.asset.eq_ignore_ascii_case("JIN") { "1 JIN" } else { "1 QUB" };
                    for (idx, row) in self.send_dialog.multi_rows.iter_mut().enumerate() {
                        ui.small(format!("{}", idx + 1));
                        let response = ui.add_enabled(!locked, egui::TextEdit::singleline(&mut row.recipient).hint_text("qub1... or recipient.qub").desired_width(260.0));
                        if response.changed() {
                            row.resolved_address.clear();
                            row.resolve_message.clear();
                        }
                        ui.add_enabled(!locked, egui::TextEdit::singleline(&mut row.amount).hint_text(amount_hint).desired_width(120.0));
                        let qns_input = row.recipient.trim().to_ascii_lowercase().ends_with(".qub");
                        if row.resolving {
                            ui.spinner();
                        } else if ui.add_enabled(!locked && qns_input && multi_resolve_idle, egui::Button::new("Resolve")).clicked() {
                            resolve_row = Some((idx, row.recipient.trim().to_string()));
                        } else {
                            ui.add_enabled(false, egui::Button::new("Resolve"));
                        }
                        if ui.add_enabled(!locked && can_remove_rows, egui::Button::new("Remove")).clicked() {
                            remove_row = Some(idx);
                        }
                        ui.end_row();
                        if !row.resolve_message.trim().is_empty() {
                            ui.small("");
                            ui.small(&row.resolve_message);
                            ui.small("");
                            ui.small("");
                            ui.small("");
                            ui.end_row();
                        }
                    }
                });
            if let Some(idx) = remove_row {
                if idx < self.send_dialog.multi_rows.len() { self.send_dialog.multi_rows.remove(idx); }
                if self.send_dialog.multi_rows.is_empty() { self.send_dialog.multi_rows.push(MultiSendRow::default()); }
            }
            if let Some((idx, recipient)) = resolve_row {
                if idx < self.send_dialog.multi_rows.len() {
                    self.start_multi_qns_resolve(idx, recipient);
                }
            }
            self.send_dialog.multi_entries = self.send_dialog.multi_rows.iter()
                .map(|row| (row.recipient.trim(), row.amount.trim()))
                .filter(|(recipient, amount)| !recipient.is_empty() || !amount.is_empty())
                .map(|(recipient, amount)| format!("{recipient},{amount}"))
                .collect::<Vec<_>>()
                .join("\n");
            ui.small("Hard cap: 256 entries. QNS names are resolved before signing. Amount is the asset amount, not part of the name.");
            if self.send_dialog.asset.eq_ignore_ascii_case("JIN") {
                ui.small("Safety: JIN sends are authorized by the source address. QUB Core auto-selects a wallet address that has enough JIN and spendable QUB marker dust; if it fails, send a tiny QUB amount to that same JIN address and retry.");
            }
            self.ui_send_status_and_actions(ui, want_close);
            return;
        }

        if self.send_dialog.send_mode == SendMode::Blast {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.send_dialog.blast_create_mode, true, "Create Blast");
                ui.selectable_value(&mut self.send_dialog.blast_create_mode, false, "Claim Blast");
            });
            if self.send_dialog.blast_create_mode {
                ui.small(format!("Blast activates at mainnet #{}. QUB Core keeps Blast v1 QUB-only: creator pays the QUB lock + fee; claimants do not need QUB for the claim fee.", MAINNET_BLAST_ACTIVATION_HEIGHT));
                ui.horizontal(|ui| {
                    ui.vertical(|ui| { ui.label("Total QUB to lock"); ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.send_dialog.blast_total).desired_width(150.0)); });
                    ui.vertical(|ui| { ui.label("QUB per claim"); ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.send_dialog.blast_per_claim).desired_width(150.0)); });
                    let max_claims_preview = compute_blast_max_claims_text(&self.send_dialog.blast_total, &self.send_dialog.blast_per_claim).unwrap_or_else(|_| "-".to_string());
                    self.send_dialog.blast_max_claims = max_claims_preview.clone();
                    ui.vertical(|ui| { ui.label("Max claims"); ui.add_enabled(false, egui::TextEdit::singleline(&mut self.send_dialog.blast_max_claims).desired_width(100.0)); });
                    ui.vertical(|ui| { ui.label("Fee QUB"); ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.send_dialog.fee).desired_width(100.0)); });
                });

                ui.label("Private code");
                ui.horizontal(|ui| {
                    ui.add_enabled(false, egui::TextEdit::singleline(&mut self.send_dialog.blast_private_code).desired_width(f32::INFINITY));
                    if ui.add_enabled(!self.send_dialog.blast_private_code.trim().is_empty(), egui::Button::new("Copy")).clicked() {
                        ui.ctx().copy_text(self.send_dialog.blast_private_code.clone());
                    }
                });
                if self.send_dialog.blast_private_code.trim().is_empty() {
                    ui.small("Generated locally when you create the Blast vault, then saved in your local blast-codes.json history.");
                }

                if !self.send_dialog.blast_last_claim_payload.trim().is_empty() {
                    ui.add_space(4.0);
                    ui.label("Claim code / QR payload");
                    let payload = self.send_dialog.blast_last_claim_payload.clone();
                    ui.horizontal(|ui| {
                        ui.add_enabled(false, egui::TextEdit::singleline(&mut self.send_dialog.blast_last_claim_payload).desired_width(f32::INFINITY));
                        if ui.button("Copy").clicked() { ui.ctx().copy_text(payload.clone()); }
                        if ui.button(if self.send_dialog.blast_show_qr { "Hide QR code" } else { "View QR code" }).clicked() {
                            self.send_dialog.blast_show_qr = !self.send_dialog.blast_show_qr;
                        }
                    });
                    if self.send_dialog.blast_show_qr {
                        if let Some(tex) = self.qr_texture_for_address(ui.ctx(), &payload) {
                            let sized = egui::load::SizedTexture::new(tex.id(), egui::vec2(220.0, 220.0));
                            ui.add(egui::Image::from_texture(sized));
                        }
                    }
                }

                let saved_codes = load_blast_code_records_for_gui(&self.prefs.config_path);
                if !saved_codes.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new("Saved Blast codes on this device").strong());
                    ui.small("Stored locally in data/mainnet/blast-codes.json. Back up this file if you create unpublished Blast codes.");
                    egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                        for rec in saved_codes.iter().rev().take(12) {
                            ui.group(|ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.small(format!("tx {}", shorten_hash(&rec.txid)));
                                    if ui.button("Copy code").clicked() { ui.ctx().copy_text(rec.private_code.clone()); }
                                    if ui.button("Copy claim payload").clicked() { ui.ctx().copy_text(rec.claim_payload.clone()); }
                                    if ui.button("Load").clicked() {
                                        self.send_dialog.blast_private_code = rec.private_code.clone();
                                        self.send_dialog.blast_last_claim_payload = rec.claim_payload.clone();
                                        self.send_dialog.blast_show_qr = false;
                                    }
                                });
                                ui.monospace(&rec.claim_payload);
                            });
                        }
                    });
                }
            } else {
                ui.label("Blast claim code / QR payload");
                ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.send_dialog.blast_claim_code).hint_text("QUBBLAST1|txid|vout|code").desired_width(f32::INFINITY));
                ui.label("Claim to address (optional; blank = default wallet address)");
                ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.send_dialog.blast_claimant_address).desired_width(f32::INFINITY));
            }
            self.ui_send_status_and_actions(ui, want_close);
            return;
        }

        ui.label("Recipient address");
        ui.horizontal(|ui| {
            let recipient_response = ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.send_dialog.recipient).hint_text("qub1... or name.qub"));
            if recipient_response.changed() {
                self.send_dialog.resolved_address.clear();
                if matches!(self.send_dialog.status, SendDialogStatus::Editing) {
                    self.send_dialog.message.clear();
                }
            }
            let scan_resp = if let Some(texture) = self.icons.get("qr-scan", ui.visuals().dark_mode) {
                let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(16.0, 16.0));
                ui.add_enabled(!locked, egui::Button::image_and_text(egui::Image::from_texture(sized), ""))
            } else {
                ui.add_enabled(!locked, egui::Button::new("Scan"))
            };
            if scan_resp.clicked() {
                if self.qr_camera_devices.is_empty() { self.qr_camera_devices = detect_camera_devices(); self.qr_camera_selected = 0; }
                self.qr_scan_dialog_open = true;
            }
        });
        let recipient_trimmed = self.send_dialog.recipient.trim().to_string();
        let looks_like_qns = recipient_trimmed.to_ascii_lowercase().ends_with(".qub");
        if !self.send_dialog.resolved_address.is_empty() {
            ui.horizontal_wrapped(|ui| {
                self.ui_icon(ui, "success", 14.0);
                ui.small("Resolved address");
                ui.monospace(&self.send_dialog.resolved_address);
            });
        } else {
            let can_resolve = !locked && looks_like_qns && !self.send_qns_resolve_in_flight;
            let resolve_label = if self.send_qns_resolve_in_flight { "Resolving..." } else { "Resolve QNS" };
            if self.ui_icon_button_enabled(ui, can_resolve, "qns", resolve_label).clicked() {
                self.start_send_qns_resolve(recipient_trimmed.clone());
            }
            if !recipient_trimmed.is_empty() && !looks_like_qns {
                ui.small("Direct QUB address detected. QNS resolve is only needed for .qub names.");
            }
        }
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label("Asset");
                egui::ComboBox::from_id_salt("send_asset_combo").selected_text(&self.send_dialog.asset).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.send_dialog.asset, "QUB".to_string(), "QUB");
                    ui.selectable_value(&mut self.send_dialog.asset, "JIN".to_string(), "JIN");
                });
            });
            ui.add_space(10.0);
            ui.vertical(|ui| {
                let is_jin = self.send_dialog.asset.eq_ignore_ascii_case("JIN");
                ui.horizontal(|ui| { self.ui_icon(ui, if is_jin { "jin" } else { "qub" }, 16.0); ui.label(format!("Amount {}", if is_jin { "JIN" } else { "QUB" })); });
                ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.send_dialog.amount).desired_width(150.0));
            });
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.label("Fee asset");
                let fee_combo_enabled = !locked && self.send_dialog.asset.eq_ignore_ascii_case("JIN");
                ui.add_enabled_ui(fee_combo_enabled, |ui| {
                    egui::ComboBox::from_id_salt("send_fee_asset_combo").selected_text(&self.send_dialog.fee_asset).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.send_dialog.fee_asset, "JIN".to_string(), "JIN");
                        ui.selectable_value(&mut self.send_dialog.fee_asset, "QUB".to_string(), "QUB");
                    });
                });
                if !self.send_dialog.asset.eq_ignore_ascii_case("JIN") { self.send_dialog.fee_asset = "QUB".to_string(); }
            });
            ui.add_space(10.0);
            ui.vertical(|ui| {
                let fee_is_jin = self.send_dialog.asset.eq_ignore_ascii_case("JIN") && self.send_dialog.fee_asset.eq_ignore_ascii_case("JIN");
                ui.horizontal(|ui| { self.ui_icon(ui, if fee_is_jin { "jin" } else { "qub" }, 16.0); ui.label(format!("Fee {}", if fee_is_jin { "JIN" } else { "QUB" })); });
                ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.send_dialog.fee).desired_width(150.0));
            });
        });

        if matches!(self.send_dialog.status, SendDialogStatus::Editing) && !self.send_dialog.message.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(255, 170, 90), &self.send_dialog.message);
        }

        ui.add_space(10.0);
        match self.send_dialog.status {
            SendDialogStatus::Editing => {
                ui.small("The transaction will be signed locally, added to your mempool, relayed to peers, then tracked until confirmed. If you type a .qub name, use Resolve QNS first to verify the destination address before sending.");
                if self.send_dialog.asset.eq_ignore_ascii_case("JIN") {
                    ui.small("Safety: JIN transfer source is selected by address, not by total-wallet balance. The source address must have enough JIN plus tiny spendable QUB dust for the marker/authorization. JIN fee asset remains supported when enabled.");
                }
                if self.miner.is_some() {
                    ui.colored_label(egui::Color32::from_rgb(255, 170, 90), "Mining is running. Send/Register can briefly make the GUI feel heavier while the local chain and mempool update.");
                }
                ui.horizontal(|ui| {
                    if self.ui_icon_button_enabled(ui, self.snapshot.wallet_keys > 0, "send", "Send").clicked() {
                        self.start_send_dialog();
                    }
                    if ui.button("Cancel").clicked() { *want_close = true; }
                });
                if self.snapshot.wallet_keys == 0 {
                    ui.colored_label(egui::Color32::from_rgb(255, 170, 90), "Send is disabled because this QUB Core has no local private key wallet.");
                }
            }
            SendDialogStatus::Sending => {
                ui.spinner();
                ui.label(&self.send_dialog.message);
                if ui.button("Cancel view").clicked() { *want_close = true; }
            }
            SendDialogStatus::Pending => {
                ui.spinner();
                ui.label(egui::RichText::new("Pending confirmation").strong());
                ui.monospace(&self.send_dialog.txid);
                ui.label(&self.send_dialog.message);
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() { *want_close = true; }
                });
            }
            SendDialogStatus::Failed => {
                ui.horizontal(|ui| { self.ui_icon(ui, "failed", 18.0); ui.colored_label(egui::Color32::from_rgb(255, 105, 105), "Failed"); });
                ui.label(&self.send_dialog.message);
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() { *want_close = true; }
                    if ui.button("Retry").clicked() {
                        self.send_dialog.status = SendDialogStatus::Editing;
                        self.send_dialog.message.clear();
                        self.send_dialog.txid.clear();
                    }
                });
            }
            SendDialogStatus::Confirmed => {
                ui.horizontal(|ui| { self.ui_icon(ui, "success", 18.0); ui.colored_label(egui::Color32::from_rgb(66, 220, 120), "Success"); });
                ui.monospace(&self.send_dialog.txid);
                ui.label(&self.send_dialog.message);
                if ui.button("Close").clicked() { *want_close = true; }
            }
        }
    }


    fn ui_library_window(&mut self, ctx: &egui::Context) {
        if !self.library_dialog.open { return; }
        let mut open = self.library_dialog.open;
        let mut action: Option<LibraryGuiAction> = None;
        egui::Window::new("Library")
            .open(&mut open)
            .default_width(980.0)
            .default_height(720.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.library_dialog.tab == LibraryTab::Browse, "Browse global library").clicked() { self.library_dialog.tab = LibraryTab::Browse; }
                    if ui.selectable_label(self.library_dialog.tab == LibraryTab::Read, "Read").clicked() { self.library_dialog.tab = LibraryTab::Read; }
                    if ui.selectable_label(self.library_dialog.tab == LibraryTab::Create, "Create post").clicked() { self.library_dialog.tab = LibraryTab::Create; }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Create post").clicked() { self.library_dialog.tab = LibraryTab::Create; }
                    });
                });
                ui.separator();
                if !self.library_dialog.message.trim().is_empty() {
                    ui.colored_label(egui::Color32::from_rgb(255, 190, 80), &self.library_dialog.message);
                    ui.separator();
                }
                self.request_library_state_refresh(ctx, false);
                ui.horizontal_wrapped(|ui| {
                    if self.library_state_in_flight {
                        ui.spinner();
                        ui.small("Loading Library from local chain in the background...");
                    }
                    if ui.button("Refresh").clicked() {
                        self.request_library_state_refresh(ctx, true);
                    }
                    if let Some(loaded) = self.library_state_last_loaded {
                        ui.small(format!("Loaded {}s ago", loaded.elapsed().as_secs()));
                    }
                });
                if let Some(err) = &self.library_state_error {
                    ui.colored_label(egui::Color32::from_rgb(255, 170, 90), format!("Library loading warning: {err}"));
                }
                let Some(state) = self.library_state_cache.clone() else {
                    ui.separator();
                    ui.spinner();
                    ui.label("Preparing Library index. The GUI remains responsive while this loads.");
                    return;
                };
                match self.library_dialog.tab {
                    LibraryTab::Browse => {
                        ui.small("Public on-chain posts. Delete is a tombstone in the current view; historical chain bytes are immutable.");
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            egui::Grid::new("library_posts_grid").striped(true).num_columns(8).spacing([12.0, 6.0]).show(ui, |ui| {
                                ui.strong("Height"); ui.strong("Category"); ui.strong("Title"); ui.strong("Author"); ui.strong("Votes"); ui.strong("Comments"); ui.strong("Read"); ui.strong("More"); ui.end_row();
                                for post in state.posts.iter().filter(|p| !p.deleted).take(200) {
                                    ui.label(format!("#{}", post.created_height));
                                    ui.label(&post.category);
                                    ui.label(&post.title);
                                    ui.monospace(shorten_hash(&post.author));
                                    ui.label(format!("+{} / -{}", post.upvotes, post.downvotes));
                                    ui.label(post.comment_count.to_string());
                                    if ui.button("Read").clicked() { self.library_dialog.selected_post_id = post.id.clone(); self.library_dialog.tab = LibraryTab::Read; }
                                    ui.menu_button("More", |ui| {
                                        if ui.button("Edit").clicked() { action = Some(LibraryGuiAction::Edit { kind: "post".to_string(), id: post.id.clone() }); ui.close_menu(); }
                                        if ui.button("Delete").clicked() { action = Some(LibraryGuiAction::Delete { kind: "post".to_string(), id: post.id.clone() }); ui.close_menu(); }
                                    });
                                    ui.end_row();
                                }
                            });
                        });
                    }
                    LibraryTab::Read => {
                        let selected = self.library_dialog.selected_post_id.clone();
                        let Some(post) = state.posts.iter().find(|p| p.id == selected && !p.deleted).or_else(|| state.posts.iter().find(|p| !p.deleted)) else { ui.label("No Library posts yet."); return; };
                        self.library_dialog.selected_post_id = post.id.clone();
                        ui.heading(&post.title);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("Category: {}", post.category));
                            ui.label(format!("Created at #{}", post.created_height));
                            if let Some(h) = post.edited_height { ui.colored_label(egui::Color32::YELLOW, format!("Edited at #{}", h)); }
                            ui.label(format!("Votes +{} / -{}", post.upvotes, post.downvotes));
                        });
                        ui.monospace(format!("post id: {}", post.id));
                        ui.separator();
                        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            egui::ScrollArea::vertical()
                                .id_salt("library_read_post_body_scroll")
                                .auto_shrink([false, false])
                                .max_height(260.0)
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.add(egui::Label::new(post.body.as_str()).wrap());
                                });
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Upvote").clicked() { action = Some(LibraryGuiAction::Vote { kind: "post".to_string(), id: post.id.clone(), up: true }); }
                            if ui.button("Downvote").clicked() { action = Some(LibraryGuiAction::Vote { kind: "post".to_string(), id: post.id.clone(), up: false }); }
                            if ui.button("Edit").clicked() { action = Some(LibraryGuiAction::Edit { kind: "post".to_string(), id: post.id.clone() }); }
                            if ui.button("Delete").clicked() { action = Some(LibraryGuiAction::Delete { kind: "post".to_string(), id: post.id.clone() }); }
                        });
                        ui.separator();
                        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                            ui.label(egui::RichText::new("Comment").strong());
                            ui.horizontal(|ui| {
                                ui.label("Parent comment id (optional)");
                                ui.add(egui::TextEdit::singleline(&mut self.library_dialog.comment_parent_id).desired_width(360.0));
                            });
                            ui.add(egui::TextEdit::multiline(&mut self.library_dialog.comment_body).desired_rows(4).desired_width(f32::INFINITY));
                            if ui.button("Post comment").clicked() {
                                let parent = if self.library_dialog.comment_parent_id.trim().is_empty() { None } else { Some(self.library_dialog.comment_parent_id.trim().to_string()) };
                                action = Some(LibraryGuiAction::Comment { post_id: post.id.clone(), parent });
                            }
                        });
                        ui.separator();
                        ui.label(egui::RichText::new("Comments").strong());
                        egui::ScrollArea::vertical()
                            .id_salt("library_read_comments_scroll")
                            .auto_shrink([false, false])
                            .max_height(360.0)
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                let mut any_comments = false;
                                for c in state.comments.iter().filter(|c| c.post_id == post.id && !c.deleted) {
                                    any_comments = true;
                                    egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.horizontal_wrapped(|ui| {
                                            ui.add_space((c.depth as f32) * 14.0);
                                            ui.monospace(shorten_hash(&c.author));
                                            ui.small(format!("#{}", c.created_height));
                                            if let Some(h) = c.edited_height { ui.colored_label(egui::Color32::YELLOW, format!("edited #{}", h)); }
                                            ui.small(format!("+{} / -{}", c.upvotes, c.downvotes));
                                        });
                                        ui.add_space(4.0);
                                        ui.add(egui::Label::new(c.body.as_str()).wrap());
                                        ui.horizontal_wrapped(|ui| {
                                            if ui.button("Reply").clicked() { self.library_dialog.comment_parent_id = c.id.clone(); }
                                            if ui.button("Upvote").clicked() { action = Some(LibraryGuiAction::Vote { kind: "comment".to_string(), id: c.id.clone(), up: true }); }
                                            if ui.button("Downvote").clicked() { action = Some(LibraryGuiAction::Vote { kind: "comment".to_string(), id: c.id.clone(), up: false }); }
                                            if ui.button("Edit").clicked() { action = Some(LibraryGuiAction::Edit { kind: "comment".to_string(), id: c.id.clone() }); }
                                            if ui.button("Delete").clicked() { action = Some(LibraryGuiAction::Delete { kind: "comment".to_string(), id: c.id.clone() }); }
                                        });
                                    });
                                    ui.add_space(6.0);
                                }
                                if !any_comments { ui.small("No comments yet."); }
                            });
                    }
                    LibraryTab::Create => {
                        ui.label("Title / subject");
                        ui.text_edit_singleline(&mut self.library_dialog.create_title);
                        ui.label("Category");
                        ui.text_edit_singleline(&mut self.library_dialog.create_category);
                        ui.label("Body");
                        ui.add(egui::TextEdit::multiline(&mut self.library_dialog.create_body).desired_rows(14).desired_width(f32::INFINITY));
                        ui.horizontal(|ui| { ui.label("Normal tx fee"); ui.text_edit_singleline(&mut self.library_dialog.fee); });
                        let body_sig = self.library_dialog.create_body.as_bytes().iter().fold(0u64, |acc, b| acc.wrapping_mul(131).wrapping_add(*b as u64));
                        let price_key = format!("{}|{}|{}|{}|{}", self.prefs.config_path, self.library_dialog.create_title, self.library_dialog.create_category, self.library_dialog.create_body.len(), body_sig);
                        if self.library_dialog.create_price_preview_key != price_key {
                            self.library_dialog.create_price_preview_key = price_key;
                            self.library_dialog.create_price_preview = match library_post_price_preview_for_gui(&self.prefs.config_path, &self.library_dialog.create_title, &self.library_dialog.create_category, &self.library_dialog.create_body) {
                                Ok(s) => s,
                                Err(err) => format!("Library price warning: {err}"),
                            };
                        }
                        let preview = self.library_dialog.create_price_preview.clone();
                        if preview.to_ascii_lowercase().contains("warning") {
                            ui.colored_label(egui::Color32::YELLOW, preview);
                        } else {
                            ui.small(preview);
                        }
                        if ui.button("Create post").clicked() { action = Some(LibraryGuiAction::Create); }
                        if !self.library_dialog.edit_target_id.trim().is_empty() {
                            ui.separator();
                            ui.colored_label(egui::Color32::YELLOW, format!("Editing {} {}", self.library_dialog.edit_target_kind, shorten_hash(&self.library_dialog.edit_target_id)));
                            if ui.button("Save edit").clicked() {
                                action = Some(LibraryGuiAction::Edit { kind: self.library_dialog.edit_target_kind.clone(), id: self.library_dialog.edit_target_id.clone() });
                            }
                            if ui.button("Cancel edit").clicked() { self.library_dialog.edit_target_id.clear(); }
                        }
                    }
                }
            });
        self.library_dialog.open = open;
        if let Some(action) = action { self.run_library_action(ctx, action); }
    }

    fn run_library_action(&mut self, ctx: &egui::Context, action: LibraryGuiAction) {
        if self.library_action_in_flight {
            self.library_dialog.message = "Library action already running in the background...".to_string();
            return;
        }

        let config = self.prefs.config_path.clone();
        let fee = self.library_dialog.fee.clone();

        enum LibraryWork {
            Create { title: String, category: String, body: String },
            Comment { post_id: String, parent: Option<String>, body: String },
            Vote { kind: String, id: String, up: bool },
            Edit { kind: String, id: String, title: String, category: String, body: String },
            Delete { kind: String, id: String },
        }

        let work = match action {
            LibraryGuiAction::Create => Some(("Publishing Library post in the background...".to_string(), LibraryWork::Create {
                title: self.library_dialog.create_title.clone(),
                category: self.library_dialog.create_category.clone(),
                body: self.library_dialog.create_body.clone(),
            })),
            LibraryGuiAction::Comment { post_id, parent } => Some(("Publishing Library comment in the background...".to_string(), LibraryWork::Comment {
                post_id,
                parent,
                body: self.library_dialog.comment_body.clone(),
            })),
            LibraryGuiAction::Vote { kind, id, up } => Some(("Publishing Library vote in the background...".to_string(), LibraryWork::Vote { kind, id, up })),
            LibraryGuiAction::Delete { kind, id } => Some(("Publishing Library delete/tombstone marker in the background...".to_string(), LibraryWork::Delete { kind, id })),
            LibraryGuiAction::Edit { kind, id } => {
                if self.library_dialog.edit_target_id == id {
                    let work = LibraryWork::Edit {
                        kind,
                        id,
                        title: self.library_dialog.create_title.clone(),
                        category: self.library_dialog.create_category.clone(),
                        body: self.library_dialog.create_body.clone(),
                    };
                    self.library_dialog.edit_target_id.clear();
                    Some(("Publishing Library edit in the background...".to_string(), work))
                } else {
                    self.library_dialog.edit_target_kind = kind.clone();
                    self.library_dialog.edit_target_id = id.clone();
                    if let Some(state) = self.library_state_cache.clone() {
                        if kind == "post" {
                            if let Some(p) = state.posts.iter().find(|p| p.id == id) {
                                self.library_dialog.create_title = p.title.clone();
                                self.library_dialog.create_category = p.category.clone();
                                self.library_dialog.create_body = p.body.clone();
                            }
                        } else if let Some(c) = state.comments.iter().find(|c| c.id == id) {
                            self.library_dialog.create_title.clear();
                            self.library_dialog.create_category.clear();
                            self.library_dialog.create_body = c.body.clone();
                        }
                        self.library_dialog.tab = LibraryTab::Create;
                        self.library_dialog.message = "Edit loaded into the fields below. Press Save edit to publish an edit marker.".to_string();
                    } else {
                        self.library_dialog.message = "Library index is still loading; try Edit again after it finishes.".to_string();
                        self.request_library_state_refresh(ctx, true);
                    }
                    None
                }
            }
        };

        let Some((message, work)) = work else { return; };
        let (tx, rx) = mpsc::channel();
        self.library_action_rx = Some(rx);
        self.library_action_in_flight = true;
        self.library_dialog.message = message;
        let repaint = ctx.clone();
        thread::spawn(move || {
            let result = match work {
                LibraryWork::Create { title, category, body } => execute_gui_library_create(&config, &title, &category, &body, &fee),
                LibraryWork::Comment { post_id, parent, body } => execute_gui_library_comment(&config, &post_id, parent.as_deref(), &body, &fee),
                LibraryWork::Vote { kind, id, up } => execute_gui_library_vote(&config, &kind, &id, up, &fee),
                LibraryWork::Edit { kind, id, title, category, body } => execute_gui_library_edit(&config, &kind, &id, &title, &category, &body, &fee),
                LibraryWork::Delete { kind, id } => execute_gui_library_delete(&config, &kind, &id, &fee),
            };
            match result {
                Ok((txid, relayed_to_peers, local_mempooltx)) => { let _ = tx.send(LibraryActionEvent::Created { txid, relayed_to_peers, local_mempooltx }); }
                Err(err) => { let _ = tx.send(LibraryActionEvent::Failed(format!("{err:#}"))); }
            }
            repaint.request_repaint();
        });
    }

    fn poll_library_action(&mut self, ctx: &egui::Context) {
        let polled = self.library_action_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(LibraryActionEvent::Created { txid, relayed_to_peers, local_mempooltx })) => {
                self.library_action_rx = None;
                self.library_action_in_flight = false;
                self.library_dialog.pending_txid = txid.clone();
                self.library_dialog.message = format!("Library pending tx: {} relayed_to_peers={} local_mempool={}. It will clear after confirmation.", shorten_hash(&txid), relayed_to_peers, local_mempooltx);
                self.last_send_status_poll = Instant::now() - Duration::from_secs(10);
                self.start_tx_status_check(txid);
                self.start_background_snapshot_refresh();
            }
            Some(Ok(LibraryActionEvent::Failed(err))) => {
                self.library_action_rx = None;
                self.library_action_in_flight = false;
                self.library_dialog.message = format!("Library action failed: {err}");
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.library_action_rx = None;
                self.library_action_in_flight = false;
                self.library_dialog.message = "Library worker disconnected.".to_string();
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
        if !self.library_dialog.pending_txid.is_empty()
            && self.last_send_status_poll.elapsed() >= Duration::from_secs(5)
        {
            self.last_send_status_poll = Instant::now();
            self.start_tx_status_check(self.library_dialog.pending_txid.clone());
        }
    }

    fn ui_update_window(&mut self, ctx: &egui::Context) {
        if !self.update_dialog.open { return; }
        let mut open = self.update_dialog.open;
        let mut want_close = false;
        egui::Window::new("Updates")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("QUB Core updater").strong());
                ui.small(format!("Current version: {}", APP_VERSION));
                if !self.update_dialog.latest_version.trim().is_empty() {
                    ui.small(format!("Latest downloaded version: {}", self.update_dialog.latest_version));
                }
                ui.add_space(8.0);
                if ui.checkbox(&mut self.prefs.auto_check_updates, "Auto check for updates").changed() { self.prefs_dirty = true; }
                if ui.checkbox(&mut self.prefs.auto_download_updates, "Auto download updates").changed() { self.prefs_dirty = true; }
                if ui.checkbox(&mut self.prefs.auto_install_updates, "Auto install downloaded updates").changed() { self.prefs_dirty = true; }
                if ui.checkbox(&mut self.prefs.stop_miner_on_update_available, "Stop miner when update is ready").changed() { self.prefs_dirty = true; }
                if ui.checkbox(&mut self.prefs.start_mining_after_update_restart, "After update restart: start mining automatically").changed() { self.prefs_dirty = true; }
                ui.indent("after_update_restart_mining_mode", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Restart mining mode");
                        if ui.radio_value(&mut self.prefs.start_mining_after_update_restart_mode, AutoMiningMode::Solo, "Solo").changed() { self.prefs_dirty = true; }
                        let pool_label = self.auto_mining_mode_label(AutoMiningMode::Pool);
                        if ui.radio_value(&mut self.prefs.start_mining_after_update_restart_mode, AutoMiningMode::Pool, pool_label).changed() { self.prefs_dirty = true; }
                    });
                    ui.small("This setting applies only to the automatic relaunch immediately after installing an update. QUB Core first-run default is On + Pool if you have a selected/joined pool, otherwise Solo. After that, QUB Core keeps your choice.");
                });
                ui.add_space(8.0);
                ui.label("Update URL");
                if ui.text_edit_singleline(&mut self.prefs.update_url).changed() { self.prefs_dirty = true; }
                ui.add_space(10.0);
                ui.label(&self.update_dialog.message);
                if let Some(at) = self.update_dialog.auto_install_at {
                    if self.update_dialog.status == UpdateStatus::Ready {
                        let secs = at.saturating_duration_since(Instant::now()).as_secs();
                        ui.small(format!("Automatic install countdown: {}s", secs));
                    }
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if self.ui_icon_button(ui, "check-now", "Check now").clicked() {
                        self.start_update_check(true);
                    }
                    if self.update_dialog.status == UpdateStatus::Ready && self.ui_icon_button(ui, "install-and-restart", "Install & restart").clicked() {
                        let _ = self.install_downloaded_update();
                    }
                    if self.update_dialog.status == UpdateStatus::Ready && ui.button("Later").clicked() {
                        self.update_dialog.auto_install_at = None;
                        want_close = true;
                    }
                    if ui.button("Close").clicked() {
                        self.update_dialog.auto_install_at = None;
                        want_close = true;
                    }
                });
            });
        if want_close { open = false; }
        self.update_dialog.open = open;
    }

    fn set_auto_start(&mut self, enabled: bool) {
        match set_windows_autostart(enabled) {
            Ok(()) => {
                self.prefs.auto_start_windows = enabled;
                self.prefs_dirty = true;
                self.last_success = Some(if enabled { "Windows autostart enabled." } else { "Windows autostart disabled." }.to_string());
            }
            Err(err) => {
                self.last_error = Some(format!("Could not update Windows autostart: {err:#}"));
            }
        }
    }
}

impl eframe::App for QubCoreApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_miner_events();
        self.poll_benchmark();
        self.poll_p2p_node();
        self.poll_send_dialog();
        self.poll_ethereum_wallet_events();
        self.poll_conversion_dialog();
        self.poll_buy_jin_dialog(ctx);
        self.poll_qns_dialog();
        self.poll_pool_dialog();
        self.poll_library_state();
        self.poll_library_action(ctx);
        self.poll_tx_status();
        self.poll_updates();
        self.poll_wallet_sync();
        self.poll_wallet_create();
        self.poll_send_qns_resolve();
        self.poll_multi_qns_resolve();
        self.poll_snapshot_refresh();
        self.poll_enjin_matrix_metrics();
        self.poll_mining_loop_sound();
        self.maybe_start_hf85_auto_catchup_sync();
        self.poll_hf85_catchup_completion();

        if !self.initial_loading && self.pending_start_mining_on_open {
            self.pending_start_mining_on_open = false;
            if self.miner.is_none() && !self.prefs.payout_address.trim().is_empty() {
                let mode = self.pending_start_mining_mode;
                self.start_mining_from_auto_mode(mode, "startup auto-mining");
            }
        }

        if !self.initial_loading {
            if let Some(restart_at) = self.auto_restart_mining_at {
                if Instant::now() >= restart_at && self.miner.is_none() && !self.manual_stop_requested {
                    let known = self.best_known_network_height();
                    let local = self.snapshot.height;
                    if known > local {
                        // HF111/v1.7.1: an auto-heal restart must not start the
                        // miner while we still know the canonical chain is ahead.
                        // Keep repairing and re-check shortly.
                        self.auto_restart_mining_at = Some(Instant::now() + Duration::from_secs(15));
                        self.status_line = format!(
                            "HF110 auto-heal is still catching up before mining restart: local #{} -> official/direct #{} ({} block(s) remaining).",
                            local, known, known.saturating_sub(local)
                        );
                        spawn_hf85_catchup_pulse(self.prefs.config_path.clone(), true);
                    } else if hf85_catchup_running() {
                        self.auto_restart_mining_at = Some(Instant::now() + Duration::from_secs(8));
                        self.status_line = "HF110 auto-heal repair is finishing; mining will resume automatically after the writer completes.".to_string();
                    } else {
                        let pool = self.desired_pool_mining_pool_id.clone();
                        self.auto_restart_mining_at = None;
                        self.hf110_autoheal_paused_mining = false;
                        if !pool.trim().is_empty() {
                            self.start_pool_mining(pool);
                        } else {
                            self.start_mining();
                        }
                    }
                }
            }
        }

        // Keep mining performance first: address-activity scans are expensive on mainnet.
        // The UI stays responsive by refreshing snapshots less aggressively and doing tx-status checks in a worker thread.
        let refresh_every = if self.miner.is_some() || matches!(self.mining_phase, MiningPhase::Preparing | MiningPhase::Mining) {
            // HF72/v1.5.8: snapshot refresh can scan wallet/activity/pool state.
            // Do not steal cycles from active hashing every few seconds.
            Duration::from_secs(20)
        } else if self.send_dialog.status == SendDialogStatus::Pending || self.qns_dialog.status == QnsDialogStatus::Pending || self.conversion_dialog.status == SendDialogStatus::Pending || self.pool_dialog.status == SendDialogStatus::Pending {
            Duration::from_secs(8)
        } else {
            Duration::from_secs(15)
        };
        if self.last_refresh.elapsed() >= refresh_every {
            self.start_background_snapshot_refresh();
        }
        if self.last_enjin_metrics_refresh.elapsed() >= Duration::from_secs(ENJIN_MATRIX_METRICS_REFRESH_SECS) {
            self.start_enjin_matrix_metrics_fetch();
        }
        if !self.eth_wallets.wallets.is_empty() && !self.eth_balance_in_flight {
            let refresh_due = self.eth_balances.updated_at.map(|t| t.elapsed() >= Duration::from_secs(45)).unwrap_or(true);
            if refresh_due { self.start_ethereum_balance_refresh(); }
        }
        if self.prefs.auto_sync_wallet_balances && !self.wallet_sync_in_flight {
            let interval = self.prefs.auto_sync_wallet_interval_secs.clamp(5, 600);
            if self.last_wallet_sync.elapsed() >= Duration::from_secs(interval) {
                self.start_wallet_sync(false);
            }
        }

        if self.last_theme_applied != self.prefs.theme {
            apply_theme(ctx, &self.prefs.theme);
            apply_visual_tuning(ctx);
            self.last_theme_applied = self.prefs.theme.clone();
        }
        self.save_prefs_if_needed();

        let active_ui = self.initial_loading
            || self.miner.is_some()
            || matches!(self.mining_phase, MiningPhase::Preparing | MiningPhase::Mining)
            || self.benchmark_running
            || self.update_check_in_flight
            || self.tx_status_in_flight
            || self.eth_balance_in_flight
            || self.eth_send_rx.is_some()
            || self.wallet_create_in_flight
            || self.send_qns_resolve_in_flight
            || self.pool_rx.is_some()
            || self.library_state_in_flight
            || matches!(self.update_dialog.status, UpdateStatus::Ready | UpdateStatus::Installing | UpdateStatus::Checking);
        let repaint_ms = if active_ui { 160 } else { 900 };
        ctx.request_repaint_after(Duration::from_millis(repaint_ms));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.initial_loading {
            egui::CentralPanel::default()
                .frame(egui::Frame::default().fill(egui::Color32::BLACK))
                .show_inside(ui, |ui| self.ui_opening_splash(ui));
            return;
        }
        egui::Panel::top("top_bar").show_inside(ui, |ui| self.ui_header(ui));
        egui::Panel::bottom("status_bar").show_inside(ui, |ui| self.ui_status(ui));
        if !self.prefs.setup_complete {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.ui_setup_wizard(ui));
            });
            return;
        }
        egui::Panel::left("settings_panel")
            .resizable(false)
            .default_size(360.0)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.ui_controls(ui));
            });
        let balances_w = if self.right_balances_collapsed { 172.0 } else { 290.0 };
        let activity_w = if self.right_activity_collapsed { 184.0 } else { 430.0 };
        let edge_w = 34.0 * 3.0 + 18.0;
        let right_width: f32 = f32::clamp(balances_w + activity_w + edge_w, 390.0_f32, 860.0_f32);
        egui::Panel::right("wallet_activity_panel")
            .resizable(false)
            .default_size(right_width)
            .width_range(right_width..=right_width)
            .show_inside(ui, |ui| {
                let max_h = ui.available_height();
                let spacing = ui.spacing().item_spacing.x;
                ui.horizontal_top(|ui| {
                    let balance_icon = if self.right_balances_collapsed { "to-left" } else { "to-right" };
                    let balance_fallback = if self.right_balances_collapsed { "<" } else { ">" };
                    let resp = self.ui_tall_icon_button(ui, balance_icon, balance_fallback, "Collapse/expand Address balances only", max_h.min(62.0));
                    if resp.clicked() {
                        self.right_balances_collapsed = !self.right_balances_collapsed;
                    }

                    ui.allocate_ui_with_layout(
                        egui::vec2(balances_w, max_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("address_balances_scroll")
                                .auto_shrink([false, false])
                                .max_height(max_h)
                                .show(ui, |ui| {
                                    ui.set_min_width(balances_w - 10.0);
                                    if self.right_balances_collapsed { self.ui_address_balances_compact(ui); }
                                    else { self.ui_address_balances(ui); }
                                });
                        },
                    );

                    ui.add_space(spacing * 0.5);
                    let activity_icon = if self.right_activity_collapsed { "to-left" } else { "to-right" };
                    let activity_fallback = if self.right_activity_collapsed { "<" } else { ">" };
                    let resp = self.ui_tall_icon_button(ui, activity_icon, activity_fallback, "Collapse/expand Address Activity only", max_h.min(62.0));
                    if resp.clicked() { self.right_activity_collapsed = !self.right_activity_collapsed; }
                    ui.add_space(spacing * 0.5);

                    ui.allocate_ui_with_layout(
                        egui::vec2(activity_w, max_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("address_activity_scroll")
                                .auto_shrink([false, false])
                                .max_height(max_h)
                                .show(ui, |ui| {
                                    ui.set_min_width(activity_w - 10.0);
                                    if self.right_activity_collapsed { self.ui_address_activity_compact(ui); }
                                    else { self.ui_address_activity(ui); }
                                });
                        },
                    );

                    ui.add_space(spacing * 0.5);
                    let full_collapsed = self.right_balances_collapsed && self.right_activity_collapsed;
                    let full_icon = if full_collapsed { "full-to-left" } else { "full-to-right" };
                    let full_fallback = if full_collapsed { "<<" } else { ">>" };
                    let resp = self.ui_tall_icon_button(ui, full_icon, full_fallback, "Full collapse/expand both right panels", max_h.min(62.0));
                    if resp.clicked() {
                        if full_collapsed {
                            self.right_balances_collapsed = false;
                            self.right_activity_collapsed = false;
                        } else {
                            self.right_balances_collapsed = true;
                            self.right_activity_collapsed = true;
                        }
                    }
                });
            });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt("main_dashboard_scroll_both")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(980.0);
                    self.ui_dashboard(ui);
                });
        });
        self.ui_send_window(ui.ctx());
        self.ui_conversion_window(ui.ctx());
        self.ui_buy_jin_window(ui.ctx());
        self.ui_ethereum_send_dialog_window(ui.ctx());
        self.ui_usdj_vault_dialog_window_hf107(ui.ctx());
        self.ui_qr_scan_window(ui.ctx());
        self.ui_import_key_window(ui.ctx());
        self.ui_qns_window(ui.ctx());
        self.ui_pools_window(ui.ctx());
        self.ui_pool_create_window(ui.ctx());
        self.ui_pool_manage_window(ui.ctx());
        self.ui_library_window(ui.ctx());
        self.ui_update_window(ui.ctx());
    }
}

impl QubCoreApp {
    fn tr(&self, en: &'static str, _el: &'static str) -> &'static str {
        // v1.5.2 safety: localization is temporarily disabled.
        // The v1.2.2 language layer rendered mojibake on some Windows systems.
        en
    }

    fn ui_language_combo(&mut self, ui: &mut egui::Ui) {
        let before = self.prefs.language;
        let selected = format!("{} ({})", self.prefs.language.label(), self.prefs.language.code());
        egui::ComboBox::from_label(self.tr("Language", "Language"))
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for lang in UiLanguage::ALL {
                    let selected = self.prefs.language == lang;
                    let resp = ui.horizontal(|ui| {
                        self.ui_icon(ui, lang.flag_icon(), 16.0);
                        ui.selectable_label(selected, format!("{} ({})", lang.label(), lang.code()))
                    }).inner;
                    if resp.clicked() { self.prefs.language = lang; }
                }
            });
        if self.prefs.language != before { self.prefs_dirty = true; }
        ui.small("Language is auto-selected only during fresh setup and then kept in settings.");
    }


    fn ui_opening_splash(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let elapsed = self.initial_loading_started_at.elapsed().as_secs_f32();
        let progress = self.sync_progress_fraction();
        let percent = (progress * 100.0).round() as u32;
        let local = self.snapshot.height;
        let known = self.best_known_network_height();
        let progress_detail = if known > local {
            let remaining = known.saturating_sub(local);
            let rate_extra = if self.sync_progress_rate_bps > 0.01 {
                let bpm = self.sync_progress_rate_bps * 60.0;
                let eta_secs = (remaining as f32 / self.sync_progress_rate_bps).max(1.0);
                let eta = if eta_secs < 90.0 { format!("{:.0}s", eta_secs) } else { format!("{:.1}m", eta_secs / 60.0) };
                format!(" | {:.1} block(s)/min | ETA {eta}", bpm)
            } else {
                " | measuring live rate".to_string()
            };
            format!("local #{local} -> known #{known} | {remaining} block(s) remaining{rate_extra}")
        } else if known > 0 {
            format!("local #{local} at known tip | final verification")
        } else {
            "discovering official/peer tip".to_string()
        };
        let message = if !self.status_line.trim().is_empty() && self.status_line != "Ready." {
            self.status_line.as_str()
        } else if elapsed < 8.0 {
            "Startup sync: contacting official seeds and checking the active chain..."
        } else if elapsed < 18.0 {
            "Fetching only missing canonical blocks from official seeds..."
        } else if elapsed < 38.0 {
            "Using verified HTTP tail snapshot if P2P suffix is slow..."
        } else {
            "Finalizing wallet balances and local activity. Keep this window open."
        };

        ui.allocate_ui_at_rect(rect, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((rect.height() * 0.06).max(24.0));
                if let Some(texture) = &self.opening_banner {
                    let max_w = (rect.width() * 0.58).min(780.0);
                    let size = texture.size_vec2();
                    let scale = if size.x > 0.0 { max_w / size.x } else { 1.0 };
                    let wanted = egui::vec2(size.x * scale, size.y * scale);
                    let sized = egui::load::SizedTexture::new(texture.id(), wanted);
                    ui.add(egui::Image::from_texture(sized));
                } else {
                    if let Some(texture) = &self.logo {
                        let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(118.0, 118.0));
                        ui.add(egui::Image::from_texture(sized).corner_radius(16));
                    }
                    ui.add_space(20.0);
                    ui.label(egui::RichText::new("Qubit Coin Core").size(72.0).strong().color(egui::Color32::WHITE));
                }
                ui.add_space(12.0);
                ui.label(egui::RichText::new(APP_VERSION).size(30.0).strong().color(egui::Color32::WHITE));
                ui.add_space(32.0);
                ui.label(egui::RichText::new(message).size(17.0).color(egui::Color32::WHITE));
                ui.add_space(28.0);

                let bar_w = (rect.width() * 0.86).min(1500.0);
                let bar_h = 38.0;
                let (bar_rect, _) = ui.allocate_exact_size(egui::vec2(bar_w, bar_h), egui::Sense::hover());
                let painter = ui.painter();
                let orange = egui::Color32::from_rgb(255, 148, 20);
                painter.rect_stroke(bar_rect, 0.0, egui::Stroke::new(4.0, orange), egui::StrokeKind::Inside);
                let fill_w = (bar_rect.width() * progress).clamp(0.0, bar_rect.width());
                let fill_rect = egui::Rect::from_min_size(bar_rect.min, egui::vec2(fill_w, bar_rect.height()));
                painter.rect_filled(fill_rect.shrink(4.0), 0.0, orange);
                painter.text(bar_rect.center(), egui::Align2::CENTER_CENTER, format!("{}%", percent), egui::FontId::proportional(30.0), egui::Color32::WHITE);
                ui.add_space(10.0);
                ui.label(egui::RichText::new(progress_detail).size(15.0).color(egui::Color32::WHITE));
            });
        });
    }

    fn ui_header(&mut self, ui: &mut egui::Ui) {
        egui::Frame::default().inner_margin(egui::Margin::same(14)).show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(texture) = &self.logo {
                    let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(54.0, 54.0));
                    ui.add(egui::Image::from_texture(sized).corner_radius(10));
                } else {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(54.0, 54.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 25.0, ui.visuals().selection.bg_fill);
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Q", egui::FontId::proportional(26.0), ui.visuals().strong_text_color());
                }
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(APP_TITLE).size(32.0).strong().italics());
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(self.version_network_label()).size(16.0).weak());
                    });
                    ui.small("Installer + auto-update ready build");
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let running = self.miner.is_some();
                    let status = if running { "MINING" } else { "IDLE" };
                    ui.label(egui::RichText::new(status).size(16.0).strong());
                    ui.add_space(12.0);
                    let button_text = update_button_caption(&self.update_dialog);
                    let update_response = if matches!(self.update_dialog.status, UpdateStatus::Ready) {
                        self.ui_icon_button(ui, "updates-available", &button_text)
                    } else {
                        ui.button(button_text)
                    };
                    if update_response.clicked() {
                        self.update_dialog.open = true;
                    }
                });
            });
        });
    }

    fn ui_setup_wizard(&mut self, ui: &mut egui::Ui) {
        ui.add_space(18.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("QUB Core setup").size(34.0).strong().italics());
            ui.label(egui::RichText::new("Full node and solo miner. The installer handles normal setup.").size(15.0).weak());
        });
        ui.add_space(18.0);
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(18)).show(ui, |ui| {
            ui.heading("1. Network channel");
            ui.add_space(8.0);
            self.prefs.setup_profile = default_setup_profile();
            ui.label(format!("This installer is configured for {}.", if build_channel() == "testnet" { "Testnet" } else { "Mainnet" }));
            ui.small("Mainnet and Testnet are separate apps. Switching networks inside one app is disabled to prevent accidental cross-network mistakes.");
            ui.add_space(14.0);

            ui.heading("2. Automatic peer discovery");
            ui.add_space(8.0);
            ui.label("No IP addresses, no bootstrap mode, no command files.");
            ui.small(match self.prefs.setup_profile {
                SetupProfile::RegtestLan => "LAN rehearsal uses automatic UDP peer discovery. Start QUB-Core.exe on every mini-PC; they find each other on the same LAN.",
                SetupProfile::Testnet => "Testnet uses built-in public DNS seed domains plus peer exchange. Users do not enter seed IPs.",
                SetupProfile::Mainnet => "Mainnet uses built-in official DNS seed domains plus peer exchange. Users do not enter seed IPs.",
            });
            ui.add_space(8.0);
            self.prefs.setup_listen_for_peers = true;
            ui.label("Peer connections: enabled automatically");
            ui.small("QUB Core runs an embedded full P2P node in the background while the GUI is open.");
            ui.add_space(14.0);

            ui.heading("3. Ready-to-run Windows app");
            ui.add_space(8.0);
            ui.label("The installer creates the normal QUB Core entry point.");
            ui.small("Advanced CLI tools stay available under tools\\ for operators only.");
            ui.add_space(14.0);

            ui.horizontal(|ui| {
                if ui.button(egui::RichText::new("Start QUB Core").strong()).clicked() {
                    self.complete_setup_wizard();
                }
                if ui.button("Reset setup").clicked() {
                    self.prefs.setup_complete = false;
                    self.prefs_dirty = true;
                    self.status_line = "Setup wizard reset.".to_string();
                }
            });
        });
    }

    fn ui_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let arrow = if self.prefs.left_mining_controls_expanded { "v" } else { ">" };
            if ui.small_button(arrow).on_hover_text("Expand/collapse Mining controls").clicked() {
                self.prefs.left_mining_controls_expanded = !self.prefs.left_mining_controls_expanded;
                self.prefs_dirty = true;
            }
            let label_response = ui.horizontal(|ui| {
                self.ui_mining_controls_header_hf99(ui);
            }).response;
            if label_response.clicked() {
                self.prefs.left_mining_controls_expanded = !self.prefs.left_mining_controls_expanded;
                self.prefs_dirty = true;
            }
        });
        if self.prefs.left_mining_controls_expanded {
            ui.add_space(4.0);
            let mining = self.miner.is_some();
            let pool_mining = mining && !self.pool_mining_pool_id.trim().is_empty();
            if !mining {
                if self.ui_icon_button_sized(ui, "start-mining", self.tr("Start solo mining", "Start solo mining"), 22.0).clicked() {
                    self.start_mining();
                }
            } else if !pool_mining {
                if self.ui_icon_button_sized(ui, "stop-mining", self.tr("Stop mining", "Stop mining"), 22.0).clicked() {
                    self.stop_mining();
                }
            } else {
                self.ui_icon_button_enabled(ui, false, "start-mining", self.tr("Start solo mining", "Start solo mining"));
                ui.small("Solo mining is disabled while pool mining is running.");
            }

            ui.add_space(10.0);
            ui.label(self.tr("Payout address", "Payout address"));
            ui.horizontal(|ui| {
                if ui.text_edit_singleline(&mut self.prefs.payout_address).changed() {
                    self.prefs_dirty = true;
                    self.update_runtime_identity();
                    self.qr_cache_address.clear();
                    self.qr_cache_texture = None;
                }
                let qr_resp = if let Some(texture) = self.icons.get("qr", ui.visuals().dark_mode) {
                    let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(16.0, 16.0));
                    ui.add(egui::Button::image_and_text(egui::Image::from_texture(sized), ""))
                } else {
                    ui.button("QR")
                };
                if qr_resp.hovered() {
                    self.qr_hover_until = Some(Instant::now() + Duration::from_secs(1));
                    self.qr_hover_address = self.prefs.payout_address.clone();
                }
                if self.ui_icon_button(ui, "sync", "Sync").clicked() {
                    self.start_wallet_sync(true);
                }
                if ui.button("Repair from official seed").clicked() {
                    self.status_line = "Repairing from official seed snapshot/tail with HF110 deep official repair...".to_string();
                    self.start_wallet_sync(true);
                }
            });
            let payout_for_qr = self.prefs.payout_address.clone();
            self.ui_address_qr_hover(ui.ctx(), &payout_for_qr);
            if let Some(warning) = qub_payout_address_warning(self.prefs.payout_address.trim()) {
                ui.colored_label(egui::Color32::from_rgb(255, 95, 95), warning);
            }
            if self.initial_loading || self.wallet_sync_in_flight || self.snapshot_in_flight {
                self.ui_sync_progress_bar_rows(ui, "Loading wallet, balances and chain tip", 2);
                ui.small("Balances may temporarily show 0 and payout address may be blank until the automatic background sync/repair finishes. No manual Sync/Repair is needed unless this stays stuck.");
            } else if self.prefs.payout_address.trim().is_empty() && self.snapshot.default_address.is_empty() {
                ui.small("No payout address is loaded yet. Create/import a wallet or paste any public qub1... / .qub address before mining.");
            }
            ui.small("Pool mining is now the recommended default on mainnet. Solo mining is still available for advanced users, but pooled mining gives smaller miners smoother rewards.");

            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                self.ui_icon(ui, "pools", 18.0);
                ui.label(egui::RichText::new("Pools").strong());
            });
            self.ensure_pool_selection_from_last();
            let selected_pool_label = self.selected_pool()
                .map(|p| format!("Selected: {} ({})", &p.name, shorten_hash(&p.pool_id)))
                .unwrap_or_else(|| "No pool selected yet. Open Browse pools and choose Mine/Join first.".to_string());
            ui.small(selected_pool_label);
            ui.horizontal_wrapped(|ui| {
                if self.ui_icon_button(ui, "pools", "Browse / Create / Manage pools").clicked() {
                    self.open_pools_window();
                }
                let selected = self.pool_dialog.selected_pool_id.clone();
                let can_pool_mine = self.pool_activation_ready()
                    && self.snapshot.wallet_keys > 0
                    && !selected.trim().is_empty()
                    && self.pool_by_id(&selected).is_some()
                    && (!self.miner.is_some() || pool_mining);
                if pool_mining {
                    if self.ui_icon_button_enabled(ui, true, "stop-mining", "Stop pool mining").clicked() { self.stop_mining(); }
                } else if self.ui_icon_button_enabled(ui, can_pool_mine, "start-mining", "Start pool mining").clicked() {
                    self.start_pool_mining(selected);
                }
            });
            if self.snapshot.wallet_keys == 0 {
                ui.small("Pool mining needs a local wallet key so QUB Core can sign share proofs. Pool creation also needs spendable QUB.");
            } else if self.pool_dialog.selected_pool_id.trim().is_empty() {
                ui.small("Start pool mining stays disabled until you choose a pool. QUB Core will not auto-select Genesis or the first pool.");
            }

            ui.add_space(12.0);
            let old_cpu = self.prefs.cpu_percent;
            ui.horizontal(|ui| { self.ui_icon(ui, "cpu", 16.0); ui.add(egui::Slider::new(&mut self.prefs.cpu_percent, 1..=100).text("CPU power %")); });
            if self.prefs.cpu_percent != old_cpu { self.prefs_dirty = true; }
            ui.horizontal(|ui| {
                let logical = logical_cpus();
                let (threads, duty) = resource_plan(logical, self.prefs.cpu_percent);
                ui.small(format!("Plan: {threads}/{logical} worker(s), {duty}% duty"));
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                self.ui_icon(ui, "gpu", 16.0);
                let response = ui.add_enabled(true, egui::Slider::new(&mut self.prefs.gpu_percent, 0..=100).text("GPU power %"));
                if response.changed() { self.prefs_dirty = true; }
            });

            if self.gpu_device_options.is_empty() && self.gpu_device_last_scan.elapsed() > Duration::from_secs(15) {
                self.refresh_gpu_devices();
            }
            let before_gpu_selector = self.normalized_gpu_device_selector();
            ui.horizontal_wrapped(|ui| {
                ui.label("GPU device");
                let selected_text = self.gpu_selector_status_label();
                let options = self.gpu_device_options.clone();
                egui::ComboBox::from_id_salt("gpu_device_selector")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.prefs.gpu_device_selector, gpu_miner::GPU_DEVICE_ALL.to_string(), "All high-performance GPUs (default)");
                        ui.selectable_value(&mut self.prefs.gpu_device_selector, gpu_miner::GPU_DEVICE_ALL_DETECTED.to_string(), "All detected GPUs (experimental)");
                        for device in options {
                            ui.selectable_value(&mut self.prefs.gpu_device_selector, device.clone(), device);
                        }
                    });
                if ui.button("Rescan GPUs").clicked() {
                    self.refresh_gpu_devices();
                }
            });
            if self.normalized_gpu_device_selector() != before_gpu_selector {
                self.prefs_dirty = true;
            }
            if self.gpu_device_options.is_empty() {
                ui.small("No OpenCL GPU detected yet by the GUI scan. Mining can still try auto-detect at start; install/update GPU drivers if this remains empty.");
            } else {
                ui.small(format!("Detected {} OpenCL GPU device(s). Default All uses high-performance devices only; All detected GPUs is available for experimental hybrid/iGPU testing.", self.gpu_device_options.len()));
            }
            ui.small("OpenCL GPU mining is available. If GPU power is 0, CPU mining is used. QUB Core keeps high-performance full double-SHA GPU batches and per-device auto-tuning. Hybrid laptops default to the strongest GPU; All detected GPUs can run iGPU+dGPU if the drivers expose both, but thermals/power sharing may lower the discrete GPU boost.");
        }

        egui::CollapsingHeader::new("Library")
            .default_open(false)
            .show(ui, |ui| {
                self.ui_heading_icon(ui, "list", "Library");
                ui.small("Global public on-chain posts, comments, and votes. Library data is public and permanent; delete creates a tombstone in the current view, but historical chain bytes remain immutable.");
                if self.ui_icon_button(ui, "list", "Browse global library").clicked() {
                    self.library_dialog.open = true;
                    self.library_dialog.tab = LibraryTab::Browse;
                    self.request_library_state_refresh(ui.ctx(), true);
                }
            });

        egui::CollapsingHeader::new("Create / import address")
            .default_open(false)
            .show(ui, |ui| {
                self.ui_heading_icon(ui, "wallet-address", "Create / import address");
                ui.small("Manage addresses by chain. QUB Chain wallet actions are live; Ethereum and Enjin wallet support is shown as roadmap UI for future bridge/smart-contract flows.");
                ui.add_space(6.0);

                egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        self.ui_icon(ui, "qub", 22.0);
                        ui.label(egui::RichText::new("QUB Chain").strong());
                    });
                    ui.small("Native QUB wallet/address. Local private keys are stored in wallet.json for v1 Core wallet mode.");
                    if ui.checkbox(&mut self.prefs.confirm_plaintext_wallet_risk, "I understand v1 local wallet stores plaintext private keys").changed() {
                        self.prefs_dirty = true;
                    }
                    ui.horizontal_wrapped(|ui| {
                        if self.ui_icon_button_enabled(ui, !self.wallet_create_in_flight, "wallet-address", if self.wallet_create_in_flight { "Creating address..." } else { "Create QUB address" }).clicked() {
                            self.create_wallet_address();
                        }
                        if self.ui_icon_button(ui, "import-private-key", "Import QUB private key").clicked() {
                            self.open_import_key_dialog();
                        }
                        if ui.button("Use QUB wallet default").clicked() {
                            if self.snapshot.default_address.is_empty() {
                                self.last_error = Some("Wallet has no default address yet.".to_string());
                            } else {
                                self.prefs.payout_address = self.snapshot.default_address.clone();
                                self.prefs_dirty = true;
                                self.update_runtime_identity();
                            }
                        }
                    });
                    if self.snapshot.wallet_keys == 0 {
                        ui.small("Send/Register actions appear in Address balances after this Core has a local QUB wallet key. Receivers only need a public address.");
                    }
                });

                ui.add_space(8.0);
                self.ui_ethereum_wallets_section_hf102(ui);

                ui.add_space(8.0);
                egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        self.ui_icon(ui, "enj", 22.0);
                        ui.label(egui::RichText::new("Enjin Matrixchain").strong());
                    });
                    ui.small("Future wallet support for JIN Token / Enjin Matrixchain bridge flows. Disabled until the bridge is live.");
                    ui.horizontal_wrapped(|ui| {
                        self.ui_icon_button_enabled(ui, false, "wallet-address", "Create Enjin wallet");
                        self.ui_icon_button_enabled(ui, false, "import-private-key", "Import Enjin wallet");
                        self.ui_icon_button_enabled(ui, false, "jin-token", "Link JIN Token wallet");
                    });
                });
            });

        egui::CollapsingHeader::new("Danger zone")
            .default_open(false)
            .show(ui, |ui| {
                self.ui_icon_label(ui, "danger-zone", "Danger zone");
                ui.checkbox(&mut self.delete_private_key_confirm, "I understand deletion removes local private keys from wallet.json and cannot recover funds without backup");
                if self.ui_icon_button_enabled(ui, self.delete_private_key_confirm, "delete-local-private-keys", "Delete local private key(s)").clicked() {
                    self.delete_local_private_keys();
                }
                ui.small("Deletion rewrites wallet.json without local keys. It is not a forensic secure-wipe guarantee on SSDs/backups.");
            });

        egui::CollapsingHeader::new("Benchmark")
            .default_open(false)
            .show(ui, |ui| {
                self.ui_heading_icon(ui, "benchmark", self.tr("Benchmark", "Benchmark"));
                ui.add(egui::Slider::new(&mut self.prefs.benchmark_seconds, 1..=30).text("seconds"));
                if ui.button(if self.benchmark_running { "Benchmark running..." } else { "Run benchmark" }).clicked() {
                    self.start_benchmark();
                }
                if let Some(result) = &self.benchmark_result { ui.label(result); }
            });

        egui::CollapsingHeader::new("Settings")
            .default_open(false)
            .show(ui, |ui| {
                self.ui_heading_icon(ui, "settings", self.tr("Settings", "Settings"));
                ui.label(self.tr("Config file", "Config file"));
                ui.horizontal(|ui| {
                    if ui.text_edit_singleline(&mut self.prefs.config_path).changed() { self.prefs_dirty = true; }
                    if ui.button(self.tr("Reload", "Reload")).clicked() { self.start_background_snapshot_refresh(); }
                });
                ui.small(format!("Network channel: {}. Mainnet and Testnet use separate installed apps; network switching inside one app is disabled to prevent mistakes.", build_channel()));
                let before_theme = self.prefs.theme.clone();
                egui::ComboBox::from_label(self.tr("Theme", "Theme"))
                    .selected_text(format!("{:?}", self.prefs.theme))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.prefs.theme, ThemeChoice::Dark, "Dark");
                        ui.selectable_value(&mut self.prefs.theme, ThemeChoice::Light, "Light");
                        ui.selectable_value(&mut self.prefs.theme, ThemeChoice::System, "System");
                    });
                if self.prefs.theme != before_theme { self.prefs_dirty = true; }
                self.ui_language_combo(ui);

                let mut auto_start = self.prefs.auto_start_windows;
                if ui.checkbox(&mut auto_start, "Auto start on Windows login").changed() { self.set_auto_start(auto_start); }
                if ui.checkbox(&mut self.prefs.start_mining_on_open, "Start mining when app opens").changed() { self.prefs_dirty = true; }
                ui.indent("start_mining_on_open_mode", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Mode");
                        if ui.radio_value(&mut self.prefs.start_mining_on_open_mode, AutoMiningMode::Solo, "Solo").changed() { self.prefs_dirty = true; }
                        let pool_label = self.auto_mining_mode_label(AutoMiningMode::Pool);
                        if ui.radio_value(&mut self.prefs.start_mining_on_open_mode, AutoMiningMode::Pool, pool_label).changed() { self.prefs_dirty = true; }
                    });
                    ui.small("QUB Core first-run post-update auto restart defaults to Pool when you already have a selected/joined pool, otherwise Solo. Normal app-open mode still follows your saved choice.");
                });
                self.prefs.allow_isolated_regtest_mining = false;
                ui.small("Mining is blocked until QUB Core discovers at least one reachable peer. This prevents accidental private forks.");
                if ui.checkbox(&mut self.prefs.sound_enabled, "Sound when block is mined").changed() { self.prefs_dirty = true; }
                if ui.checkbox(&mut self.prefs.network_sound_enabled, "Sound when network mines a block").changed() { self.prefs_dirty = true; }
                if ui.checkbox(&mut self.prefs.mining_loop_sound_enabled, "Mining loop sound while hashing").changed() { self.prefs_dirty = true; }
                if ui.checkbox(&mut self.prefs.visual_enabled, "Visual mined-block card").changed() { self.prefs_dirty = true; }
                if ui.checkbox(&mut self.prefs.auto_sync_wallet_balances, "Auto sync wallet balances").changed() { self.prefs_dirty = true; }
                if self.prefs.auto_sync_wallet_balances {
                    ui.horizontal(|ui| {
                        ui.label("Sync interval seconds");
                        if ui.add(egui::DragValue::new(&mut self.prefs.auto_sync_wallet_interval_secs).range(5..=600)).changed() { self.prefs_dirty = true; }
                    });
                    ui.small("Manual Sync is still available next to the payout address. Default is off to avoid extra network load on early mainnet.");
                }
            });
    }



    fn selected_ethereum_wallet(&self) -> Option<EthereumWalletEntry> {
        if self.eth_wallets.wallets.is_empty() { return None; }
        let idx = self.eth_wallets.selected_index.min(self.eth_wallets.wallets.len().saturating_sub(1));
        self.eth_wallets.wallets.get(idx).cloned()
    }

    fn save_ethereum_wallets(&mut self) {
        match save_ethereum_wallet_book(&self.eth_wallets) {
            Ok(()) => self.eth_wallet_dialog.message = "Ethereum wallet book saved locally.".to_string(),
            Err(err) => self.last_error = Some(format!("Ethereum wallet save failed: {err:#}")),
        }
    }

    fn create_ethereum_wallet_hf102(&mut self) {
        match generate_ethereum_wallet_entry(self.eth_wallet_dialog.import_label.trim()) {
            Ok(entry) => {
                let address = entry.address.clone();
                self.eth_wallets.wallets.push(entry);
                self.eth_wallets.selected_index = self.eth_wallets.wallets.len().saturating_sub(1);
                self.save_ethereum_wallets();
                self.eth_wallet_dialog.message = format!("Created Ethereum wallet {address}. Back up the local wallet file before using real funds.");
                self.start_ethereum_balance_refresh();
            }
            Err(err) => self.last_error = Some(format!("Ethereum wallet create failed: {err:#}")),
        }
    }

    fn import_ethereum_wallet_hf102(&mut self) {
        let key = self.eth_wallet_dialog.import_private_key.trim();
        if key.is_empty() {
            self.eth_wallet_dialog.message = "Paste an Ethereum private key first.".to_string();
            return;
        }
        match ethereum_wallet_entry_from_private_key(key, self.eth_wallet_dialog.import_label.trim()) {
            Ok(entry) => {
                if self.eth_wallets.wallets.iter().any(|w| w.address.eq_ignore_ascii_case(&entry.address)) {
                    self.eth_wallet_dialog.message = format!("Ethereum wallet {} is already imported.", entry.address);
                    return;
                }
                let address = entry.address.clone();
                self.eth_wallets.wallets.push(entry);
                self.eth_wallets.selected_index = self.eth_wallets.wallets.len().saturating_sub(1);
                self.eth_wallet_dialog.import_private_key.clear();
                self.save_ethereum_wallets();
                self.eth_wallet_dialog.message = format!("Imported Ethereum wallet {address}.");
                self.start_ethereum_balance_refresh();
            }
            Err(err) => self.eth_wallet_dialog.message = format!("Import failed: {err:#}"),
        }
    }

    fn start_ethereum_balance_refresh(&mut self) {
        if self.eth_balance_in_flight { return; }
        let Some(wallet) = self.selected_ethereum_wallet() else { return; };
        let rpc = self.eth_wallets.rpc_url.trim().to_string();
        let usdj_contract = self.fiatj_token_contract_hf108(StablecoinFamily::Usd);
        let vault_contract = self.fiatj_vault_contract_hf108(StablecoinFamily::Usd);
        let usdt_contract = self.ethereum_asset_contract_hf107(EthereumAsset::Usdt).unwrap_or_else(|| ETHEREUM_USDT_ADDRESS.to_string());
        let usdc_contract = self.ethereum_asset_contract_hf107(EthereumAsset::Usdc).unwrap_or_else(|| ETHEREUM_USDC_ADDRESS.to_string());
        let eurj_contract = self.fiatj_token_contract_hf108(StablecoinFamily::Eur);
        let eur_vault_contract = self.fiatj_vault_contract_hf108(StablecoinFamily::Eur);
        let eurc_contract = self.ethereum_asset_contract_hf107(EthereumAsset::Eurc).unwrap_or_default();
        let eurs_contract = self.ethereum_asset_contract_hf107(EthereumAsset::Eurs).unwrap_or_default();
        let xauj_contract = self.fiatj_token_contract_hf108(StablecoinFamily::Gold);
        let xau_vault_contract = self.fiatj_vault_contract_hf108(StablecoinFamily::Gold);
        let paxg_contract = self.ethereum_asset_contract_hf107(EthereumAsset::Paxg).unwrap_or_default();
        let xaut_contract = self.ethereum_asset_contract_hf107(EthereumAsset::Xaut).unwrap_or_default();
        let (tx, rx) = mpsc::channel();
        self.eth_balance_rx = Some(rx);
        self.eth_balance_in_flight = true;
        self.eth_balances.status = format!("Refreshing Ethereum balances for {}...", shorten_eth_address(&wallet.address));
        thread::spawn(move || {
            let result = fetch_ethereum_balances_hf108(&rpc, &wallet.address, &usdt_contract, &usdc_contract, &usdj_contract, &vault_contract, &eurc_contract, &eurs_contract, &eurj_contract, &eur_vault_contract, &paxg_contract, &xaut_contract, &xauj_contract, &xau_vault_contract);
            match result {
                Ok((eth, usdt, usdc, usdj_eth, usdt_reserve, usdc_reserve, eurc, eurs, eurj_eth, eurc_reserve, eurs_reserve, paxg, xaut, xauj_eth, paxg_reserve, xaut_reserve, reserve_status, eur_reserve_status, gold_reserve_status, status)) => {
                    let _ = tx.send(EthereumWalletEvent::Balance { eth, usdt, usdc, usdj_eth, usdt_reserve, usdc_reserve, eurc, eurs, eurj_eth, eurc_reserve, eurs_reserve, paxg, xaut, xauj_eth, paxg_reserve, xaut_reserve, reserve_status, eur_reserve_status, gold_reserve_status, status });
                }
                Err(err) => { let _ = tx.send(EthereumWalletEvent::Failed(format!("{err:#}"))); }
            }
        });
    }


    fn poll_ethereum_wallet_events(&mut self) {
        if let Some(rx) = &self.eth_balance_rx {
            match rx.try_recv() {
                Ok(EthereumWalletEvent::Balance { eth, usdt, usdc, usdj_eth, usdt_reserve, usdc_reserve, eurc, eurs, eurj_eth, eurc_reserve, eurs_reserve, paxg, xaut, xauj_eth, paxg_reserve, xaut_reserve, reserve_status, eur_reserve_status, gold_reserve_status, status }) => {
                    self.eth_balances.eth = eth;
                    self.eth_balances.usdt = usdt;
                    self.eth_balances.usdc = usdc;
                    self.eth_balances.usdj_eth = usdj_eth;
                    self.eth_balances.usdt_reserve = usdt_reserve;
                    self.eth_balances.usdc_reserve = usdc_reserve;
                    self.eth_balances.eurc = eurc;
                    self.eth_balances.eurs = eurs;
                    self.eth_balances.eurj_eth = eurj_eth;
                    self.eth_balances.eurc_reserve = eurc_reserve;
                    self.eth_balances.eurs_reserve = eurs_reserve;
                    self.eth_balances.paxg = paxg;
                    self.eth_balances.xaut = xaut;
                    self.eth_balances.xauj_eth = xauj_eth;
                    self.eth_balances.paxg_reserve = paxg_reserve;
                    self.eth_balances.xaut_reserve = xaut_reserve;
                    self.eth_balances.reserve_status = reserve_status;
                    self.eth_balances.eur_reserve_status = eur_reserve_status;
                    self.eth_balances.gold_reserve_status = gold_reserve_status;
                    self.eth_balances.status = status;
                    self.eth_balances.updated_at = Some(Instant::now());
                    self.eth_balance_in_flight = false;
                    self.eth_balance_rx = None;
                }
                Ok(EthereumWalletEvent::Failed(err)) => {
                    self.eth_balances.status = format!("Ethereum RPC refresh failed: {err}");
                    self.eth_balance_in_flight = false;
                    self.eth_balance_rx = None;
                }
                Ok(EthereumWalletEvent::SendCreated { .. }) | Ok(EthereumWalletEvent::UsdjActionCreated { .. }) => {}
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.eth_balances.status = "Ethereum balance worker disconnected.".to_string();
                    self.eth_balance_in_flight = false;
                    self.eth_balance_rx = None;
                }
            }
        }
        if let Some(rx) = &self.eth_send_rx {
            match rx.try_recv() {
                Ok(EthereumWalletEvent::SendCreated { txids, message }) => {
                    self.eth_send_dialog.status = SendDialogStatus::Pending;
                    self.eth_send_dialog.txid = txids.join(", ");
                    self.eth_send_dialog.message = message;
                    self.eth_send_rx = None;
                    self.last_success = Some("Ethereum transaction broadcast.".to_string());
                    self.start_ethereum_balance_refresh();
                }
                Ok(EthereumWalletEvent::Failed(err)) => {
                    self.eth_send_dialog.status = SendDialogStatus::Failed;
                    self.eth_send_dialog.message = err;
                    self.eth_send_rx = None;
                }
                Ok(EthereumWalletEvent::Balance { .. }) | Ok(EthereumWalletEvent::UsdjActionCreated { .. }) => {}
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.eth_send_dialog.status = SendDialogStatus::Failed;
                    self.eth_send_dialog.message = "Ethereum send worker disconnected.".to_string();
                    self.eth_send_rx = None;
                }
            }
        }

        if let Some(rx) = &self.usdj_vault_rx {
            match rx.try_recv() {
                Ok(EthereumWalletEvent::UsdjActionCreated { txids, message }) => {
                    self.usdj_vault_dialog.status = SendDialogStatus::Pending;
                    self.usdj_vault_dialog.txid = txids.join(", ");
                    self.usdj_vault_dialog.message = message;
                    self.usdj_vault_rx = None;
                    self.last_success = Some("USDJ vault transaction broadcast.".to_string());
                    self.start_ethereum_balance_refresh();
                }
                Ok(EthereumWalletEvent::Failed(err)) => {
                    self.usdj_vault_dialog.status = SendDialogStatus::Failed;
                    self.usdj_vault_dialog.message = err;
                    self.usdj_vault_rx = None;
                }
                Ok(EthereumWalletEvent::Balance { .. }) | Ok(EthereumWalletEvent::SendCreated { .. }) => {}
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.usdj_vault_dialog.status = SendDialogStatus::Failed;
                    self.usdj_vault_dialog.message = "USDJ vault worker disconnected.".to_string();
                    self.usdj_vault_rx = None;
                }
            }
        }
    }

    fn open_ethereum_send_dialog(&mut self, asset: EthereumAsset) {
        self.eth_send_dialog.open = true;
        self.eth_send_dialog.asset = asset;
        self.eth_send_dialog.status = SendDialogStatus::Editing;
        self.eth_send_dialog.message.clear();
        self.eth_send_dialog.txid.clear();
        if self.eth_send_dialog.amount.trim().is_empty() { self.eth_send_dialog.amount = "0".to_string(); }
    }

    fn start_ethereum_send_dialog(&mut self) {
        let Some(wallet) = self.selected_ethereum_wallet() else {
            self.eth_send_dialog.status = SendDialogStatus::Failed;
            self.eth_send_dialog.message = "Create/import an Ethereum wallet first.".to_string();
            return;
        };
        let rpc = self.eth_wallets.rpc_url.trim().to_string();
        let asset = self.eth_send_dialog.asset;
        let mode = self.eth_send_dialog.mode;
        let gas_price_gwei = self.eth_send_dialog.gas_price_gwei.trim().to_string();
        let chain_id = self.eth_wallets.chain_id;
        let token_contract = self.ethereum_asset_contract_hf107(asset);
        let jobs: Vec<(String, String)> = match mode {
            EthereumSendMode::Single => {
                let recipient = self.eth_send_dialog.recipient.trim().to_string();
                let amount = self.eth_send_dialog.amount.trim().to_string();
                if !is_valid_eth_address(&recipient) || amount.is_empty() {
                    self.eth_send_dialog.status = SendDialogStatus::Failed;
                    self.eth_send_dialog.message = "Valid 0x recipient and amount are required.".to_string();
                    return;
                }
                vec![(recipient, amount)]
            }
            EthereumSendMode::Multi => {
                let mut out = Vec::new();
                for (idx, row) in self.eth_send_dialog.multi_rows.iter().enumerate() {
                    let recipient = row.recipient.trim();
                    let amount = row.amount.trim();
                    if recipient.is_empty() && amount.is_empty() { continue; }
                    if !is_valid_eth_address(recipient) || amount.is_empty() {
                        self.eth_send_dialog.status = SendDialogStatus::Failed;
                        self.eth_send_dialog.message = format!("Invalid multi-send row {}. Use a valid 0x address and amount.", idx + 1);
                        return;
                    }
                    out.push((recipient.to_string(), amount.to_string()));
                }
                if out.is_empty() {
                    self.eth_send_dialog.status = SendDialogStatus::Failed;
                    self.eth_send_dialog.message = "Add at least one multi-send row.".to_string();
                    return;
                }
                if out.len() > 32 {
                    self.eth_send_dialog.status = SendDialogStatus::Failed;
                    self.eth_send_dialog.message = "Ethereum multi-send is capped at 32 separate transactions.".to_string();
                    return;
                }
                out
            }
        };
        if asset != EthereumAsset::Eth && token_contract.as_deref().map(|c| !is_valid_eth_address(c)).unwrap_or(true) {
            self.eth_send_dialog.status = SendDialogStatus::Failed;
            self.eth_send_dialog.message = format!("Paste a valid {} token contract before sending.", asset.symbol());
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.eth_send_rx = Some(rx);
        self.eth_send_dialog.status = SendDialogStatus::Sending;
        self.eth_send_dialog.message = format!("Signing and broadcasting {} {} transaction(s)...", jobs.len(), asset.symbol());
        thread::spawn(move || {
            match execute_ethereum_sends_hf102(&rpc, chain_id, &wallet, asset, token_contract.as_deref(), jobs, &gas_price_gwei) {
                Ok(txids) => {
                    let message = format!("Broadcast {} Ethereum transaction(s): {}", txids.len(), txids.iter().map(|t| shorten_eth_address(t)).collect::<Vec<_>>().join(", "));
                    let _ = tx.send(EthereumWalletEvent::SendCreated { txids, message });
                }
                Err(err) => { let _ = tx.send(EthereumWalletEvent::Failed(format!("{err:#}"))); }
            }
        });
    }


    fn ethereum_asset_contract_hf107(&self, asset: EthereumAsset) -> Option<String> {
        match asset {
            EthereumAsset::Eth => None,
            EthereumAsset::Usdt => {
                let c = self.prefs.eth_usdt_contract_override.trim();
                Some(if is_valid_eth_address(c) { c.to_string() } else { ETHEREUM_USDT_ADDRESS.to_string() })
            }
            EthereumAsset::Usdc => {
                let c = self.prefs.eth_usdc_contract_override.trim();
                Some(if is_valid_eth_address(c) { c.to_string() } else { ETHEREUM_USDC_ADDRESS.to_string() })
            }
            EthereumAsset::Usdj => {
                let c = self.prefs.eth_usdj_token_contract.trim();
                if is_valid_eth_address(c) { Some(c.to_string()) } else if is_valid_eth_address(ETHEREUM_USDJ_ADDRESS_DEFAULT) { Some(ETHEREUM_USDJ_ADDRESS_DEFAULT.to_string()) } else { None }
            }
            EthereumAsset::Eurc => {
                let c = self.prefs.eth_eurc_contract_override.trim();
                Some(if is_valid_eth_address(c) { c.to_string() } else { ETHEREUM_EURC_ADDRESS.to_string() })
            }
            EthereumAsset::Eurs => {
                let c = self.prefs.eth_eurs_contract_override.trim();
                Some(if is_valid_eth_address(c) { c.to_string() } else { ETHEREUM_EURS_ADDRESS.to_string() })
            }
            EthereumAsset::Eurj => {
                let c = self.prefs.eth_eurj_token_contract.trim();
                if is_valid_eth_address(c) { Some(c.to_string()) } else if is_valid_eth_address(ETHEREUM_EURJ_ADDRESS_DEFAULT) { Some(ETHEREUM_EURJ_ADDRESS_DEFAULT.to_string()) } else { None }
            }
            EthereumAsset::Paxg => {
                let c = self.prefs.eth_paxg_contract_override.trim();
                Some(if is_valid_eth_address(c) { c.to_string() } else { ETHEREUM_PAXG_ADDRESS.to_string() })
            }
            EthereumAsset::Xaut => {
                let c = self.prefs.eth_xaut_contract_override.trim();
                Some(if is_valid_eth_address(c) { c.to_string() } else { ETHEREUM_XAUT_ADDRESS.to_string() })
            }
            EthereumAsset::Xauj => {
                let c = self.prefs.eth_xauj_token_contract.trim();
                if is_valid_eth_address(c) { Some(c.to_string()) } else if is_valid_eth_address(ETHEREUM_XAUJ_ADDRESS_DEFAULT) { Some(ETHEREUM_XAUJ_ADDRESS_DEFAULT.to_string()) } else { None }
            }
        }
    }

    fn fiatj_token_contract_hf108(&self, family: StablecoinFamily) -> String {
        let custom = match family { StablecoinFamily::Usd => self.prefs.eth_usdj_token_contract.trim(), StablecoinFamily::Eur => self.prefs.eth_eurj_token_contract.trim(), StablecoinFamily::Gold => self.prefs.eth_xauj_token_contract.trim() };
        let default = match family { StablecoinFamily::Usd => ETHEREUM_USDJ_ADDRESS_DEFAULT, StablecoinFamily::Eur => ETHEREUM_EURJ_ADDRESS_DEFAULT, StablecoinFamily::Gold => ETHEREUM_XAUJ_ADDRESS_DEFAULT };
        if is_valid_eth_address(custom) { custom.to_string() } else { default.to_string() }
    }

    fn fiatj_vault_contract_hf108(&self, family: StablecoinFamily) -> String {
        let custom = match family { StablecoinFamily::Usd => self.prefs.eth_usdj_vault_contract.trim(), StablecoinFamily::Eur => self.prefs.eth_eurj_vault_contract.trim(), StablecoinFamily::Gold => self.prefs.eth_xauj_vault_contract.trim() };
        let default = match family { StablecoinFamily::Usd => ETHEREUM_USDJ_VAULT_ADDRESS_DEFAULT, StablecoinFamily::Eur => ETHEREUM_EURJ_VAULT_ADDRESS_DEFAULT, StablecoinFamily::Gold => ETHEREUM_XAUJ_VAULT_ADDRESS_DEFAULT };
        if is_valid_eth_address(custom) { custom.to_string() } else { default.to_string() }
    }

    fn fiatj_contracts_ready_hf108(&self, family: StablecoinFamily) -> bool {
        is_valid_eth_address(&self.fiatj_token_contract_hf108(family)) && is_valid_eth_address(&self.fiatj_vault_contract_hf108(family))
    }

    fn usdj_contracts_ready_hf107(&self) -> bool { self.fiatj_contracts_ready_hf108(StablecoinFamily::Usd) }

    fn open_usdj_vault_dialog_hf107(&mut self, mode: UsdjVaultMode, asset: EthereumAsset) {
        self.usdj_vault_dialog.open = true;
        self.usdj_vault_dialog.mode = mode;
        self.usdj_vault_dialog.asset = match asset { EthereumAsset::Usdc => EthereumAsset::Usdc, EthereumAsset::Eurc => EthereumAsset::Eurc, EthereumAsset::Eurs => EthereumAsset::Eurs, EthereumAsset::Paxg => EthereumAsset::Paxg, EthereumAsset::Xaut => EthereumAsset::Xaut, _ => EthereumAsset::Usdt };
        self.usdj_vault_dialog.status = SendDialogStatus::Editing;
        self.usdj_vault_dialog.txid.clear();
        self.usdj_vault_dialog.message.clear();
        if let Some(w) = self.selected_ethereum_wallet() {
            if self.usdj_vault_dialog.receiver.trim().is_empty() { self.usdj_vault_dialog.receiver = w.address; }
        }
    }

    fn start_usdj_vault_action_hf107(&mut self) {
        let Some(wallet) = self.selected_ethereum_wallet() else {
            self.usdj_vault_dialog.status = SendDialogStatus::Failed;
            self.usdj_vault_dialog.message = "Create/import an Ethereum wallet first.".to_string();
            return;
        };
        let family = self.usdj_vault_dialog.asset.family().unwrap_or(StablecoinFamily::Usd);
        if !self.fiatj_contracts_ready_hf108(family) {
            self.usdj_vault_dialog.status = SendDialogStatus::Failed;
            self.usdj_vault_dialog.message = format!("Paste valid {} token and ReserveVault contract addresses first.", family.token_symbol());
            return;
        }
        let receiver = self.usdj_vault_dialog.receiver.trim().to_string();
        if !is_valid_eth_address(&receiver) {
            self.usdj_vault_dialog.status = SendDialogStatus::Failed;
            self.usdj_vault_dialog.message = "Receiver must be a valid 0x Ethereum address.".to_string();
            return;
        }
        let rpc = self.eth_wallets.rpc_url.trim().to_string();
        let usdj_contract = self.fiatj_token_contract_hf108(family);
        let vault_contract = self.fiatj_vault_contract_hf108(family);
        let mode = self.usdj_vault_dialog.mode;
        let asset = self.usdj_vault_dialog.asset;
        let amount = self.usdj_vault_dialog.amount.trim().to_string();
        let gas_price_gwei = self.usdj_vault_dialog.gas_price_gwei.trim().to_string();
        let Some(stable_contract) = self.ethereum_asset_contract_hf107(asset) else {
            self.usdj_vault_dialog.status = SendDialogStatus::Failed;
            self.usdj_vault_dialog.message = format!("Paste a valid {} token contract first.", asset.symbol());
            return;
        };
        let chain_id = self.eth_wallets.chain_id;
        let (tx, rx) = mpsc::channel();
        self.usdj_vault_rx = Some(rx);
        self.usdj_vault_dialog.status = SendDialogStatus::Sending;
        self.usdj_vault_dialog.message = match mode {
            UsdjVaultMode::Infuse => format!("Approving and minting {amount} {} into {}...", asset.symbol(), family.token_symbol()),
            UsdjVaultMode::Melt => format!("Melting {amount} {} for {}...", family.token_symbol(), asset.symbol()),
        };
        thread::spawn(move || {
            match execute_usdj_vault_action_hf107(&rpc, chain_id, &wallet, &usdj_contract, &vault_contract, &stable_contract, mode, asset, &amount, &receiver, &gas_price_gwei) {
                Ok(txids) => {
                    let action = match mode { UsdjVaultMode::Infuse => "stablecoin mint", UsdjVaultMode::Melt => "stablecoin melt" };
                    let message = format!("{action} broadcast {} Ethereum transaction(s): {}", txids.len(), txids.iter().map(|t| shorten_eth_address(t)).collect::<Vec<_>>().join(", "));
                    let _ = tx.send(EthereumWalletEvent::UsdjActionCreated { txids, message });
                }
                Err(err) => { let _ = tx.send(EthereumWalletEvent::Failed(format!("{err:#}"))); }
            }
        });
    }

    fn ui_usdj_vault_dialog_window_hf107(&mut self, ctx: &egui::Context) {
        if !self.usdj_vault_dialog.open { return; }
        let mut open = self.usdj_vault_dialog.open;
        let mut close_requested = false;
        let family = self.usdj_vault_dialog.asset.family().unwrap_or(StablecoinFamily::Usd);
        let title = match self.usdj_vault_dialog.mode { UsdjVaultMode::Infuse => format!("Mint {}", family.token_symbol()), UsdjVaultMode::Melt => format!("Melt {} for {}", family.token_symbol(), self.usdj_vault_dialog.asset.symbol()) };
        egui::Window::new(title)
            .open(&mut open)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    self.ui_icon(ui, family.token_icon(), 20.0);
                    ui.label("Ethereum pooled-reserve mint/redeem. QUB bridge remains disabled until the bridge is live.");
                });
                if let Some(w) = self.selected_ethereum_wallet() { ui.small(format!("From: {}", w.address)); }
                let token_contract = self.fiatj_token_contract_hf108(family);
                let vault_contract = self.fiatj_vault_contract_hf108(family);
                ui.small(format!("{} token: {}", family.token_symbol(), if token_contract.trim().is_empty() { "not configured".to_string() } else { shorten_eth_address(token_contract.trim()) }));
                ui.small(format!("Reserve vault: {}", if vault_contract.trim().is_empty() { "not configured".to_string() } else { shorten_eth_address(vault_contract.trim()) }));
                ui.separator();
                ui.horizontal(|ui| {
                    match family {
                        StablecoinFamily::Usd => {
                            ui.radio_value(&mut self.usdj_vault_dialog.asset, EthereumAsset::Usdt, "USDT bucket");
                            ui.radio_value(&mut self.usdj_vault_dialog.asset, EthereumAsset::Usdc, "USDC bucket");
                        }
                        StablecoinFamily::Eur => {
                            ui.radio_value(&mut self.usdj_vault_dialog.asset, EthereumAsset::Eurc, "EURC bucket");
                            ui.radio_value(&mut self.usdj_vault_dialog.asset, EthereumAsset::Eurs, "EURS bucket");
                        }
                        StablecoinFamily::Gold => {
                            ui.radio_value(&mut self.usdj_vault_dialog.asset, EthereumAsset::Paxg, "PAXG bucket");
                            ui.radio_value(&mut self.usdj_vault_dialog.asset, EthereumAsset::Xaut, "XAUt bucket");
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Amount");
                    ui.text_edit_singleline(&mut self.usdj_vault_dialog.amount);
                    ui.label(match self.usdj_vault_dialog.mode { UsdjVaultMode::Infuse => self.usdj_vault_dialog.asset.symbol(), UsdjVaultMode::Melt => family.token_symbol() });
                });
                ui.horizontal(|ui| {
                    ui.label("Receiver");
                    ui.text_edit_singleline(&mut self.usdj_vault_dialog.receiver);
                    if ui.button("Self").clicked() {
                        if let Some(w) = self.selected_ethereum_wallet() { self.usdj_vault_dialog.receiver = w.address; }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Gas price gwei");
                    ui.text_edit_singleline(&mut self.usdj_vault_dialog.gas_price_gwei);
                    ui.small("blank = RPC eth_gasPrice");
                });
                match self.usdj_vault_dialog.mode {
                    UsdjVaultMode::Infuse => {
                        ui.small(format!("Mint sends two Ethereum transactions: approve {} allowance, then vault infuse() mints {}.", self.usdj_vault_dialog.asset.symbol(), family.token_symbol()));
                    }
                    UsdjVaultMode::Melt => {
                        ui.small(format!("Melt burns Ethereum {} through the vault and redeems the selected {} reserve bucket if liquidity exists.", family.token_symbol(), self.usdj_vault_dialog.asset.symbol()));
                    }
                }
                if self.usdj_vault_dialog.status == SendDialogStatus::Sending { ui.label("Broadcasting..."); }
                if !self.usdj_vault_dialog.txid.is_empty() { ui.monospace(&self.usdj_vault_dialog.txid); }
                if !self.usdj_vault_dialog.message.is_empty() { ui.small(&self.usdj_vault_dialog.message); }
                ui.horizontal(|ui| {
                    let caption = match self.usdj_vault_dialog.mode { UsdjVaultMode::Infuse => format!("Mint {}", family.token_symbol()), UsdjVaultMode::Melt => format!("Melt for {}", self.usdj_vault_dialog.asset.symbol()) };
                    let action_icon = match self.usdj_vault_dialog.mode { UsdjVaultMode::Infuse => "infuse", UsdjVaultMode::Melt => "melt" };
                    if self.ui_icon_button_enabled(ui, self.usdj_vault_dialog.status != SendDialogStatus::Sending, action_icon, &caption).clicked() {
                        self.start_usdj_vault_action_hf107();
                    }
                    if ui.button("Close").clicked() { close_requested = true; }
                });
            });
        self.usdj_vault_dialog.open = if close_requested { false } else { open };
    }

    fn ui_contract_address_text_hf111(&self, ui: &mut egui::Ui, label: &str, addr: &str) {
        ui.label(label);
        let shown = if addr.trim().is_empty() { "not configured" } else { addr.trim() };
        ui.monospace(shown);
        ui.end_row();
    }

    fn ui_stablecoin_contracts_readonly_hf111(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("USDJ / USDT / USDC").strong());
        egui::Grid::new("usdj_contracts_readonly_hf111").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            self.ui_contract_address_text_hf111(ui, "USDT token", ETHEREUM_USDT_ADDRESS);
            self.ui_contract_address_text_hf111(ui, "USDC token", ETHEREUM_USDC_ADDRESS);
            self.ui_contract_address_text_hf111(ui, "USDJ token", &self.fiatj_token_contract_hf108(StablecoinFamily::Usd));
            self.ui_contract_address_text_hf111(ui, "USDJ Reserve vault", &self.fiatj_vault_contract_hf108(StablecoinFamily::Usd));
        });
        ui.separator();
        ui.label(egui::RichText::new("EURJ / EURC / EURS").strong());
        egui::Grid::new("eurj_contracts_readonly_hf111").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            self.ui_contract_address_text_hf111(ui, "EURC token", ETHEREUM_EURC_ADDRESS);
            self.ui_contract_address_text_hf111(ui, "EURS token", ETHEREUM_EURS_ADDRESS);
            self.ui_contract_address_text_hf111(ui, "EURJ token", &self.fiatj_token_contract_hf108(StablecoinFamily::Eur));
            self.ui_contract_address_text_hf111(ui, "EURJ Reserve vault", &self.fiatj_vault_contract_hf108(StablecoinFamily::Eur));
        });
        ui.separator();
        ui.label(egui::RichText::new("XAUJ / PAXG / XAUt").strong());
        egui::Grid::new("xauj_contracts_readonly_hf111").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            self.ui_contract_address_text_hf111(ui, "PAXG token", ETHEREUM_PAXG_ADDRESS);
            self.ui_contract_address_text_hf111(ui, "XAUt token", ETHEREUM_XAUT_ADDRESS);
            self.ui_contract_address_text_hf111(ui, "XAUJ token", &self.fiatj_token_contract_hf108(StablecoinFamily::Gold));
            self.ui_contract_address_text_hf111(ui, "XAUJ Reserve vault", &self.fiatj_vault_contract_hf108(StablecoinFamily::Gold));
        });
    }

    fn ui_ethereum_wallets_section_hf102(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.horizontal(|ui| {
                self.ui_icon(ui, "eth", 22.0);
                ui.label(egui::RichText::new("Ethereum").strong());
                ui.label(egui::RichText::new("Ethereum wallet and stablecoin contracts").weak());
            });
            ui.small("Create/import an Ethereum wallet for ETH and supported stablecoin transfers. Stablecoin mint/redeem uses Ethereum contracts; QUB bridge mint/burn remains disabled until the bridge is live.");
            ui.colored_label(egui::Color32::from_rgb(255, 175, 75), "Security: imported Ethereum private keys are stored locally in QUB Core's data folder. Back them up and protect this PC before using real funds.");
            ui.horizontal(|ui| {
                ui.label("RPC");
                if ui.text_edit_singleline(&mut self.eth_wallets.rpc_url).changed() { self.save_ethereum_wallets(); }
                if self.ui_icon_button_enabled(ui, !self.eth_balance_in_flight, "sync", "Refresh ETH balances").clicked() { self.start_ethereum_balance_refresh(); }
            });
            ui.horizontal(|ui| {
                ui.label("Chain ID");
                if ui.add(egui::DragValue::new(&mut self.eth_wallets.chain_id).range(1..=u64::MAX)).changed() { self.save_ethereum_wallets(); }
                ui.small("1=Ethereum mainnet, 11155111=Sepolia, 31337=Anvil/local. Must match RPC.");
            });
            egui::CollapsingHeader::new("Stablecoin Ethereum contracts")
                .default_open(false)
                .show(ui, |ui| {
                    ui.small("Official contract addresses are bundled with QUB Core. They are shown here for verification; they are not editable in the production UI.");
                    self.ui_stablecoin_contracts_readonly_hf111(ui);
                });
            ui.horizontal_wrapped(|ui| {
                if self.ui_icon_button_enabled(ui, true, "wallet-address", "Create Ethereum wallet").clicked() { self.create_ethereum_wallet_hf102(); }
                self.ui_icon(ui, "import-private-key", 18.0);
                ui.label("Import private key");
                ui.add(egui::TextEdit::singleline(&mut self.eth_wallet_dialog.import_private_key).password(true).desired_width(220.0));
                ui.label("Label");
                ui.text_edit_singleline(&mut self.eth_wallet_dialog.import_label);
                if self.ui_icon_button_enabled(ui, true, "import-private-key", "Import").clicked() { self.import_ethereum_wallet_hf102(); }
            });
            if !self.eth_wallet_dialog.message.is_empty() { ui.small(&self.eth_wallet_dialog.message); }
            ui.separator();
            if self.eth_wallets.wallets.is_empty() {
                ui.small("No Ethereum wallets yet.");
            } else {
                ui.label(egui::RichText::new("Ethereum public addresses").strong());
                for idx in 0..self.eth_wallets.wallets.len() {
                    let w = self.eth_wallets.wallets[idx].clone();
                    ui.horizontal_wrapped(|ui| {
                        if ui.radio_value(&mut self.eth_wallets.selected_index, idx, "").changed() { self.save_ethereum_wallets(); self.start_ethereum_balance_refresh(); }
                        self.ui_icon(ui, "eth", 16.0);
                        ui.monospace(shorten_eth_address(&w.address));
                        ui.small(&w.label);
                        if ui.button("Copy").clicked() { ui.ctx().copy_text(w.address.clone()); }
                        let qr_resp = self.ui_icon_only_button_enabled(ui, true, "qr", "Show ETH address QR");
                        if qr_resp.hovered() {
                            self.qr_hover_until = Some(Instant::now() + Duration::from_secs(1));
                            self.qr_hover_address = w.address.clone();
                        }
                    });
                    if self.qr_hover_until.is_some() { self.ui_address_qr_hover(ui.ctx(), &w.address); }
                }
            }
        });
    }

    fn ui_eth_balance_card_hf102(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                self.ui_icon(ui, "eth", 24.0);
                ui.label(egui::RichText::new("ETH").size(20.0).strong());
                ui.label(egui::RichText::new("Ethereum mainnet wallet").weak());
                if self.ui_icon_button_enabled(ui, !self.eth_balance_in_flight && !self.eth_wallets.wallets.is_empty(), "sync", "Refresh").clicked() { self.start_ethereum_balance_refresh(); }
            });
            if let Some(w) = self.selected_ethereum_wallet() {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Address");
                    ui.monospace(shorten_eth_address(&w.address));
                    if ui.button("Copy").clicked() { ui.ctx().copy_text(w.address.clone()); }
                    let qr_resp = self.ui_icon_only_button_enabled(ui, true, "qr", "Show ETH QR");
                    if qr_resp.hovered() {
                        self.qr_hover_until = Some(Instant::now() + Duration::from_secs(1));
                        self.qr_hover_address = w.address.clone();
                    }
                });
                self.ui_address_qr_hover(ui.ctx(), &w.address);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Spendable").strong());
                    ui.label(egui::RichText::new(format!("{} ETH", self.eth_balances.eth)).size(18.0).strong());
                });
                ui.small(format!("USDT: {} | USDC: {} | USDJ: {} | EURC: {} | EURS: {} | EURJ: {} | PAXG: {} | XAUt: {} | XAUJ: {} | {}", self.eth_balances.usdt, self.eth_balances.usdc, self.eth_balances.usdj_eth, self.eth_balances.eurc, self.eth_balances.eurs, self.eth_balances.eurj_eth, self.eth_balances.paxg, self.eth_balances.xaut, self.eth_balances.xauj_eth, self.eth_balances.status));
                ui.horizontal_wrapped(|ui| {
                    if self.ui_icon_button_enabled(ui, true, "send", "Send ETH").clicked() { self.open_ethereum_send_dialog(EthereumAsset::Eth); }
                    self.ui_icon_button_enabled(ui, false, "buy", "Buy");
                    self.ui_icon_button_enabled(ui, false, "sell", "Sell");
                });
            } else {
                ui.small("Create/import an Ethereum wallet in Create / import address to enable ETH, USDT and USDC transfers.");
            }
        });
    }

    fn ui_ethereum_send_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.eth_send_dialog.open { return; }
        let mut open = self.eth_send_dialog.open;
        egui::Window::new(format!("Send {} on Ethereum", self.eth_send_dialog.asset.symbol()))
            .open(&mut open)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    self.ui_icon(ui, "eth", 20.0);
                    ui.label("Ethereum mainnet transaction. Multi-send broadcasts separate transactions; each pays gas.");
                });
                if let Some(w) = self.selected_ethereum_wallet() { ui.small(format!("From: {}", w.address)); }
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.eth_send_dialog.mode, EthereumSendMode::Single, "Single");
                    ui.radio_value(&mut self.eth_send_dialog.mode, EthereumSendMode::Multi, "Multi");
                });
                ui.horizontal(|ui| {
                    ui.label("Gas price gwei");
                    ui.text_edit_singleline(&mut self.eth_send_dialog.gas_price_gwei);
                    ui.small("blank = RPC eth_gasPrice");
                });
                match self.eth_send_dialog.mode {
                    EthereumSendMode::Single => {
                        ui.label("Recipient 0x address");
                        ui.text_edit_singleline(&mut self.eth_send_dialog.recipient);
                        ui.label(format!("Amount {}", self.eth_send_dialog.asset.symbol()));
                        ui.text_edit_singleline(&mut self.eth_send_dialog.amount);
                    }
                    EthereumSendMode::Multi => {
                        ui.label(format!("Recipients for {}. Add rows; each row broadcasts one Ethereum transaction and pays gas.", self.eth_send_dialog.asset.symbol()));
                        egui::Grid::new("eth_multi_send_rows_hf108").num_columns(4).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
                            ui.strong("#"); ui.strong("Recipient 0x address"); ui.strong(format!("Amount {}", self.eth_send_dialog.asset.symbol())); ui.strong(""); ui.end_row();
                            let mut remove_idx: Option<usize> = None;
                            for (idx, row) in self.eth_send_dialog.multi_rows.iter_mut().enumerate() {
                                ui.label((idx + 1).to_string());
                                ui.text_edit_singleline(&mut row.recipient);
                                ui.text_edit_singleline(&mut row.amount);
                                if ui.button("Remove").clicked() { remove_idx = Some(idx); }
                                ui.end_row();
                            }
                            if let Some(idx) = remove_idx {
                                if self.eth_send_dialog.multi_rows.len() > 1 { self.eth_send_dialog.multi_rows.remove(idx); }
                                else if let Some(row) = self.eth_send_dialog.multi_rows.get_mut(0) { *row = EthereumSendRow::default(); }
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Add row").clicked() { self.eth_send_dialog.multi_rows.push(EthereumSendRow::default()); }
                            if ui.button("Clear rows").clicked() { self.eth_send_dialog.multi_rows = vec![EthereumSendRow::default()]; }
                        });
                    }
                }
                match self.eth_send_dialog.status {
                    SendDialogStatus::Editing => {
                        if self.ui_icon_button_enabled(ui, !self.eth_wallets.wallets.is_empty(), "send", "Sign & broadcast").clicked() { self.start_ethereum_send_dialog(); }
                    }
                    SendDialogStatus::Sending => { ui.spinner(); ui.label(&self.eth_send_dialog.message); }
                    SendDialogStatus::Pending => {
                        ui.horizontal(|ui| { self.ui_icon(ui, "success", 18.0); ui.label("Broadcast"); });
                        ui.label(&self.eth_send_dialog.message);
                        ui.monospace(&self.eth_send_dialog.txid);
                        if ui.button("New Ethereum send").clicked() { self.eth_send_dialog.status = SendDialogStatus::Editing; self.eth_send_dialog.txid.clear(); self.eth_send_dialog.message.clear(); }
                    }
                    SendDialogStatus::Failed => {
                        ui.colored_label(egui::Color32::from_rgb(255, 105, 105), "Failed");
                        ui.label(&self.eth_send_dialog.message);
                        if ui.button("Retry").clicked() { self.eth_send_dialog.status = SendDialogStatus::Editing; self.eth_send_dialog.message.clear(); }
                    }
                    SendDialogStatus::Confirmed => {}
                }
            });
        self.eth_send_dialog.open = open;
    }

    fn tr_activity_type(&self, filter: ActivityTypeFilter) -> &'static str {
        match filter {
            ActivityTypeFilter::All => self.tr("All", "All"),
            ActivityTypeFilter::Transfer => self.tr("Transfer", "Transfer"),
            ActivityTypeFilter::Mining => self.tr("Mining", "Mining"),
            ActivityTypeFilter::QnsRegistration => self.tr("QNS Registration", "QNS Registration"),
            ActivityTypeFilter::Library => self.tr("Library", "Library"),
            ActivityTypeFilter::Infusion => self.tr("Infusion", "Infusion"),
            ActivityTypeFilter::Melt => self.tr("Melt", "Melt"),
            ActivityTypeFilter::Conversion => self.tr("Conversion", "Conversion"),
        }
    }

    fn tr_activity_status(&self, filter: ActivityStatusFilter) -> &'static str {
        match filter {
            ActivityStatusFilter::All => self.tr("All", "All"),
            ActivityStatusFilter::Mempool => self.tr("Mempool", "Mempool"),
            ActivityStatusFilter::Confirmed => self.tr("Confirmed", "Confirmed"),
            ActivityStatusFilter::Immature => self.tr("Immature", "Immature"),
            ActivityStatusFilter::PendingDecision => self.tr("Pending Decision", "Pending Decision"),
            ActivityStatusFilter::Matured => self.tr("Matured", "Matured"),
        }
    }

    fn tr_activity_direction(&self, filter: ActivityDirectionFilter) -> &'static str {
        match filter {
            ActivityDirectionFilter::All => self.tr("All", "All"),
            ActivityDirectionFilter::Incoming => self.tr("Incoming", "Incoming"),
            ActivityDirectionFilter::Outgoing => self.tr("Outgoing", "Outgoing"),
        }
    }

    fn tr_activity_asset(&self, filter: ActivityAssetFilter) -> &'static str {
        match filter {
            ActivityAssetFilter::All => self.tr("All", "All"),
            ActivityAssetFilter::Qub => "QUB",
            ActivityAssetFilter::Jin => "JIN",
        }
    }

    fn ui_address_balances(&mut self, ui: &mut egui::Ui) {
        self.ui_heading_icon(ui, "address-balances", self.tr("Address balances", "Address balances"));
        ui.small("Spendable balances and assets for the selected payout/default address. This is public-address readable and does not require private keys.");
        if self.initial_loading || self.wallet_sync_in_flight || self.snapshot_in_flight || HF105_CATCHUP_IN_FLIGHT.load(Ordering::SeqCst) {
            self.ui_sync_progress_bar_rows(ui, "Loading crypto balances", 3);
            ui.small("Temporary 0 balances during startup mean the chain/wallet view is still loading, not that funds are gone. JIN balances show only 2+ confirmed JIN.");
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if self.ui_icon_selectable_button(ui, self.prefs.balance_tab == BalanceTab::Crypto, "crypto", "Crypto").clicked() {
                self.prefs.balance_tab = BalanceTab::Crypto;
                self.prefs_dirty = true;
            }
            if self.ui_icon_selectable_button(ui, self.prefs.balance_tab == BalanceTab::Qns, "qns", "QNS").clicked() {
                self.prefs.balance_tab = BalanceTab::Qns;
                self.prefs_dirty = true;
            }
        });
        ui.separator();
        match self.prefs.balance_tab {
            BalanceTab::Crypto => {
                let qub_spendable = self.snapshot.spendable.to_string();
                let qub_immature = format!("{} QUB", self.snapshot.immature);
                let jin_total = self.snapshot.jin_total.to_string();
                let jin_confirmed_infusion = confirmed_jin_total_infusion_display(&jin_total, &self.enjin_metrics.per_jin_infusion);

                self.ui_crypto_balance_card(ui, "qub", "QUB", &qub_spendable, &qub_immature, "0 JIN", true);
                ui.add_space(8.0);
                self.ui_jin_balance_card_hf99(ui, &jin_total, &jin_confirmed_infusion);
                ui.add_space(8.0);
                self.ui_eth_balance_card_hf102(ui);
                ui.add_space(8.0);
                self.ui_stablecoins_balance_card_hf108(ui);
            }
            BalanceTab::Qns => {
                ui.horizontal_wrapped(|ui| {
                    if self.ui_icon_button_enabled(ui, self.snapshot.wallet_keys > 0, "register-qub", "Register .qub").clicked() {
                        self.open_qns_dialog();
                    }
                    ui.small(format!("{} held name(s)", self.snapshot.owned_qns.len()));
                });
                ui.add_space(6.0);
                if self.snapshot.owned_qns.is_empty() {
                    ui.small("No .qub names held by this address yet.");
                } else {
                    for name in self.snapshot.owned_qns.clone() {
                        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                            self.ui_icon_label(ui, "qns", name.clone());
                            ui.small("Permanent QNS name owned by the selected address.");
                            ui.horizontal_wrapped(|ui| {
                                self.ui_icon_button_enabled(ui, false, "sell", "Sell");
                                self.ui_icon_button_enabled(ui, false, "list-asset", "List");
                                self.ui_icon_button_enabled(ui, false, "auction", "Auction");
                            });
                        });
                        ui.add_space(6.0);
                    }
                }
            }
        }
    }

    fn ui_crypto_balance_card(&mut self, ui: &mut egui::Ui, icon: &str, symbol: &str, spendable: &str, immature: &str, infusion: &str, is_qub: bool) {
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.horizontal(|ui| {
                self.ui_icon(ui, icon, 24.0);
                ui.label(egui::RichText::new(symbol).size(20.0).strong());
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Spendable").strong());
                ui.label(egui::RichText::new(format!("{} {}", spendable, symbol)).size(18.0).strong());
            });
            ui.small(format!("Immature / pending rewards: {}", immature));
            if !is_qub {
                ui.small("QUB Core: JIN balance displays confirmed JIN only (2+ confirmations); pending buys/transfers stay in Activity until confirmed.");
            }
            ui.horizontal(|ui| {
                ui.small("Price: -");
                ui.separator();
                if is_qub {
                    ui.small(format!("Infusion per {}: {}", symbol, infusion));
                    self.ui_info_tip(ui, "Infusion is the recoverable backing value attached to an asset. QUB will later be meltable to reclaim JIN.");
                } else {
                    ui.small(format!("Infusion: {}", infusion));
                    self.ui_info_tip(ui, "Total confirmed ENJ infusion for your displayed JIN balance: confirmed JIN balance multiplied by the Live Chain Data per-JIN ENJ infusion. Pending or <2-confirmation JIN is not included.");
                }
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if self.ui_icon_button_enabled(ui, self.snapshot.wallet_keys > 0, "send", "Send").clicked() {
                    self.open_send_dialog_for(symbol);
                }
                if !is_qub {
                    let buy = egui::Button::new(egui::RichText::new("Buy JIN").strong().color(egui::Color32::WHITE))
                        .fill(egui::Color32::from_rgb(0, 146, 214));
                    if ui.add_enabled(self.snapshot.wallet_keys > 0, buy).clicked() { self.open_buy_jin_dialog(ui.ctx()); }
                } else {
                    self.ui_icon_button_enabled(ui, false, "buy", "Buy");
                }
                self.ui_icon_button_enabled(ui, false, "sell", "Sell");
                self.ui_icon_button_enabled(ui, false, "list-asset", "List");
                self.ui_icon_button_enabled(ui, false, "offer", "Offer");
                if is_qub {
                    self.ui_icon_button_enabled(ui, false, "melt", "Melt");
                } else {
                    self.ui_icon_button_enabled(ui, false, "infuse", "Infuse");
                    self.ui_icon_button_enabled(ui, false, "convert", "Convert disabled until bridge is live");
                }
            });
        });
    }

    fn ui_jin_balance_card_hf99(&mut self, ui: &mut egui::Ui, confirmed_jin: &str, confirmed_infusion: &str) {
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.horizontal(|ui| {
                self.ui_icon(ui, "jin", 24.0);
                ui.label(egui::RichText::new("JIN").size(20.0).strong());
                ui.add_space(8.0);
                if self.ui_icon_selectable_button(ui, self.prefs.jin_balance_tab == JinBalanceSubTab::Coin, "jin", "Coin").clicked() {
                    self.prefs.jin_balance_tab = JinBalanceSubTab::Coin;
                    self.prefs_dirty = true;
                }
                if self.ui_icon_selectable_button(ui, self.prefs.jin_balance_tab == JinBalanceSubTab::Token, "jin-token", "Token").clicked() {
                    self.prefs.jin_balance_tab = JinBalanceSubTab::Token;
                    self.prefs_dirty = true;
                }
            });
            ui.separator();
            match self.prefs.jin_balance_tab {
                JinBalanceSubTab::Coin => self.ui_jin_coin_card_body_hf99(ui, confirmed_jin, confirmed_infusion),
                JinBalanceSubTab::Token => self.ui_jin_token_card_body_hf99(ui),
            }
        });
    }

    fn ui_jin_coin_card_body_hf99(&mut self, ui: &mut egui::Ui, confirmed_jin: &str, confirmed_infusion: &str) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Spendable").strong());
            ui.label(egui::RichText::new(format!("{} JIN", confirmed_jin)).size(18.0).strong());
        });
        ui.small("QUB Core: JIN Coin balance displays confirmed JIN only (2+ confirmations); pending buys/transfers stay in Activity until confirmed.");
        ui.horizontal(|ui| {
            ui.small("Price: -");
            ui.separator();
            ui.small(format!("Infusion: {}", confirmed_infusion));
            self.ui_info_tip(ui, "Total confirmed ENJ infusion for your displayed JIN Coin balance: confirmed JIN balance multiplied by the Live Chain Data per-JIN ENJ infusion. Pending or <2-confirmation JIN is not included.");
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if self.ui_icon_button_enabled(ui, self.snapshot.wallet_keys > 0, "send", "Send").clicked() {
                self.open_send_dialog_for("JIN");
            }
            let buy = egui::Button::new(egui::RichText::new("Buy JIN").strong().color(egui::Color32::WHITE))
                .fill(egui::Color32::from_rgb(0, 146, 214));
            if ui.add_enabled(self.snapshot.wallet_keys > 0, buy).clicked() { self.open_buy_jin_dialog(ui.ctx()); }
            self.ui_icon_button_enabled(ui, false, "sell", "Sell");
            self.ui_icon_button_enabled(ui, false, "list-asset", "List");
            self.ui_icon_button_enabled(ui, false, "offer", "Offer");
            self.ui_icon_button_enabled(ui, false, "infuse", "Infuse");
            self.ui_icon_button_enabled(ui, false, "convert", "Convert disabled until bridge is live");
        });
    }

    fn ui_jin_token_card_body_hf99(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            self.ui_icon(ui, "jin-token", 24.0);
            ui.label(egui::RichText::new("JIN Token").size(18.0).strong());
        });
        ui.small("Enjin Matrixchain multitoken item 4423-1. QUB Core shows this as external token telemetry only until the bridge is live.");
        ui.horizontal(|ui| {
            ui.small(format!("True max supply: {}", self.enjin_metrics.true_max_jin_supply));
            ui.separator();
            ui.small(format!("Per-token infusion: {}", self.enjin_metrics.per_jin_infusion));
        });
        ui.horizontal_wrapped(|ui| {
            let subscan = self.ui_icon_only_button_enabled(ui, true, "subscan-logo", "Open JIN Token on Matrix Subscan");
            if subscan.clicked() { let _ = webbrowser::open(JIN_TOKEN_SUBSCAN_URL); }
            let nftio = self.ui_icon_only_button_enabled(ui, true, "nft-io-logo", "Open JIN Token on NFT.io");
            if nftio.clicked() { let _ = webbrowser::open(JIN_TOKEN_NFT_IO_URL); }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            self.ui_icon_button_enabled(ui, false, "send", "Send");
            self.ui_icon_button_enabled(ui, false, "buy", "Buy");
            self.ui_icon_button_enabled(ui, false, "sell", "Sell");
            self.ui_icon_button_enabled(ui, false, "list-asset", "List");
            self.ui_icon_button_enabled(ui, false, "offer", "Offer");
            self.ui_icon_button_enabled(ui, false, "melt", "Melt");
            self.ui_icon_button_enabled(ui, false, "convert", "Convert disabled until bridge is live");
        });
    }


    fn ui_stablecoins_balance_card_hf108(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                self.ui_icon(ui, "stablecoins", 24.0);
                ui.label(egui::RichText::new("Stablecoins").size(20.0).strong());
                ui.add_space(8.0);
                egui::ComboBox::from_id_salt("stablecoin_family_hf111")
                    .selected_text(self.prefs.stablecoin_family.label())
                    .show_ui(ui, |ui| {
                        ui.horizontal(|ui| { self.ui_icon(ui, "usd", 16.0); if ui.selectable_label(self.prefs.stablecoin_family == StablecoinFamily::Usd, "USD").clicked() { self.prefs.stablecoin_family = StablecoinFamily::Usd; self.prefs_dirty = true; ui.close(); } });
                        ui.horizontal(|ui| { self.ui_icon(ui, "eur", 16.0); if ui.selectable_label(self.prefs.stablecoin_family == StablecoinFamily::Eur, "EUR").clicked() { self.prefs.stablecoin_family = StablecoinFamily::Eur; self.prefs_dirty = true; ui.close(); } });
                        ui.horizontal(|ui| { self.ui_icon(ui, "gold", 16.0); if ui.selectable_label(self.prefs.stablecoin_family == StablecoinFamily::Gold, "Gold").clicked() { self.prefs.stablecoin_family = StablecoinFamily::Gold; self.prefs_dirty = true; ui.close(); } });
                    });
            });
            ui.separator();
            match self.prefs.stablecoin_family {
                StablecoinFamily::Usd => self.ui_usdj_family_card_hf108(ui),
                StablecoinFamily::Eur => self.ui_eurj_family_card_hf108(ui),
                StablecoinFamily::Gold => self.ui_xauj_family_card_hf111(ui),
            }
        });
    }

    fn ui_usdj_family_card_hf108(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if self.ui_icon_selectable_button(ui, self.prefs.usdj_balance_tab == UsdjBalanceSubTab::Usdj, "usdj", "USDJ").clicked() { self.prefs.usdj_balance_tab = UsdjBalanceSubTab::Usdj; self.prefs_dirty = true; }
            if self.ui_icon_selectable_button(ui, self.prefs.usdj_balance_tab == UsdjBalanceSubTab::Usdt, "usdt", "USDT").clicked() { self.prefs.usdj_balance_tab = UsdjBalanceSubTab::Usdt; self.prefs_dirty = true; }
            if self.ui_icon_selectable_button(ui, self.prefs.usdj_balance_tab == UsdjBalanceSubTab::Usdc, "usdc", "USDC").clicked() { self.prefs.usdj_balance_tab = UsdjBalanceSubTab::Usdc; self.prefs_dirty = true; }
        });
        ui.separator();
        match self.prefs.usdj_balance_tab {
            UsdjBalanceSubTab::Usdj => self.ui_fiatj_main_card_body_hf108(ui, StablecoinFamily::Usd),
            UsdjBalanceSubTab::Usdt => self.ui_stable_asset_card_body_hf108(ui, EthereumAsset::Usdt, "Tether USD", "USDJ pooled reserve input / redemption bucket."),
            UsdjBalanceSubTab::Usdc => self.ui_stable_asset_card_body_hf108(ui, EthereumAsset::Usdc, "USD Coin", "USDJ pooled reserve input / redemption bucket."),
        }
    }

    fn ui_eurj_family_card_hf108(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if self.ui_icon_selectable_button(ui, self.prefs.eurj_balance_tab == EurjBalanceSubTab::Eurj, "eurj", "EURJ").clicked() { self.prefs.eurj_balance_tab = EurjBalanceSubTab::Eurj; self.prefs_dirty = true; }
            if self.ui_icon_selectable_button(ui, self.prefs.eurj_balance_tab == EurjBalanceSubTab::Eurc, "eurc", "EURC").clicked() { self.prefs.eurj_balance_tab = EurjBalanceSubTab::Eurc; self.prefs_dirty = true; }
            if self.ui_icon_selectable_button(ui, self.prefs.eurj_balance_tab == EurjBalanceSubTab::Eurs, "eurs", "EURS").clicked() { self.prefs.eurj_balance_tab = EurjBalanceSubTab::Eurs; self.prefs_dirty = true; }
        });
        ui.separator();
        match self.prefs.eurj_balance_tab {
            EurjBalanceSubTab::Eurj => self.ui_fiatj_main_card_body_hf108(ui, StablecoinFamily::Eur),
            EurjBalanceSubTab::Eurc => self.ui_stable_asset_card_body_hf108(ui, EthereumAsset::Eurc, "EUR Coin", "EURJ pooled reserve input / redemption bucket."),
            EurjBalanceSubTab::Eurs => self.ui_stable_asset_card_body_hf108(ui, EthereumAsset::Eurs, "STASIS EURS", "EURJ pooled reserve input / redemption bucket."),
        }
    }

    fn ui_xauj_family_card_hf111(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if self.ui_icon_selectable_button(ui, self.prefs.xauj_balance_tab == XaujBalanceSubTab::Xauj, "xauj", "XAUJ").clicked() { self.prefs.xauj_balance_tab = XaujBalanceSubTab::Xauj; self.prefs_dirty = true; }
            if self.ui_icon_selectable_button(ui, self.prefs.xauj_balance_tab == XaujBalanceSubTab::Paxg, "paxg", "PAXG").clicked() { self.prefs.xauj_balance_tab = XaujBalanceSubTab::Paxg; self.prefs_dirty = true; }
            if self.ui_icon_selectable_button(ui, self.prefs.xauj_balance_tab == XaujBalanceSubTab::Xaut, "xaut", "XAUt").clicked() { self.prefs.xauj_balance_tab = XaujBalanceSubTab::Xaut; self.prefs_dirty = true; }
        });
        ui.separator();
        match self.prefs.xauj_balance_tab {
            XaujBalanceSubTab::Xauj => self.ui_fiatj_main_card_body_hf108(ui, StablecoinFamily::Gold),
            XaujBalanceSubTab::Paxg => self.ui_stable_asset_card_body_hf108(ui, EthereumAsset::Paxg, "PAX Gold", "XAUJ pooled reserve input / redemption bucket."),
            XaujBalanceSubTab::Xaut => self.ui_stable_asset_card_body_hf108(ui, EthereumAsset::Xaut, "Tether Gold", "XAUJ pooled reserve input / redemption bucket."),
        }
    }

    fn fiatj_display_balance_hf108(&self, family: StablecoinFamily) -> String {
        match family { StablecoinFamily::Usd => self.eth_balances.usdj_eth.clone(), StablecoinFamily::Eur => self.eth_balances.eurj_eth.clone(), StablecoinFamily::Gold => self.eth_balances.xauj_eth.clone() }
    }

    fn fiatj_reserves_hf108(&self, family: StablecoinFamily) -> (String, String, &'static str, &'static str, &'static str, &'static str) {
        match family {
            StablecoinFamily::Usd => (self.eth_balances.usdt_reserve.clone(), self.eth_balances.usdc_reserve.clone(), "usdt", "USDT", "usdc", "USDC"),
            StablecoinFamily::Eur => (self.eth_balances.eurc_reserve.clone(), self.eth_balances.eurs_reserve.clone(), "eurc", "EURC", "eurs", "EURS"),
            StablecoinFamily::Gold => (self.eth_balances.paxg_reserve.clone(), self.eth_balances.xaut_reserve.clone(), "paxg", "PAXG", "xaut", "XAUt"),
        }
    }

    fn ui_fiatj_main_card_body_hf108(&mut self, ui: &mut egui::Ui, family: StablecoinFamily) {
        let contracts_ready = self.fiatj_contracts_ready_hf108(family);
        let token = family.token_symbol();
        let (r0, r1, icon0, sym0, icon1, sym1) = self.fiatj_reserves_hf108(family);
        ui.horizontal(|ui| {
            self.ui_icon(ui, family.token_icon(), 24.0);
            ui.label(egui::RichText::new(format!("Ethereum {token}")).strong());
            ui.label(egui::RichText::new(format!("{} {token}", self.fiatj_display_balance_hf108(family))).size(18.0).strong());
        });
        ui.small(match family {
            StablecoinFamily::Usd => "Jinex USD pooled reserve: one public USDJ, backed by USDT + USDC buckets.",
            StablecoinFamily::Eur => EURJ_ETH_BACKING_NOTE,
            StablecoinFamily::Gold => XAUJ_ETH_BACKING_NOTE,
        });
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                self.ui_icon(ui, "eth", 18.0);
                ui.label(egui::RichText::new("Backed by:").strong());
            });
            egui::Grid::new(format!("{}_pooled_backing_grid_hf111", token))
                .num_columns(3)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(r0).strong()); self.ui_icon(ui, icon0, 18.0); ui.label(sym0); ui.end_row();
                    ui.label(egui::RichText::new(r1).strong()); self.ui_icon(ui, icon1, 18.0); ui.label(sym1); ui.end_row();
                });
            ui.small("One fungible token backed by pooled reserves. Melt chooses the output bucket if liquidity exists.");
            ui.small(match family { StablecoinFamily::Usd => &self.eth_balances.reserve_status, StablecoinFamily::Eur => &self.eth_balances.eur_reserve_status, StablecoinFamily::Gold => &self.eth_balances.gold_reserve_status });
            if !contracts_ready { ui.colored_label(egui::Color32::from_rgb(255, 175, 75), format!("Official {token} token + ReserveVault addresses are not configured yet.")); }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if self.ui_icon_button_enabled(ui, contracts_ready && !self.eth_wallets.wallets.is_empty(), "send", &format!("Send Ethereum {token}")).clicked() {
                self.open_ethereum_send_dialog(match family { StablecoinFamily::Usd => EthereumAsset::Usdj, StablecoinFamily::Eur => EthereumAsset::Eurj, StablecoinFamily::Gold => EthereumAsset::Xauj });
            }
            self.ui_icon_button_enabled(ui, false, "send", &format!("Send QUB-chain {token} disabled until bridge is live"));
            if family == StablecoinFamily::Usd {
                ui.add_space(4.0);
                egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        self.ui_icon(ui, "bridge", 18.0);
                        ui.label(egui::RichText::new("USDJ bridge preview").strong());
                    });
                    ui.small(format!("Toll: {} bps (1%). Protocol address: {}", USDJ_BRIDGE_TOLL_BPS, QUB_USDJ_BRIDGE_PROTOCOL_ADDRESS));
                    ui.small("ETH -> QUB: lock 100 USDJ on Ethereum, claim 99 QUB-chain USDJ and route 1 USDJ toll to the protocol address.");
                    ui.small("QUB -> ETH: burn/pay 101 QUB-chain USDJ, route 1 USDJ toll to the protocol address, and release 100 Ethereum USDJ after proof verification.");
                    let gw = ETHEREUM_USDJ_BRIDGE_ADDRESS_DEFAULT;
                    ui.small(format!("Ethereum bridge gateway: {}", if gw.trim().is_empty() { "not deployed yet" } else { gw }));
                    ui.horizontal_wrapped(|ui| {
                        self.ui_icon_button_enabled(ui, false, "bridge", "Bridge ETH -> QUB disabled until gateway + QUB proof path are live");
                        self.ui_icon_button_enabled(ui, false, "bridge", "Bridge QUB -> ETH disabled until QUB burn proofs are live");
                    });
                });
            }
            let (a0, a1) = match family { StablecoinFamily::Usd => (EthereumAsset::Usdt, EthereumAsset::Usdc), StablecoinFamily::Eur => (EthereumAsset::Eurc, EthereumAsset::Eurs), StablecoinFamily::Gold => (EthereumAsset::Paxg, EthereumAsset::Xaut) };
            if self.ui_icon_button_enabled(ui, contracts_ready && !self.eth_wallets.wallets.is_empty(), "melt", &format!("Melt for {sym0}")).clicked() { self.open_usdj_vault_dialog_hf107(UsdjVaultMode::Melt, a0); }
            if self.ui_icon_button_enabled(ui, contracts_ready && !self.eth_wallets.wallets.is_empty(), "melt", &format!("Melt for {sym1}")).clicked() { self.open_usdj_vault_dialog_hf107(UsdjVaultMode::Melt, a1); }
        });
    }

    fn stable_asset_balance_hf108(&self, asset: EthereumAsset) -> String {
        match asset { EthereumAsset::Usdt => self.eth_balances.usdt.clone(), EthereumAsset::Usdc => self.eth_balances.usdc.clone(), EthereumAsset::Eurc => self.eth_balances.eurc.clone(), EthereumAsset::Eurs => self.eth_balances.eurs.clone(), EthereumAsset::Paxg => self.eth_balances.paxg.clone(), EthereumAsset::Xaut => self.eth_balances.xaut.clone(), EthereumAsset::Usdj => self.eth_balances.usdj_eth.clone(), EthereumAsset::Eurj => self.eth_balances.eurj_eth.clone(), EthereumAsset::Xauj => self.eth_balances.xauj_eth.clone(), EthereumAsset::Eth => self.eth_balances.eth.clone() }
    }

    fn ui_stable_asset_card_body_hf108(&mut self, ui: &mut egui::Ui, asset: EthereumAsset, name: &str, detail: &str) {
        let family = asset.family().unwrap_or(StablecoinFamily::Usd);
        let token = family.token_symbol();
        ui.horizontal(|ui| { self.ui_icon(ui, asset.icon(), 24.0); ui.label(egui::RichText::new(format!("{} on Ethereum", asset.symbol())).size(18.0).strong()); });
        ui.horizontal(|ui| { ui.label(egui::RichText::new("Spendable").strong()); ui.label(egui::RichText::new(format!("{} {}", self.stable_asset_balance_hf108(asset), asset.symbol())).size(18.0).strong()); });
        ui.small(format!("{name}. {detail}"));
        ui.small(format!("Mint sends a stablecoin approval and a vault mint transaction for {token}. QUB bridge remains disabled until the bridge is live."));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if self.ui_icon_button_enabled(ui, !self.eth_wallets.wallets.is_empty(), "send", &format!("Send Ethereum {}", asset.symbol())).clicked() { self.open_ethereum_send_dialog(asset); }
            if self.ui_icon_button_enabled(ui, self.fiatj_contracts_ready_hf108(family) && !self.eth_wallets.wallets.is_empty(), "infuse", &format!("Mint {token}")).clicked() { self.open_usdj_vault_dialog_hf107(UsdjVaultMode::Infuse, asset); }
        });
    }

    fn ui_stablecoins_balance_card_compact_hf108(&mut self, ui: &mut egui::Ui) {
        let card = egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8));
        card.show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                self.ui_icon(ui, "stablecoins", 18.0);
                ui.label(egui::RichText::new("Stablecoins").strong());
                if self.ui_icon_selectable_button(ui, self.prefs.stablecoin_family == StablecoinFamily::Usd, "usd", "USD").clicked() { self.prefs.stablecoin_family = StablecoinFamily::Usd; self.prefs_dirty = true; }
                if self.ui_icon_selectable_button(ui, self.prefs.stablecoin_family == StablecoinFamily::Eur, "eur", "EUR").clicked() { self.prefs.stablecoin_family = StablecoinFamily::Eur; self.prefs_dirty = true; }
                if self.ui_icon_selectable_button(ui, self.prefs.stablecoin_family == StablecoinFamily::Gold, "gold", "Gold").clicked() { self.prefs.stablecoin_family = StablecoinFamily::Gold; self.prefs_dirty = true; }
            });
            ui.add_space(3.0);
            match self.prefs.stablecoin_family { StablecoinFamily::Usd => self.ui_usdj_family_card_hf108(ui), StablecoinFamily::Eur => self.ui_eurj_family_card_hf108(ui), StablecoinFamily::Gold => self.ui_xauj_family_card_hf111(ui) }
        });
    }

    fn ui_address_balances_compact(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            self.ui_icon(ui, "address-balances", 18.0);
            ui.label(egui::RichText::new("Balances").strong());
        });
        if self.initial_loading || self.wallet_sync_in_flight || self.snapshot_in_flight || HF105_CATCHUP_IN_FLIGHT.load(Ordering::SeqCst) {
            self.ui_sync_progress_bar_rows(ui, "Loading crypto balances", 3);
        }
        ui.separator();

        let card = egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8));
        card.show(ui, |ui| {
            self.ui_icon(ui, "qub", 22.0);
            ui.label(egui::RichText::new("QUB").strong());
            ui.label(egui::RichText::new(format!("{}", self.snapshot.spendable)).strong());
            ui.small(format!("imm {}", self.snapshot.immature));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if self.ui_icon_only_button_enabled(ui, self.snapshot.wallet_keys > 0, "send", "Send QUB").clicked() { self.open_send_dialog_for("QUB"); }
                self.ui_icon_only_button_enabled(ui, false, "buy", "Buy QUB");
                self.ui_icon_only_button_enabled(ui, false, "sell", "Sell QUB");
                self.ui_icon_only_button_enabled(ui, false, "list-asset", "List QUB");
                self.ui_icon_only_button_enabled(ui, false, "offer", "Offer QUB");
                self.ui_icon_only_button_enabled(ui, false, "melt", "Melt QUB");
            });
        });

        ui.add_space(8.0);
        let card = egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8));
        card.show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if self.ui_icon_selectable_button(ui, self.prefs.jin_balance_tab == JinBalanceSubTab::Coin, "jin", "Coin").clicked() {
                    self.prefs.jin_balance_tab = JinBalanceSubTab::Coin;
                    self.prefs_dirty = true;
                }
                if self.ui_icon_selectable_button(ui, self.prefs.jin_balance_tab == JinBalanceSubTab::Token, "jin-token", "Token").clicked() {
                    self.prefs.jin_balance_tab = JinBalanceSubTab::Token;
                    self.prefs_dirty = true;
                }
            });
            ui.add_space(3.0);
            match self.prefs.jin_balance_tab {
                JinBalanceSubTab::Coin => {
                    self.ui_icon(ui, "jin", 22.0);
                    ui.label(egui::RichText::new("JIN Coin").strong());
                    ui.label(egui::RichText::new(&self.snapshot.jin_total).strong());
                    ui.small("confirmed 2+");
                    ui.small(format!("Infusion: {}", confirmed_jin_total_infusion_display(&self.snapshot.jin_total, &self.enjin_metrics.per_jin_infusion)));
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        if self.ui_icon_only_button_enabled(ui, self.snapshot.wallet_keys > 0, "send", "Send JIN Coin").clicked() { self.open_send_dialog_for("JIN"); }
                        if self.ui_icon_only_button_enabled(ui, self.snapshot.wallet_keys > 0, "buy", "Buy JIN public sale").clicked() { self.open_buy_jin_dialog(ui.ctx()); }
                        self.ui_icon_only_button_enabled(ui, false, "sell", "Sell JIN Coin");
                        self.ui_icon_only_button_enabled(ui, false, "list-asset", "List JIN Coin");
                        self.ui_icon_only_button_enabled(ui, false, "offer", "Offer JIN Coin");
                        self.ui_icon_only_button_enabled(ui, false, "infuse", "Infuse JIN Coin");
                        self.ui_icon_only_button_enabled(ui, false, "convert", "Convert disabled until bridge is live");
                    });
                }
                JinBalanceSubTab::Token => {
                    self.ui_icon(ui, "jin-token", 22.0);
                    ui.label(egui::RichText::new("JIN Token").strong());
                    ui.small("Enjin Matrixchain 4423-1");
                    ui.small(format!("Infusion: {}", self.enjin_metrics.per_jin_infusion));
                    ui.horizontal_wrapped(|ui| {
                        if self.ui_icon_only_button_enabled(ui, true, "subscan-logo", "Open Matrix Subscan").clicked() { let _ = webbrowser::open(JIN_TOKEN_SUBSCAN_URL); }
                        if self.ui_icon_only_button_enabled(ui, true, "nft-io-logo", "Open NFT.io").clicked() { let _ = webbrowser::open(JIN_TOKEN_NFT_IO_URL); }
                    });
                    ui.horizontal_wrapped(|ui| {
                        self.ui_icon_only_button_enabled(ui, false, "send", "Send JIN Token");
                        self.ui_icon_only_button_enabled(ui, false, "buy", "Buy JIN Token");
                        self.ui_icon_only_button_enabled(ui, false, "sell", "Sell JIN Token");
                        self.ui_icon_only_button_enabled(ui, false, "list-asset", "List JIN Token");
                        self.ui_icon_only_button_enabled(ui, false, "offer", "Offer JIN Token");
                        self.ui_icon_only_button_enabled(ui, false, "melt", "Melt JIN Token");
                        self.ui_icon_only_button_enabled(ui, false, "convert", "Convert disabled until bridge is live");
                    });
                }
            }
        });
        ui.add_space(8.0);
        self.ui_stablecoins_balance_card_compact_hf108(ui);
    }

    fn activity_filter_summary(&self) -> String {
        let type_s = self.tr_activity_type(self.prefs.activity_type_filter);
        let status_s = self.tr_activity_status(self.prefs.activity_status_filter);
        let dir_s = self.tr_activity_direction(self.prefs.activity_direction_filter);
        let asset_s = self.tr_activity_asset(self.prefs.activity_asset_filter);
        if type_s == "All" && status_s == "All" && dir_s == "All" && asset_s == "All" {
            "All".to_string()
        } else {
            format!("{} / {} / {} / {}", type_s, status_s, dir_s, asset_s)
        }
    }

    fn ui_address_activity_compact(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            self.ui_icon(ui, "wallet-address", 18.0);
            ui.label(egui::RichText::new("Activity").strong());
            ui.small(self.activity_filter_summary());
        });
        if self.initial_loading || self.wallet_sync_in_flight || self.snapshot_in_flight {
            self.ui_sync_progress_bar_rows(ui, "Loading activity", 3);
        }
        ui.separator();
        let filtered = self.snapshot.activity.iter()
            .filter(|e| e.matches_type(self.prefs.activity_type_filter) && e.matches_status(self.prefs.activity_status_filter) && e.matches_direction(self.prefs.activity_direction_filter) && e.matches_asset(self.prefs.activity_asset_filter))
            .take(12)
            .cloned()
            .collect::<Vec<_>>();
        if filtered.is_empty() {
            ui.small("-");
            return;
        }
        for entry in filtered {
            let icon = match entry.activity_type.as_str() { "Mining" => "next-reward", "QNS Registration" => "qns", "Library" => "list", "Conversion" => "convert", "Infusion" => "infuse", "Melt" => "melt", _ => "send" };
            let status_color = match entry.status.as_str() {
                "Mempool" | "Immature" | "Pending Decision" => egui::Color32::from_rgb(236, 190, 78),
                "Confirmed" | "Matured" => egui::Color32::from_rgb(72, 210, 120),
                _ => ui.visuals().text_color(),
            };
            let direction_color = match entry.direction.as_str() {
                "Incoming" => egui::Color32::from_rgb(72, 210, 120),
                "Outgoing" => egui::Color32::from_rgb(255, 105, 96),
                _ => ui.visuals().weak_text_color(),
            };
            let height = entry.height.map(|h| format!("#{}", h)).unwrap_or_else(|| "mem".to_string());
            ui.horizontal(|ui| {
                self.ui_icon(ui, icon, 12.0);
                let t = entry.activity_type.chars().next().unwrap_or('-');
                let s = entry.status.chars().next().unwrap_or('-');
                let d = entry.direction.chars().next().unwrap_or('-');
                ui.label(egui::RichText::new(t.to_string()).strong().size(12.0));
                ui.label(egui::RichText::new(s.to_string()).strong().size(12.0).color(status_color));
                ui.label(egui::RichText::new(d.to_string()).strong().size(12.0).color(direction_color));
                ui.label(egui::RichText::new(height).size(11.0));
            });
            ui.add_space(2.0);
        }
    }

    fn ui_address_activity(&mut self, ui: &mut egui::Ui) {
        self.ui_heading_icon(ui, "wallet-address", self.tr("Address Activity", "Address Activity"));
        ui.small("Public activity for the selected payout/default address. No private key is required.");
        if self.initial_loading || self.wallet_sync_in_flight || self.snapshot_in_flight {
            self.ui_sync_progress_bar_rows(ui, "Loading address activity", 3);
            ui.small(&self.status_line);
        }
        ui.add_space(6.0);
        let before_type = self.prefs.activity_type_filter;
        let before_status = self.prefs.activity_status_filter;
        let before_direction = self.prefs.activity_direction_filter;
        let before_asset = self.prefs.activity_asset_filter;

        let type_options = [
            (ActivityTypeFilter::All, self.tr_activity_type(ActivityTypeFilter::All)),
            (ActivityTypeFilter::Transfer, self.tr_activity_type(ActivityTypeFilter::Transfer)),
            (ActivityTypeFilter::Mining, self.tr_activity_type(ActivityTypeFilter::Mining)),
            (ActivityTypeFilter::QnsRegistration, self.tr_activity_type(ActivityTypeFilter::QnsRegistration)),
            (ActivityTypeFilter::Library, self.tr_activity_type(ActivityTypeFilter::Library)),
            (ActivityTypeFilter::Infusion, self.tr_activity_type(ActivityTypeFilter::Infusion)),
            (ActivityTypeFilter::Melt, self.tr_activity_type(ActivityTypeFilter::Melt)),
            (ActivityTypeFilter::Conversion, self.tr_activity_type(ActivityTypeFilter::Conversion)),
        ];

        let status_options = [
            (ActivityStatusFilter::All, self.tr_activity_status(ActivityStatusFilter::All)),
            (ActivityStatusFilter::Mempool, self.tr_activity_status(ActivityStatusFilter::Mempool)),
            (ActivityStatusFilter::Confirmed, self.tr_activity_status(ActivityStatusFilter::Confirmed)),
            (ActivityStatusFilter::Immature, self.tr_activity_status(ActivityStatusFilter::Immature)),
            (ActivityStatusFilter::PendingDecision, self.tr_activity_status(ActivityStatusFilter::PendingDecision)),
            (ActivityStatusFilter::Matured, self.tr_activity_status(ActivityStatusFilter::Matured)),
        ];

        let direction_options = [
            (ActivityDirectionFilter::All, self.tr_activity_direction(ActivityDirectionFilter::All)),
            (ActivityDirectionFilter::Incoming, self.tr_activity_direction(ActivityDirectionFilter::Incoming)),
            (ActivityDirectionFilter::Outgoing, self.tr_activity_direction(ActivityDirectionFilter::Outgoing)),
        ];

        let asset_options = [
            (ActivityAssetFilter::All, self.tr_activity_asset(ActivityAssetFilter::All)),
            (ActivityAssetFilter::Qub, self.tr_activity_asset(ActivityAssetFilter::Qub)),
            (ActivityAssetFilter::Jin, self.tr_activity_asset(ActivityAssetFilter::Jin)),
        ];

        ui.vertical(|ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.small(self.tr("Type", "Type"));
                    egui::ComboBox::from_id_salt("activity_type_filter")
                        .selected_text(self.tr_activity_type(self.prefs.activity_type_filter))
                        .show_ui(ui, |ui| {
                            for (f, label) in type_options {
                                ui.selectable_value(&mut self.prefs.activity_type_filter, f, label);
                            }
                        });
                });

                ui.vertical(|ui| {
                    ui.small(self.tr("Status", "Status"));
                    egui::ComboBox::from_id_salt("activity_status_filter")
                        .selected_text(self.tr_activity_status(self.prefs.activity_status_filter))
                        .show_ui(ui, |ui| {
                            for (f, label) in status_options {
                                ui.selectable_value(&mut self.prefs.activity_status_filter, f, label);
                            }
                        });
                });

                ui.vertical(|ui| {
                    ui.small(self.tr("Direction", "Direction"));
                    egui::ComboBox::from_id_salt("activity_direction_filter")
                        .selected_text(self.tr_activity_direction(self.prefs.activity_direction_filter))
                        .show_ui(ui, |ui| {
                            for (f, label) in direction_options {
                                ui.selectable_value(&mut self.prefs.activity_direction_filter, f, label);
                            }
                        });
                });
            });

            ui.add_space(4.0);

            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.small(self.tr("Asset", "Asset"));
                    egui::ComboBox::from_id_salt("activity_asset_filter")
                        .selected_text(self.tr_activity_asset(self.prefs.activity_asset_filter))
                        .show_ui(ui, |ui| {
                            for (f, label) in asset_options {
                                ui.selectable_value(&mut self.prefs.activity_asset_filter, f, label);
                            }
                        });
                });
            });
        });

        if before_type != self.prefs.activity_type_filter
            || before_status != self.prefs.activity_status_filter
            || before_direction != self.prefs.activity_direction_filter
            || before_asset != self.prefs.activity_asset_filter
        {
            self.prefs.activity_page = 0;
            self.prefs_dirty = true;
        }

        ui.add_space(6.0);

        let filtered = self.snapshot.activity.iter()
            .filter(|e| e.matches_type(self.prefs.activity_type_filter) && e.matches_status(self.prefs.activity_status_filter) && e.matches_direction(self.prefs.activity_direction_filter) && e.matches_asset(self.prefs.activity_asset_filter))
            .cloned()
            .collect::<Vec<_>>();
        let per_page = 8usize;
        let pages = filtered.len().saturating_add(per_page - 1) / per_page;
        if pages == 0 { self.prefs.activity_page = 0; }
        else if self.prefs.activity_page >= pages { self.prefs.activity_page = pages - 1; }
        let start = self.prefs.activity_page.saturating_mul(per_page);
        let end = (start + per_page).min(filtered.len());

        ui.horizontal(|ui| {
            if ui.add_enabled(self.prefs.activity_page > 0, egui::Button::new(self.tr("Previous", "Previous"))).clicked() {
                self.prefs.activity_page = self.prefs.activity_page.saturating_sub(1);
                self.prefs_dirty = true;
            }
            ui.label(format!("{}/{}", if pages == 0 { 0 } else { self.prefs.activity_page + 1 }, pages.max(1)));
            if ui.add_enabled(self.prefs.activity_page + 1 < pages, egui::Button::new(self.tr("Next", "Next"))).clicked() {
                self.prefs.activity_page += 1;
                self.prefs_dirty = true;
            }

        });
        ui.separator();

        if filtered.is_empty() {
            ui.small(self.tr("No address activity for the current filters yet.", "No address activity for the current filters yet."));
            return;
        }

        for entry in &filtered[start..end] {
            egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let icon = match entry.activity_type.as_str() { "Mining" => "next-reward", "QNS Registration" => "qns", "Library" => "list", "Conversion" => "convert", "Infusion" => "infuse", "Melt" => "melt", _ => "send" };
                    self.ui_icon(ui, icon, 16.0);
                    let status_color = match entry.status.as_str() {
                        "Mempool" | "Immature" | "Pending Decision" => egui::Color32::from_rgb(236, 190, 78),
                        "Confirmed" | "Matured" => egui::Color32::from_rgb(72, 210, 120),
                        _ => ui.visuals().text_color(),
                    };
                    let direction_color = match entry.direction.as_str() {
                        "Incoming" => egui::Color32::from_rgb(72, 210, 120),
                        "Outgoing" => egui::Color32::from_rgb(255, 105, 96),
                        _ => ui.visuals().weak_text_color(),
                    };
                    ui.label(egui::RichText::new(&entry.activity_type).strong());
                    ui.label(egui::RichText::new("-").weak());
                    ui.label(egui::RichText::new(&entry.status).strong().color(status_color));
                    ui.label(egui::RichText::new("-").weak());
                    ui.label(egui::RichText::new(&entry.direction).strong().color(direction_color));
                });
                ui.horizontal_wrapped(|ui| {
                    self.ui_icon(ui, "qub", 14.0);
                    ui.label(egui::RichText::new(&entry.amount).strong());
                    if entry.fee != "0 QUB" { ui.separator(); ui.label(format!("fee {}", entry.fee)); }
                });
                ui.monospace(shorten_hash(&entry.txid));
                let height = entry.height.map(|h| format!("#{}", h)).unwrap_or_else(|| "mempool".to_string());
                let counterparty_display = if entry.counterparty.trim().is_empty() {
                    "-".to_string()
                } else {
                    shorten_hash(&entry.counterparty)
                };
                
                ui.small(format!("Block: {} | Confirmations: {}", height, entry.confirmations));
                ui.horizontal_wrapped(|ui| {
                    ui.small("Counterparty:");
                    ui.monospace(counterparty_display);
                });
                if !entry.qns_name.is_empty() { ui.small(format!("QNS: {}", entry.qns_name)); }
                ui.small(&entry.details);
            });
            ui.add_space(6.0);
        }
    }

    fn ui_dashboard(&mut self, ui: &mut egui::Ui) {
        let relay_label = if self.snapshot.relay_capable { "relay capable" } else if self.snapshot.nat_private { "NAT/private" } else { "outbound" };
        let p2p_value = if self.snapshot.p2p_enabled {
            format!("{} global live / {} direct / {} known / {}", self.snapshot.global_live_peers, self.snapshot.direct_reachable_peers, self.snapshot.known_peers, relay_label)
        } else { "disabled".to_string() };

        egui::CollapsingHeader::new(format!("Live chain data   |   {}   |   #{}", self.snapshot.network, self.snapshot.height))
            .id_salt("central_live_chain_data")
            .default_open(false)
            .show(ui, |ui| {
                self.ui_heading_icon(ui, "live-chain-data", self.tr("Live chain data", "Live chain data"));
                ui.add_space(8.0);
                egui::Grid::new("chain_grid").num_columns(2).spacing([28.0, 8.0]).show(ui, |ui| {
                    self.metric_info(ui, "network", "Network", &self.snapshot.network, "Current installed network channel. Mainnet and Testnet are separate apps.");
                    self.metric_info(ui, "height", "Height", self.snapshot.height.to_string(), "Current local best block height after sync.");
                    self.metric_info(ui, "best-block", "Best block", shorten_hash(&self.snapshot.best_hash), "Hash of the current local best block.");
                    self.metric_info(ui, "mempool-tx", "Mempool tx", self.snapshot.mempool_txs.to_string(), "Transactions known locally and waiting to be mined.");
                    self.metric_info(ui, "coinbase-maturity", "Coinbase maturity", format!("{} blocks", self.snapshot.coinbase_maturity), "Mining rewards become spendable after this many confirmations.");
                    self.metric_info(ui, "block-target", "Block target", format!("{} seconds", self.snapshot.target_spacing_secs), "The difficulty adjustment aims for this average block time.");
                    self.metric_info(ui, "height", "Known network tip", if self.snapshot.known_network_height == 0 { "discovering".to_string() } else { format!("#{} {}", self.snapshot.known_network_height, shorten_hash(&self.snapshot.known_network_hash)) }, "Latest official/direct network tip currently visible to the GUI. Used only for transparency and sync progress, not as consensus by itself.");
                    self.metric_info(ui, "block-target", "Recent avg block time", if self.snapshot.recent_avg_10_secs == 0 { "collecting".to_string() } else { format!("{}s / {}s", self.snapshot.recent_avg_10_secs, self.snapshot.recent_avg_20_secs) }, "Average spacing over recent 10 / 20 local blocks. This helps decide whether a future DAA consensus review is actually needed.");
                    self.metric_info(ui, "i", "DAA observation", &self.snapshot.daa_observation, "Telemetry only. QUB Core does not change difficulty adjustment rules.");
                    self.metric_info(ui, "next-reward", "Next reward", format!("{} QUB", self.snapshot.block_reward), "Base QUB block subsidy for the next block, before transaction fees.");
                    self.metric_info(ui, "halving-interval", "Halving interval", format!("{} blocks", format_u64(self.snapshot.halving_interval)), "How often the QUB block subsidy halves.");
                    self.metric_info(ui, "pow-bits", "PoW bits", &self.snapshot.pow_bits, "Compact proof-of-work target currently used by blocks.");
                    self.metric_info(ui, "p2p-peers", "P2P peers", p2p_value.clone(), "Direct peers are active TCP connections. Relay capable means this node can advertise a public address; NAT/private nodes still help with outbound block/tx relay.");
                    if !self.snapshot.stale_warning.trim().is_empty() {
                        ui.label(egui::RichText::new(&self.snapshot.stale_warning).color(egui::Color32::YELLOW));
                        ui.end_row();
                    }
                    self.metric_info(ui, "qns", "QNS", format!("{} names", self.snapshot.qns_count), "Number of QNS names registered on the active chain.");
                    let pools_metric = if self.pool_activation_ready() { format!("{} pools / active", self.snapshot.pools_count) } else { format!("{} pools / pending", self.snapshot.pools_count) };
                    self.metric_info(ui, "mining-controls", "Pools", pools_metric, "Deterministic non-custodial pooled mining with direct PPLNS coinbase outputs.");
                    let verified_metric = if self.snapshot.height + 1 >= self.snapshot.verified_governance_activation_height { format!("{} wallets / {} pools / {} mods", self.snapshot.verified_wallets_count, self.snapshot.verified_pools_count, self.snapshot.active_moderators_count) } else { format!("activates at #{}", self.snapshot.verified_governance_activation_height) };
                    self.metric_info(ui, "verified", "Verified Governance", verified_metric, "Verified Governance v1: JIN-locked verified wallets/pools, report cases, elected moderators and bounded slashing primitives. No admin custody or manual override.");
                    self.metric_info(ui, "verified", "Report cases", self.snapshot.report_cases_count.to_string(), "On-chain report cases. Reports are signals, not automatic slashes; moderator/governance review is required.");
                    self.metric_info(ui, "qub", "Initial max QUB supply", "21,000,000", "The original QUB hard cap before future QUB melt mechanics.");
                    self.metric_info(ui, "next-reward", "Mined QUB supply", &self.snapshot.mined_qub_supply, "Cumulative QUB block subsidy minted up to the current local chain height. Transaction fees are not newly minted supply.");
                    self.metric_info(ui, "melt", "Melted QUB supply", "0 QUB", "QUB permanently melted/redeemed so far. This is 0 until the QUB melt feature is activated.");
                    self.metric_info(ui, "qub", "True max QUB supply", "21,000,000 QUB", "Initial max QUB supply minus melted QUB. This will decrease if QUB melt is activated.");
                    self.metric_info(ui, "infuse", "Total infused JIN into all QUB", "0 JIN", "Total JIN currently infused into QUB. QUB infusion accounting is not activated yet.");
                    self.metric_info(ui, "infuse", "Per QUB infusion", "0 JIN/QUB", "Average JIN backing currently infused into each QUB. This is 0 until the QUB infusion system is activated.");
                    self.metric_info(ui, "jin", "Initial max JIN supply", "105,000,000 JIN", "Native JIN Coin fixed supply on QUB. JIN Token on Enjin Matrixchain is fetched separately for tokenomics telemetry.");
                    self.metric_info(ui, "melt", "Melted JIN supply", &self.enjin_metrics.melted_jin_supply, "JIN Token supply melted/redeemed on Enjin Matrixchain. Fetched directly from Matrixchain RPC when available; this is telemetry, not QUB consensus.");
                    self.metric_info(ui, "jin", "True max JIN supply", &self.enjin_metrics.true_max_jin_supply, "Current JIN Token max supply after melts on Enjin Matrixchain. Direct Matrixchain telemetry; QUB consensus still treats native JIN Coin separately.");
                    self.metric_info(ui, "jin-token", "Total infused ENJ into all JIN", &self.enjin_metrics.total_infused_enj, "Total ENJ infused into JIN Token on Enjin Matrixchain. Direct Matrixchain telemetry when available.");
                    self.metric_info(ui, "jin-token", "Per JIN infusion", &self.enjin_metrics.per_jin_infusion, "Average ENJ infusion per JIN Token on Enjin Matrixchain. Direct Matrixchain telemetry when available.");
                });
            });
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            let arrow = if self.prefs.central_miner_expanded { "v" } else { ">" };
            if ui.small_button(arrow).on_hover_text("Expand/collapse Miner telemetry").clicked() {
                self.prefs.central_miner_expanded = !self.prefs.central_miner_expanded;
                self.prefs_dirty = true;
            }
            let header_response = ui.horizontal(|ui| {
                self.ui_miner_telemetry_header_hf99(ui);
            }).response;
            if header_response.clicked() {
                self.prefs.central_miner_expanded = !self.prefs.central_miner_expanded;
                self.prefs_dirty = true;
            }
        });
        if self.prefs.central_miner_expanded {
            ui.add_space(6.0);
            egui::Grid::new("miner_grid_hf99").num_columns(2).spacing([28.0, 8.0]).show(ui, |ui| {
                self.metric_info(ui, "hashrate", "CPU hashrate", format_hps(self.hash_rate_hps), "Current CPU hashing speed measured by this GUI miner.");
                self.metric_info(ui, "gpu", "GPU hashrate", if self.gpu_workers == 0 { "off".to_string() } else { format_hps(self.gpu_hash_rate_hps) }, "Current OpenCL GPU hashing contribution. It can take a short warm-up before showing non-zero values.");
                self.metric_info(ui, "total-hashes", "CPU total hashes", format_u64(self.total_hashes), "Total CPU hashes attempted since this mining session started.");
                self.metric_info(ui, "gpu", "GPU total hashes", if self.gpu_workers == 0 { "-".to_string() } else { format_u64(self.gpu_total_hashes) }, "Total GPU work items completed since this mining session started.");
                self.metric_info(ui, "cpu-workers", "CPU workers", self.miner_threads.to_string(), "CPU worker threads used by the current resource plan.");
                self.metric_info(ui, "gpu", "GPU workers", if self.gpu_workers == 0 { "off".to_string() } else { self.gpu_workers.to_string() }, "OpenCL GPU work-item batch size used by the miner.");
                self.metric_info(ui, "gpu", "GPU device", if self.gpu_device.is_empty() { "-".to_string() } else { self.gpu_device.clone() }, "OpenCL GPU selected by QUB Core for mining.");
                self.metric_info(ui, "duty", "Duty", if self.miner_duty == 0 { "-".to_string() } else { format!("{}% CPU / {}% GPU", self.miner_duty, self.prefs.gpu_percent) }, "Resource duty requested by your CPU/GPU sliders. GPU 100% uses high-performance full double-SHA OpenCL with per-device auto-tuned batches. Hybrid laptops default to the high-performance GPU path, and every GPU-found block is still CPU-verified.");
                self.metric_info(ui, "target-block", "Target block", if self.target_height == 0 { "-".to_string() } else { format!("#{}", self.target_height) }, "Block height currently targeted by the miner candidate.");
            });
            ui.add_space(10.0);
            self.ui_hashrate_vertical_meters(ui);
        }
        ui.add_space(10.0);

        egui::CollapsingHeader::new(format!("Peers / block stream   |   {} direct   |   {} known", self.snapshot.direct_reachable_peers, self.snapshot.known_peers))
            .id_salt("central_peers_block_stream")
            .default_open(false)
            .show(ui, |ui| self.ui_peer_modes(ui));
        ui.add_space(10.0);

        egui::CollapsingHeader::new(format!("Recent global blocks   |   latest #{}", self.snapshot.recent_blocks.first().map(|b| b.height).unwrap_or(0)))
            .id_salt("central_recent_global_blocks")
            .default_open(true)
            .show(ui, |ui| self.ui_recent_global_blocks(ui));

        ui.add_space(14.0);
        if self.prefs.visual_enabled {
            if let Some(card) = &self.last_block_card {
                let age = card.at.elapsed().as_secs();
                let confirmed_icon = if self.icons.get("your-confirmed-mined-block", ui.visuals().dark_mode).is_some() { "your-confirmed-mined-block" } else { "your-pending-mined-block" };
                let (icon, title, desc) = if card.confirmed {
                    (
                        confirmed_icon,
                        format!("Your confirmed mined block #{}", card.height),
                        format!("Confirmed on the active chain with {} confirmation(s). Reward: {} | Transactions: {} | solved {}s ago", card.confirmations.max(1), card.reward, card.txs, age),
                    )
                } else {
                    (
                        "your-pending-mined-block",
                        format!("Your pending mined block #{}", card.height),
                        format!("Pending active-chain confirmation. Reward candidate: {} | Transactions: {} | {}s ago", card.reward, card.txs, age),
                    )
                };
                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| { self.ui_icon(ui, icon, 30.0); ui.label(egui::RichText::new(title).size(24.0).strong()); });
                        ui.label(desc);
                        ui.monospace(shorten_hash(&card.hash));
                        if card.confirmed {
                            ui.small("This card remains visible until your next mined-block candidate appears. Coinbase rewards still mature according to network rules.");
                        } else {
                            ui.small("If a competing block wins, this pending card is removed as stale. The confirmation sound plays only after active-chain confirmation.");
                        }
                    });
            }
        }

        ui.add_space(12.0);
        egui::CollapsingHeader::new("Feature status")
            .default_open(false)
            .show(ui, |ui| {
                ui.label(&self.snapshot.features);
                ui.small("Pooled mining is active on mainnet after #9999. JIN native coin is enabled after network activation.");
            });
    }

    fn ui_peer_modes(&mut self, ui: &mut egui::Ui) {
        self.ui_heading_icon(ui, "peers-block-stream", self.tr("Peers / block stream", "Peers / block stream"));
        if self.snapshot_in_flight || HF105_CATCHUP_IN_FLIGHT.load(Ordering::SeqCst) || self.snapshot.peers.is_empty() {
            self.ui_sync_progress_bar_rows(ui, "Loading peer/block-stream view", 2);
            ui.small("QUB Core shows cached/local state first, then refreshes official/direct peer telemetry when the bounded catch-up pulse completes.");
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let web_selected = self.prefs.peer_view_mode == PeerViewMode::Web;
            let list_selected = self.prefs.peer_view_mode == PeerViewMode::List;
            if self.ui_icon_selectable_button(ui, web_selected, "web-map", "Web map").clicked() { self.prefs.peer_view_mode = PeerViewMode::Web; self.prefs_dirty = true; }
            if self.ui_icon_selectable_button(ui, list_selected, "list", "List").clicked() { self.prefs.peer_view_mode = PeerViewMode::List; self.prefs_dirty = true; }
            ui.add_space(12.0);
            ui.label("Zoom");
            if ui.add(egui::Slider::new(&mut self.prefs.peer_zoom, 0.65..=1.80).show_value(false)).changed() { self.prefs_dirty = true; }
        });
        ui.add_space(8.0);
        match self.prefs.peer_view_mode {
            PeerViewMode::Web => self.ui_peer_web(ui),
            PeerViewMode::List => self.ui_peer_list(ui),
        }
    }

    fn ui_peer_web(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width().max(820.0);
        let stream_w = (280.0_f32 * self.prefs.peer_zoom).clamp(245.0_f32, 360.0_f32);
        let height = (520.0_f32 * self.prefs.peer_zoom).clamp(420.0_f32, 720.0_f32);
        let map_side = (height - 58.0).min(width - stream_w - 64.0).max(360.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let bg = if ui.visuals().dark_mode { egui::Color32::from_rgba_unmultiplied(15, 18, 25, 235) } else { egui::Color32::from_rgba_unmultiplied(236, 241, 247, 245) };
        painter.rect_filled(rect, 12, bg);

        let t = self.app_started.elapsed().as_secs_f32();
        let text = ui.visuals().strong_text_color();
        let muted = if ui.visuals().dark_mode { egui::Color32::from_gray(80) } else { egui::Color32::from_gray(165) };
        let map_rect = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(16.0, 40.0),
            egui::vec2(map_side, map_side),
        );
        draw_privacy_world_map(&painter, map_rect, ui.visuals().dark_mode);

        painter.text(
            rect.left_top() + egui::vec2(18.0, 10.0),
            egui::Align2::LEFT_TOP,
            "Global miner map - approximate / privacy-preserving",
            egui::FontId::proportional(13.0),
            text,
        );

        // Local user node. This is deliberately a coarse pseudo-region: the GUI never
        // geolocates or reveals exact IPs.
        let local_key = if self.prefs.payout_address.trim().is_empty() { "local" } else { self.prefs.payout_address.trim() };
        let local_pos = privacy_region_position(local_key, 0, map_rect, t, true);
        let local_active = self.local_mined_active();
        let local_color = if local_active { egui::Color32::from_rgb(66, 220, 120) } else { ui.visuals().selection.bg_fill };
        painter.circle_filled(local_pos, 10.0 * self.prefs.peer_zoom, local_color);
        painter.circle_stroke(local_pos, 18.0 + (t.sin() + 1.0) * 2.0, egui::Stroke::new(1.0, local_color));
        painter.text(local_pos + egui::vec2(0.0, 20.0), egui::Align2::CENTER_CENTER, "you", egui::FontId::proportional(12.0), text);
        if local_active {
            painter.text(local_pos + egui::vec2(0.0, -22.0), egui::Align2::CENTER_CENTER, " mined", egui::FontId::proportional(11.0), egui::Color32::from_rgb(66, 220, 120));
        }

        // Show pools as larger privacy-preserving clusters. We do not expose IPs or exact
        // coordinates; dots are only a visual hint of active pool size.
        for (pidx, pool) in self.snapshot.pools.iter().enumerate().take(12) {
            let key = if pool.pool_id.is_empty() { pool.name.as_str() } else { pool.pool_id.as_str() };
            let pos = privacy_region_position(key, pidx + 97, map_rect, t, false);
            let radius = (18.0 + (pool.active_miners as f32).sqrt() * 3.0).clamp(18.0, 42.0) * self.prefs.peer_zoom.clamp(0.75, 1.25);
            let color = egui::Color32::from_rgb(245, 158, 66);
            painter.circle_stroke(pos, radius, egui::Stroke::new(1.6, color));
            painter.circle_filled(pos, 4.0, color);
            let dot_count = pool.active_miners.min(18);
            for didx in 0..dot_count {
                let angle = (didx as f32 / dot_count.max(1) as f32) * std::f32::consts::TAU + t * 0.15;
                let dpos = pos + egui::vec2(angle.cos() * radius * 0.55, angle.sin() * radius * 0.55);
                painter.circle_filled(dpos, 2.4, egui::Color32::from_rgb(255, 215, 115));
            }
            let label = format!("{} {}", if pool.name.is_empty() { "Pool" } else { pool.name.as_str() }, shorten_hash(&pool.pool_id));
            painter.text(pos + egui::vec2(0.0, radius + 12.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(10.0), text);
        }

        let peers = &self.snapshot.peers;
        for (idx, peer) in peers.iter().enumerate().take(64) {
            let key = peer_region_key(peer);
            let pos = privacy_region_position(&key, idx + 1, map_rect, t, false);
            let color = peer_status_color(peer, ui);
            let radius = if peer.reachable { 7.5 } else if peer.global_live { 6.5 } else { 5.0 } * self.prefs.peer_zoom;
            if peer.reachable || peer.global_live {
                painter.line_segment([local_pos, pos], egui::Stroke::new(if peer.reachable { 1.1 } else { 0.6 }, color.linear_multiply(0.55)));
                let pulse = 1.0 + ((t * 2.0 + idx as f32).sin() + 1.0) * 0.5;
                painter.circle_stroke(pos, radius + 4.0 + pulse, egui::Stroke::new(0.7, color.linear_multiply(0.55)));
            }
            painter.circle_filled(pos, radius, color);
            if idx < 18 {
                painter.text(pos + egui::vec2(0.0, 15.0), egui::Align2::CENTER_CENTER, peer_identity_label(peer), egui::FontId::monospace(9.5), text);
            }
        }
        if peers.is_empty() {
            painter.text(map_rect.center(), egui::Align2::CENTER_CENTER, "Waiting for global peer telemetry...", egui::FontId::proportional(14.0), muted);
        }

        let legend_y = map_rect.bottom() - 20.0;
        let legend_x = map_rect.left() + 12.0;
        draw_legend_dot(&painter, egui::pos2(legend_x, legend_y), egui::Color32::from_rgb(72, 168, 255), "direct", text);
        draw_legend_dot(&painter, egui::pos2(legend_x + 86.0, legend_y), egui::Color32::from_rgb(66, 220, 120), "global live", text);
        draw_legend_dot(&painter, egui::pos2(legend_x + 210.0, legend_y), muted, "offline/stale", text);

        painter.text(
            map_rect.left_top() + egui::vec2(12.0, 12.0),
            egui::Align2::LEFT_TOP,
            format!("{} global live - {} direct - {} known", self.snapshot.global_live_peers, self.snapshot.direct_reachable_peers, self.snapshot.known_peers),
            egui::FontId::proportional(12.0),
            text,
        );

        let block_x = map_rect.right() + 24.0;
        let block_w = (rect.right() - block_x - 16.0).max(240.0);
        let block_h = (54.0_f32 * self.prefs.peer_zoom).clamp(48.0_f32, 72.0_f32);
        let blocks = &self.snapshot.recent_blocks;
        painter.text(egui::pos2(block_x, rect.top() + 10.0), egui::Align2::LEFT_TOP, "Global block stream", egui::FontId::proportional(13.0), text);
        for (idx, card) in blocks.iter().take(8).enumerate() {
            let age = block_age_secs(card) as f32;
            let alpha = (238.0 - idx as f32 * 34.0 - age * 0.7).clamp(35.0, 238.0) as u8;
            let y = rect.top() + 34.0 + idx as f32 * (block_h + 15.0) + age.min(1.0) * 2.0;
            if y > rect.bottom() - block_h { continue; }
            let block_rect = egui::Rect::from_min_size(egui::pos2(block_x, y), egui::vec2(block_w, block_h));
            let fill = if is_local_block(card, &self.prefs.payout_address) { egui::Color32::from_rgba_unmultiplied(64, 150, 96, alpha) } else { egui::Color32::from_rgba_unmultiplied(66, 102, 145, alpha) };
            painter.rect_filled(block_rect, 8, fill);
            painter.text(block_rect.left_top() + egui::vec2(10.0, 10.0), egui::Align2::LEFT_TOP, format!("#{}  {}", card.height, card.reward), egui::FontId::proportional(13.0), egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha));
            painter.text(block_rect.left_bottom() + egui::vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM, shorten_hash(&card.hash), egui::FontId::monospace(11.0), egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha));
            let miner_label = block_miner_label(card, &self.prefs.payout_address);
            painter.text(block_rect.right_top() + egui::vec2(-10.0, 10.0), egui::Align2::RIGHT_TOP, miner_label, egui::FontId::proportional(11.0), egui::Color32::from_rgba_unmultiplied(190, 255, 210, alpha));
        }
        if blocks.is_empty() {
            painter.text(egui::pos2(block_x, rect.top() + 36.0), egui::Align2::LEFT_TOP, "Global blocks will stream here", egui::FontId::proportional(13.0), text);
        }
    }

    fn ui_connection_icon(&self, ui: &mut egui::Ui, online: bool) {
        let texture = if online { self.online_icon.as_ref() } else { self.offline_icon.as_ref() };
        if let Some(texture) = texture {
            let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(14.0, 14.0));
            ui.add(egui::Image::from_texture(sized));
        } else {
            let color = if online { egui::Color32::from_rgb(66, 220, 120) } else { ui.visuals().weak_text_color() };
            ui.colored_label(color, if online { "online" } else { "offline" });
        }
    }

    fn ui_peer_list(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                self.ui_connection_icon(ui, self.miner.is_some());
                ui.label(egui::RichText::new("Local miner").strong());
                ui.monospace(if self.prefs.payout_address.trim().is_empty() { "Guest".to_string() } else { shorten_hash(self.prefs.payout_address.trim()) });
                ui.separator();
                ui.label(format_hps(self.hash_rate_hps));
                ui.separator();
                ui.label(if self.miner.is_some() { "mining" } else { "idle" });
            });
            ui.small("Green dot means this local public address mined a block recently. IPs are hidden; peer status is global when direct TCP is not possible.");
        });
        ui.add_space(8.0);

        self.ui_icon_label(ui, "peers-other-miners", "Peers / other miners");
        if self.snapshot.peers.is_empty() {
            ui.small("No global peer telemetry yet. QUB Core is using automatic discovery / DNS seed peer exchange.");
        } else {
            egui::Grid::new("peer_status_grid").num_columns(5).spacing([18.0, 6.0]).striped(true).show(ui, |ui| {
                ui.label(egui::RichText::new("Status").weak());
                ui.label(egui::RichText::new("Public address").weak());
                ui.label(egui::RichText::new("Height").weak());
                ui.label(egui::RichText::new("Tip").weak());
                ui.label(egui::RichText::new("Role").weak());
                ui.end_row();
                for peer in &self.snapshot.peers {
                    ui.horizontal(|ui| {
                        self.ui_connection_icon(ui, peer.reachable || peer.global_live);
                        ui.colored_label(peer_status_color(peer, ui), peer_status_text(peer));
                    });
                    ui.monospace(peer_identity_label(peer));
                    ui.label(peer.height.map(|h| format!("#{h}")).unwrap_or_else(|| "-".to_string()));
                    ui.monospace(if peer.tip_hash.is_empty() { "-".to_string() } else { shorten_hash(&peer.tip_hash) });
                    ui.small(peer_activity_text(peer));
                    ui.end_row();
                }
            });
        }
    }


    fn ui_recent_global_blocks(&mut self, ui: &mut egui::Ui) {
        self.ui_icon_label(ui, "recent-global-blocks", "Recent global blocks");
        if self.snapshot_in_flight || HF105_CATCHUP_IN_FLIGHT.load(Ordering::SeqCst) {
            self.ui_sync_progress_bar_rows(ui, "Refreshing recent global blocks", 2);
            ui.small("Recent blocks stay visible while QUB Core checks whether the official/network tip moved.");
        }
        if self.snapshot.recent_blocks.is_empty() {
            ui.small("No global blocks loaded yet. Local disk state loads first; network catch-up runs detached.");
        } else {
            let known_tip = self.best_known_network_height()
                .max(self.snapshot.known_network_height)
                .max(self.snapshot.direct_network_height);
            let local_latest = self.snapshot.recent_blocks.first().map(|b| b.height).unwrap_or(0);
            if known_tip > local_latest {
                ui.small(format!(
                    "Network tip is #{}; local block list is still catching up from #{}. The first row below is the latest known network block and remains provisional until local validation catches up.",
                    known_tip,
                    local_latest
                ));
            }
            egui::Grid::new("block_history_grid").num_columns(6).spacing([30.0, 6.0]).striped(true).show(ui, |ui| {
                ui.label(egui::RichText::new("Block").weak());
                ui.label(egui::RichText::new("Reward").weak());
                ui.label(egui::RichText::new("Txs").weak());
                ui.label(egui::RichText::new("Age").weak());
                ui.label(egui::RichText::new("Mined by").weak());
                ui.label(egui::RichText::new("Hash").weak());
                ui.end_row();
                if known_tip > local_latest {
                    let latest_tip_help = "Latest known network block is provisional and has not been locally fetched/validated yet. Treat mined blocks as pending until 2+ confirmations; a competing tip can replace the newest row.";
                    ui_recent_block_cell(ui, format!("pending #{}", known_tip), true, false, latest_tip_help);
                    ui_recent_block_cell(ui, "pending local validation", true, false, latest_tip_help);
                    ui_recent_block_cell(ui, "-", true, false, latest_tip_help);
                    ui_recent_block_cell(ui, "catching up", true, false, latest_tip_help);
                    ui_recent_block_cell(ui, "official/direct tip", true, false, latest_tip_help);
                    let tip_hash = if self.snapshot.known_network_hash.trim().is_empty() { "-".to_string() } else { shorten_hash(&self.snapshot.known_network_hash) };
                    ui_recent_block_cell(ui, tip_hash, true, true, latest_tip_help);
                    ui.end_row();
                    if known_tip > local_latest.saturating_add(1) {
                        let missing = known_tip.saturating_sub(local_latest).saturating_sub(1);
                        let gap_help = "QUB Core knows the official/direct tip is ahead and is fetching the missing intermediate blocks. Mining waits for the canonical chain view; this row is informational only.";
                        ui_recent_block_cell(ui, format!("gap #{}..#{}", local_latest.saturating_add(1), known_tip.saturating_sub(1)), true, false, gap_help);
                        ui_recent_block_cell(ui, format!("fetching {missing} block(s)"), true, false, gap_help);
                        ui_recent_block_cell(ui, "-", true, false, gap_help);
                        ui_recent_block_cell(ui, "syncing", true, false, gap_help);
                        ui_recent_block_cell(ui, "official/direct gap", true, false, gap_help);
                        ui_recent_block_cell(ui, "-", true, false, gap_help);
                        ui.end_row();
                    }
                }
                let mut previous_rendered_height: Option<u32> = None;
                for (idx, card) in self.snapshot.recent_blocks.iter().enumerate() {
                    if let Some(prev_height) = previous_rendered_height {
                        if prev_height > card.height.saturating_add(1) {
                            let missing = prev_height.saturating_sub(card.height).saturating_sub(1);
                            let gap_help = "There is a gap between visible block rows. QUB Core is fetching those intermediate blocks from the official/direct chain view before treating the list as complete.";
                            ui_recent_block_cell(ui, format!("gap #{}..#{}", card.height.saturating_add(1), prev_height.saturating_sub(1)), true, false, gap_help);
                            ui_recent_block_cell(ui, format!("fetching {missing} block(s)"), true, false, gap_help);
                            ui_recent_block_cell(ui, "-", true, false, gap_help);
                            ui_recent_block_cell(ui, "syncing", true, false, gap_help);
                            ui_recent_block_cell(ui, "missing intermediate blocks", true, false, gap_help);
                            ui_recent_block_cell(ui, "-", true, false, gap_help);
                            ui.end_row();
                        }
                    }
                    previous_rendered_height = Some(card.height);
                    let confirmations = if known_tip >= card.height {
                        known_tip.saturating_sub(card.height).saturating_add(1)
                    } else {
                        0
                    };
                    let is_actual_latest_row = known_tip > 0 && card.height == known_tip;
                    let is_local_latest_without_network_tip = known_tip == 0 && idx == 0;
                    let pending_finality = is_actual_latest_row || is_local_latest_without_network_tip;
                    let finality_hover = if pending_finality {
                        if confirmations <= 1 {
                            "Latest known block: 1 confirmation. Treat it as pending until 2+ confirmations. If another valid block wins, this row can be replaced and a mined reward may disappear before finality.".to_string()
                        } else {
                            "Latest visible block. Confirmations are still being measured; QUB Core treats blocks as much safer at 2+ confirmations.".to_string()
                        }
                    } else {
                        format!("{} confirmation(s). This block is past the latest-row pending-finality warning.", confirmations.max(1))
                    };
                    let block_label = if pending_finality { format!("pending #{}", card.height) } else { format!("#{}", card.height) };
                    ui_recent_block_cell(ui, block_label, pending_finality, false, &finality_hover);
                    ui_recent_block_cell(ui, card.reward.clone(), pending_finality, false, &finality_hover);
                    ui_recent_block_cell(ui, card.txs.to_string(), pending_finality, false, &finality_hover);
                    ui_recent_block_cell(ui, format_block_age(card), pending_finality, false, &finality_hover);
                    ui_recent_block_miner_cell(ui, card, block_miner_label(card, &self.prefs.payout_address), pending_finality, &finality_hover);
                    ui_recent_block_cell(ui, shorten_hash(&card.hash), pending_finality, true, &finality_hover);
                    ui.end_row();
                }
            });
        }
    }

    fn local_mined_active(&self) -> bool {
        self.last_local_mined_at
            .map(|at| at.elapsed() <= Duration::from_secs(LOCAL_ACTIVE_DOT_SECS))
            .unwrap_or(false)
    }

    fn mining_status_texture(&self, dark_mode: bool) -> Option<&egui::TextureHandle> {
        let elapsed = self.mining_phase_started_at.map(|t| t.elapsed()).unwrap_or_else(|| self.app_started.elapsed());
        match self.mining_phase {
            MiningPhase::Off => {
                if self.prefs.payout_address.trim().is_empty() { None }
                else if dark_mode { self.mining_off_icon_white.as_ref().or(self.mining_off_icon.as_ref()) }
                else { self.mining_off_icon.as_ref().or(self.mining_off_icon_white.as_ref()) }
            }
            MiningPhase::Preparing => {
                let anim = if dark_mode { self.mining_prep_anim_white.as_ref().or(self.mining_prep_anim.as_ref()) }
                    else { self.mining_prep_anim.as_ref().or(self.mining_prep_anim_white.as_ref()) };
                anim.and_then(|a| a.texture_at(elapsed))
                    .or_else(|| if dark_mode { self.mining_off_icon_white.as_ref().or(self.mining_off_icon.as_ref()) } else { self.mining_off_icon.as_ref().or(self.mining_off_icon_white.as_ref()) })
            }
            MiningPhase::Mining => {
                let anim = if dark_mode { self.mining_on_anim_white.as_ref().or(self.mining_on_anim.as_ref()) }
                    else { self.mining_on_anim.as_ref().or(self.mining_on_anim_white.as_ref()) };
                anim.and_then(|a| a.texture_at(elapsed))
                    .or_else(|| if dark_mode { self.mining_off_icon_white.as_ref().or(self.mining_off_icon.as_ref()) } else { self.mining_off_icon.as_ref().or(self.mining_off_icon_white.as_ref()) })
            }
        }
    }

    fn ui_status(&mut self, ui: &mut egui::Ui) {
        egui::Frame::default().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(texture) = self.mining_status_texture(ui.visuals().dark_mode) {
                    let sized = egui::load::SizedTexture::new(texture.id(), egui::vec2(40.0, 40.0));
                    ui.add(egui::Image::from_texture(sized));
                    ui.add_space(6.0);
                }
                let preparing_recent = matches!(self.mining_phase, MiningPhase::Preparing)
                    && self.mining_phase_started_at.map(|at| at.elapsed() < Duration::from_secs(45)).unwrap_or(false);
                let wallet_sync_recent = self.wallet_sync_in_flight && self.last_wallet_sync.elapsed() < Duration::from_secs(60);
                let snapshot_recent = self.snapshot_in_flight && self.last_refresh.elapsed() < Duration::from_secs(60);
                if preparing_recent || wallet_sync_recent || snapshot_recent || self.update_check_in_flight || self.tx_status_in_flight || self.pool_rx.is_some() {
                    ui.spinner();
                }
                ui.label(egui::RichText::new(&self.status_line).strong());
                if let Some(success) = &self.last_success {
                    ui.separator();
                    ui.label(success);
                }
                if let Some(err) = &self.last_error {
                    ui.separator();
                    let lower = err.to_lowercase();
                    let is_warning = lower.contains("paused")
                        || lower.contains("sync is taking")
                        || lower.contains("startup sync")
                        || lower.contains("background")
                        || lower.contains("snapshot refresh took too long")
                        || lower.contains("released")
                        || lower.contains("warning")
                        || lower.contains("stale")
                        || lower.contains("retrying")
                        || lower.contains("candidate");
                    let color = if is_warning { egui::Color32::from_rgb(236, 190, 78) } else { egui::Color32::RED };
                    ui.colored_label(color, err);
                }
            });
        });
    }
}

fn metric(ui: &mut egui::Ui, label: impl Into<String>, value: impl Into<String>) {
    ui.label(egui::RichText::new(label.into()).weak());
    ui.label(egui::RichText::new(value.into()).strong());
    ui.end_row();
}

fn cli_config_path() -> Option<String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let pos = args.iter().position(|a| a == "--config")?;
    args.get(pos + 1).cloned()
}

fn load_gui_prefs() -> Result<GuiPrefs> {
    let path = app_write_path(PREFS_FILE);
    let raw = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

fn save_gui_prefs(prefs: &GuiPrefs) -> Result<()> {
    let path = app_write_path(PREFS_FILE);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(path, serde_json::to_string_pretty(prefs)?)?;
    Ok(())
}

fn app_base_dir() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("config").is_dir() || cwd.join("Cargo.toml").is_file() {
        return cwd;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.to_path_buf();
        }
    }
    cwd
}

fn app_write_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() { p } else { app_base_dir().join(p) }
}

fn write_hf86_post_update_restart_marker() -> Result<()> {
    let path = app_write_path(HF105_POST_UPDATE_RESTART_MARKER);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(path, b"post-update-restart")?;
    Ok(())
}

fn consume_hf86_post_update_restart_marker() -> bool {
    let path = app_write_path(HF105_POST_UPDATE_RESTART_MARKER);
    if path.exists() {
        let _ = std::fs::remove_file(path);
        true
    } else {
        false
    }
}

fn resolve_app_read_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() || p.exists() {
        return p;
    }
    let base = app_base_dir();
    let beside_base = base.join(&p);
    if beside_base.exists() {
        return beside_base;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let beside_exe = exe_dir.join(&p);
            if beside_exe.exists() {
                return beside_exe;
            }
            if let Some(project_root) = exe_dir.parent().and_then(|p| p.parent()) {
                let project_candidate = project_root.join(&p);
                if project_candidate.exists() {
                    return project_candidate;
                }
            }
        }
    }
    p
}

fn chain_file_height(data_dir: &Path) -> Option<u32> {
    let raw = std::fs::read_to_string(data_dir.join("chain.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let blocks = json.get("blocks")?.as_array()?;
    Some(blocks.len().saturating_sub(1) as u32)
}

fn best_sibling_data_dir_for_network(network: &str) -> Option<PathBuf> {
    let base = app_base_dir();
    let parent = base.parent()?;
    let mut best: Option<(u32, PathBuf)> = None;
    for entry in std::fs::read_dir(parent).ok()? {
        let Ok(entry) = entry else { continue; };
        let Ok(ft) = entry.file_type() else { continue; };
        if !ft.is_dir() { continue; }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("qubd-v") { continue; }
        let candidate = entry.path().join("data").join(network);
        let Some(height) = chain_file_height(&candidate) else { continue; };
        if height == 0 { continue; }
        if best.as_ref().map(|(h, _)| height > *h).unwrap_or(true) {
            best = Some((height, candidate));
        }
    }
    best.map(|(_, path)| path)
}

fn resolve_gui_data_dir(settings: &Settings) -> PathBuf {
    let configured = PathBuf::from(&settings.node.data_dir);
    let resolved = if configured.is_absolute() { configured } else { app_base_dir().join(configured) };

    // Local dev/test smoke often extracts a new qubd-vX.Y.Z folder without copying
    // data/mainnet. Do not silently create/use a fresh genesis chain when a sibling
    // qubd-v* folder has the real network chain. Installer installs normally keep
    // the same application base, so this fallback is only used when the configured
    // data dir has no useful chain history.
    let local_height = chain_file_height(&resolved).unwrap_or(0);
    if local_height > 0 || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return resolved;
    }

    best_sibling_data_dir_for_network(&settings.network.name).unwrap_or(resolved)
}

fn load_gui_settings(config_path: &str) -> Result<Settings> {
    let resolved_config = resolve_app_read_path(config_path);
    let mut settings = Settings::load_from_path(&resolved_config)?;
    let data_dir = resolve_gui_data_dir(&settings);
    settings.node.data_dir = data_dir.to_string_lossy().to_string();
    Ok(settings)
}

fn format_activity_amount(net_atoms: i128) -> String {
    let sign = if net_atoms < 0 { "-" } else { "" };
    let atoms = net_atoms.unsigned_abs() as u64;
    let amt = Amount::from_atoms(atoms).map(|a| a.to_string()).unwrap_or_else(|_| "0".to_string());
    format!("{}{} QUB", sign, amt)
}

fn format_activity_fee(fee_atoms: u64) -> String {
    Amount::from_atoms(fee_atoms).map(|a| format!("{} QUB", a)).unwrap_or_else(|_| "0 QUB".to_string())
}

fn build_address_activity(settings: &Settings, chain: &ChainState, wallet: &WalletFile, wallet_scripts: &std::collections::HashSet<Vec<u8>>, payout_address: &str) -> Vec<AddressActivityEntry> {
    build_address_activity_window(settings, chain, wallet, wallet_scripts, payout_address, 1)
}

fn build_address_activity_recent(settings: &Settings, chain: &ChainState, wallet: &WalletFile, wallet_scripts: &std::collections::HashSet<Vec<u8>>, payout_address: &str, window_blocks: usize) -> Vec<AddressActivityEntry> {
    // HF106/v1.6.9: keep UI snapshots strictly bounded. The right activity panel
    // needs recent/pending context for debugging; it must not pre-index the whole
    // historical chain on every GUI tick.
    let bounded = window_blocks.clamp(128, 768);
    let start = chain.blocks.len().saturating_sub(bounded).max(1);
    build_address_activity_window(settings, chain, wallet, wallet_scripts, payout_address, start)
}

fn build_address_activity_window(settings: &Settings, chain: &ChainState, wallet: &WalletFile, wallet_scripts: &std::collections::HashSet<Vec<u8>>, payout_address: &str, activity_start_height: usize) -> Vec<AddressActivityEntry> {
    let mut owned_addresses = wallet.keys.iter().map(|k| k.address.clone()).collect::<std::collections::HashSet<_>>();
    if !payout_address.trim().is_empty() { owned_addresses.insert(payout_address.trim().to_string()); }
    let mut known_outputs: HashMap<OutPoint, (TxOut, u32, bool)> = HashMap::new();
    let mut rows: Vec<AddressActivityEntry> = Vec::new();
    let tip_height = chain.height();

    let mut handle_tx = |tx: &Transaction, height: Option<u32>, block_time: u32, status_override: Option<&str>, block_jin_reward_units: u128, known_outputs: &mut HashMap<OutPoint, (TxOut, u32, bool)>, rows: &mut Vec<AddressActivityEntry>| {
        let txid = tx.txid();
        let is_coinbase = tx.is_coinbase();
        let mined_jin_reward_units = if is_coinbase { block_jin_reward_units } else { 0 };
        let regs = qns_registrations_in_tx(tx, settings);
        let qns_name = regs.first().map(|(_, reg, _)| reg.name.clone()).unwrap_or_default();
        let qns_targets_owned = regs.iter().any(|(_, reg, _)| owned_addresses.contains(&reg.address));
        let library_markers = library_markers_in_tx(tx, settings);
        let library_label = library_markers.first().map(|(_, marker, _)| match marker {
            LibraryMarker::Post { title, .. } => format!("Library post: {}", title),
            LibraryMarker::Comment { post_id, .. } => format!("Library comment on {}", shorten_hash(post_id)),
            LibraryMarker::Vote { target_kind, target_id, up, .. } => format!("Library {}vote on {} {}", if *up { "up" } else { "down" }, target_kind, shorten_hash(target_id)),
            LibraryMarker::Edit { target_kind, target_id, .. } => format!("Library edit {} {}", target_kind, shorten_hash(target_id)),
            LibraryMarker::Delete { target_kind, target_id, .. } => format!("Library delete {} {}", target_kind, shorten_hash(target_id)),
        }).unwrap_or_default();
        let library_owned = library_markers.iter().any(|(_, marker, _)| match marker {
            LibraryMarker::Post { author, .. } | LibraryMarker::Comment { author, .. } | LibraryMarker::Vote { author, .. } | LibraryMarker::Edit { author, .. } | LibraryMarker::Delete { author, .. } => owned_addresses.contains(author),
        });
        let jin_transfers = jin_transfers_in_tx(tx, settings);
        let jin_conversions = jin_conversions_in_tx(tx, settings);
        let mut jin_net: i128 = 0;
        let mut jin_fee_units: u128 = 0;
        let mut jin_counterparty = String::new();
        let mut conversion_matrix = String::new();
        let mut conversion_net: i128 = 0;
        let mut conversion_fee_units: u128 = 0;
        for (_, jt, _) in &jin_transfers {
            if owned_addresses.contains(&jt.from) { jin_net -= jt.amount_units as i128; if jt.fee_asset == "JIN" { jin_net -= jt.fee_units as i128; jin_fee_units = jin_fee_units.saturating_add(jt.fee_units); } jin_counterparty = jt.to.clone(); }
            if owned_addresses.contains(&jt.to) { jin_net += jt.amount_units as i128; jin_counterparty = jt.from.clone(); }
        }
        for (_, jc, _) in &jin_conversions {
            if owned_addresses.contains(&jc.from) {
                conversion_net -= jc.amount_units as i128;
                if jc.fee_asset == "JIN" { conversion_net -= jc.fee_units as i128; conversion_fee_units = conversion_fee_units.saturating_add(jc.fee_units); }
                conversion_matrix = jc.matrix_address.clone();
            }
        }

        let incoming_atoms = tx.outputs.iter()
            .filter(|out| wallet_scripts.contains(&out.script_pubkey.0))
            .map(|out| out.value.atoms())
            .sum::<u64>();

        let mut spent_atoms = 0u64;
        let mut input_owned = false;
        for input in &tx.inputs {
            if let Some((prev_out, _, _)) = known_outputs.get(&input.previous_output) {
                if wallet_scripts.contains(&prev_out.script_pubkey.0) {
                    input_owned = true;
                    spent_atoms = spent_atoms.saturating_add(prev_out.value.atoms());
                }
            }
        }

        let touches = incoming_atoms > 0 || input_owned || qns_targets_owned || library_owned || jin_net != 0 || conversion_net != 0;
        if touches {
            let total_out = tx.outputs.iter().map(|out| out.value.atoms()).sum::<u64>();
            let fee_atoms = if !is_coinbase && input_owned { spent_atoms.saturating_sub(total_out) } else { 0 };
            let net = incoming_atoms as i128 - spent_atoms as i128;
            let activity_type = if is_coinbase { "Mining" } else if conversion_net != 0 { "Conversion" } else if !regs.is_empty() { "QNS Registration" } else if !library_markers.is_empty() { "Library" } else { "Transfer" };
            let confirmations = height.map(|h| tip_height.saturating_sub(h).saturating_add(1)).unwrap_or(0);
            let status = if let Some(s) = status_override { s.to_string() }
                else if is_coinbase {
                    let h = height.unwrap_or(0);
                    if h.saturating_add(settings.consensus.coinbase_maturity) > tip_height { "Immature".to_string() } else { "Matured".to_string() }
                } else { "Confirmed".to_string() };
            let direction = if conversion_net != 0 { "Outgoing" } else if jin_net > 0 { "Incoming" } else if jin_net < 0 { "Outgoing" } else if net > 0 { "Incoming" } else if net < 0 { "Outgoing" } else { "Neutral" };
            let counterparty = if input_owned {
                tx.outputs.iter()
                    .filter_map(|out| address_from_script_pubkey(&settings.network.address_prefix, &out.script_pubkey).map(|a| a.to_string()))
                    .find(|addr| !owned_addresses.contains(addr))
                    .unwrap_or_else(|| if qns_name.is_empty() { "self/change".to_string() } else { qns_name.clone() })
            } else {
                tx.outputs.iter()
                    .filter(|out| wallet_scripts.contains(&out.script_pubkey.0))
                    .filter_map(|out| address_from_script_pubkey(&settings.network.address_prefix, &out.script_pubkey).map(|a| a.to_string()))
                    .next()
                    .unwrap_or_else(|| if qns_name.is_empty() { "address activity".to_string() } else { qns_name.clone() })
            };
            let details = match activity_type {
                "Mining" => format!("Coinbase reward. Matures after {} blocks.", settings.consensus.coinbase_maturity),
                "QNS Registration" => if qns_name.is_empty() { "QNS registration payment/marker.".to_string() } else { format!("QNS registration for {}.", qns_name) },
                "Library" => if library_label.is_empty() { "Library action.".to_string() } else { library_label.clone() },
                _ => format!("{} transfer. Fee: {}.", direction, format_activity_fee(fee_atoms)),
            };
            let mining_pending_decision = is_coinbase && confirmations < 2;
            let display_status = if mining_pending_decision { "Pending Decision".to_string() } else { status };
            let display_amount = if mining_pending_decision {
                "Pending decision".to_string()
            } else if is_coinbase && mined_jin_reward_units > 0 {
                format!("{} + {} JIN", format_activity_amount(net), format_jin_amount(mined_jin_reward_units))
            } else if conversion_net != 0 {
                format!("{}{} JIN", if conversion_net < 0 { "-" } else { "" }, format_jin_amount(conversion_net.unsigned_abs() as u128))
            } else if jin_net != 0 {
                format!("{}{} JIN", if jin_net < 0 { "-" } else { "" }, format_jin_amount(jin_net.unsigned_abs() as u128))
            } else {
                format_activity_amount(net)
            };
            let display_details = if mining_pending_decision {
                "Mining reward is waiting for active-chain decision. The amount is hidden until 2+ confirmations so stale/orphan candidates do not look like credited rewards.".to_string()
            } else if is_coinbase && mined_jin_reward_units > 0 {
                format!("Coinbase reward plus {} JIN fee reward. QUB coinbase matures after {} blocks; JIN fee reward is credited by the JIN ledger.", format_jin_amount(mined_jin_reward_units), settings.consensus.coinbase_maturity)
            } else if conversion_net != 0 {
                format!("JIN Coin -> JIN Token conversion request. Matrix recipient: {}. Fee: {}.", conversion_matrix, if conversion_fee_units > 0 { format!("{} JIN", format_jin_amount(conversion_fee_units)) } else { format_activity_fee(fee_atoms) })
            } else if jin_net != 0 {
                format!("JIN transfer. Fee: {}.", if jin_fee_units > 0 { format!("{} JIN", format_jin_amount(jin_fee_units)) } else { format_activity_fee(fee_atoms) })
            } else {
                details
            };
            rows.push(AddressActivityEntry {
                txid: txid.to_string(),
                activity_type: activity_type.to_string(),
                status: display_status,
                direction: direction.to_string(),
                amount: display_amount,
                fee: if conversion_fee_units > 0 { format!("{} JIN", format_jin_amount(conversion_fee_units)) } else if jin_fee_units > 0 { format!("{} JIN", format_jin_amount(jin_fee_units)) } else { format_activity_fee(fee_atoms) },
                height,
                confirmations,
                time: block_time,
                counterparty: if !conversion_matrix.is_empty() { conversion_matrix.clone() } else if !jin_counterparty.is_empty() { jin_counterparty.clone() } else { counterparty },
                details: display_details,
                qns_name,
            });
        }

        if height.is_some() {
            for (vout, out) in tx.outputs.iter().enumerate() {
                known_outputs.insert(OutPoint { txid, vout: vout as u32 }, (out.clone(), height.unwrap_or(0), is_coinbase));
            }
        }
    };

    let activity_start_height = activity_start_height.max(1).min(chain.blocks.len().saturating_sub(1).max(1));
    // HF106: quick snapshots still show recent activity, but they do not parse
    // every historical marker on every UI tick. Pre-seed only owned historical
    // outputs so recent outgoing spends can still be recognized.
    if activity_start_height > 1 {
        // HF106: bounded pre-seed only. This preserves common recent outgoing/
        // mempool fee detection without turning every refresh into a full-chain
        // owned-output index build.
        let preseed_start = activity_start_height.saturating_sub(512).max(1);
        for (height, block) in chain.blocks.iter().enumerate().skip(preseed_start).take(activity_start_height.saturating_sub(preseed_start)) {
            for tx in &block.transactions {
                for (vout, out) in tx.outputs.iter().enumerate() {
                    if wallet_scripts.contains(&out.script_pubkey.0) {
                        known_outputs.insert(OutPoint { txid: tx.txid(), vout: vout as u32 }, (out.clone(), height as u32, tx.is_coinbase()));
                    }
                }
            }
        }
    }

    for (height, block) in chain.blocks.iter().enumerate().skip(activity_start_height) {
        if height == 0 { continue; }
        for tx in &block.transactions {
            handle_tx(tx, Some(height as u32), block.header.time, None, block_jin_fee_units(settings, block), &mut known_outputs, &mut rows);
        }
    }

    let now = unix_time_u32();
    for tx in &chain.mempool {
        handle_tx(tx, None, now, Some("Mempool"), 0, &mut known_outputs, &mut rows);
    }

    rows.sort_by(|a, b| {
        let ah = a.height.unwrap_or(u32::MAX);
        let bh = b.height.unwrap_or(u32::MAX);
        bh.cmp(&ah).then_with(|| b.time.cmp(&a.time)).then_with(|| b.txid.cmp(&a.txid))
    });
    rows.dedup_by(|a, b| a.txid == b.txid && a.activity_type == b.activity_type);
    rows
}

fn average_block_spacing_secs(chain: &ChainState, window: usize) -> Option<u32> {
    if chain.blocks.len() <= 2 { return None; }
    let last_index = chain.blocks.len().saturating_sub(1);
    let first_index = last_index.saturating_sub(window.max(2));
    if first_index >= last_index { return None; }
    let first_time = chain.blocks.get(first_index)?.header.time;
    let last_time = chain.blocks.get(last_index)?.header.time;
    let blocks = (last_index - first_index) as u64;
    if blocks == 0 { return None; }
    let secs = last_time.saturating_sub(first_time) as u64;
    Some((secs / blocks).max(1).min(u32::MAX as u64) as u32)
}

fn daa_observation_text(target_spacing_secs: u32, avg10: u32, avg20: u32) -> String {
    if target_spacing_secs == 0 || avg10 == 0 {
        return "Collecting enough recent blocks for DAA telemetry".to_string();
    }
    let target = target_spacing_secs.max(1);
    if avg20 > target.saturating_mul(8) {
        format!("Very slow: recent 20-block average is {}s vs {}s target. Watch QUB Core sync/miner reach first; DAA may need a future consensus review if this persists.", avg20, target)
    } else if avg20 > target.saturating_mul(4) {
        format!("Slow: recent 20-block average is {}s vs {}s target. Likely low network hashrate / recovery phase; keep monitoring before changing DAA.", avg20, target)
    } else if avg10 < (target / 3).max(1) {
        format!("Fast: recent 10-block average is {}s vs {}s target. DAA appears to be reacting downward/upward; keep monitoring.", avg10, target)
    } else {
        format!("Stable enough for now: recent avg10 {}s / avg20 {}s vs {}s target. No QUB Core consensus change.", avg10, avg20, target)
    }
}


fn hf88_read_text_atomicish(path: &Path, label: &str) -> Result<String> {
    let mut last_err = String::new();
    for attempt in 0..8u64 {
        match std::fs::read_to_string(path) {
            Ok(raw) if !raw.trim().is_empty() => return Ok(raw),
            Ok(_) => last_err = format!("{label} file was empty"),
            Err(err) => last_err = err.to_string(),
        }
        thread::sleep(Duration::from_millis(12 + attempt.saturating_mul(8)));
    }
    anyhow::bail!("{label} is temporarily unavailable: {last_err}")
}

fn hf88_load_chain_for_gui_snapshot(settings: &Settings) -> Result<ChainState> {
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    if !paths.chain_file.exists() {
        // First run / empty data dir: normal initializer is fine because no
        // detached writer has had a chance to start yet.
        return load_or_init_chain(settings);
    }
    let raw = hf88_read_text_atomicish(&paths.chain_file, "chain.json")?;
    let persisted: PersistedChainState = serde_json::from_str(&raw)?;
    ChainState::from_persisted_unchecked_for_ui(persisted, settings)
}

fn hf88_load_wallet_for_gui_snapshot(settings: &Settings) -> Result<WalletFile> {
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    if !paths.wallet_file.exists() {
        return load_or_init_wallet(settings);
    }
    let raw = hf88_read_text_atomicish(&paths.wallet_file, "wallet.json")?;
    let wallet: WalletFile = serde_json::from_str(&raw)?;
    wallet.ensure_network(settings)?;
    Ok(wallet)
}

fn read_snapshot(config_path: &str) -> Result<ChainSnapshot> {
    read_snapshot_for_payout(config_path, "")
}

fn read_snapshot_for_payout(config_path: &str, payout_address: &str) -> Result<ChainSnapshot> {
    read_snapshot_for_payout_inner(config_path, payout_address, true, false, true)
}

fn read_snapshot_for_payout_quick(config_path: &str, payout_address: &str) -> Result<ChainSnapshot> {
    // HF106/v1.6.9: quick UI snapshots use the fast local chain loader plus the
    // cached/very-bounded peer view. Startup remains pure local, but normal quick
    // refreshes must still update peers/known tip/activity without full replay.
    read_snapshot_for_payout_inner(config_path, payout_address, false, true, true)
}

fn read_snapshot_for_payout_startup(config_path: &str, payout_address: &str) -> Result<ChainSnapshot> {
    // HF106/v1.6.9: first paint must be pure local disk state. No HTTP, no peer
    // registry probes, no full address-activity scan, and no catch-up writer.
    read_snapshot_for_payout_inner(config_path, payout_address, false, true, false)
}

fn jin_balance_units_for_address_confirmed(settings: &Settings, chain: &ChainState, address: &str, min_confirmations: u32) -> Result<u128> {
    Address::parse_with_prefix(address, &settings.network.address_prefix)?;
    let keep = min_confirmations.saturating_sub(1) as usize;
    let confirmed_len = chain.blocks.len().saturating_sub(keep);
    let ledger = jin_ledger_from_blocks(settings, &chain.blocks[..confirmed_len])?;
    Ok(ledger.get(address).copied().unwrap_or(0))
}

fn read_snapshot_for_payout_inner(config_path: &str, payout_address: &str, include_network_probe: bool, fast_local: bool, include_cached_peer_view: bool) -> Result<ChainSnapshot> {
    let settings = load_gui_settings(config_path)?;
    // HF106/v1.6.9: quick/startup GUI snapshots use a UI-only fast chain loader.
    // Full consensus replay stays in mining/sync/tx paths. This removes repeated
    // 45s/90s snapshot releases caused by validating the entire chain on each paint.
    let chain = if fast_local {
        hf88_load_chain_for_gui_snapshot(&settings).or_else(|_| load_or_init_chain(&settings))?
    } else {
        load_or_init_chain(&settings)?
    };
    let wallet = if fast_local {
        hf88_load_wallet_for_gui_snapshot(&settings).or_else(|_| load_or_init_wallet(&settings))?
    } else {
        load_or_init_wallet(&settings)?
    };
    let mut wallet_scripts = wallet.scripts().unwrap_or_default();
    if let Ok(payout) = Address::parse_with_prefix(payout_address.trim(), &settings.network.address_prefix) {
        wallet_scripts.insert(payout.script_pubkey().0);
    }
    let qns_registry = qns_registry_from_blocks(&settings, &chain.blocks).unwrap_or_default();
    let mut qns_by_address: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for rec in qns_registry.values() { qns_by_address.entry(rec.address.clone()).or_default().push(rec.name.clone()); }
    for names in qns_by_address.values_mut() { names.sort(); }
    let spendable = chain.balance_for_scripts(&wallet_scripts, &settings, false);
    let total = chain.balance_for_scripts(&wallet_scripts, &settings, true);
    let pools_registry = pools_registry_from_blocks(&settings, &chain.blocks).unwrap_or_default();

    let recent_blocks = chain.blocks
        .iter()
        .enumerate()
        .skip(1)
        .rev()
        .take(BLOCK_HISTORY_LIMIT)
        .map(|(height, block)| {
            let reward_atoms = block.transactions
                .first()
                .map(|tx| tx.outputs.iter().map(|out| out.value.atoms()).sum::<u64>())
                .unwrap_or(0);
            let pool_id_opt = parse_pool_block_marker(block);
            let pool_block = pool_id_opt.is_some();
            let pool_id = pool_id_opt.map(|id| id.to_string()).unwrap_or_default();
            let pool_name = pool_id_opt
                .and_then(|id| pools_registry.get(&id).map(|p| p.name.clone()))
                .unwrap_or_default();
            let local = if pool_block { false } else {
                block.transactions
                    .first()
                    .map(|tx| tx.outputs.iter().any(|out| wallet_scripts.contains(&out.script_pubkey.0)))
                    .unwrap_or(false)
            };
            let miner_address = block.transactions
                .first()
                .and_then(|tx| tx.outputs.first())
                .and_then(|out| address_from_script_pubkey(&settings.network.address_prefix, &out.script_pubkey))
                .map(|a| a.to_string())
                .unwrap_or_else(|| "Guest".to_string());
            let miner_qns = qns_by_address.get(&miner_address).cloned().unwrap_or_default();
            let jin_reward_units = block_jin_fee_units(&settings, block);
            let reward = format!("{} QUB + {} JIN", Amount::from_atoms(reward_atoms).map(|a| a.to_string()).unwrap_or_else(|_| "0".to_string()), format_jin_amount(jin_reward_units));
            SnapshotBlock {
                height: height as u32,
                hash: block.block_hash().to_string(),
                txs: block.transactions.len(),
                reward,
                time: block.header.time,
                first_seen_unix: block.header.time,
                canonical: true,
                local,
                miner_address,
                miner_qns,
                pool_block,
                pool_id,
                pool_name,
            }
        })
        .collect::<Vec<_>>();

    let recent_avg_10_secs = average_block_spacing_secs(&chain, 10).unwrap_or(0);
    let recent_avg_20_secs = average_block_spacing_secs(&chain, 20).unwrap_or(0);
    let daa_observation = daa_observation_text(settings.consensus.target_spacing_secs, recent_avg_10_secs, recent_avg_20_secs);

    let display_address_for_jin = if !payout_address.trim().is_empty() { payout_address.trim().to_string() } else { wallet.default_address().unwrap_or_default().to_string() };
    // HF106/v1.6.9: GUI balances show JIN only after 2+ confirmations.
    // Pending mempool JIN purchases/transfers still appear in Address Activity,
    // but the spendable JIN balance no longer jumps optimistically on buy.
    let jin_total = if display_address_for_jin.is_empty() { 0 } else { jin_balance_units_for_address_confirmed(&settings, &chain, &display_address_for_jin, 2).unwrap_or(0) };

    let wallet_addresses = wallet.keys.iter().map(|k| k.address.clone()).collect::<std::collections::HashSet<_>>();
    let mut pools = pools_registry.values().map(|pool| {
        // HF106/v1.6.9: pool/member visibility must not be false-zero in quick
        // snapshots. Share scoring is bounded to the protocol PPLNS window, so it
        // is safe enough for GUI refreshes and fixes the Pools window member count.
        let scores = pool_share_scores_from_blocks(&settings, &chain.blocks, chain.height() + 1, pool.pool_id);
        let active_miners = scores.len();
        let recent_shares = scores.values().copied().sum::<u128>();
        let your_shares = scores.get(&display_address_for_jin).copied().unwrap_or(0);
        let open_slots = pool.capacity_slots as i64 - active_miners as i64;
        PoolUiSummary {
            pool_id: pool.pool_id.to_string(),
            name: pool.name.clone(),
            manager_address: pool.manager_address.clone(),
            commission_bps: pool.commission_bps,
            capacity_slots: pool.capacity_slots,
            active_miners,
            open_slots,
            recent_shares,
            your_shares,
            your_active: your_shares > 0,
            is_manager: wallet_addresses.contains(&pool.manager_address),
            created_height: pool.created_height,
            total_paid_qub: Amount::from_atoms(pool.total_paid_atoms).map(|a| a.to_string()).unwrap_or_else(|_| "0".to_string()),
        }
    }).collect::<Vec<_>>();
    pools.sort_by(|a, b| a.created_height.cmp(&b.created_height).then(a.pool_id.cmp(&b.pool_id)));
    let pools_count = pools_registry.len().max(pools.len());
    let verified_governance_state = verified_governance_state_from_blocks(&settings, &chain.blocks).unwrap_or_default();
    let verified_wallets_count = verified_governance_state.wallets.len();
    let verified_pools_count = verified_governance_state.pools.len();
    let report_cases_count = verified_governance_state.reports.len();
    let active_moderators_count = verified_governance_state.moderators.values().filter(|m| m.status == VerifiedStatus::Active).count();

    let activity = if fast_local { build_address_activity_recent(&settings, &chain, &wallet, &wallet_scripts, payout_address, 512) } else { build_address_activity(&settings, &chain, &wallet, &wallet_scripts, payout_address) };

    let p2p_snapshot = if include_cached_peer_view {
        // HF106/v1.6.9: cached/official-tip peer view is allowed only after the
        // first local paint. Startup snapshots are zero-network local state.
        p2p::peer_status_cached(&settings).unwrap_or_else(|_| p2p::P2PNetworkSnapshot { enabled: settings.p2p.enabled, ..Default::default() })
    } else {
        p2p::P2PNetworkSnapshot { enabled: settings.p2p.enabled, ..Default::default() }
    };
    let mut known_peers = p2p_snapshot.known_peers;
    let mut reachable_peers = p2p_snapshot.reachable_peers;
    let mut direct_reachable_peers = p2p_snapshot.direct_reachable_peers;
    let mut global_live_peers = p2p_snapshot.globally_live_peers;
    let relay_capable = p2p_snapshot.relay_capable;
    let nat_private = p2p_snapshot.nat_private;
    let mut stale_warning = p2p_snapshot.stale_warning.clone();
    let mut known_network_height = chain.height();
    let mut known_network_hash = chain.tip_hash().to_string();

    // HF106/v1.6.9: GUI canonical progress must be based only on official seed
    // rows, not on arbitrary cached/reachable peers. Random peers can still be
    // shown in the peer table as telemetry, but they must not create the blue
    // "pending local validation / official-direct tip" row or make rewards look
    // confirmed/stale against a fake future height.
    let official_peer_tip = p2p_snapshot.peers.iter()
        .filter(|p| {
            let role = p.role.as_deref().unwrap_or("").to_ascii_lowercase();
            let ua = p.user_agent.as_deref().unwrap_or("").to_ascii_lowercase();
            let addr = p.addr.to_ascii_lowercase();
            role.contains("seed") || ua.contains("official") || addr.contains("seed-") || addr.contains("qubit-coin.io")
        })
        .filter_map(|p| p.height.map(|h| (h, p.tip_hash.clone().unwrap_or_default())))
        .max_by(|a, b| a.0.cmp(&b.0));

    if let Some((official_h, official_hash)) = official_peer_tip.clone() {
        if official_h > known_network_height && !official_hash.trim().is_empty() {
            known_network_hash = official_hash.clone();
        }
        known_network_height = known_network_height.max(official_h);
        if official_h > chain.height() && stale_warning.trim().is_empty() {
            stale_warning = format!(
                "Local chain #{} is behind official seed network #{} ({}...). QUB Core detached catch-up is retrying automatically; wallet UI stays live.",
                chain.height(),
                official_h,
                shorten_hash(&known_network_hash)
            );
        }
    }

    let direct_tip = if include_network_probe { official_peer_tip } else { None };
    let direct_network_height = direct_tip.as_ref().map(|(h, _)| *h).unwrap_or(0);
    let mut peers = p2p_snapshot.peers.into_iter().map(|peer| PeerUiStatus {
        addr: peer.addr,
        reachable: peer.reachable,
        global_live: peer.global_live,
        height: peer.height,
        tip_hash: peer.tip_hash.unwrap_or_default(),
        user_agent: peer.user_agent.unwrap_or_default(),
        error: peer.error.unwrap_or_default(),
        node_id: peer.node_id.unwrap_or_default(),
        observed_addr: peer.observed_addr.unwrap_or_default(),
        listen_addr: peer.listen_addr.unwrap_or_default(),
        role: peer.role.unwrap_or_default(),
        miner_address: peer.miner_address.clone().unwrap_or_default(),
        last_seen_unix: peer.last_seen_unix,
        seen_age_secs: peer.seen_age_secs,
        qns_names: peer.miner_address.as_ref().and_then(|a| qns_by_address.get(a).cloned()).unwrap_or_default(),
    }).collect::<Vec<_>>();

    if peers.is_empty() && fast_local {
        // HF106: snapshots are network-free, but the Peer/Block Stream should not
        // look dead. Build privacy-preserving pseudo peers from recent canonical
        // block producers that are already in the local snapshot.
        let now = unix_time_u32() as u64;
        let mut seen_miners = std::collections::HashSet::<String>::new();
        for block in recent_blocks.iter().take(48) {
            let miner = block.miner_address.trim();
            if miner.is_empty() || miner == "Guest" { continue; }
            if !seen_miners.insert(miner.to_string()) { continue; }
            let age = now.saturating_sub(block.time as u64);
            peers.push(PeerUiStatus {
                addr: if block.local { "local miner".to_string() } else { format!("miner:{}", shorten_hash(miner)) },
                reachable: false,
                global_live: age <= 7_200,
                height: Some(block.height),
                tip_hash: block.hash.clone(),
                user_agent: "observed-from-local-block-stream".to_string(),
                role: if block.local { "local miner".to_string() } else { "recent block producer".to_string() },
                miner_address: miner.to_string(),
                last_seen_unix: Some(block.time as u64),
                seen_age_secs: Some(age),
                qns_names: block.miner_qns.clone(),
                ..Default::default()
            });
        }
        peers.sort_by(|a, b| b.height.unwrap_or(0).cmp(&a.height.unwrap_or(0)).then_with(|| a.addr.cmp(&b.addr)));
    }
    known_peers = known_peers.max(peers.len());
    global_live_peers = global_live_peers.max(peers.iter().filter(|p| p.reachable || p.global_live).count());
    reachable_peers = reachable_peers.max(peers.iter().filter(|p| p.reachable).count());
    direct_reachable_peers = direct_reachable_peers.max(peers.iter().filter(|p| p.reachable).count());

    Ok(ChainSnapshot {
        network: settings.network.name.clone(),
        height: chain.height(),
        best_hash: chain.tip_hash().to_string(),
        known_network_height,
        known_network_hash,
        direct_network_height,
        recent_avg_10_secs,
        recent_avg_20_secs,
        daa_observation,
        mempool_txs: chain.mempool.len(),
        spendable: Amount::from_atoms(spendable)?.to_string(),
        immature: Amount::from_atoms(total.saturating_sub(spendable))?.to_string(),
        total: Amount::from_atoms(total)?.to_string(),
        jin_total: format_jin_amount(jin_total),
        wallet_keys: wallet.keys.len(),
        default_address: wallet.default_address().unwrap_or_default().to_string(),
        coinbase_maturity: settings.consensus.coinbase_maturity,
        target_spacing_secs: settings.consensus.target_spacing_secs,
        block_reward: Amount::from_atoms(block_subsidy(chain.height() as u64 + 1, &settings))?.to_string(),
        mined_qub_supply: mined_qub_supply_display(chain.height(), &settings)?,
        halving_interval: settings.consensus.subsidy_halving_interval,
        pow_bits: settings.consensus.pow_bits.clone(),
        data_dir: settings.node.data_dir.clone(),
        qns_count: qns_registry.len(),
        pools_count,
        verified_wallets_count,
        verified_pools_count,
        report_cases_count,
        active_moderators_count,
        verified_governance_activation_height: settings.verified_governance.activation_height,
        pools_activation_height: settings.pools.activation_height,
        pools_protocol_address: settings.pools.protocol_address.clone(),
        pools,
        qns_activation_height: settings.qns.activation_height,
        qns_protocol_name: settings.qns.protocol_name.clone(),
        qns_protocol_address: settings.qns.protocol_address.clone(),
        owned_qns: qns_by_address.get(&display_address_for_jin).cloned().unwrap_or_default(),
        features: v1_feature_notice(&settings),
        p2p_enabled: settings.p2p.enabled,
        known_peers,
        reachable_peers,
        direct_reachable_peers,
        global_live_peers,
        relay_capable,
        nat_private,
        stale_warning,
        peers,
        recent_blocks,
        activity,
    })
}



fn mined_qub_supply_atoms(height: u32, settings: &Settings) -> u64 {
    let mut total = 0u64;
    for h in 1..=height as u64 {
        total = total.saturating_add(block_subsidy(h, settings));
    }
    total
}

fn mined_qub_supply_display(height: u32, settings: &Settings) -> Result<String> {
    Ok(format!("{} QUB", Amount::from_atoms(mined_qub_supply_atoms(height, settings))?.to_string()))
}


fn format_integer_with_commas(raw: u128) -> String {
    let s = raw.to_string();
    let mut out = String::new();
    for (idx, ch) in s.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 { out.push(','); }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_enj_raw(raw: u128) -> String {
    const UNIT: u128 = 1_000_000_000_000_000_000u128;
    const SIX_DEC_SCALE: u128 = 1_000_000_000_000u128;

    // Round to 6 decimals for human display.
    let rounded_six = raw.saturating_add(SIX_DEC_SCALE / 2) / SIX_DEC_SCALE;
    let whole = rounded_six / 1_000_000u128;
    let frac6 = rounded_six % 1_000_000u128;

    if frac6 == 0 {
        format!("{} ENJ", format_integer_with_commas(whole))
    } else {
        format!("{}.{:06} ENJ", format_integer_with_commas(whole), frac6)
    }
}

fn parse_decimal_to_raw_units(input: &str, decimals: usize) -> Option<u128> {
    let token = input
        .trim()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches("/JIN")
        .trim_end_matches("ENJ")
        .replace(',', "");
    if token.is_empty() || token == "-" || token.starts_with('-') { return None; }
    let mut parts = token.split('.');
    let whole = parts.next().unwrap_or("0");
    let frac = parts.next().unwrap_or("");
    if parts.next().is_some() { return None; }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) { return None; }
    if frac.len() > decimals { return None; }
    let scale = 10u128.checked_pow(decimals as u32)?;
    let mut out = whole.parse::<u128>().ok()?.checked_mul(scale)?;
    if !frac.is_empty() {
        let mut frac_padded = frac.to_string();
        while frac_padded.len() < decimals { frac_padded.push('0'); }
        out = out.checked_add(frac_padded.parse::<u128>().ok()?)?;
    }
    Some(out)
}

fn confirmed_jin_total_infusion_display(jin_balance: &str, per_jin_infusion: &str) -> String {
    const ENJ_DECIMALS: usize = 18;
    let Some(jin_units) = parse_decimal_to_raw_units(jin_balance, JIN_DECIMALS as usize) else {
        return "0 ENJ".to_string();
    };
    let Some(per_jin_enj_raw) = parse_decimal_to_raw_units(per_jin_infusion, ENJ_DECIMALS) else {
        return "0 ENJ".to_string();
    };
    if jin_units == 0 || per_jin_enj_raw == 0 { return "0 ENJ".to_string(); }
    let whole_jin = jin_units / JIN_UNITS_PER_COIN;
    let frac_jin = jin_units % JIN_UNITS_PER_COIN;
    let total_raw = per_jin_enj_raw
        .saturating_mul(whole_jin)
        .saturating_add(per_jin_enj_raw.saturating_mul(frac_jin) / JIN_UNITS_PER_COIN);
    format_enj_raw(total_raw)
}

fn parse_hex_bytes(hex: &str) -> Vec<u8> {
    let clean = hex.trim().trim_start_matches("0x");
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < clean.len() {
        if let Ok(b) = u8::from_str_radix(&clean[i..i + 2], 16) { out.push(b); }
        i += 2;
    }
    out
}

fn read_u128_le_at(bytes: &[u8], offset: usize) -> Option<u128> {
    if offset + 16 > bytes.len() { return None; }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(&bytes[offset..offset + 16]);
    Some(u128::from_le_bytes(arr))
}

fn read_scale_compact_u128_at(bytes: &[u8], offset: usize) -> Option<u128> {
    if offset >= bytes.len() { return None; }

    let first = bytes[offset];
    match first & 0b11 {
        0 => Some((first >> 2) as u128),
        1 => {
            if offset + 2 > bytes.len() { return None; }
            let v = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
            Some((v >> 2) as u128)
        }
        2 => {
            if offset + 4 > bytes.len() { return None; }
            let v = u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]);
            Some((v >> 2) as u128)
        }
        _ => {
            let byte_len = ((first >> 2) as usize).saturating_add(4);
            if byte_len == 0 || byte_len > 16 || offset + 1 + byte_len > bytes.len() {
                return None;
            }

            let mut arr = [0u8; 16];
            arr[..byte_len].copy_from_slice(&bytes[offset + 1..offset + 1 + byte_len]);
            Some(u128::from_le_bytes(arr))
        }
    }
}

fn best_effort_jin_tokenomics_from_storage_hex(storage_hex: &str) -> EnjinMatrixMetrics {
    let bytes = parse_hex_bytes(storage_hex);

    // Enjin verifier side reads:
    // tok.supply -> true max JIN Token supply after melts
    // tok.infusion/backing-like field -> per-token ENJ infusion raw
    //
    // In the GUI we avoid backend usage, so this is direct Matrixchain RPC + raw storage decode.
    // We scan both little-endian u128 and SCALE compact values.
    let mut values = Vec::<u128>::new();

    for off in 0..bytes.len() {
        if let Some(v) = read_scale_compact_u128_at(&bytes, off) {
            values.push(v);
        }

        if off + 16 <= bytes.len() {
            if let Some(v) = read_u128_le_at(&bytes, off) {
                values.push(v);
            }
        }
    }

    // The old decoder accidentally picked the INITIAL_MAX_SUPPLY value when it existed in storage.
    // Correct behavior: prefer the largest plausible current supply BELOW initial max.
    let true_supply_below_initial = values
        .iter()
        .copied()
        .filter(|v| *v >= 1_000_000u128 && *v < ENJIN_MATRIX_JIN_INITIAL_MAX_SUPPLY)
        .max();

    let true_supply = true_supply_below_initial
        .or_else(|| {
            values
                .iter()
                .copied()
                .filter(|v| *v >= 1_000_000u128 && *v <= ENJIN_MATRIX_JIN_INITIAL_MAX_SUPPLY)
                .max()
        })
        .unwrap_or(ENJIN_MATRIX_JIN_INITIAL_MAX_SUPPLY);

    let melted = ENJIN_MATRIX_JIN_INITIAL_MAX_SUPPLY.saturating_sub(true_supply);

    // Per-JIN infusion raw ENJ units.
    // 1 ENJ = 10^18 raw.
    // Current JIN is around 0.006393 ENJ/JIN, so this range rejects tiny counters
    // and rejects huge total-backing values.
    let per_jin_infusion_raw = values
        .iter()
        .copied()
        .filter(|v| {
            *v >= 1_000_000_000_000_000u128
                && *v <= 1_000_000_000_000_000_000u128
        })
        .min()
        .unwrap_or(0);

    let total_infused_raw = true_supply.saturating_mul(per_jin_infusion_raw);

    let per_jin_infusion = if per_jin_infusion_raw == 0 {
        "0 ENJ/JIN".to_string()
    } else {
        format!("{}/JIN", format_enj_raw(per_jin_infusion_raw))
    };

    EnjinMatrixMetrics {
        melted_jin_supply: format!("{} JIN", format_integer_with_commas(melted)),
        true_max_jin_supply: format!("{} JIN", format_integer_with_commas(true_supply)),
        total_infused_enj: format_enj_raw(total_infused_raw),
        per_jin_infusion,
        last_status: "Matrixchain RPC fetched directly; decoded JIN Token supply and per-token ENJ infusion".to_string(),
        updated_at: Some(Instant::now()),
    }
}

#[cfg(target_os = "windows")]
fn fetch_enjin_matrix_metrics_direct() -> Result<EnjinMatrixMetrics> {
    let mut last_err = String::new();
    for rpc in ENJIN_MATRIX_RPC_URLS {
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"state_getStorage","params":["{}"]}}"#,
            ENJIN_MATRIX_JIN_TOKEN_STORAGE_KEY
        );
        let cmd = format!(
            "$body = '{}'; $r = Invoke-RestMethod -Uri '{}' -Method Post -ContentType 'application/json' -Body $body -TimeoutSec 8; $r | ConvertTo-Json -Compress -Depth 20",
            body.replace("'", "''"),
            rpc.replace("'", "''")
        );
        match powershell_json(&cmd) {
            Ok(v) => {
                if let Some(err) = v.get("error") {
                    last_err = format!("{} returned error: {}", rpc, err);
                    continue;
                }
                if let Some(hex) = v.get("result").and_then(|x| x.as_str()) {
                    if hex.trim().is_empty() || hex == "0x" {
                        last_err = format!("{} returned empty JIN token storage", rpc);
                        continue;
                    }
                    return Ok(best_effort_jin_tokenomics_from_storage_hex(hex));
                }
                last_err = format!("{} returned no result", rpc);
            }
            Err(err) => last_err = format!("{} failed: {err:#}", rpc),
        }
    }
    anyhow::bail!(last_err)
}

#[cfg(not(target_os = "windows"))]
fn fetch_enjin_matrix_metrics_direct() -> Result<EnjinMatrixMetrics> {
    anyhow::bail!("Matrixchain direct RPC metrics are only fetched by the Windows GUI build")
}


fn default_fee_for_config(config_path: &str) -> Option<String> {
    let settings = load_gui_settings(config_path).ok()?;
    Amount::from_atoms(settings.mempool.min_relay_fee_atoms).ok().map(|a| a.to_string())
}

fn normalize_gui_recipient_input(input: &str) -> String {
    let cleaned = input
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .chars()
        .filter(|c| !matches!(*c, '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}'))
        .collect::<String>();
    if cleaned.to_ascii_lowercase().starts_with("qub1") || cleaned.to_ascii_lowercase().starts_with("tqub1") {
        cleaned.to_ascii_lowercase()
    } else {
        cleaned
    }
}

fn resolve_recipient_for_gui(config_path: &str, recipient: &str) -> std::result::Result<String, String> {
    let settings = load_gui_settings(config_path).map_err(|err| format!("could not load config: {err:#}"))?;
    let chain = load_or_init_chain(&settings).map_err(|err| format!("could not load chain: {err:#}"))?;
    let cleaned = normalize_gui_recipient_input(recipient);
    let trimmed = cleaned.trim();
    if trimmed.to_ascii_lowercase().ends_with(".qub") {
        let rec = qns_resolve(&settings, &chain, trimmed)
            .map_err(|err| format!("{err:#}"))?
            .ok_or_else(|| format!("QNS name not found: {trimmed}"))?;
        return Ok(rec.address);
    }
    Address::parse_with_prefix(trimmed, &settings.network.address_prefix)
        .map(|a| a.to_string())
        .map_err(|err| format!("{err:#}"))
}

fn validate_recipient_address(config_path: &str, recipient: &str) -> std::result::Result<(), String> {
    resolve_recipient_for_gui(config_path, recipient).map(|_| ())
}

fn validate_send_form(config_path: &str, recipient: &str, amount: &str, fee: &str, asset: &str, fee_asset: &str) -> std::result::Result<(), String> {
    validate_recipient_address(config_path, recipient)?;
    if asset.eq_ignore_ascii_case("JIN") {
        let amount = parse_jin_amount(amount.trim()).map_err(|err| format!("invalid JIN amount: {err:#}"))?;
        if amount == 0 { return Err("amount must be greater than 0".to_string()); }
        if fee_asset.eq_ignore_ascii_case("JIN") {
            let fee = parse_jin_amount(fee.trim()).map_err(|err| format!("invalid JIN fee: {err:#}"))?;
            if fee == 0 { return Err("fee must be greater than 0".to_string()); }
        } else {
            let fee = Amount::from_str(fee.trim()).map_err(|err| format!("invalid QUB fee: {err:#}"))?;
            if fee.atoms() == 0 { return Err("fee must be greater than 0".to_string()); }
        }
    } else {
        let amount = Amount::from_str(amount.trim()).map_err(|err| format!("invalid amount: {err:#}"))?;
        let fee = Amount::from_str(fee.trim()).map_err(|err| format!("invalid fee: {err:#}"))?;
        if amount.atoms() == 0 { return Err("amount must be greater than 0".to_string()); }
        if fee.atoms() == 0 { return Err("fee must be greater than 0".to_string()); }
    }
    Ok(())
}


fn jin_sale_listings_for_gui(config_path: &str) -> Result<Vec<JinSaleUiListing>> {
    let settings = load_gui_settings(config_path)?;
    let chain = load_or_init_chain(&settings)?;
    let listings = jin_sale_listings(&settings, &chain)?;
    Ok(listings.into_iter().map(|l| JinSaleUiListing {
        listing_id: l.listing_id,
        price_qub_per_jin: Amount::from_atoms(l.price_atoms_per_jin).map(|a| a.to_string()).unwrap_or_else(|_| l.price_atoms_per_jin.to_string()),
        total_jin: format_jin_amount(l.total_units),
        sold_jin: format_jin_amount(l.sold_units),
        remaining_jin: format_jin_amount(l.remaining_units),
        remaining_units: l.remaining_units,
    }).collect())
}

fn jin_swap_sale_price_atoms_for_ui(config_path: &str, listing_id: u32, amount_units: u128) -> Result<u64> {
    let settings = load_gui_settings(config_path)?;
    jin_swap_sale_price_atoms(&settings, listing_id, amount_units)
}

fn jin_swap_fee_split_for_ui(config_path: &str, price_atoms: u64) -> Result<(u64, u64)> {
    let settings = load_gui_settings(config_path)?;
    jin_swap_fee_split_atoms(&settings, price_atoms)
}


fn hf79_pre_tx_fast_sync(settings: &Settings) {
    if !settings.p2p.enabled { return; }
    // QUB Core/v1.6.9: transaction builders run in worker threads. Use the same
    // bounded self-healing catch-up path as mining so JIN buys/sends are signed
    // against a fresh ledger even when the local node was parked behind.
    let _ = p2p::hf90_auto_catchup(settings, 45_000);
    let _ = p2p::rebroadcast_local_mempool(settings, 8);
}

fn execute_gui_buy_jin(config_path: &str, listing_id: &str, amount_jin: &str, fee: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    hf79_pre_tx_fast_sync(&settings);
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let listing_id = listing_id.trim().parse::<u32>()?;
    let units = parse_jin_amount(amount_jin.trim())?;
    let tx = wallet.create_jin_public_sale_buy_transaction(&chain, &settings, listing_id, units, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let mut relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    // HF113: JIN buys are high-value UX actions. Keep them hot in the official
    // relay path immediately after creation so they are not silently lost behind
    // ordinary mempool traffic during fast block races.
    for _ in 0..3 {
        relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 64).unwrap_or(0));
        thread::sleep(Duration::from_millis(180));
    }
    Ok((txid, relayed, chain.mempool.len()))
}

fn execute_gui_send_transaction(config_path: &str, recipient: &str, amount: &str, fee: &str, asset: &str, fee_asset: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    hf79_pre_tx_fast_sync(&settings);
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let recipient_clean = normalize_gui_recipient_input(recipient);
    let to = if recipient_clean.trim().to_ascii_lowercase().ends_with(".qub") {
        let rec = qns_resolve(&settings, &chain, recipient_clean.trim())?.with_context(|| format!("QNS name not found: {}", recipient_clean.trim()))?;
        Address::parse_with_prefix(&rec.address, &settings.network.address_prefix)?
    } else {
        Address::parse_with_prefix(recipient_clean.trim(), &settings.network.address_prefix)?
    };
    let tx = if asset.eq_ignore_ascii_case("JIN") {
        let fee_asset = fee_asset.trim().to_ascii_uppercase();
        let (qub_fee, jin_fee_units) = if fee_asset == "QUB" { (Amount::from_str(fee.trim())?, 0u128) } else { (Amount::from_atoms(0)?, parse_jin_amount(fee.trim())?) };
        wallet.create_jin_transfer_transaction(&chain, &settings, &to, parse_jin_amount(amount.trim())?, qub_fee, jin_fee_units, &fee_asset)?
    } else {
        wallet.create_signed_transaction(
            &chain,
            &settings,
            &to,
            Amount::from_str(amount.trim())?,
            Amount::from_str(fee.trim())?,
        )?
    };
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok((txid, relayed, chain.mempool.len()))
}


fn parse_gui_multi_entries(settings: &Settings, chain: &ChainState, entries: &str, asset: &str) -> Result<Vec<(Address, String)>> {
    let mut out = Vec::new();
    for raw in entries.lines() {
        let raw = raw.trim();
        if raw.is_empty() { continue; }
        let pair = raw.split_once(',').or_else(|| raw.split_once(':')).context("each multi-send line must be address_or_qns,amount")?;
        let to_clean = normalize_gui_recipient_input(pair.0);
        let to = to_clean.trim();
        let amount = pair.1.trim();
        if amount.is_empty() { anyhow::bail!("multi-send line has empty amount"); }
        let addr = if to.to_ascii_lowercase().ends_with(".qub") {
            let rec = qns_resolve(settings, chain, to)?.with_context(|| format!("QNS name not found: {to}"))?;
            Address::parse_with_prefix(&rec.address, &settings.network.address_prefix)?
        } else {
            Address::parse_with_prefix(to, &settings.network.address_prefix)?
        };
        out.push((addr, amount.to_string()));
    }
    if out.is_empty() { anyhow::bail!("multi-send needs at least one entry"); }
    if out.len() > MAX_SEND_ENTRIES_PER_TX { anyhow::bail!("multi-send supports at most {} entries", MAX_SEND_ENTRIES_PER_TX); }
    if !asset.eq_ignore_ascii_case("QUB") && !asset.eq_ignore_ascii_case("JIN") { anyhow::bail!("asset must be QUB or JIN"); }
    Ok(out)
}

fn execute_gui_multi_send_transaction(config_path: &str, entries: &str, fee: &str, asset: &str, fee_asset: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    hf79_pre_tx_fast_sync(&settings);
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let asset = asset.trim().to_ascii_uppercase();
    let parsed = parse_gui_multi_entries(&settings, &chain, entries, &asset)?;
    let tx = if asset == "JIN" {
        let payments = parsed.iter().map(|(a, amt)| parse_jin_amount(amt).map(|u| (a.clone(), u))).collect::<Result<Vec<_>>>()?;
        let fee_asset = fee_asset.trim().to_ascii_uppercase();
        let (qub_fee, jin_fee_units) = if fee_asset == "QUB" { (Amount::from_str(fee.trim())?, 0u128) } else { (Amount::from_atoms(0)?, parse_jin_amount(fee.trim())?) };
        wallet.create_jin_multi_transfer_transaction(&chain, &settings, &payments, qub_fee, jin_fee_units, &fee_asset)?
    } else {
        let payments = parsed.iter().map(|(a, amt)| Amount::from_str(amt).map(|q| (a.clone(), q))).collect::<Result<Vec<_>>>()?;
        wallet.create_multi_signed_transaction(&chain, &settings, &payments, Amount::from_str(fee.trim())?)?
    };
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok((txid, relayed, chain.mempool.len()))
}

fn compute_blast_max_claims_text(total: &str, per_claim: &str) -> Result<String> {
    let total = Amount::from_str(total.trim())?;
    let per_claim = Amount::from_str(per_claim.trim())?;
    if per_claim.atoms() == 0 { anyhow::bail!("per-claim amount must be greater than zero"); }
    let remainder = total.atoms() % per_claim.atoms();
    if remainder != 0 { anyhow::bail!("total QUB must divide evenly by QUB per claim in Blast v1"); }
    let claims = total.atoms() / per_claim.atoms();
    if claims == 0 { anyhow::bail!("total amount must cover at least one claim"); }
    if claims > 256 { anyhow::bail!("Blast v1 supports at most 256 claims"); }
    Ok(claims.to_string())
}

fn blast_codes_file_for_settings(settings: &Settings) -> PathBuf {
    NodePaths::from_settings(settings).data_dir.join("blast-codes.json")
}

fn load_blast_code_records_for_gui(config_path: &str) -> Vec<BlastCodeRecord> {
    let Ok(settings) = load_gui_settings(config_path) else { return Vec::new(); };
    let path = blast_codes_file_for_settings(&settings);
    let Ok(raw) = std::fs::read_to_string(path) else { return Vec::new(); };
    serde_json::from_str::<Vec<BlastCodeRecord>>(&raw).unwrap_or_default()
}

fn save_blast_code_record_for_gui(config_path: &str, txid: &str, private_code: &str, claim_payload: &str) -> Result<()> {
    let settings = load_gui_settings(config_path)?;
    let path = blast_codes_file_for_settings(&settings);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let mut records = if let Ok(raw) = std::fs::read_to_string(&path) {
        serde_json::from_str::<Vec<BlastCodeRecord>>(&raw).unwrap_or_default()
    } else {
        Vec::new()
    };
    records.retain(|r| r.txid != txid || r.private_code != private_code);
    let saved_unix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    records.push(BlastCodeRecord {
        txid: txid.to_string(),
        private_code: private_code.to_string(),
        claim_payload: claim_payload.to_string(),
        saved_unix,
    });
    if records.len() > 256 {
        let keep_from = records.len() - 256;
        records = records.split_off(keep_from);
    }
    std::fs::write(path, serde_json::to_string_pretty(&records)?)?;
    Ok(())
}

fn generate_gui_blast_code() -> String {
    let secret = generate_secret_key();
    format!("b{}", secret_key_to_hex(&secret))
}

fn execute_gui_blast_create(config_path: &str, total: &str, per_claim: &str, max_claims: &str, private_code: &str, fee: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    hf79_pre_tx_fast_sync(&settings);
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let code = private_code.trim().to_string();
    let tx = wallet.create_blast_create_transaction_qub(&chain, &settings, Amount::from_str(total.trim())?, Amount::from_str(per_claim.trim())?, max_claims.trim().parse::<u32>()?, &code, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok((txid, relayed, chain.mempool.len()))
}

fn execute_gui_blast_claim(config_path: &str, claim_code: &str, claimant: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    hf79_pre_tx_fast_sync(&settings);
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let claimant = if claimant.trim().is_empty() { None } else if claimant.trim().to_ascii_lowercase().ends_with(".qub") {
        let rec = qns_resolve(&settings, &chain, claimant.trim())?.with_context(|| format!("QNS name not found: {}", claimant.trim()))?;
        Some(Address::parse_with_prefix(&rec.address, &settings.network.address_prefix)?)
    } else {
        Some(Address::parse_with_prefix(claimant.trim(), &settings.network.address_prefix)?)
    };
    let tx = wallet.create_blast_claim_transaction_qub(&chain, &settings, claim_code.trim(), claimant.as_ref())?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok((txid, relayed, chain.mempool.len()))
}


fn library_state_for_gui(config_path: &str) -> Result<LibraryState> {
    let settings = load_gui_settings(config_path)?;
    let chain = load_or_init_chain(&settings)?;
    library_state_from_blocks(&settings, &chain.blocks)
}

fn library_post_price_preview_for_gui(config_path: &str, title: &str, category: &str, body: &str) -> std::result::Result<String, String> {
    let settings = load_gui_settings(config_path).map_err(|err| format!("{err:#}"))?;
    let atoms = library_post_price_atoms(&settings, title, category, body).map_err(|err| format!("{err:#}"))?;
    let page_count = ((body.as_bytes().len().max(1) + settings.library.max_page_bytes - 1) / settings.library.max_page_bytes).max(1);
    Amount::from_atoms(atoms)
        .map(|a| format!("Library protocol fee to miner: {a} QUB. Pages: {} / max {}. Activation #{}.", page_count, settings.library.max_pages_per_post, library_activation_height(&settings)))
        .map_err(|err| format!("{err:#}"))
}

fn execute_gui_library_create(config_path: &str, title: &str, category: &str, body: &str, fee: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let tx = wallet.create_library_post_transaction(&chain, &settings, title, category, body, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok((txid, relayed, chain.mempool.len()))
}

fn execute_gui_library_comment(config_path: &str, post_id: &str, parent: Option<&str>, body: &str, fee: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let tx = wallet.create_library_comment_transaction(&chain, &settings, post_id, parent, body, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok((txid, relayed, chain.mempool.len()))
}

fn execute_gui_library_vote(config_path: &str, kind: &str, id: &str, up: bool, fee: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let tx = wallet.create_library_vote_transaction(&chain, &settings, kind, id, up, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok((txid, relayed, chain.mempool.len()))
}

fn execute_gui_library_edit(config_path: &str, kind: &str, id: &str, title: &str, category: &str, body: &str, fee: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let tx = wallet.create_library_edit_transaction(&chain, &settings, kind, id, title, category, body, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok((txid, relayed, chain.mempool.len()))
}

fn execute_gui_library_delete(config_path: &str, kind: &str, id: &str, fee: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let tx = wallet.create_library_delete_transaction(&chain, &settings, kind, id, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok((txid, relayed, chain.mempool.len()))
}

fn qns_price_for_gui(config_path: &str, name: &str) -> std::result::Result<String, String> {
    let settings = load_gui_settings(config_path).map_err(|err| format!("{err:#}"))?;
    let normalized = normalize_qns_name(name, settings.qns.max_label_chars).map_err(|err| format!("{err:#}"))?;
    let atoms = qns_registration_price_atoms(&settings, &normalized).map_err(|err| format!("{err:#}"))?;
    Amount::from_atoms(atoms).map(|a| format!("{a} QUB")).map_err(|err| format!("{err:#}"))
}

fn execute_gui_qns_register(config_path: &str, name: &str, target: &str, fee: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let target = Address::parse_with_prefix(target.trim(), &settings.network.address_prefix)?;
    let tx = wallet.create_qns_registration_transaction(&chain, &settings, name.trim(), &target, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok((txid, relayed, chain.mempool.len()))
}

fn execute_gui_jin_token_conversion(config_path: &str, matrix_address: &str, amount: &str, fee: &str, fee_asset: &str) -> Result<(String, usize, usize)> {
    let settings = load_gui_settings(config_path)?;
    hf79_pre_tx_fast_sync(&settings);
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let fee_asset = fee_asset.trim().to_ascii_uppercase();
    let (qub_fee, jin_fee_units) = if fee_asset == "QUB" {
        (Amount::from_str(fee.trim())?, 0u128)
    } else {
        (Amount::from_atoms(0)?, parse_jin_amount(fee.trim())?)
    };
    let tx = wallet.create_jin_token_conversion_transaction(&chain, &settings, matrix_address.trim(), parse_jin_amount(amount.trim())?, qub_fee, jin_fee_units, &fee_asset)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok((txid, relayed, chain.mempool.len()))
}

fn pool_create_price_preview(config_path: &str, capacity_slots: u32) -> String {
    match load_gui_settings(config_path).and_then(|settings| pool_create_price_atoms(&settings, capacity_slots).and_then(Amount::from_atoms)) {
        Ok(amount) => format!("Create cost: {} QUB for {} slots", amount, capacity_slots),
        Err(err) => format!("Create cost unavailable: {err:#}"),
    }
}

fn pool_topup_price_preview(config_path: &str, extra_slots: u32) -> String {
    match load_gui_settings(config_path).and_then(|settings| pool_topup_price_atoms(&settings, extra_slots).and_then(Amount::from_atoms)) {
        Ok(amount) => format!("Top-up cost: {} QUB for +{} slots", amount, extra_slots),
        Err(err) => format!("Top-up cost unavailable: {err:#}"),
    }
}

fn execute_gui_pool_create(config_path: &str, name: &str, commission_bps: &str, capacity_slots: u32, manager_address: &str, fee: &str) -> Result<PoolGuiEvent> {
    let settings = load_gui_settings(config_path)?;
    if settings.p2p.enabled { let _ = p2p::sync_until_converged(&settings, 3, 250); }
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.is_empty() { anyhow::bail!("wallet has no local private key"); }
    let manager = if manager_address.trim().is_empty() {
        Address::parse_with_prefix(wallet.default_address().context("wallet empty")?, &settings.network.address_prefix)?
    } else {
        Address::parse_with_prefix(manager_address.trim(), &settings.network.address_prefix)?
    };
    let normalized = normalize_pool_name(name, settings.pools.max_name_chars, settings.pools.max_name_bytes)?;
    let bps = commission_bps.trim().parse::<u16>()?;
    let price = pool_create_price_atoms(&settings, capacity_slots)?;
    let tx = wallet.create_pool_create_transaction(&chain, &settings, &normalized, &manager, bps, capacity_slots, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok(PoolGuiEvent::Created {
        action: "create".to_string(),
        txid: txid.clone(),
        pool_id: txid.clone(),
        relayed_to_peers: relayed,
        local_mempooltx: chain.mempool.len(),
        message: format!("Pool '{}' pending. Cost {} QUB, 50/50 protocol/miner split. Relayed to {} peer(s).", normalized, Amount::from_atoms(price)?, relayed),
    })
}

fn execute_gui_pool_topup(config_path: &str, pool_id: &str, extra_slots: u32, fee: &str) -> Result<PoolGuiEvent> {
    let settings = load_gui_settings(config_path)?;
    if settings.p2p.enabled { let _ = p2p::sync_until_converged(&settings, 3, 250); }
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    let pool_id_h = Hash256::from_hex(pool_id.trim())?;
    let price = pool_topup_price_atoms(&settings, extra_slots)?;
    let tx = wallet.create_pool_topup_transaction(&chain, &settings, pool_id_h, extra_slots, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok(PoolGuiEvent::Created {
        action: "capacity top-up".to_string(),
        txid,
        pool_id: pool_id_h.to_string(),
        relayed_to_peers: relayed,
        local_mempooltx: chain.mempool.len(),
        message: format!("Capacity top-up +{} slots pending. Cost {} QUB. Relayed to {} peer(s).", extra_slots, Amount::from_atoms(price)?, relayed),
    })
}

fn execute_gui_pool_set_commission(config_path: &str, pool_id: &str, new_bps: &str, fee: &str) -> Result<PoolGuiEvent> {
    let settings = load_gui_settings(config_path)?;
    if settings.p2p.enabled { let _ = p2p::sync_until_converged(&settings, 3, 250); }
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    let pool_id_h = Hash256::from_hex(pool_id.trim())?;
    let bps = new_bps.trim().parse::<u16>()?;
    let tx = wallet.create_pool_set_commission_transaction(&chain, &settings, pool_id_h, bps, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok(PoolGuiEvent::Created {
        action: "commission decrease".to_string(),
        txid,
        pool_id: pool_id_h.to_string(),
        relayed_to_peers: relayed,
        local_mempooltx: chain.mempool.len(),
        message: format!("Commission decrease to {:.2}% pending. Relayed to {} peer(s).", bps as f64 / 100.0, relayed),
    })
}

fn execute_gui_pool_rename(config_path: &str, pool_id: &str, new_name: &str, fee: &str) -> Result<PoolGuiEvent> {
    let settings = load_gui_settings(config_path)?;
    if settings.p2p.enabled { let _ = p2p::sync_until_converged(&settings, 3, 250); }
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    let pool_id_h = Hash256::from_hex(pool_id.trim())?;
    let normalized = normalize_pool_name(new_name, settings.pools.max_name_chars, settings.pools.max_name_bytes)?;
    let tx = wallet.create_pool_rename_transaction(&chain, &settings, pool_id_h, &normalized, Amount::from_str(fee.trim())?)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), &settings)?.to_string();
    save_chain(&settings, &chain)?;
    let relayed = p2p::broadcast_tx(&settings, &tx).unwrap_or(0);
    let relayed = relayed.saturating_add(p2p::rebroadcast_local_mempool(&settings, 16).unwrap_or(0));
    Ok(PoolGuiEvent::Created {
        action: "rename".to_string(),
        txid,
        pool_id: pool_id_h.to_string(),
        relayed_to_peers: relayed,
        local_mempooltx: chain.mempool.len(),
        message: format!("Pool rename to '{}' pending. Relayed to {} peer(s).", normalized, relayed),
    })
}

fn wallet_key_for_gui_pool(settings: &Settings, wallet: &WalletFile, address: &str) -> Result<WalletKey> {
    let target = if address.trim().is_empty() {
        wallet.default_address().context("wallet empty; create/import a local key first")?.to_string()
    } else {
        Address::parse_with_prefix(address.trim(), &settings.network.address_prefix)?.to_string()
    };
    wallet.keys.iter().find(|k| k.address == target).cloned().with_context(|| format!("wallet does not contain private key for {target}"))
}

fn find_gui_pool_share_nonce(settings: &Settings, pool_id: Hash256, miner_address: &str, parent_height: u32, parent_hash: Hash256, start_nonce: u64) -> Result<u64> {
    let mut nonce = start_nonce;
    loop {
        if pool_share_meets_target(settings, pool_id, miner_address, parent_height, parent_hash, nonce)? { return Ok(nonce); }
        nonce = nonce.wrapping_add(1);
        if nonce == start_nonce { anyhow::bail!("pool share nonce space exhausted"); }
    }
}

fn create_gui_local_pool_share(settings: &Settings, chain: &mut ChainState, pool_id: Hash256, miner_key: &WalletKey, start_nonce: u64) -> Result<(Hash256, u64, usize)> {
    let registry = pools_registry_from_blocks(settings, &chain.blocks)?;
    if !registry.contains_key(&pool_id) { anyhow::bail!("unknown pool_id; create/confirm pool first"); }
    let parent_height = chain.height();
    let parent_hash = chain.tip_hash();
    let nonce = find_gui_pool_share_nonce(settings, pool_id, &miner_key.address, parent_height, parent_hash, start_nonce)?;
    let tx = create_pool_share_transaction(settings, pool_id, miner_key, parent_height, parent_hash, nonce)?;
    let txid = chain.accept_transaction_to_mempool(tx.clone(), settings)?;
    save_chain(settings, chain)?;
    let relayed = p2p::broadcast_tx(settings, &tx).unwrap_or(0);
    Ok((txid, nonce, relayed))
}

fn execute_gui_pool_join(config_path: &str, pool_id: &str, miner_address: &str) -> Result<PoolGuiEvent> {
    let settings = load_gui_settings(config_path)?;
    if settings.p2p.enabled { let _ = p2p::sync_until_converged(&settings, 3, 250); }
    let mut chain = load_or_init_chain(&settings)?;
    let wallet = load_or_init_wallet(&settings)?;
    let pool_id_h = Hash256::from_hex(pool_id.trim())?;
    let key = wallet_key_for_gui_pool(&settings, &wallet, miner_address)?;
    let (txid, nonce, relayed) = create_gui_local_pool_share(&settings, &mut chain, pool_id_h, &key, 0)?;
    Ok(PoolGuiEvent::Created {
        action: "join".to_string(),
        txid: txid.to_string(),
        pool_id: pool_id_h.to_string(),
        relayed_to_peers: relayed,
        local_mempooltx: chain.mempool.len(),
        message: format!("Pool share submitted with nonce {} and relayed to {} peer(s). It becomes active after confirmation in a block.", nonce, relayed),
    })
}

fn query_gui_tx_status(config_path: &str, txid: &str) -> Result<TxUiStatus> {
    let settings = load_gui_settings(config_path)?;
    let chain = load_or_init_chain(&settings)?;
    for (height, block) in chain.blocks.iter().enumerate().skip(1) {
        if block.transactions.iter().any(|tx| tx.txid().to_string() == txid) {
            let h = height as u32;
            let confirmations = chain.height().saturating_sub(h).saturating_add(1);
            return Ok(TxUiStatus::Confirmed { height: h, confirmations });
        }
    }
    if chain.mempool.iter().any(|tx| tx.txid().to_string() == txid) {
        let _ = p2p::rebroadcast_local_mempool(&settings, 64);
        return Ok(TxUiStatus::PendingMempool);
    }
    Ok(TxUiStatus::NotFound)
}

fn create_local_wallet_address(config_path: &str, accepted_plaintext_risk: bool) -> Result<String> {
    if accepted_plaintext_risk {
        std::env::set_var("QUB_ALLOW_PLAINTEXT_WALLET", "1");
    }
    let settings = load_gui_settings(config_path)?;
    let chain = load_or_init_chain(&settings)?;
    let mut wallet = load_or_init_wallet(&settings)?;
    let key = wallet.create_key(&settings, "gui-miner", chain.height())?;
    save_wallet(&settings, &wallet)?;
    Ok(key.address)
}

fn import_private_key_hex(config_path: &str, secret_hex: &str, label: &str) -> Result<String> {
    std::env::set_var("QUB_ALLOW_PLAINTEXT_WALLET", "1");
    let settings = load_gui_settings(config_path)?;
    let chain = load_or_init_chain(&settings)?;
    let secret = secret_key_from_hex(secret_hex.trim())?;
    let public = public_key_from_secret(&secret);
    let address = address_from_public_key(&settings.network.address_prefix, &public).to_string();
    let mut wallet = load_or_init_wallet(&settings)?;
    if wallet.keys.iter().any(|k| k.address == address) {
        anyhow::bail!("wallet already contains this address");
    }
    wallet.keys.push(WalletKey {
        address: address.clone(),
        public_key_hex: hex::encode(public.serialize()),
        secret_key_hex: secret_key_to_hex(&secret),
        label: if label.trim().is_empty() { "imported-gui-key".to_string() } else { label.trim().to_string() },
        created_height: chain.height(),
    });
    if wallet.default_index.is_none() { wallet.default_index = Some(wallet.keys.len().saturating_sub(1)); }
    save_wallet(&settings, &wallet)?;
    Ok(address)
}

fn delete_local_wallet_private_keys(config_path: &str) -> Result<usize> {
    let settings = load_gui_settings(config_path)?;
    let mut wallet = load_or_init_wallet(&settings)?;
    let deleted = wallet.keys.len();
    wallet.keys.clear();
    wallet.default_index = None;
    save_wallet(&settings, &wallet)?;
    Ok(deleted)
}


fn gpu_device_selectors_for_mining(selected: &str) -> Vec<String> {
    let selected = selected.trim();
    if selected.eq_ignore_ascii_case(gpu_miner::GPU_DEVICE_ALL_DETECTED) {
        // QUB Core/v1.6.9: optional experimental mode. This deliberately includes
        // integrated GPUs as well as discrete GPUs. It is useful for testing, but
        // on some hybrid laptops it can reduce dGPU boost because both devices
        // share the same thermal/power envelope.
        return match gpu_miner::available_gpu_device_labels() {
            Ok(labels) if !labels.is_empty() => labels,
            _ => vec![String::new()],
        };
    }
    if selected.is_empty() || selected.eq_ignore_ascii_case(gpu_miner::GPU_DEVICE_ALL) {
        // high-performance/v1.6.0: "All" means all high-performance GPUs by default. On
        // hybrid laptops this avoids letting the integrated OpenCL device drag
        // the discrete RTX device into a shared low-power scheduling path. The
        // integrated GPU is still visible in the dropdown and can be selected explicitly.
        match gpu_miner::preferred_gpu_device_labels() {
            Ok(labels) if !labels.is_empty() => labels,
            _ => match gpu_miner::available_gpu_device_labels() {
                Ok(labels) if !labels.is_empty() => labels,
                _ => vec![String::new()],
            },
        }
    } else {
        vec![selected.to_string()]
    }
}

fn run_miner(config_path: String, payout: String, cpu_percent: u8, gpu_percent: u8, gpu_device_selector: String, pace_to_target_spacing: bool, events: mpsc::Sender<MinerEvent>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match run_miner_inner(config_path.clone(), payout.clone(), cpu_percent, gpu_percent, gpu_device_selector.clone(), pace_to_target_spacing, &events, stop.clone()) {
            Ok(_) => break,
            Err(err) => {
                let _ = events.send(MinerEvent::Status(format!("Miner worker recovered from transient error: {err:#}. Retrying in 5s...")));
                for _ in 0..50 {
                    if stop.load(Ordering::Relaxed) { break; }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    stop.store(true, Ordering::Relaxed);
    let _ = events.send(MinerEvent::Stopped);
}

fn run_pool_miner(config_path: String, pool_id_s: String, miner_address: String, cpu_percent: u8, gpu_percent: u8, gpu_device_selector: String, events: mpsc::Sender<MinerEvent>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match run_pool_miner_inner(config_path.clone(), pool_id_s.clone(), miner_address.clone(), cpu_percent, gpu_percent, gpu_device_selector.clone(), &events, stop.clone()) {
            Ok(_) => break,
            Err(err) => {
                let _ = events.send(MinerEvent::Status(format!("Pool miner worker recovered from transient error: {err:#}. Retrying in 5s...")));
                for _ in 0..50 {
                    if stop.load(Ordering::Relaxed) { break; }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    stop.store(true, Ordering::Relaxed);
    let _ = events.send(MinerEvent::Stopped);
}

fn run_pool_miner_inner(config_path: String, pool_id_s: String, miner_address: String, cpu_percent: u8, gpu_percent: u8, gpu_device_selector: String, events: &mpsc::Sender<MinerEvent>, stop: Arc<AtomicBool>) -> Result<()> {
    let pool_id = Hash256::from_hex(&pool_id_s)?;
    let logical = logical_cpus();
    let (threads, duty) = resource_plan(logical, cpu_percent);
    let mut total_hashes = 0u64;
    let mut share_nonce_start = 0u64;

    while !stop.load(Ordering::Relaxed) {
        let mut settings = load_gui_settings(&config_path)?;
        settings.mining.enabled = true;
        settings.mining.miner_address = miner_address.clone();
        if settings.p2p.enabled {
            match p2p::mining_safety_check(&settings) {
                Ok(report) if report.peers_contacted > 0 => {
                    let _ = events.send(MinerEvent::Status(format!("Pool mining safety OK: peers={} adopted={} connected={} height=#{}", report.peers_contacted, report.chains_adopted, report.blocks_connected, report.height)));
                }
                Ok(_) => {}
                Err(err) => {
                    let _ = events.send(MinerEvent::Status(format!("Pool mining is waiting for the official green-light tip before hashing: {err:#}. Retrying...")));
                    for _ in 0..20 {
                        if stop.load(Ordering::Relaxed) { break; }
                        thread::sleep(Duration::from_millis(100));
                    }
                    continue;
                }
            }
        }

        let mut chain = load_or_init_chain(&settings)?;
        let wallet = load_or_init_wallet(&settings)?;
        let key = wallet_key_for_gui_pool(&settings, &wallet, &miner_address)?;
        if settings.p2p.enabled {
            match p2p::mining_parent_guard(&settings, chain.height(), chain.tip_hash()) {
                Ok(report) if report.peers_contacted > 0 => {
                    let _ = events.send(MinerEvent::Status(format!("Pool template guard OK: peers={} height=#{}", report.peers_contacted, report.height)));
                }
                Ok(_) => {}
                Err(err) => {
                    let _ = events.send(MinerEvent::Status(format!("Pool miner is refreshing the latest candidate from the official tip: {err:#}. Retrying...")));
                    for _ in 0..30 {
                        if stop.load(Ordering::Relaxed) { break; }
                        thread::sleep(Duration::from_millis(100));
                    }
                    continue;
                }
            }
        }
        match create_gui_local_pool_share(&settings, &mut chain, pool_id, &key, share_nonce_start) {
            Ok((share_txid, nonce, _relayed)) => {
                share_nonce_start = nonce.wrapping_add(1);
                let _ = events.send(MinerEvent::Status(format!("Pool share submitted: {} nonce={}", shorten_hash(&share_txid.to_string()), nonce)));
                chain = load_or_init_chain(&settings)?;
            }
            Err(err) => {
                let _ = events.send(MinerEvent::Status(format!("Pool share warning: {err:#}")));
            }
        }

        let scores = pool_share_scores_from_blocks(&settings, &chain.blocks, chain.height() + 1, pool_id);
        if scores.get(&key.address).copied().unwrap_or(0) == 0 {
            let _ = events.send(MinerEvent::Status("Waiting for your first pool share to confirm before mining pool blocks.".to_string()));
            for _ in 0..20 {
                if stop.load(Ordering::Relaxed) { break; }
                thread::sleep(Duration::from_millis(100));
            }
            continue;
        }

        let target_height = chain.height() + 1;
        let base_mempool_fingerprint = mempool_fingerprint(&chain);
        let _ = events.send(MinerEvent::Started { threads, duty, target_height });
        let round_started = Instant::now();
        let round_stop = Arc::new(AtomicBool::new(false));
        let hash_counter = Arc::new(AtomicU64::new(0));
        let gpu_hash_counter = Arc::new(AtomicU64::new(0));
        let gpu_enabled = gpu_percent > 0;
        let gpu_selectors = if gpu_enabled { gpu_device_selectors_for_mining(&gpu_device_selector) } else { Vec::new() };
        let gpu_device_count = if gpu_enabled { gpu_selectors.len().max(1) } else { 0 };
        let gpu_total_lanes = gpu_miner::initial_work_items(gpu_percent).saturating_mul(gpu_device_count);
        let mut gpu_total_hashes = 0u64;
        let (found_tx, found_rx) = mpsc::channel::<Block>();
        let mut joins = Vec::with_capacity(threads + gpu_device_count);

        for worker_id in 0..threads {
            let worker_chain = chain.clone();
            let worker_settings = settings.clone();
            let worker_found_tx = found_tx.clone();
            let worker_events = events.clone();
            let worker_stop = stop.clone();
            let worker_round_stop = round_stop.clone();
            let worker_hashes = hash_counter.clone();
            joins.push(thread::spawn(move || {
                pool_mine_worker(worker_id, threads, duty, worker_chain, worker_settings, pool_id, worker_hashes, worker_found_tx, worker_events, worker_stop, worker_round_stop);
            }));
        }
        if gpu_enabled {
            for (gpu_index, gpu_selector) in gpu_selectors.into_iter().enumerate() {
                let gpu_chain = chain.clone();
                let gpu_settings = settings.clone();
                let gpu_found_tx = found_tx.clone();
                let gpu_events = events.clone();
                let gpu_stop = stop.clone();
                let gpu_round_stop = round_stop.clone();
                let gpu_hashes = gpu_hash_counter.clone();
                joins.push(thread::spawn(move || {
                    gpu_pool_mine_worker(gpu_chain, gpu_settings, pool_id, gpu_percent, gpu_selector, gpu_index, target_height, gpu_hashes, gpu_found_tx, gpu_events, gpu_stop, gpu_round_stop);
                }));
            }
        }
        drop(found_tx);

        let mut last_tick = Instant::now();
        let mut last_network_check = Instant::now();
        let mut last_template_check = Instant::now();
        let mut found_block = None;
        while !stop.load(Ordering::Relaxed) && !round_stop.load(Ordering::Relaxed) {
            match found_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(block) => { found_block = Some(block); break; }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if last_template_check.elapsed() >= Duration::from_secs(4) {
                if let Ok(current) = load_or_init_chain(&settings) {
                    if current.tip_hash() != chain.tip_hash() {
                        round_stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    if mempool_fingerprint(&current) != base_mempool_fingerprint {
                        let _ = events.send(MinerEvent::Status("Mempool changed; rebuilding pool block template so pending txs are not left behind.".to_string()));
                        round_stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                last_template_check = Instant::now();
            }
            if settings.p2p.enabled && last_network_check.elapsed() >= Duration::from_secs(5) {
                if let Some(reason) = p2p::hf113_live_tip_pause_reason(&settings, chain.height(), chain.tip_hash(), 520) {
                    let _ = events.send(MinerEvent::Status(format!("Pool candidate paused immediately by HF113 canonical watcher: {reason}. Rebuilding after catch-up.")));
                    round_stop.store(true, Ordering::Relaxed);
                    break;
                }
                if let Ok(current) = load_or_init_chain(&settings) {
                    if current.tip_hash() != chain.tip_hash() {
                        round_stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                last_network_check = Instant::now();
            }
            if last_tick.elapsed() >= Duration::from_secs(1) {
                let elapsed = last_tick.elapsed().as_secs_f64().max(0.001);
                let delta = hash_counter.swap(0, Ordering::Relaxed);
                total_hashes = total_hashes.saturating_add(delta);
                let _ = events.send(MinerEvent::Hashrate { hps: delta as f64 / elapsed, total_hashes, threads, duty, target_height });
                if gpu_enabled {
                    // Per-device GPU workers now report their own measured rates.
                    // Drain the aggregate counter so it cannot overwrite the faster
                    // per-device telemetry with a stale/low controller estimate.
                    let _ = gpu_hash_counter.swap(0, Ordering::Relaxed);
                }
                last_tick = Instant::now();
            }
        }

        round_stop.store(true, Ordering::Relaxed);
        for join in joins { let _ = join.join(); }
        let delta = hash_counter.swap(0, Ordering::Relaxed);
        total_hashes = total_hashes.saturating_add(delta);
        let elapsed = round_started.elapsed().as_secs_f64().max(0.001);
        let _ = events.send(MinerEvent::Hashrate { hps: delta as f64 / elapsed, total_hashes, threads, duty, target_height });
        if gpu_enabled {
            let _ = gpu_hash_counter.swap(0, Ordering::Relaxed);
        }

        if stop.load(Ordering::Relaxed) { break; }
        if let Some(block) = found_block {
            let candidate_parent_hash = block.header.prev_block_hash;
            let candidate_parent_height = target_height.saturating_sub(1);
            if settings.p2p.enabled {
                if let Err(err) = p2p::mining_parent_submit_guard(&settings, candidate_parent_height, candidate_parent_hash) {
                    let _ = events.send(MinerEvent::Status(format!("Pool block discarded before submit by the HF113 fast canonical submit guard: {err:#}")));
                    continue;
                }
            }
            let mut active = load_or_init_chain(&settings)?;
            if active.height() != candidate_parent_height || active.tip_hash() != candidate_parent_hash {
                let _ = events.send(MinerEvent::Status("Tip changed while pool mining; candidate discarded before submit.".to_string()));
                continue;
            }
            let reward_atoms = block.transactions.first().map(|tx| tx.outputs.iter().map(|out| out.value.atoms()).sum::<u64>()).unwrap_or(0);
            let txs = block.transactions.len();
            let relay_block = block.clone();
            let hash = active.connect_block(block, &settings)?;
            let height = active.height();
            save_chain(&settings, &active)?;
            if settings.p2p.enabled {
                match p2p::broadcast_block(&settings, &relay_block) {
                    Ok(sent) if sent > 0 => { let _ = events.send(MinerEvent::Status(format!("Pool block relayed to {sent} peer(s)."))); }
                    Ok(_) => {}
                    Err(err) => { let _ = events.send(MinerEvent::Status(format!("P2P relay warning: {err:#}"))); }
                }
            }
            if settings.p2p.enabled {
                if let Ok(report) = p2p::sync_until_converged(&settings, 2, 150) {
                    if report.chains_adopted > 0 || report.blocks_connected > 0 {
                        let _ = events.send(MinerEvent::Status(format!("Network selected tip #{}; pool block checked against canonical chain.", report.height)));
                    }
                }
            }
            let jin_reward_units = block_jin_fee_units(&settings, &relay_block);
            let reward = format!("{} QUB + {} JIN", Amount::from_atoms(reward_atoms)?, format_jin_amount(jin_reward_units));
            let active_chain = load_or_init_chain(&settings)?;
            let active_hash_at_height = active_chain.blocks.get(height as usize).map(|block| block.block_hash());
            if active_hash_at_height == Some(hash) {
                let _ = events.send(MinerEvent::BlockFound { height, hash: hash.to_string(), txs, reward });
            } else {
                let winner_hash = active_hash_at_height
                    .map(|h| h.to_string())
                    .unwrap_or_else(|| active_chain.tip_hash().to_string());
                let _ = events.send(MinerEvent::BlockStale { height, hash: hash.to_string(), winner_hash });
            }
        } else {
            let _ = events.send(MinerEvent::Status("Pool mining latest chain tip...".to_string()));
        }
    }
    Ok(())
}

fn run_miner_inner(config_path: String, payout: String, cpu_percent: u8, gpu_percent: u8, gpu_device_selector: String, _pace_to_target_spacing: bool, events: &mpsc::Sender<MinerEvent>, stop: Arc<AtomicBool>) -> Result<()> {
    let logical = logical_cpus();
    let (threads, duty) = resource_plan(logical, cpu_percent);
    let mut total_hashes = 0u64;
    let mut gpu_total_hashes = 0u64;

    while !stop.load(Ordering::Relaxed) {
        let mut settings = load_gui_settings(&config_path)?;
        settings.mining.enabled = true;
        settings.mining.miner_address = payout.clone();
        if settings.p2p.enabled {
            match p2p::mining_safety_check(&settings) {
                Ok(report) if report.peers_contacted > 0 => {
                    let _ = events.send(MinerEvent::Status(format!("Mining safety OK: peers={} adopted={} connected={} height=#{}", report.peers_contacted, report.chains_adopted, report.blocks_connected, report.height)));
                }
                Ok(_) => {}
                Err(err) => {
                    let _ = events.send(MinerEvent::Status(format!("Mining is waiting for the official green-light tip before hashing: {err:#}. Retrying...")));
                    for _ in 0..20 {
                        if stop.load(Ordering::Relaxed) { break; }
                        thread::sleep(Duration::from_millis(100));
                    }
                    continue;
                }
            }
        }
        let base_chain = load_or_init_chain(&settings)?;
        if settings.p2p.enabled {
            match p2p::mining_parent_guard(&settings, base_chain.height(), base_chain.tip_hash()) {
                Ok(report) if report.peers_contacted > 0 => {
                    let _ = events.send(MinerEvent::Status(format!("Template guard OK: peers={} height=#{}", report.peers_contacted, report.height)));
                }
                Ok(_) => {}
                Err(err) => {
                    let _ = events.send(MinerEvent::Status(format!("Miner is refreshing the latest green-light candidate: {err:#}. Retrying...")));
                    for _ in 0..30 {
                        if stop.load(Ordering::Relaxed) { break; }
                        thread::sleep(Duration::from_millis(100));
                    }
                    continue;
                }
            }
        }
        let miner = Address::parse_with_prefix(&payout, &settings.network.address_prefix)?;
        let target_height = base_chain.height() + 1;
        let base_mempool_fingerprint = mempool_fingerprint(&base_chain);
        let _ = events.send(MinerEvent::Started { threads, duty, target_height });
        let round_started = Instant::now();
        let round_stop = Arc::new(AtomicBool::new(false));
        let hash_counter = Arc::new(AtomicU64::new(0));
        let (found_tx, found_rx) = mpsc::channel::<Block>();
        let mut joins = Vec::with_capacity(threads);

        for worker_id in 0..threads {
            let worker_chain = base_chain.clone();
            let worker_settings = settings.clone();
            let worker_miner = miner.clone();
            let worker_found_tx = found_tx.clone();
            let worker_events = events.clone();
            let worker_stop = stop.clone();
            let worker_round_stop = round_stop.clone();
            let worker_hashes = hash_counter.clone();
            joins.push(thread::spawn(move || {
                mine_worker(
                    worker_id,
                    threads,
                    duty,
                    worker_chain,
                    worker_settings,
                    worker_miner,
                    worker_hashes,
                    worker_found_tx,
                    worker_events,
                    worker_stop,
                    worker_round_stop,
                );
            }));
        }

        let gpu_hash_counter = Arc::new(AtomicU64::new(0));
        let gpu_enabled = gpu_percent > 0;
        let gpu_selectors = if gpu_enabled { gpu_device_selectors_for_mining(&gpu_device_selector) } else { Vec::new() };
        let gpu_device_count = if gpu_enabled { gpu_selectors.len().max(1) } else { 0 };
        let gpu_total_lanes = gpu_miner::initial_work_items(gpu_percent).saturating_mul(gpu_device_count);
        if gpu_enabled {
            for (gpu_index, gpu_selector) in gpu_selectors.into_iter().enumerate() {
                let gpu_chain = base_chain.clone();
                let gpu_settings = settings.clone();
                let gpu_miner_addr = miner.clone();
                let gpu_found_tx = found_tx.clone();
                let gpu_events = events.clone();
                let gpu_stop = stop.clone();
                let gpu_round_stop = round_stop.clone();
                let gpu_hashes = gpu_hash_counter.clone();
                joins.push(thread::spawn(move || {
                    gpu_mine_worker(
                        gpu_chain,
                        gpu_settings,
                        gpu_miner_addr,
                        gpu_percent,
                        gpu_selector,
                        gpu_index,
                        target_height,
                        gpu_hashes,
                        gpu_found_tx,
                        gpu_events,
                        gpu_stop,
                        gpu_round_stop,
                    );
                }));
            }
        }
        drop(found_tx);

        let mut last_tick = Instant::now();
        let mut last_network_check = Instant::now();
        let mut last_template_check = Instant::now();
        let mut found_block = None;
        while !stop.load(Ordering::Relaxed) && !round_stop.load(Ordering::Relaxed) {
            match found_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(block) => {
                    found_block = Some(block);
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if last_template_check.elapsed() >= Duration::from_secs(4) {
                if let Ok(current) = load_or_init_chain(&settings) {
                    if current.tip_hash() != base_chain.tip_hash() {
                        round_stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    if mempool_fingerprint(&current) != base_mempool_fingerprint {
                        let _ = events.send(MinerEvent::Status("Mempool changed; rebuilding block template so pending txs are picked up quickly.".to_string()));
                        round_stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                last_template_check = Instant::now();
            }
            if settings.p2p.enabled && last_network_check.elapsed() >= Duration::from_secs(5) {
                if let Some(reason) = p2p::hf113_live_tip_pause_reason(&settings, base_chain.height(), base_chain.tip_hash(), 520) {
                    let _ = events.send(MinerEvent::Status(format!("Mining candidate paused immediately by HF113 canonical watcher: {reason}. Rebuilding after catch-up.")));
                    round_stop.store(true, Ordering::Relaxed);
                    break;
                }
                if let Ok(current) = load_or_init_chain(&settings) {
                    if current.tip_hash() != base_chain.tip_hash() {
                        round_stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                last_network_check = Instant::now();
            }
            if last_tick.elapsed() >= Duration::from_secs(1) {
                let elapsed = last_tick.elapsed().as_secs_f64().max(0.001);
                let delta = hash_counter.swap(0, Ordering::Relaxed);
                total_hashes = total_hashes.saturating_add(delta);
                let _ = events.send(MinerEvent::Hashrate { hps: delta as f64 / elapsed, total_hashes, threads, duty, target_height });
                if gpu_enabled {
                    // Per-device GPU workers now report their own measured rates.
                    // Drain the aggregate counter so it cannot overwrite the faster
                    // per-device telemetry with a stale/low controller estimate.
                    let _ = gpu_hash_counter.swap(0, Ordering::Relaxed);
                }
                last_tick = Instant::now();
            }
        }

        round_stop.store(true, Ordering::Relaxed);
        for join in joins { let _ = join.join(); }
        let delta = hash_counter.swap(0, Ordering::Relaxed);
        total_hashes = total_hashes.saturating_add(delta);
        let elapsed = round_started.elapsed().as_secs_f64().max(0.001);
        let _ = events.send(MinerEvent::Hashrate { hps: delta as f64 / elapsed, total_hashes, threads, duty, target_height });
        if gpu_enabled {
            let _ = gpu_hash_counter.swap(0, Ordering::Relaxed);
        }

        if stop.load(Ordering::Relaxed) { break; }
        if let Some(block) = found_block {
            let candidate_parent_hash = block.header.prev_block_hash;
            let candidate_parent_height = target_height.saturating_sub(1);
            if settings.p2p.enabled {
                if let Err(err) = p2p::mining_parent_submit_guard(&settings, candidate_parent_height, candidate_parent_hash) {
                    let _ = events.send(MinerEvent::Status(format!("Block discarded before submit by the HF113 fast canonical submit guard: {err:#}")));
                    continue;
                }
            }
            let mut chain = load_or_init_chain(&settings)?;
            if chain.height() != candidate_parent_height || chain.tip_hash() != candidate_parent_hash {
                let _ = events.send(MinerEvent::Status("Tip changed while mining; candidate discarded before submit.".to_string()));
                continue;
            }
            let reward_atoms = block.transactions.first()
                .map(|tx| tx.outputs.iter().map(|out| out.value.atoms()).sum::<u64>())
                .unwrap_or(0);
            let txs = block.transactions.len();
            let relay_block = block.clone();
            let hash = chain.connect_block(block, &settings)?;
            let height = chain.height();
            save_chain(&settings, &chain)?;
            if settings.p2p.enabled {
                match p2p::broadcast_block(&settings, &relay_block) {
                    Ok(sent) if sent > 0 => { let _ = events.send(MinerEvent::Status(format!("Block relayed to {sent} peer(s)."))); }
                    Ok(_) => {}
                    Err(err) => { let _ = events.send(MinerEvent::Status(format!("P2P relay warning: {err:#}"))); }
                }
                if let Ok(report) = p2p::sync_until_converged(&settings, 2, 150) {
                    if report.chains_adopted > 0 || report.blocks_connected > 0 {
                        let _ = events.send(MinerEvent::Status(format!("Network selected tip #{}; local chain reconciled.", report.height)));
                    }
                }
            }
            let jin_reward_units = block_jin_fee_units(&settings, &relay_block);
            let reward = format!("{} QUB + {} JIN", Amount::from_atoms(reward_atoms)?, format_jin_amount(jin_reward_units));
            let active_chain = load_or_init_chain(&settings)?;
            let active_hash_at_height = active_chain.blocks.get(height as usize).map(|block| block.block_hash());
            if active_hash_at_height == Some(hash) {
                let _ = events.send(MinerEvent::BlockFound { height, hash: hash.to_string(), txs, reward });
            } else {
                let winner_hash = active_hash_at_height
                    .map(|h| h.to_string())
                    .unwrap_or_else(|| active_chain.tip_hash().to_string());
                let _ = events.send(MinerEvent::BlockStale { height, hash: hash.to_string(), winner_hash });
            }
        } else {
            let _ = events.send(MinerEvent::Status("Mining latest chain tip...".to_string()));
        }
    }
    Ok(())
}

fn wait_for_target_spacing(chain: &ChainState, settings: &Settings, events: &mpsc::Sender<MinerEvent>, stop: &Arc<AtomicBool>) -> Result<()> {
    if chain.height() == 0 { return Ok(()); }
    let Some(tip) = chain.blocks.last() else { return Ok(()); };
    let jitter_secs = if settings.p2p.enabled && !settings.mining.miner_address.trim().is_empty() {
        // Spread GUI miners across a small per-height slot window so LAN/testnet miners do not
        // all start hashing the same height in the same second. This is only mining policy, not consensus.
        let mut seed = Vec::new();
        seed.extend_from_slice(settings.mining.miner_address.as_bytes());
        seed.extend_from_slice(&chain.height().saturating_add(1).to_le_bytes());
        let slot_window = (settings.consensus.target_spacing_secs / 3).clamp(4, 20);
        Hash256::double_sha256(&seed).0[0] as u32 % slot_window
    } else { 0 };
    let next_allowed = tip.header.time.saturating_add(settings.consensus.target_spacing_secs).saturating_add(jitter_secs);
    loop {
        if stop.load(Ordering::Relaxed) { return Ok(()); }
        let now = unix_time_u32();
        if now >= next_allowed { return Ok(()); }
        let remaining = next_allowed.saturating_sub(now);
        let _ = events.send(MinerEvent::Status(format!("Target spacing: next block opens in {remaining}s.")));
        if settings.p2p.enabled {
            if let Ok(report) = p2p::sync_until_converged(settings, 2, 150) {
                if report.chains_adopted > 0 || report.blocks_connected > 0 || report.height > chain.height() {
                    let _ = events.send(MinerEvent::Status(format!("Network tip moved to #{} during wait.", report.height)));
                    return Ok(());
                }
            }
        }
        for _ in 0..10 {
            if stop.load(Ordering::Relaxed) { return Ok(()); }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn gpu_mine_worker(
    base_chain: ChainState,
    settings: Settings,
    miner: Address,
    gpu_percent: u8,
    gpu_selector: String,
    gpu_index: usize,
    target_height: u32,
    hash_counter: Arc<AtomicU64>,
    found_tx: mpsc::Sender<Block>,
    events: mpsc::Sender<MinerEvent>,
    stop: Arc<AtomicBool>,
    round_stop: Arc<AtomicBool>,
) {
    let mut backend = match gpu_miner::OpenClGpuMiner::new_for_selector(&gpu_selector) {
        Ok(backend) => backend,
        Err(err) => {
            let _ = events.send(MinerEvent::GpuStatus(format!("OpenCL GPU unavailable for selector {}. CPU mining continues. {err:#}", if gpu_selector.trim().is_empty() { gpu_miner::GPU_DEVICE_ALL } else { gpu_selector.trim() })));
            return;
        }
    };

    let device = backend.device_name().to_string();
    let workers = gpu_miner::initial_work_items(gpu_percent);
    let _ = events.send(MinerEvent::GpuStarted { device: device.clone(), workers, power: gpu_percent, target_height });

    let mut extra_nonce = 9_000_000_000u64.saturating_add((gpu_index as u64).saturating_mul(100_000_000));
    let mut local_total_hashes = 0u64;
    let mut last_report = Instant::now();
    let mut last_report_total = 0u64;
    let mut tune_candidates = gpu_miner::tuning_work_item_candidates(gpu_percent);
    if tune_candidates.is_empty() { tune_candidates.push(gpu_miner::initial_work_items(gpu_percent)); }
    let mut tune_index = 0usize;
    let mut best_batch = gpu_miner::initial_work_items(gpu_percent);
    let mut best_hps = 0.0f64;
    let mut tuning_announced = false;
    while !stop.load(Ordering::Relaxed) && !round_stop.load(Ordering::Relaxed) {
        let mut block = match build_candidate_block(&base_chain, &settings, &miner, extra_nonce) {
            Ok(block) => block,
            Err(err) => {
                let _ = events.send(MinerEvent::GpuStatus(format!("GPU candidate build failed; CPU mining continues. {err:#}")));
                return;
            }
        };

        let serialized = block.header.serialize();
        if serialized.len() != 80 {
            let _ = events.send(MinerEvent::GpuStatus("GPU candidate header had unexpected size; CPU mining continues.".to_string()));
            return;
        }
        let mut prefix = [0u8; 76];
        prefix.copy_from_slice(&serialized[..76]);
        let target = match target_from_compact(block.header.bits) {
            Ok(target) => target,
            Err(err) => {
                let _ = events.send(MinerEvent::GpuStatus(format!("GPU target decode failed; CPU mining continues. {err:#}")));
                return;
            }
        };

        let mut start_nonce = 0u64;
        while start_nonce <= u32::MAX as u64 && !stop.load(Ordering::Relaxed) && !round_stop.load(Ordering::Relaxed) {
            let remaining = (u32::MAX as u64 + 1).saturating_sub(start_nonce);
            let preferred_batch = if tune_index < tune_candidates.len() { tune_candidates[tune_index] } else { best_batch };
            let batch = preferred_batch.min(remaining as usize).max(1);
            let batch_was_capped = batch < preferred_batch;
            let scan_started = Instant::now();
            match backend.scan_nonce_range(&prefix, &target, start_nonce, batch) {
                Ok(scan) => {
                    let scan_elapsed = scan_started.elapsed().as_secs_f64().max(0.001);
                    let instant_hps = scan.hashes as f64 / scan_elapsed;
                    if scan.hashes > 0 && !batch_was_capped {
                        if tune_index < tune_candidates.len() {
                            if instant_hps > best_hps {
                                best_hps = instant_hps;
                                best_batch = batch;
                            }
                            tune_index = tune_index.saturating_add(1);
                            if tune_index >= tune_candidates.len() && !tuning_announced {
                                tuning_announced = true;
                                let _ = events.send(MinerEvent::GpuStatus(format!("GPU auto-tune selected {} work items on {} (observed {}).", best_batch, device, format_hps(best_hps))));
                            }
                        } else if instant_hps > best_hps * 1.12 {
                            best_hps = instant_hps;
                            best_batch = batch;
                        }
                    }
                    hash_counter.fetch_add(scan.hashes, Ordering::Relaxed);
                    local_total_hashes = local_total_hashes.saturating_add(scan.hashes);
                    if scan.hashes > 0 && (last_report_total == 0 || last_report.elapsed() >= Duration::from_millis(250)) {
                        let elapsed = last_report.elapsed().as_secs_f64().max(scan_elapsed).max(0.001);
                        let delta = local_total_hashes.saturating_sub(last_report_total).max(scan.hashes);
                        last_report_total = local_total_hashes;
                        last_report = Instant::now();
                        let _ = events.send(MinerEvent::GpuHashrate {
                            hps: delta as f64 / elapsed,
                            total_hashes: local_total_hashes,
                            workers: batch,
                            device: device.clone(),
                            target_height,
                        });
                    }
                    if let Some(nonce) = scan.nonce {
                        block.header.nonce = nonce;
                        match verify_header_pow(&block.header) {
                            Ok(true) => {
                                round_stop.store(true, Ordering::Relaxed);
                                let _ = found_tx.send(block);
                                return;
                            }
                            Ok(false) => {
                                let _ = events.send(MinerEvent::GpuStatus("GPU candidate failed CPU PoW verification and was ignored.".to_string()));
                            }
                            Err(err) => {
                                let _ = events.send(MinerEvent::GpuStatus(format!("GPU candidate CPU verification failed: {err:#}")));
                            }
                        }
                    }
                    start_nonce = start_nonce.saturating_add(scan.hashes);
                }
                Err(err) => {
                    let _ = events.send(MinerEvent::GpuStatus(format!("OpenCL scan failed; CPU mining continues. {err:#}")));
                    return;
                }
            }
        }
        extra_nonce = extra_nonce.wrapping_add(1);
    }
}

#[allow(clippy::too_many_arguments)]
fn mine_worker(
    worker_id: usize,
    threads: usize,
    duty: u8,
    base_chain: ChainState,
    settings: Settings,
    miner: Address,
    hash_counter: Arc<AtomicU64>,
    found_tx: mpsc::Sender<Block>,
    events: mpsc::Sender<MinerEvent>,
    stop: Arc<AtomicBool>,
    round_stop: Arc<AtomicBool>,
) {
    let mut extra_nonce = worker_id as u64;
    let step = threads.max(1) as u64;
    let batch_size = 8_192u64;

    while !stop.load(Ordering::Relaxed) && !round_stop.load(Ordering::Relaxed) {
        let mut block = match build_candidate_block(&base_chain, &settings, &miner, extra_nonce) {
            Ok(block) => block,
            Err(err) => {
                round_stop.store(true, Ordering::Relaxed);
                let _ = events.send(MinerEvent::Status(format!("Candidate rebuild warning: {err:#}")));
                return;
            }
        };
        let target = match target_from_compact(block.header.bits) {
            Ok(target) => target,
            Err(err) => {
                round_stop.store(true, Ordering::Relaxed);
                let _ = events.send(MinerEvent::Status(format!("PoW target decode warning: {err:#}")));
                return;
            }
        };
        let mut header_bytes = match header_bytes_for_mining(&block.header) {
            Ok(bytes) => bytes,
            Err(err) => {
                round_stop.store(true, Ordering::Relaxed);
                let _ = events.send(MinerEvent::Status(format!("Header serialization warning: {err:#}")));
                return;
            }
        };
        let mut nonce = 0u32;
        let mut batch_hashes = 0u64;
        let mut batch_start = Instant::now();
        loop {
            if stop.load(Ordering::Relaxed) || round_stop.load(Ordering::Relaxed) {
                if batch_hashes > 0 { hash_counter.fetch_add(batch_hashes, Ordering::Relaxed); }
                return;
            }
            header_bytes[76..80].copy_from_slice(&nonce.to_le_bytes());
            if mining_header_meets_target(&header_bytes, &target) {
                block.header.nonce = nonce;
                match verify_header_pow(&block.header) {
                    Ok(true) => {
                        hash_counter.fetch_add(batch_hashes.saturating_add(1), Ordering::Relaxed);
                        round_stop.store(true, Ordering::Relaxed);
                        let _ = found_tx.send(block);
                        return;
                    }
                    Ok(false) => {
                        let _ = events.send(MinerEvent::Status("Fast PoW hit failed consensus recheck and was ignored.".to_string()));
                    }
                    Err(err) => {
                        round_stop.store(true, Ordering::Relaxed);
                        let _ = events.send(MinerEvent::Status(format!("PoW verification warning: {err:#}")));
                        return;
                    }
                }
            }
            batch_hashes += 1;
            if batch_hashes >= batch_size {
                hash_counter.fetch_add(batch_hashes, Ordering::Relaxed);
                batch_hashes = 0;
                throttle_batch(duty, batch_start.elapsed());
                batch_start = Instant::now();
            }
            if nonce == u32::MAX { break; }
            nonce = nonce.wrapping_add(1);
        }
        if batch_hashes > 0 { hash_counter.fetch_add(batch_hashes, Ordering::Relaxed); }
        extra_nonce = extra_nonce.wrapping_add(step);
    }
}


#[allow(clippy::too_many_arguments)]
fn gpu_pool_mine_worker(
    base_chain: ChainState,
    settings: Settings,
    pool_id: Hash256,
    gpu_percent: u8,
    gpu_selector: String,
    gpu_index: usize,
    target_height: u32,
    hash_counter: Arc<AtomicU64>,
    found_tx: mpsc::Sender<Block>,
    events: mpsc::Sender<MinerEvent>,
    stop: Arc<AtomicBool>,
    round_stop: Arc<AtomicBool>,
) {
    let mut backend = match gpu_miner::OpenClGpuMiner::new_for_selector(&gpu_selector) {
        Ok(backend) => backend,
        Err(err) => {
            let _ = events.send(MinerEvent::GpuStatus(format!("OpenCL GPU unavailable for pool mining selector {}. CPU pool mining continues. {err:#}", if gpu_selector.trim().is_empty() { gpu_miner::GPU_DEVICE_ALL } else { gpu_selector.trim() })));
            return;
        }
    };

    let device = backend.device_name().to_string();
    let workers = gpu_miner::initial_work_items(gpu_percent);
    let _ = events.send(MinerEvent::GpuStarted { device: device.clone(), workers, power: gpu_percent, target_height });

    let mut extra_nonce = 12_000_000_000u64.saturating_add((gpu_index as u64).saturating_mul(100_000_000));
    let mut local_total_hashes = 0u64;
    let mut last_report = Instant::now();
    let mut last_report_total = 0u64;
    let mut tune_candidates = gpu_miner::tuning_work_item_candidates(gpu_percent);
    if tune_candidates.is_empty() { tune_candidates.push(gpu_miner::initial_work_items(gpu_percent)); }
    let mut tune_index = 0usize;
    let mut best_batch = gpu_miner::initial_work_items(gpu_percent);
    let mut best_hps = 0.0f64;
    let mut tuning_announced = false;
    while !stop.load(Ordering::Relaxed) && !round_stop.load(Ordering::Relaxed) {
        let mut block = match build_candidate_pool_block(&base_chain, &settings, pool_id, extra_nonce) {
            Ok(block) => block,
            Err(err) => {
                let _ = events.send(MinerEvent::GpuStatus(format!("GPU pool candidate build failed; CPU pool mining continues. {err:#}")));
                return;
            }
        };

        let serialized = block.header.serialize();
        if serialized.len() != 80 {
            let _ = events.send(MinerEvent::GpuStatus("GPU pool candidate header had unexpected size; CPU pool mining continues.".to_string()));
            return;
        }
        let mut prefix = [0u8; 76];
        prefix.copy_from_slice(&serialized[..76]);
        let target = match target_from_compact(block.header.bits) {
            Ok(target) => target,
            Err(err) => {
                let _ = events.send(MinerEvent::GpuStatus(format!("GPU pool target decode failed; CPU pool mining continues. {err:#}")));
                return;
            }
        };

        let mut start_nonce = 0u64;
        while start_nonce <= u32::MAX as u64 && !stop.load(Ordering::Relaxed) && !round_stop.load(Ordering::Relaxed) {
            let remaining = (u32::MAX as u64 + 1).saturating_sub(start_nonce);
            let preferred_batch = if tune_index < tune_candidates.len() { tune_candidates[tune_index] } else { best_batch };
            let batch = preferred_batch.min(remaining as usize).max(1);
            let batch_was_capped = batch < preferred_batch;
            let scan_started = Instant::now();
            match backend.scan_nonce_range(&prefix, &target, start_nonce, batch) {
                Ok(scan) => {
                    let scan_elapsed = scan_started.elapsed().as_secs_f64().max(0.001);
                    let instant_hps = scan.hashes as f64 / scan_elapsed;
                    if scan.hashes > 0 && !batch_was_capped {
                        if tune_index < tune_candidates.len() {
                            if instant_hps > best_hps {
                                best_hps = instant_hps;
                                best_batch = batch;
                            }
                            tune_index = tune_index.saturating_add(1);
                            if tune_index >= tune_candidates.len() && !tuning_announced {
                                tuning_announced = true;
                                let _ = events.send(MinerEvent::GpuStatus(format!("GPU auto-tune selected {} work items on {} (observed {}).", best_batch, device, format_hps(best_hps))));
                            }
                        } else if instant_hps > best_hps * 1.12 {
                            best_hps = instant_hps;
                            best_batch = batch;
                        }
                    }
                    hash_counter.fetch_add(scan.hashes, Ordering::Relaxed);
                    local_total_hashes = local_total_hashes.saturating_add(scan.hashes);
                    if scan.hashes > 0 && (last_report_total == 0 || last_report.elapsed() >= Duration::from_millis(250)) {
                        let elapsed = last_report.elapsed().as_secs_f64().max(scan_elapsed).max(0.001);
                        let delta = local_total_hashes.saturating_sub(last_report_total).max(scan.hashes);
                        last_report_total = local_total_hashes;
                        last_report = Instant::now();
                        let _ = events.send(MinerEvent::GpuHashrate {
                            hps: delta as f64 / elapsed,
                            total_hashes: local_total_hashes,
                            workers: batch,
                            device: device.clone(),
                            target_height,
                        });
                    }
                    if let Some(nonce) = scan.nonce {
                        block.header.nonce = nonce;
                        match verify_header_pow(&block.header) {
                            Ok(true) => {
                                round_stop.store(true, Ordering::Relaxed);
                                let _ = found_tx.send(block);
                                return;
                            }
                            Ok(false) => {
                                let _ = events.send(MinerEvent::GpuStatus("GPU pool candidate failed CPU PoW verification and was ignored.".to_string()));
                            }
                            Err(err) => {
                                let _ = events.send(MinerEvent::GpuStatus(format!("GPU pool candidate CPU verification failed: {err:#}")));
                            }
                        }
                    }
                    start_nonce = start_nonce.saturating_add(scan.hashes);
                }
                Err(err) => {
                    let _ = events.send(MinerEvent::GpuStatus(format!("OpenCL pool scan failed; CPU pool mining continues. {err:#}")));
                    return;
                }
            }
        }
        extra_nonce = extra_nonce.wrapping_add(1);
    }
}

fn pool_mine_worker(
    worker_id: usize,
    threads: usize,
    duty: u8,
    base_chain: ChainState,
    settings: Settings,
    pool_id: Hash256,
    hash_counter: Arc<AtomicU64>,
    found_tx: mpsc::Sender<Block>,
    events: mpsc::Sender<MinerEvent>,
    stop: Arc<AtomicBool>,
    round_stop: Arc<AtomicBool>,
) {
    let mut extra_nonce = worker_id as u64;
    let step = threads.max(1) as u64;
    let batch_size = 8_192u64;

    while !stop.load(Ordering::Relaxed) && !round_stop.load(Ordering::Relaxed) {
        let mut block = match build_candidate_pool_block(&base_chain, &settings, pool_id, extra_nonce) {
            Ok(block) => block,
            Err(err) => {
                round_stop.store(true, Ordering::Relaxed);
                let _ = events.send(MinerEvent::Status(format!("Pool candidate rebuild warning: {err:#}")));
                return;
            }
        };
        let target = match target_from_compact(block.header.bits) {
            Ok(target) => target,
            Err(err) => {
                round_stop.store(true, Ordering::Relaxed);
                let _ = events.send(MinerEvent::Status(format!("Pool PoW target decode warning: {err:#}")));
                return;
            }
        };
        let mut header_bytes = match header_bytes_for_mining(&block.header) {
            Ok(bytes) => bytes,
            Err(err) => {
                round_stop.store(true, Ordering::Relaxed);
                let _ = events.send(MinerEvent::Status(format!("Pool header serialization warning: {err:#}")));
                return;
            }
        };
        let mut nonce = 0u32;
        let mut batch_hashes = 0u64;
        let mut batch_start = Instant::now();
        loop {
            if stop.load(Ordering::Relaxed) || round_stop.load(Ordering::Relaxed) {
                if batch_hashes > 0 { hash_counter.fetch_add(batch_hashes, Ordering::Relaxed); }
                return;
            }
            header_bytes[76..80].copy_from_slice(&nonce.to_le_bytes());
            if mining_header_meets_target(&header_bytes, &target) {
                block.header.nonce = nonce;
                match verify_header_pow(&block.header) {
                    Ok(true) => {
                        hash_counter.fetch_add(batch_hashes.saturating_add(1), Ordering::Relaxed);
                        round_stop.store(true, Ordering::Relaxed);
                        let _ = found_tx.send(block);
                        return;
                    }
                    Ok(false) => {
                        let _ = events.send(MinerEvent::Status("Fast pool PoW hit failed consensus recheck and was ignored.".to_string()));
                    }
                    Err(err) => {
                        round_stop.store(true, Ordering::Relaxed);
                        let _ = events.send(MinerEvent::Status(format!("Pool PoW verification warning: {err:#}")));
                        return;
                    }
                }
            }
            batch_hashes += 1;
            if batch_hashes >= batch_size {
                hash_counter.fetch_add(batch_hashes, Ordering::Relaxed);
                batch_hashes = 0;
                throttle_batch(duty, batch_start.elapsed());
                batch_start = Instant::now();
            }
            if nonce == u32::MAX { break; }
            nonce = nonce.wrapping_add(1);
        }
        if batch_hashes > 0 { hash_counter.fetch_add(batch_hashes, Ordering::Relaxed); }
        extra_nonce = extra_nonce.wrapping_add(step);
    }
}

fn mempool_fingerprint(chain: &ChainState) -> Vec<Hash256> {
    chain.mempool.iter().map(|tx| tx.txid()).collect()
}

fn header_bytes_for_mining(header: &BlockHeader) -> Result<[u8; 80]> {
    let serialized = header.serialize();
    if serialized.len() != 80 { anyhow::bail!("mining header must serialize to 80 bytes, got {}", serialized.len()); }
    let mut out = [0u8; 80];
    out.copy_from_slice(&serialized);
    Ok(out)
}

fn mining_header_meets_target(header: &[u8; 80], target: &[u8; 32]) -> bool {
    let first = Sha256::digest(header);
    let second = Sha256::digest(first);
    let mut hash_for_compare = [0u8; 32];
    hash_for_compare.copy_from_slice(&second);
    hash_for_compare.reverse();
    hash_for_compare.as_slice() <= target.as_slice()
}

fn benchmark_hashing(config_path: &str, payout: &str, seconds: u64) -> Result<(f64, f64)> {
    let settings = load_gui_settings(config_path)?;
    let chain = load_or_init_chain(&settings)?;
    let miner = Address::parse_with_prefix(payout, &settings.network.address_prefix)?;
    let mut block = build_candidate_block(&chain, &settings, &miner, 0)?;
    let pow_target = target_from_compact(block.header.bits)?;
    let target = Duration::from_secs(seconds.max(1));
    let started = Instant::now();
    let mut hashes = 0u64;
    let mut extra_nonce = 0u64;
    while started.elapsed() < target {
        let mut header_bytes = header_bytes_for_mining(&block.header)?;
        for nonce in 0..=u32::MAX {
            header_bytes[76..80].copy_from_slice(&nonce.to_le_bytes());
            let _ = mining_header_meets_target(&header_bytes, &pow_target);
            hashes = hashes.saturating_add(1);
            if hashes % 65_536 == 0 && started.elapsed() >= target { break; }
        }
        extra_nonce = extra_nonce.wrapping_add(1);
        block = build_candidate_block(&chain, &settings, &miner, extra_nonce)?;
    }
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    Ok((hashes as f64 / elapsed, elapsed))
}

fn logical_cpus() -> usize {
    thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1)
}

fn resource_plan(logical_cpus: usize, cpu_percent: u8) -> (usize, u8) {
    let cpu_percent_u8 = cpu_percent.clamp(1, 100);
    let cpu_percent = cpu_percent_u8 as f32;
    let logical = logical_cpus.max(1);
    // high-performance/v1.6.0: 100% in the GUI is now a stronger high-cap mining mode. Some Windows
    // systems report conservative available parallelism or leave cores underfed
    // with one worker per logical CPU because the hash loop is memory/driver noisy.
    // Low/mid settings remain light; only the top slider position oversubscribes.
    let worker_cap = if cpu_percent_u8 >= 100 {
        logical.saturating_mul(2).min(256).max(1)
    } else {
        logical
    };
    let desired_capacity = (worker_cap as f32 * cpu_percent / 100.0).max(0.05);
    let threads = desired_capacity.ceil().clamp(1.0, worker_cap as f32) as usize;
    let duty = ((desired_capacity / threads as f32) * 100.0).round().clamp(1.0, 100.0) as u8;
    (threads.max(1), duty.max(1))
}

fn throttle_batch(duty: u8, work_elapsed: Duration) {
    if duty >= 100 { return; }
    let duty = duty.max(1) as f64;
    let sleep_secs = work_elapsed.as_secs_f64() * ((100.0 - duty) / duty);
    if sleep_secs > 0.000_5 {
        thread::sleep(Duration::from_secs_f64(sleep_secs.min(0.25)));
    }
}

fn install_optional_font(ctx: &egui::Context) {
    let Some(bytes) = read_asset_bytes(FONT_PATH) else { return; };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "ubuntu_bold_italic".to_string(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    fonts.families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "ubuntu_bold_italic".to_string());
    ctx.set_fonts(fonts);
}

fn load_window_icon() -> Option<egui::IconData> {
    let bytes = read_asset_bytes(LOGO_PATH)?;
    eframe::icon_data::from_png_bytes(&bytes).ok()
}

fn load_logo_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    load_asset_texture(ctx, LOGO_PATH, "qubit-coin-logo")
}

fn load_asset_texture(ctx: &egui::Context, path: &str, name: &str) -> Option<egui::TextureHandle> {
    let bytes = read_asset_bytes(path)?;
    let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let pixels = image.into_raw();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
    Some(ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR))
}

fn load_gif_animation(ctx: &egui::Context, path: &str, name: &str) -> Option<AnimatedAsset> {
    let resolved = resolve_app_read_path(path);
    if !resolved.exists() { return None; }
    let file = std::fs::File::open(&resolved).ok()?;
    let reader = std::io::BufReader::new(file);
    let decoder = image::codecs::gif::GifDecoder::new(reader).ok()?;
    let frames = decoder.into_frames().collect_frames().ok()?;
    if frames.is_empty() { return None; }
    let mut textures = Vec::with_capacity(frames.len());
    let mut frame_ms = Vec::with_capacity(frames.len());
    let mut total_ms = 0u64;
    for (idx, frame) in frames.into_iter().enumerate() {
        let (num, den) = frame.delay().numer_denom_ms();
        let delay_ms = if den == 0 { 100 } else { ((num as u64 + den as u64 - 1) / den as u64).clamp(20, 1_000) };
        let image = frame.into_buffer();
        let size = [image.width() as usize, image.height() as usize];
        let pixels = image.into_raw();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
        textures.push(ctx.load_texture(format!("{name}-{idx}"), color_image, egui::TextureOptions::LINEAR));
        frame_ms.push(delay_ms);
        total_ms = total_ms.saturating_add(delay_ms);
    }
    Some(AnimatedAsset { frames: textures, frame_ms, total_ms })
}


fn detect_camera_devices() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
$items = Get-CimInstance Win32_PnPEntity | Where-Object { $_.Name -match 'camera|webcam|video|capture' } | Select-Object -ExpandProperty Name
if ($items) { $items | ForEach-Object { $_ } }
"#;
        let mut cmd = Command::new("powershell.exe");
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script]);
        if let Ok(out) = cmd.output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                let devices = text.lines()
                    .map(|line| line.trim().to_string())
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>();
                if !devices.is_empty() { return devices; }
            }
        }
    }
    Vec::new()
}

fn open_windows_camera_app() {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd.exe");
        cmd.creation_flags(CREATE_NO_WINDOW);
        let _ = cmd.args(["/C", "start", "", "microsoft.windows.camera:"]).spawn();
    }
}

fn read_asset_bytes(path: &str) -> Option<Vec<u8>> {
    let direct = PathBuf::from(path);
    if direct.exists() { return std::fs::read(direct).ok(); }
    let base = app_base_dir().join(path);
    if base.exists() { return std::fs::read(base).ok(); }
    let exe = std::env::current_exe().ok()?;
    let beside_exe = exe.parent()?.join(path);
    if beside_exe.exists() { return std::fs::read(beside_exe).ok(); }
    let parent_assets = exe.parent()?.parent()?.join(path);
    if parent_assets.exists() { return std::fs::read(parent_assets).ok(); }
    None
}

fn apply_theme(ctx: &egui::Context, theme: &ThemeChoice) {
    match theme {
        ThemeChoice::Dark => ctx.set_theme(egui::ThemePreference::Dark),
        ThemeChoice::Light => ctx.set_theme(egui::ThemePreference::Light),
        ThemeChoice::System => ctx.set_theme(egui::ThemePreference::System),
    }
}

fn apply_visual_tuning(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 7.0);
    style.spacing.interact_size.y = style.spacing.interact_size.y.max(24.0);
    ctx.set_global_style(style);
}

fn peer_identity_label(peer: &PeerUiStatus) -> String {
    if let Some(name) = peer.qns_names.first() { return name.clone(); }
    let candidate = peer.miner_address.trim();
    if candidate.is_empty() {
        if peer.reachable { "Seed node".to_string() } else { "Guest".to_string() }
    } else { shorten_hash(candidate) }
}

fn peer_role_label(peer: &PeerUiStatus) -> String {
    let role = peer.role.trim();
    if role.is_empty() {
        if peer.user_agent.trim().is_empty() { "QUB Core".to_string() } else { peer.user_agent.clone() }
    } else {
        role.to_string()
    }
}

fn peer_status_color(peer: &PeerUiStatus, ui: &egui::Ui) -> egui::Color32 {
    if peer.reachable { egui::Color32::from_rgb(72, 168, 255) }
    else if peer.global_live { egui::Color32::from_rgb(66, 220, 120) }
    else { ui.visuals().weak_text_color() }
}

fn peer_status_text(peer: &PeerUiStatus) -> &'static str {
    if peer.reachable { " direct" }
    else if peer.global_live { " online" }
    else { " offline" }
}

fn peer_activity_text(peer: &PeerUiStatus) -> String {
    if peer.reachable { return format!("{} - direct", peer_role_label(peer)); }
    if peer.global_live { return format!("{} - online, seen {} ago", peer_role_label(peer), format_seen_age(peer.seen_age_secs)); }
    if peer.seen_age_secs.is_some() { return format!("last seen {} ago", format_seen_age(peer.seen_age_secs)); }
    "offline".to_string()
}

fn format_seen_age(age: Option<u64>) -> String {
    let Some(sec) = age else { return "unknown".to_string(); };
    if sec < 60 { format!("{}s", sec) }
    else if sec < 3600 { format!("{}m", sec / 60) }
    else { format!("{}h", sec / 3600) }
}

fn peer_region_key(peer: &PeerUiStatus) -> String {
    if !peer.node_id.trim().is_empty() { peer.node_id.clone() }
    else if !peer.miner_address.trim().is_empty() { peer.miner_address.clone() }
    else if !peer.listen_addr.trim().is_empty() { peer.listen_addr.clone() }
    else if !peer.observed_addr.trim().is_empty() { peer.observed_addr.clone() }
    else { peer.addr.clone() }
}

fn stable_hash64(input: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in input.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn privacy_region_position(key: &str, idx: usize, rect: egui::Rect, t: f32, local: bool) -> egui::Pos2 {
    // Coarse world buckets. This is intentionally privacy-preserving: it is not
    // IP geolocation and it must never be used as exact location evidence.
    const REGIONS: &[(f32, f32)] = &[
        (-100.0, 42.0), // North America
        (-62.0, -16.0), // South America
        (12.0, 50.0),   // Europe
        (22.0, 4.0),    // Africa
        (47.0, 28.0),   // Middle East
        (102.0, 34.0),  // Asia
        (136.0, -25.0), // Oceania
    ];
    let h = stable_hash64(key);
    let region = if local { REGIONS[(h as usize + 2) % REGIONS.len()] } else { REGIONS[(h as usize) % REGIONS.len()] };
    let jitter_lon = (((h >> 8) & 0xff) as f32 / 255.0 - 0.5) * 34.0;
    let jitter_lat = (((h >> 16) & 0xff) as f32 / 255.0 - 0.5) * 18.0;
    let drift_x = (t * 0.45 + idx as f32 * 1.37).sin() * 3.0;
    let drift_y = (t * 0.39 + idx as f32 * 0.91).cos() * 2.0;
    geo_project(rect, region.0 + jitter_lon, region.1 + jitter_lat) + egui::vec2(drift_x, drift_y)
}

fn geo_project(rect: egui::Rect, lon: f32, lat: f32) -> egui::Pos2 {
    let x = rect.left() + ((lon + 180.0) / 360.0).clamp(0.0, 1.0) * rect.width();
    let y = rect.top() + ((90.0 - lat) / 180.0).clamp(0.0, 1.0) * rect.height();
    egui::pos2(x, y)
}

fn draw_privacy_world_map(painter: &egui::Painter, rect: egui::Rect, dark: bool) {
    let ocean = if dark { egui::Color32::from_rgba_unmultiplied(20, 37, 55, 180) } else { egui::Color32::from_rgba_unmultiplied(210, 229, 244, 160) };
    let grid = if dark { egui::Color32::from_rgba_unmultiplied(90, 120, 150, 55) } else { egui::Color32::from_rgba_unmultiplied(80, 120, 150, 55) };
    let land = if dark { egui::Color32::from_rgba_unmultiplied(75, 92, 86, 120) } else { egui::Color32::from_rgba_unmultiplied(120, 155, 132, 115) };
    painter.rect_filled(rect, 10, ocean);
    for i in 1..6 {
        let x = rect.left() + rect.width() * i as f32 / 6.0;
        painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(0.6, grid));
    }
    for i in 1..4 {
        let y = rect.top() + rect.height() * i as f32 / 4.0;
        painter.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], egui::Stroke::new(0.6, grid));
    }
    // Stylized continent blobs, deliberately approximate.
    for (lon, lat, radius) in [
        (-105.0, 46.0, 34.0), (-76.0, 16.0, 18.0), (-60.0, -18.0, 24.0),
        (12.0, 48.0, 16.0), (22.0, 2.0, 28.0), (66.0, 35.0, 24.0),
        (105.0, 35.0, 34.0), (138.0, -25.0, 18.0),
    ] {
        let pos = geo_project(rect, lon, lat);
        painter.circle_filled(pos, radius, land);
    }
    painter.rect_stroke(rect, 10, egui::Stroke::new(1.0, grid), egui::StrokeKind::Inside);
}

fn draw_legend_dot(painter: &egui::Painter, pos: egui::Pos2, color: egui::Color32, label: &str, text: egui::Color32) {
    painter.circle_filled(pos, 4.0, color);
    painter.text(pos + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(11.0), text);
}

fn is_local_block(block: &SnapshotBlock, local_payout: &str) -> bool {
    if block.pool_block { return false; }
    block.local || (!local_payout.trim().is_empty() && block.miner_address.trim() == local_payout.trim())
}

fn block_miner_label(block: &SnapshotBlock, local_payout: &str) -> String {
    if block.pool_block {
        if !block.pool_name.trim().is_empty() { return block.pool_name.clone(); }
        if !block.pool_id.trim().is_empty() { return format!("pool {}", shorten_hash(&block.pool_id)); }
        return "pool".to_string();
    }
    if is_local_block(block, local_payout) { "you".to_string() }
    else if let Some(name) = block.miner_qns.first() { name.clone() }
    else if block.miner_address.trim().is_empty() || block.miner_address == "Guest" { "Guest".to_string() }
    else { shorten_hash(&block.miner_address) }
}

fn split_bootnodes(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|p| p.trim().trim_start_matches("tcp://").to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn write_wizard_config(profile: &SetupProfile, _bootnodes: &str, _advertise_addr: &str, _listen_for_peers: bool, _seed_node_mode: bool) -> Result<String> {
    let template = resolve_app_read_path(profile.template_config());
    let mut settings = Settings::load_from_path(&template)?;
    settings.p2p.enabled = true;

    // Normal users never type bootnodes or public IPs.
    // - regtest-lan: automatic UDP LAN discovery fills peers.json.
    // - testnet/mainnet: official DNS seed domains are shipped in config and reinforced in p2p::release_bootnodes().
    if matches!(profile, SetupProfile::RegtestLan) {
        settings.p2p.bootnodes.clear();
    }
    settings.p2p.advertise_addr.clear();
    let port = settings.network.default_port;
    settings.p2p.bind = format!("0.0.0.0:{port}");
    settings.mining.enabled = false;
    settings.mining.miner_address.clear();
    let rel_path = profile.generated_config().to_string();
    let path = app_write_path(&rel_path);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(&path, toml::to_string_pretty(&settings)?)?;
    Ok(rel_path)
}

fn shorten_hash(hash: &str) -> String {
    if hash.len() <= 18 { return hash.to_string(); }
    format!("{}...{}", &hash[..10], &hash[hash.len() - 8..])
}

fn ui_recent_block_cell(ui: &mut egui::Ui, text: impl Into<String>, pending_finality: bool, monospace: bool, hover: &str) {
    let mut rich = egui::RichText::new(text.into());
    if monospace { rich = rich.monospace(); }
    if pending_finality {
        rich = rich.color(egui::Color32::from_rgb(236, 190, 78)).italics().weak();
    }
    let response = ui.label(rich);
    if pending_finality { response.on_hover_text(hover); }
}

fn ui_recent_block_miner_cell(ui: &mut egui::Ui, block: &SnapshotBlock, text: impl Into<String>, pending_finality: bool, hover: &str) {
    let text = text.into();
    if block.pool_block {
        let bg = if pending_finality {
            egui::Color32::from_rgb(132, 94, 34)
        } else {
            egui::Color32::from_rgb(36, 104, 188)
        };
        let fg = if pending_finality {
            egui::Color32::from_rgb(255, 224, 132)
        } else {
            egui::Color32::from_rgb(230, 244, 255)
        };
        let mut rich = egui::RichText::new(format!("  {}  ", text))
            .strong()
            .color(fg)
            .background_color(bg);
        if pending_finality {
            rich = rich.italics();
        }
        let response = ui.label(rich);
        response.on_hover_text(format!("Pool-mined block by {text}. The colored pool pill distinguishes pooled rewards from solo miner addresses. {hover}"));
    } else {
        ui_recent_block_cell(ui, text, pending_finality, false, hover);
    }
}

fn block_age_secs(block: &SnapshotBlock) -> u64 {
    unix_time_u32().saturating_sub(block.time) as u64
}

fn format_block_age(block: &SnapshotBlock) -> String {
    let now = unix_time_u32();
    if block.time > now {
        format!("clock +{}s", block.time.saturating_sub(now))
    } else {
        format!("{}s", now.saturating_sub(block.time))
    }
}

fn format_hps(hps: f64) -> String {
    if hps >= 1_000_000_000.0 { format!("{:.2} GH/s", hps / 1_000_000_000.0) }
    else if hps >= 1_000_000.0 { format!("{:.2} MH/s", hps / 1_000_000.0) }
    else if hps >= 1_000.0 { format!("{:.2} kH/s", hps / 1_000.0) }
    else { format!("{:.0} H/s", hps) }
}

fn format_u64(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { out.push(','); }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(target_os = "windows")]
fn hide_command_window(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(target_os = "windows")]
fn set_windows_autostart(enabled: bool) -> Result<()> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let name = "Qubit Coin Core";
    if enabled {
        let exe = std::env::current_exe().context("current exe path unavailable")?;
        let value = format!("\"{}\"", exe.display());
        let mut command = Command::new("reg.exe");
        command.args(["ADD", key, "/v", name, "/t", "REG_SZ", "/d", &value, "/f"]);
        hide_command_window(&mut command);
        let status = command.status()?;
        if !status.success() { anyhow::bail!("reg.exe ADD failed"); }
    } else {
        let mut command = Command::new("reg.exe");
        command.args(["DELETE", key, "/v", name, "/f"]);
        hide_command_window(&mut command);
        let _ = command.status()?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn set_windows_autostart(_enabled: bool) -> Result<()> {
    anyhow::bail!("Windows autostart is only available on Windows builds")
}

#[cfg(target_os = "windows")]
static MINED_SOUND_READY: OnceLock<bool> = OnceLock::new();
#[cfg(target_os = "windows")]
static NETWORK_MINED_SOUND_READY: OnceLock<bool> = OnceLock::new();
#[cfg(target_os = "windows")]
static MINING_LOOP_SOUND_READY: OnceLock<bool> = OnceLock::new();


#[cfg(target_os = "windows")]
fn preload_core_audio() {
    thread::spawn(|| {
        let _ = preload_mp3_alias(MINED_SOUND_PATH, "qub_mined", &MINED_SOUND_READY);
        let _ = preload_mp3_alias(NETWORK_MINED_SOUND_PATH, "qub_network_mined", &NETWORK_MINED_SOUND_READY);
        let _ = preload_mp3_alias(MINING_ON_SOUND_PATH, "qub_mining_loop", &MINING_LOOP_SOUND_READY);
    });
}

#[cfg(target_os = "windows")]
fn preload_mp3_alias(path: &str, alias: &str, ready: &'static OnceLock<bool>) -> Result<()> {
    if ready.get().is_none() {
        let resolved = resolve_app_read_path(path);
        if !resolved.exists() { anyhow::bail!("missing {path}"); }
        let _ = mci_command(&format!("close {alias}"));
        mci_command(&format!("open \"{}\" type mpegvideo alias {alias}", resolved.display()))?;
        let _ = ready.set(true);
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn preload_core_audio() {}

#[cfg(target_os = "windows")]
fn play_block_sound() {
    thread::spawn(|| { let _ = play_mp3_cached(MINED_SOUND_PATH, "qub_mined", &MINED_SOUND_READY); });
}

#[cfg(target_os = "windows")]
fn play_network_mined_sound() {
    thread::spawn(|| { let _ = play_mp3_cached(NETWORK_MINED_SOUND_PATH, "qub_network_mined", &NETWORK_MINED_SOUND_READY); });
}

#[cfg(target_os = "windows")]
fn play_mining_loop_tick_sound() {
    thread::spawn(|| { let _ = play_mp3_cached(MINING_ON_SOUND_PATH, "qub_mining_loop", &MINING_LOOP_SOUND_READY); });
}

#[cfg(target_os = "windows")]
fn play_mp3_cached(path: &str, alias: &str, ready: &'static OnceLock<bool>) -> Result<()> {
    preload_mp3_alias(path, alias, ready)?;
    let _ = mci_command(&format!("stop {alias}"));
    let _ = mci_command(&format!("seek {alias} to start"));
    match mci_command(&format!("play {alias} from 0")) {
        Ok(()) => Ok(()),
        Err(_) => {
            // One recovery attempt if Windows MCI closed the alias behind our back.
            let resolved = resolve_app_read_path(path);
            let _ = mci_command(&format!("close {alias}"));
            mci_command(&format!("open \"{}\" type mpegvideo alias {alias}", resolved.display()))?;
            mci_command(&format!("play {alias} from 0"))
        }
    }
}

#[cfg(target_os = "windows")]
fn mci_command(command: &str) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let wide = std::ffi::OsStr::new(command).encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let code = unsafe { mciSendStringW(wide.as_ptr(), std::ptr::null_mut(), 0, std::ptr::null_mut()) };
    if code == 0 { Ok(()) } else { anyhow::bail!("mciSendStringW failed with code {code}") }
}

#[cfg(not(target_os = "windows"))]
fn play_block_sound() {}
#[cfg(not(target_os = "windows"))]
fn play_network_mined_sound() {}
#[cfg(not(target_os = "windows"))]
fn play_mining_loop_tick_sound() {}



fn qub_payout_address_warning(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() { return None; }
    if trimmed.to_ascii_lowercase().starts_with("0x") {
        return Some("Wrong payout address format: Ethereum 0x... addresses cannot receive QUB mining rewards. Create/import a QUB Chain wallet in Create / import address -> QUB Chain, then use its qub1... address here.".to_string());
    }
    if trimmed.contains(':') || trimmed.contains('@') || trimmed.contains('/') {
        return Some("Wrong payout address format: use a QUB Chain qub1... address or a .qub QNS name for mining payout. Create/import a QUB wallet in Create / import address -> QUB Chain.".to_string());
    }
    if trimmed.ends_with(".qub") { return None; }
    match Address::parse_with_prefix(trimmed, "qub") {
        Ok(_) => None,
        Err(_) => Some("Wrong payout address format: mining payout must be a QUB Chain qub1... address or .qub name. Create/import a QUB wallet in Create / import address -> QUB Chain.".to_string()),
    }
}

fn ethereum_wallet_file_path() -> PathBuf { app_write_path(ETHEREUM_WALLETS_FILE) }

fn load_ethereum_wallet_book() -> Result<EthereumWalletBook> {
    let path = ethereum_wallet_file_path();
    if !path.exists() { return Ok(EthereumWalletBook::default()); }
    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut book: EthereumWalletBook = serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    if book.selected_index >= book.wallets.len() { book.selected_index = book.wallets.len().saturating_sub(1); }
    if book.rpc_url.trim().is_empty() { book.rpc_url = ETHEREUM_DEFAULT_RPC_URLS[0].to_string(); }
    if book.chain_id == 0 { book.chain_id = ETHEREUM_CHAIN_ID; }
    Ok(book)
}

fn save_ethereum_wallet_book(book: &EthereumWalletBook) -> Result<()> {
    let path = ethereum_wallet_file_path();
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let json = serde_json::to_string_pretty(book)?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn now_unix_secs() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }

fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut k = Keccak::v256();
    let mut out = [0u8; 32];
    k.update(data);
    k.finalize(&mut out);
    out
}

fn ethereum_address_from_secret(secret: &SecretKey) -> String {
    let secp = Secp256k1::new();
    let public = PublicKey::from_secret_key(&secp, secret);
    let uncompressed = public.serialize_uncompressed();
    let h = keccak256(&uncompressed[1..]);
    ethereum_checksum_address(&h[12..])
}

fn ethereum_checksum_address(addr20: &[u8]) -> String {
    let lower = hex::encode(addr20);
    let hash = hex::encode(keccak256(lower.as_bytes()));
    let mut out = String::from("0x");
    for (idx, ch) in lower.chars().enumerate() {
        let nibble = u8::from_str_radix(&hash[idx..idx+1], 16).unwrap_or(0);
        if ch.is_ascii_hexdigit() && ch.is_ascii_alphabetic() && nibble >= 8 { out.push(ch.to_ascii_uppercase()); }
        else { out.push(ch); }
    }
    out
}

fn normalize_eth_address(input: &str) -> Result<String> {
    let s = input.trim();
    let hexpart = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if hexpart.len() != 40 || !hexpart.chars().all(|c| c.is_ascii_hexdigit()) { anyhow::bail!("invalid Ethereum address"); }
    let bytes = hex::decode(hexpart)?;
    Ok(ethereum_checksum_address(&bytes))
}

fn is_valid_eth_address(input: &str) -> bool { normalize_eth_address(input).is_ok() }

fn shorten_eth_address(input: &str) -> String {
    let s = input.trim();
    if s.len() <= 18 { return s.to_string(); }
    format!("{}...{}", &s[..10.min(s.len())], &s[s.len().saturating_sub(8)..])
}

fn ethereum_wallet_entry_from_private_key(input: &str, label: &str) -> Result<EthereumWalletEntry> {
    let key = input.trim().trim_start_matches("0x").trim_start_matches("0X");
    if key.len() != 64 || !key.chars().all(|c| c.is_ascii_hexdigit()) { anyhow::bail!("Ethereum private key must be 32 bytes hex"); }
    let bytes = hex::decode(key)?;
    let secret = SecretKey::from_slice(&bytes).context("invalid secp256k1 private key")?;
    let address = ethereum_address_from_secret(&secret);
    Ok(EthereumWalletEntry { address, private_key_hex: hex::encode(secret.secret_bytes()), label: if label.trim().is_empty() { "Ethereum wallet".to_string() } else { label.trim().to_string() }, created_unix: now_unix_secs() })
}

fn generate_ethereum_wallet_entry(label: &str) -> Result<EthereumWalletEntry> {
    let mut rng = rand::rngs::OsRng;
    let secret = SecretKey::new(&mut rng);
    let address = ethereum_address_from_secret(&secret);
    Ok(EthereumWalletEntry { address, private_key_hex: hex::encode(secret.secret_bytes()), label: if label.trim().is_empty() { "Ethereum wallet".to_string() } else { label.trim().to_string() }, created_unix: now_unix_secs() })
}

fn effective_eth_rpc_urls(custom: &str) -> Vec<String> {
    let mut out = Vec::new();
    let c = custom.trim();
    if !c.is_empty() { out.push(c.to_string()); }
    for url in ETHEREUM_DEFAULT_RPC_URLS { if !out.iter().any(|u| u == url) { out.push((*url).to_string()); } }
    out
}

fn ps_quote(s: &str) -> String { s.replace("'", "''") }

#[cfg(target_os = "windows")]
fn ethereum_rpc_call_hf102(rpc_url: &str, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
    let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string();
    let cmd = format!(
        "$body='{}'; $r=Invoke-WebRequest -Uri '{}' -Method POST -ContentType 'application/json' -Body $body -UseBasicParsing; [string]$r.Content",
        ps_quote(&body), ps_quote(rpc_url)
    );
    let value = powershell_json(&cmd)?;
    if let Some(err) = value.get("error") { anyhow::bail!("Ethereum RPC {method} error: {err}"); }
    Ok(value.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

#[cfg(not(target_os = "windows"))]
fn ethereum_rpc_call_hf102(_rpc_url: &str, _method: &str, _params: serde_json::Value) -> Result<serde_json::Value> {
    anyhow::bail!("Ethereum JSON-RPC is enabled in the Windows GUI build")
}

fn ethereum_rpc_call_any_hf102(custom_rpc: &str, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
    let mut last = None;
    for url in effective_eth_rpc_urls(custom_rpc) {
        match ethereum_rpc_call_hf102(&url, method, params.clone()) {
            Ok(v) => return Ok(v),
            Err(err) => last = Some(format!("{url}: {err:#}")),
        }
    }
    anyhow::bail!("all Ethereum RPC endpoints failed: {}", last.unwrap_or_else(|| "no endpoint".to_string()))
}

fn hex_quantity_to_biguint(value: &str) -> Result<BigUint> {
    let h = value.trim().trim_start_matches("0x");
    if h.is_empty() { return Ok(BigUint::zero()); }
    Ok(BigUint::from_str_radix(h, 16)?)
}

fn biguint_to_quantity(v: &BigUint) -> String { if v.is_zero() { "0x0".to_string() } else { format!("0x{}", v.to_str_radix(16)) } }

fn parse_decimal_units(input: &str, decimals: u32) -> Result<BigUint> {
    let s = input.trim().replace('_', "");
    if s.is_empty() { anyhow::bail!("amount is required"); }
    let parts = s.split('.').collect::<Vec<_>>();
    if parts.len() > 2 { anyhow::bail!("invalid decimal amount"); }
    let whole = parts[0];
    if !whole.chars().all(|c| c.is_ascii_digit()) { anyhow::bail!("invalid whole amount"); }
    let mut units = BigUint::from_str_radix(if whole.is_empty() { "0" } else { whole }, 10)? * BigUint::from(10u32).pow(decimals);
    if parts.len() == 2 {
        let frac = parts[1];
        if frac.len() > decimals as usize { anyhow::bail!("too many decimals; {} supports {} decimals", input, decimals); }
        if !frac.chars().all(|c| c.is_ascii_digit()) { anyhow::bail!("invalid fractional amount"); }
        let mut padded = frac.to_string();
        while padded.len() < decimals as usize { padded.push('0'); }
        if !padded.is_empty() { units += BigUint::from_str_radix(&padded, 10)?; }
    }
    if units.is_zero() { anyhow::bail!("amount must be greater than zero"); }
    Ok(units)
}

fn format_decimal_units(units: &BigUint, decimals: u32, max_frac: usize) -> String {
    let base = BigUint::from(10u32).pow(decimals);
    let whole = units / &base;
    let frac = units % &base;
    if frac.is_zero() { return whole.to_str_radix(10); }
    let mut f = frac.to_str_radix(10);
    while f.len() < decimals as usize { f.insert(0, '0'); }
    while f.ends_with('0') { f.pop(); }
    if f.len() > max_frac { f.truncate(max_frac); }
    format!("{}.{}", whole.to_str_radix(10), f)
}

fn erc20_balance_call_data(address: &str) -> Result<String> {
    let mut data = String::from(ETHEREUM_ERC20_BALANCE_OF_SELECTOR);
    let addr = normalize_eth_address(address)?;
    data.push_str(&format!("{:0>64}", addr.trim_start_matches("0x")));
    Ok(format!("0x{data}"))
}

fn erc20_transfer_data(to: &str, amount: &BigUint) -> Result<Vec<u8>> {
    let addr = normalize_eth_address(to)?;
    let mut data = hex::decode(ETHEREUM_ERC20_TRANSFER_SELECTOR)?;
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(&hex::decode(addr.trim_start_matches("0x"))?);
    let amount_bytes = amount.to_bytes_be();
    if amount_bytes.len() > 32 { anyhow::bail!("token amount too large"); }
    let mut padded = vec![0u8; 32 - amount_bytes.len()];
    padded.extend_from_slice(&amount_bytes);
    data.extend_from_slice(&padded);
    Ok(data)
}

fn fetch_ethereum_balances_hf102(rpc: &str, address: &str) -> Result<(String, String, String, String)> {
    let addr = normalize_eth_address(address)?;
    let eth_raw = ethereum_rpc_call_any_hf102(rpc, "eth_getBalance", serde_json::json!([addr, "latest"]))?;
    let eth_units = hex_quantity_to_biguint(eth_raw.as_str().unwrap_or("0x0"))?;
    let usdt_units = fetch_erc20_balance_hf102(rpc, ETHEREUM_USDT_ADDRESS, &addr)?;
    let usdc_units = fetch_erc20_balance_hf102(rpc, ETHEREUM_USDC_ADDRESS, &addr)?;
    Ok((
        format_decimal_units(&eth_units, 18, 6),
        format_decimal_units(&usdt_units, 6, 6),
        format_decimal_units(&usdc_units, 6, 6),
        format!("ETH RPC refreshed via {}", effective_eth_rpc_urls(rpc).first().cloned().unwrap_or_default()),
    ))
}

fn fetch_erc20_balance_hf102(rpc: &str, contract: &str, address: &str) -> Result<BigUint> {
    let data = erc20_balance_call_data(address)?;
    let res = ethereum_rpc_call_any_hf102(rpc, "eth_call", serde_json::json!([{ "to": contract, "data": data }, "latest"]))?;
    hex_quantity_to_biguint(res.as_str().unwrap_or("0x0"))
}

fn fetch_ethereum_balances_hf108(rpc: &str, address: &str, usdt_contract: &str, usdc_contract: &str, usdj_contract: &str, vault_contract: &str, eurc_contract: &str, eurs_contract: &str, eurj_contract: &str, eur_vault_contract: &str, paxg_contract: &str, xaut_contract: &str, xauj_contract: &str, xau_vault_contract: &str) -> Result<(String, String, String, String, String, String, String, String, String, String, String, String, String, String, String, String, String, String, String, String)> {
    let addr = normalize_eth_address(address)?;
    let eth_raw = ethereum_rpc_call_any_hf102(rpc, "eth_getBalance", serde_json::json!([addr, "latest"]))?;
    let eth_units = hex_quantity_to_biguint(eth_raw.as_str().unwrap_or("0x0"))?;
    let usdt_units = fetch_erc20_balance_hf102(rpc, usdt_contract, address)?;
    let usdc_units = fetch_erc20_balance_hf102(rpc, usdc_contract, address)?;
    let eth = format_decimal_units(&eth_units, 18, 6);
    let usdt = format_decimal_units(&usdt_units, 6, 6);
    let usdc = format_decimal_units(&usdc_units, 6, 6);
    let status = format!("ETH RPC refreshed via {}", effective_eth_rpc_urls(rpc).first().cloned().unwrap_or_default());

    let mut usdj_eth = "-".to_string();
    let mut usdt_reserve = "-".to_string();
    let mut usdc_reserve = "-".to_string();
    let mut reserve_status = "USDJ contracts not configured".to_string();
    if is_valid_eth_address(usdj_contract) {
        let units = fetch_erc20_balance_hf102(rpc, usdj_contract, address)?;
        usdj_eth = format_decimal_units(&units, StablecoinFamily::Usd.token_decimals(), 6);
    }
    if is_valid_eth_address(vault_contract) {
        let usdt_units = fetch_usdj_vault_reserve_hf107(rpc, vault_contract, 0).unwrap_or_else(|_| BigUint::zero());
        let usdc_units = fetch_usdj_vault_reserve_hf107(rpc, vault_contract, 1).unwrap_or_else(|_| BigUint::zero());
        usdt_reserve = format_decimal_units(&usdt_units, EthereumAsset::Usdt.decimals(), 6);
        usdc_reserve = format_decimal_units(&usdc_units, EthereumAsset::Usdc.decimals(), 6);
        reserve_status = format!("USDJ vault reserves refreshed from {}", shorten_eth_address(vault_contract));
    }

    let mut eurc = "-".to_string();
    let mut eurs = "-".to_string();
    let mut eurj_eth = "-".to_string();
    let mut eurc_reserve = "-".to_string();
    let mut eurs_reserve = "-".to_string();
    let mut eur_reserve_status = "EURJ contracts not configured".to_string();
    if is_valid_eth_address(eurc_contract) {
        eurc = format_decimal_units(&fetch_erc20_balance_hf102(rpc, eurc_contract, address)?, EthereumAsset::Eurc.decimals(), 6);
    }
    if is_valid_eth_address(eurs_contract) {
        eurs = format_decimal_units(&fetch_erc20_balance_hf102(rpc, eurs_contract, address)?, EthereumAsset::Eurs.decimals(), 2);
    }
    if is_valid_eth_address(eurj_contract) {
        eurj_eth = format_decimal_units(&fetch_erc20_balance_hf102(rpc, eurj_contract, address)?, StablecoinFamily::Eur.token_decimals(), 6);
    }
    if is_valid_eth_address(eur_vault_contract) {
        let eurc_units = fetch_usdj_vault_reserve_hf107(rpc, eur_vault_contract, 0).unwrap_or_else(|_| BigUint::zero());
        let eurs_units = fetch_usdj_vault_reserve_hf107(rpc, eur_vault_contract, 1).unwrap_or_else(|_| BigUint::zero());
        eurc_reserve = format_decimal_units(&eurc_units, EthereumAsset::Eurc.decimals(), 6);
        eurs_reserve = format_decimal_units(&eurs_units, EthereumAsset::Eurs.decimals(), 2);
        eur_reserve_status = format!("EURJ vault reserves refreshed from {}", shorten_eth_address(eur_vault_contract));
    }

    let mut paxg = "-".to_string();
    let mut xaut = "-".to_string();
    let mut xauj_eth = "-".to_string();
    let mut paxg_reserve = "-".to_string();
    let mut xaut_reserve = "-".to_string();
    let mut gold_reserve_status = "XAUJ contracts not configured".to_string();
    if is_valid_eth_address(paxg_contract) {
        paxg = format_decimal_units(&fetch_erc20_balance_hf102(rpc, paxg_contract, address)?, EthereumAsset::Paxg.decimals(), 6);
    }
    if is_valid_eth_address(xaut_contract) {
        xaut = format_decimal_units(&fetch_erc20_balance_hf102(rpc, xaut_contract, address)?, EthereumAsset::Xaut.decimals(), 6);
    }
    if is_valid_eth_address(xauj_contract) {
        xauj_eth = format_decimal_units(&fetch_erc20_balance_hf102(rpc, xauj_contract, address)?, StablecoinFamily::Gold.token_decimals(), 6);
    }
    if is_valid_eth_address(xau_vault_contract) {
        let paxg_units = fetch_usdj_vault_reserve_hf107(rpc, xau_vault_contract, 0).unwrap_or_else(|_| BigUint::zero());
        let xaut_units = fetch_usdj_vault_reserve_hf107(rpc, xau_vault_contract, 1).unwrap_or_else(|_| BigUint::zero());
        paxg_reserve = format_decimal_units(&paxg_units, EthereumAsset::Paxg.decimals(), 6);
        xaut_reserve = format_decimal_units(&xaut_units, EthereumAsset::Xaut.decimals(), 6);
        gold_reserve_status = format!("XAUJ vault reserves refreshed from {}", shorten_eth_address(xau_vault_contract));
    }

    Ok((eth, usdt, usdc, usdj_eth, usdt_reserve, usdc_reserve, eurc, eurs, eurj_eth, eurc_reserve, eurs_reserve, paxg, xaut, xauj_eth, paxg_reserve, xaut_reserve, reserve_status, eur_reserve_status, gold_reserve_status, status))
}

fn function_selector_hex(signature: &str) -> String {
    let h = keccak256(signature.as_bytes());
    hex::encode(&h[0..4])
}

fn abi_word_biguint(value: &BigUint) -> Result<Vec<u8>> {
    let bytes = value.to_bytes_be();
    if bytes.len() > 32 { anyhow::bail!("ABI integer too large"); }
    let mut out = vec![0u8; 32 - bytes.len()];
    out.extend_from_slice(&bytes);
    Ok(out)
}

fn abi_word_u8(value: u8) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    out[31] = value;
    out
}

fn abi_word_address(address: &str) -> Result<Vec<u8>> {
    let addr = normalize_eth_address(address)?;
    let mut out = vec![0u8; 12];
    out.extend_from_slice(&hex::decode(addr.trim_start_matches("0x"))?);
    Ok(out)
}

fn usdj_vault_reserve_call_data_hf107(asset_id: u8) -> Vec<u8> {
    let mut data = hex::decode(function_selector_hex("reserveOf(uint8)")).unwrap_or_default();
    data.extend_from_slice(&abi_word_u8(asset_id));
    data
}

fn fetch_usdj_vault_reserve_hf107(rpc: &str, vault: &str, asset_id: u8) -> Result<BigUint> {
    let data = format!("0x{}", hex::encode(usdj_vault_reserve_call_data_hf107(asset_id)));
    let res = ethereum_rpc_call_any_hf102(rpc, "eth_call", serde_json::json!([{ "to": normalize_eth_address(vault)?, "data": data }, "latest"]))?;
    hex_quantity_to_biguint(res.as_str().unwrap_or("0x0"))
}

fn erc20_approve_data_hf107(spender: &str, amount: &BigUint) -> Result<Vec<u8>> {
    let mut data = hex::decode("095ea7b3")?;
    data.extend_from_slice(&abi_word_address(spender)?);
    data.extend_from_slice(&abi_word_biguint(amount)?);
    Ok(data)
}

fn usdj_vault_action_data_hf107(mode: UsdjVaultMode, asset: EthereumAsset, amount: &BigUint, receiver: &str) -> Result<Vec<u8>> {
    let signature = match mode { UsdjVaultMode::Infuse => "infuse(uint8,uint256,address)", UsdjVaultMode::Melt => "melt(uint8,uint256,address)" };
    let mut data = hex::decode(function_selector_hex(signature))?;
    let asset_id = asset.reserve_asset_id();
    data.extend_from_slice(&abi_word_u8(asset_id));
    data.extend_from_slice(&abi_word_biguint(amount)?);
    data.extend_from_slice(&abi_word_address(receiver)?);
    Ok(data)
}

fn execute_usdj_vault_action_hf107(rpc: &str, chain_id: u64, wallet: &EthereumWalletEntry, usdj_contract: &str, vault_contract: &str, stable_contract: &str, mode: UsdjVaultMode, asset: EthereumAsset, amount_text: &str, receiver: &str, gas_price_gwei: &str) -> Result<Vec<String>> {
    let usdj_contract = normalize_eth_address(usdj_contract)?;
    let vault_contract = normalize_eth_address(vault_contract)?;
    let receiver = normalize_eth_address(receiver)?;
    let asset = match asset { EthereumAsset::Usdc => EthereumAsset::Usdc, EthereumAsset::Eurc => EthereumAsset::Eurc, EthereumAsset::Eurs => EthereumAsset::Eurs, EthereumAsset::Paxg => EthereumAsset::Paxg, EthereumAsset::Xaut => EthereumAsset::Xaut, _ => EthereumAsset::Usdt };
    let stable_contract = normalize_eth_address(stable_contract)?;
    let amount_decimals = match mode {
        UsdjVaultMode::Infuse => asset.decimals(),
        UsdjVaultMode::Melt => asset.family().unwrap_or(StablecoinFamily::Usd).token_decimals(),
    };
    let amount = parse_decimal_units(amount_text, amount_decimals)?;
    let nonce_val = ethereum_rpc_call_any_hf102(rpc, "eth_getTransactionCount", serde_json::json!([wallet.address.clone(), "pending"]))?;
    let mut nonce = hex_quantity_to_biguint(nonce_val.as_str().unwrap_or("0x0"))?.to_u64().ok_or_else(|| anyhow::anyhow!("nonce too large"))?;
    let gas_price = if gas_price_gwei.trim().is_empty() {
        let gp = ethereum_rpc_call_any_hf102(rpc, "eth_gasPrice", serde_json::json!([]))?;
        hex_quantity_to_biguint(gp.as_str().unwrap_or("0x0"))?
    } else {
        parse_decimal_units(gas_price_gwei, 9)?
    };
    let mut txids = Vec::new();
    if mode == UsdjVaultMode::Infuse {
        let approve = erc20_approve_data_hf107(&vault_contract, &amount)?;
        let raw = sign_ethereum_legacy_tx_hf102(wallet, chain_id, nonce, &gas_price, FIATJ_APPROVE_GAS_LIMIT, &stable_contract, &BigUint::zero(), &approve)?;
        let sent = ethereum_rpc_call_any_hf102(rpc, "eth_sendRawTransaction", serde_json::json!([format!("0x{}", hex::encode(raw))]))?;
        let txid = sent.as_str().unwrap_or_default().to_string();
        if txid.is_empty() { anyhow::bail!("Ethereum RPC returned empty approve transaction hash"); }
        txids.push(txid);
        nonce = nonce.saturating_add(1);
    }
    let data = usdj_vault_action_data_hf107(mode, asset, &amount, &receiver)?;
    let gas_limit = match mode { UsdjVaultMode::Infuse => FIATJ_INFUSE_GAS_LIMIT, UsdjVaultMode::Melt => FIATJ_MELT_GAS_LIMIT };
    let raw = sign_ethereum_legacy_tx_hf102(wallet, chain_id, nonce, &gas_price, gas_limit, &vault_contract, &BigUint::zero(), &data)?;
    let sent = ethereum_rpc_call_any_hf102(rpc, "eth_sendRawTransaction", serde_json::json!([format!("0x{}", hex::encode(raw))]))?;
    let txid = sent.as_str().unwrap_or_default().to_string();
    if txid.is_empty() { anyhow::bail!("Ethereum RPC returned empty vault transaction hash"); }
    txids.push(txid);
    let _ = usdj_contract; // explicit normalization check; balance refresh uses this address after broadcast
    Ok(txids)
}


fn rlp_len_prefix(offset: u8, len: usize) -> Vec<u8> {
    if len < 56 { vec![offset + len as u8] } else {
        let mut be = Vec::new();
        let mut n = len;
        let mut stack = Vec::new();
        while n > 0 { stack.push((n & 0xff) as u8); n >>= 8; }
        for b in stack.iter().rev() { be.push(*b); }
        let mut out = vec![offset + 55 + be.len() as u8];
        out.extend(be);
        out
    }
}

fn rlp_encode_bytes(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() == 1 && bytes[0] < 0x80 { return vec![bytes[0]]; }
    let mut out = rlp_len_prefix(0x80, bytes.len());
    out.extend_from_slice(bytes);
    out
}

fn rlp_encode_biguint(v: &BigUint) -> Vec<u8> { if v.is_zero() { rlp_encode_bytes(&[]) } else { rlp_encode_bytes(&v.to_bytes_be()) } }
fn rlp_encode_u64(v: u64) -> Vec<u8> { rlp_encode_biguint(&BigUint::from(v)) }
fn rlp_encode_list(items: &[Vec<u8>]) -> Vec<u8> { let payload = items.concat(); let mut out = rlp_len_prefix(0xc0, payload.len()); out.extend(payload); out }

fn sign_ethereum_legacy_tx_hf102(wallet: &EthereumWalletEntry, chain_id: u64, nonce: u64, gas_price: &BigUint, gas_limit: u64, to: &str, value: &BigUint, data: &[u8]) -> Result<Vec<u8>> {
    let to_norm = normalize_eth_address(to)?;
    let to_bytes = hex::decode(to_norm.trim_start_matches("0x"))?;
    let unsigned = rlp_encode_list(&[
        rlp_encode_u64(nonce),
        rlp_encode_biguint(gas_price),
        rlp_encode_u64(gas_limit),
        rlp_encode_bytes(&to_bytes),
        rlp_encode_biguint(value),
        rlp_encode_bytes(data),
        rlp_encode_u64(chain_id),
        rlp_encode_u64(0),
        rlp_encode_u64(0),
    ]);
    let hash = keccak256(&unsigned);
    let secret = SecretKey::from_slice(&hex::decode(wallet.private_key_hex.trim_start_matches("0x"))?)?;
    let secp = Secp256k1::new();
    let msg = Message::from_digest_slice(&hash)?;
    let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, &secret);
    let (rec_id, compact) = sig.serialize_compact();
    let v = BigUint::from((rec_id.to_i32() as u64) + 35 + chain_id * 2);
    let r = BigUint::from_bytes_be(&compact[0..32]);
    let ss = BigUint::from_bytes_be(&compact[32..64]);
    let signed = rlp_encode_list(&[
        rlp_encode_u64(nonce),
        rlp_encode_biguint(gas_price),
        rlp_encode_u64(gas_limit),
        rlp_encode_bytes(&to_bytes),
        rlp_encode_biguint(value),
        rlp_encode_bytes(data),
        rlp_encode_biguint(&v),
        rlp_encode_biguint(&r),
        rlp_encode_biguint(&ss),
    ]);
    Ok(signed)
}

fn execute_ethereum_sends_hf102(rpc: &str, chain_id: u64, wallet: &EthereumWalletEntry, asset: EthereumAsset, token_contract: Option<&str>, jobs: Vec<(String, String)>, gas_price_gwei: &str) -> Result<Vec<String>> {
    let nonce_val = ethereum_rpc_call_any_hf102(rpc, "eth_getTransactionCount", serde_json::json!([wallet.address.clone(), "pending"]))?;
    let mut nonce = hex_quantity_to_biguint(nonce_val.as_str().unwrap_or("0x0"))?.to_u64().ok_or_else(|| anyhow::anyhow!("nonce too large"))?;
    let gas_price = if gas_price_gwei.trim().is_empty() {
        let gp = ethereum_rpc_call_any_hf102(rpc, "eth_gasPrice", serde_json::json!([]))?;
        hex_quantity_to_biguint(gp.as_str().unwrap_or("0x0"))?
    } else {
        parse_decimal_units(gas_price_gwei, 9)?
    };
    let mut txids = Vec::new();
    for (recipient, amount_text) in jobs {
        let recipient = normalize_eth_address(&recipient)?;
        let units = parse_decimal_units(&amount_text, asset.decimals())?;
        let (to, value, data) = if asset != EthereumAsset::Eth {
            let contract = token_contract.ok_or_else(|| anyhow::anyhow!("Paste a valid {} token contract before sending.", asset.symbol()))?;
            (normalize_eth_address(contract)?, BigUint::zero(), erc20_transfer_data(&recipient, &units)?)
        } else {
            (recipient.clone(), units, Vec::new())
        };
        let raw = sign_ethereum_legacy_tx_hf102(wallet, chain_id, nonce, &gas_price, asset.gas_limit(), &to, &value, &data)?;
        let raw_hex = format!("0x{}", hex::encode(raw));
        let sent = ethereum_rpc_call_any_hf102(rpc, "eth_sendRawTransaction", serde_json::json!([raw_hex]))?;
        let txid = sent.as_str().unwrap_or_default().to_string();
        if txid.is_empty() { anyhow::bail!("Ethereum RPC returned empty transaction hash"); }
        txids.push(txid);
        nonce = nonce.saturating_add(1);
    }
    Ok(txids)
}

fn update_button_caption(dialog: &UpdateDialog) -> String {
    match dialog.status {
        UpdateStatus::Ready => format!("Update {} ready", dialog.latest_version),
        UpdateStatus::Checking => "Checking updates".to_string(),
        UpdateStatus::Installing => "Installing update".to_string(),
        UpdateStatus::Failed => "Update issue".to_string(),
        UpdateStatus::UpToDate | UpdateStatus::Idle => "Updates".to_string(),
    }
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    fn parts(s: &str) -> Vec<u32> {
        s.trim_start_matches('v')
            .split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect()
    }
    let mut av = parts(a);
    let mut bv = parts(b);
    let n = av.len().max(bv.len());
    av.resize(n, 0);
    bv.resize(n, 0);
    av.cmp(&bv)
}

#[cfg(target_os = "windows")]
fn powershell_json(command: &str) -> Result<serde_json::Value> {
    let mut powershell = Command::new("powershell.exe");
    powershell.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", command]);
    hide_command_window(&mut powershell);
    let out = powershell.output().context("spawn powershell")?;
    if !out.status.success() {
        anyhow::bail!("PowerShell command failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let stdout = String::from_utf8(out.stdout).context("powershell stdout utf8")?;
    let cleaned = stdout
        .trim()
        .trim_start_matches('\u{feff}')
        .trim_start();
    let json_start = cleaned
        .find(|c| c == '{' || c == '[')
        .ok_or_else(|| anyhow::anyhow!("PowerShell returned no JSON. stdout='{}' stderr='{}'", cleaned, String::from_utf8_lossy(&out.stderr)))?;
    let json = &cleaned[json_start..];
    Ok(serde_json::from_str(json).context("parse powershell json")?)
}

#[cfg(target_os = "windows")]
fn windows_check_and_stage_update(url: &str, current_version: &str, last_signature: &str, allow_unsigned_update: bool) -> Result<UpdateEvent> {
    let update_url = url.trim();
    if update_url.to_ascii_lowercase().ends_with(".json") {
        return windows_check_and_stage_manifest_update(update_url, current_version, last_signature, allow_unsigned_update);
    }
    windows_check_and_stage_legacy_exe_update(update_url, current_version, last_signature, allow_unsigned_update)
}

#[cfg(target_os = "windows")]
fn windows_check_and_stage_manifest_update(manifest_url: &str, current_version: &str, last_signature: &str, allow_unsigned_update: bool) -> Result<UpdateEvent> {
    let manifest_cmd = format!(
        "$r = Invoke-WebRequest -Uri '{}' -UseBasicParsing; [string]$r.Content",
        manifest_url.replace("'", "''")
    );
    let manifest = powershell_json(&manifest_cmd)?;
    let version = manifest.get("version").and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();
    let artifact_url = manifest.get("url").and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();
    let sha256 = manifest.get("sha256").and_then(|v| v.as_str()).unwrap_or_default().trim().to_ascii_lowercase();
    let channel = manifest.get("channel").and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();
    let network = manifest.get("network").and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();
    let published_at = manifest.get("published_at").and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();

    if version.is_empty() {
        anyhow::bail!("Update manifest is missing version");
    }
    if artifact_url.is_empty() {
        anyhow::bail!("Update manifest is missing installer URL");
    }
    let expected_channel = format!("{}-windows-x64", build_channel());
    if !channel.is_empty() && channel != expected_channel {
        anyhow::bail!("Update manifest channel mismatch: {}, expected {}", channel, expected_channel);
    }
    if !network.is_empty() && network != build_channel() {
        anyhow::bail!("Update manifest network mismatch: {}, expected {}", network, build_channel());
    }

    let signature = format!("manifest|{}|{}|{}|{}", channel, version, sha256, published_at);
    if !signature.is_empty() && signature == last_signature && compare_versions(&version, current_version) != std::cmp::Ordering::Greater {
        return Ok(UpdateEvent::UpToDate { signature, message: format!("Current version {} is up to date.", current_version) });
    }
    if compare_versions(&version, current_version) != std::cmp::Ordering::Greater {
        return Ok(UpdateEvent::UpToDate { signature, message: format!("Current version {} is up to date.", current_version) });
    }

    let dest = app_write_path(UPDATE_DOWNLOAD_PATH);
    if let Some(parent) = dest.parent() { std::fs::create_dir_all(parent)?; }

    let dl_cmd = format!(
        "Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing; $item = Get-Item '{}'; $hash = (Get-FileHash -Algorithm SHA256 '{}').Hash.ToLowerInvariant(); $sig = Get-AuthenticodeSignature -FilePath '{}'; [pscustomobject]@{{version=$item.VersionInfo.ProductVersion; sha256=$hash; signature_status=[string]$sig.Status; signer=if($sig.SignerCertificate){{$sig.SignerCertificate.Subject}}else{{''}}}} | ConvertTo-Json -Compress",
        artifact_url.replace("'", "''"),
        dest.display().to_string().replace("'", "''"),
        dest.display().to_string().replace("'", "''"),
        dest.display().to_string().replace("'", "''"),
        dest.display().to_string().replace("'", "''")
    );
    let downloaded = powershell_json(&dl_cmd)?;
    let downloaded_version = downloaded.get("version").and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();
    let downloaded_sha = downloaded.get("sha256").and_then(|v| v.as_str()).unwrap_or_default().trim().to_ascii_lowercase();
    let sig_status = downloaded.get("signature_status").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let signer = downloaded.get("signer").and_then(|v| v.as_str()).unwrap_or_default().to_string();

    if !sha256.is_empty() && downloaded_sha != sha256 {
        anyhow::bail!("Downloaded installer SHA256 mismatch: got {}, expected {}", downloaded_sha, sha256);
    }
    if !downloaded_version.is_empty() && compare_versions(&downloaded_version, &version) != std::cmp::Ordering::Equal {
        anyhow::bail!("Downloaded installer version mismatch: got {}, manifest says {}", downloaded_version, version);
    }

    if !allow_unsigned_update {
        if sig_status != "Valid" {
            anyhow::bail!("Downloaded installer signature is {}, expected Valid", sig_status);
        }
        if !signer.contains("Alexander Proestakis") {
            anyhow::bail!("Downloaded installer signer mismatch: {}", signer);
        }
    }

    Ok(UpdateEvent::Ready {
        version: version.clone(),
        installer_path: dest.to_string_lossy().to_string(),
        signature,
        message: format!(
            "QUB Core {} was verified by update manifest and is ready to install{}.",
            version,
            if allow_unsigned_update && sig_status != "Valid" { " (unsigned private build accepted)" } else { "" }
        ),
    })
}

#[cfg(target_os = "windows")]
fn windows_check_and_stage_legacy_exe_update(url: &str, current_version: &str, last_signature: &str, allow_unsigned_update: bool) -> Result<UpdateEvent> {
    let head_cmd = format!(
        "$r = Invoke-WebRequest -Uri '{}' -Method Head -UseBasicParsing; $etag=[string]$r.Headers['ETag']; $lm=[string]$r.Headers['Last-Modified']; $len=[string]$r.Headers['Content-Length']; if([string]::IsNullOrWhiteSpace($len)){{$len='0'}}; [pscustomobject]@{{signature=($etag+'|'+$lm+'|'+$len); bytes=[int64]$len}} | ConvertTo-Json -Compress",
        url.replace("'", "''")
    );
    let head = powershell_json(&head_cmd)?;
    let signature = head.get("signature").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    if !signature.is_empty() && signature == last_signature {
        return Ok(UpdateEvent::UpToDate { signature, message: format!("Current version {} is up to date.", current_version) });
    }

    let dest = app_write_path(UPDATE_DOWNLOAD_PATH);
    if let Some(parent) = dest.parent() { std::fs::create_dir_all(parent)?; }

    let dl_cmd = format!(
        "Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing; $item = Get-Item '{}'; $sig = Get-AuthenticodeSignature -FilePath '{}'; [pscustomobject]@{{version=$item.VersionInfo.ProductVersion; signature_status=[string]$sig.Status; signer=if($sig.SignerCertificate){{$sig.SignerCertificate.Subject}}else{{''}}}} | ConvertTo-Json -Compress",
        url.replace("'", "''"),
        dest.display().to_string().replace("'", "''"),
        dest.display().to_string().replace("'", "''"),
        dest.display().to_string().replace("'", "''")
    );
    let downloaded = powershell_json(&dl_cmd)?;
    let version = downloaded.get("version").and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();
    let sig_status = downloaded.get("signature_status").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let signer = downloaded.get("signer").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    if compare_versions(&version, current_version) != std::cmp::Ordering::Greater {
        return Ok(UpdateEvent::UpToDate { signature, message: format!("Current version {} is up to date.", current_version) });
    }

    if !allow_unsigned_update {
        if sig_status != "Valid" {
            anyhow::bail!("Downloaded installer signature is {}, expected Valid", sig_status);
        }
        if !signer.contains("Alexander Proestakis") {
            anyhow::bail!("Downloaded installer signer mismatch: {}", signer);
        }
    }

    Ok(UpdateEvent::Ready {
        version: if version.is_empty() { "unknown".to_string() } else { version.clone() },
        installer_path: dest.to_string_lossy().to_string(),
        signature,
        message: format!("QUB Core {} was downloaded successfully and is ready to install{}.", version, if allow_unsigned_update && sig_status != "Valid" { " (unsigned private build accepted)" } else { "" }),
    })
}

#[cfg(not(target_os = "windows"))]
fn windows_check_and_stage_update(_url: &str, current_version: &str, last_signature: &str, _allow_unsigned_update: bool) -> Result<UpdateEvent> {
    Ok(UpdateEvent::UpToDate { signature: last_signature.to_string(), message: format!("Current version {} is up to date.", current_version) })
}

#[cfg(test)]
mod tests {
    use super::resource_plan;

    #[test]
    fn resource_plan_is_bounded() {
        for logical in 1..=128 {
            for pct in 1..=100 {
                let (threads, duty) = resource_plan(logical, pct);
                let cap = if pct == 100 { logical.saturating_mul(2).min(256).max(1) } else { logical };
                assert!(threads >= 1 && threads <= cap);
                assert!((1..=100).contains(&duty));
            }
        }
    }

    #[test]
    fn resource_plan_uses_all_workers_at_full_power() {
        let (threads, duty) = resource_plan(8, 100);
        assert_eq!(threads, 16);
        assert_eq!(duty, 100);
    }
}
