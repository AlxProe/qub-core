//! HF134 Windows update handoff. Loaded before eframe in helper mode.
//! Paths are arguments/data, never shell source. The original GUI stays open
//! until the copied helper has verified the installer and opened its process
//! handle; the helper waits for that exact process to exit before installation.
use super::*;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::process::Child;

pub const HELPER_FLAG: &str = "--hf134-update-helper";

#[derive(Clone, Serialize, Deserialize)]
pub struct Request {
    pub installer: PathBuf,
    pub sha256: String,
    pub bytes: u64,
    pub version: String,
    pub allow_unsigned: bool,
    pub config: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Plan {
    request: Request,
    parent_pid: u32,
    app: PathBuf,
    run: PathBuf,
    marker: PathBuf,
    result: PathBuf,
}

pub struct Handoff {
    child: Child,
    run: PathBuf,
    armed: bool,
}

impl Handoff {
    pub fn arm(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_some() { anyhow::bail!("update helper exited before handoff"); }
        write_new(&self.run.join("armed"), b"HF134\n")?;
        self.armed = true;
        Ok(())
    }
}

impl Drop for Handoff {
    fn drop(&mut self) {
        // A dropped receiver/error before GUI acknowledgement cannot install.
        if !self.armed { let _ = self.child.kill(); let _ = self.child.wait(); }
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn verify_installer(request: &Request) -> Result<File> {
    // Keep this handle until Setup exits: deny writes/deletion between hashing
    // and CreateProcess, while allowing the loader to read/execute the file.
    let held = OpenOptions::new().read(true).share_mode(1).open(&request.installer)?;
    hf128_validate_update_file_size(&request.installer, Some(request.bytes), UPDATE_INSTALLER_MAX_BYTES)?;
    if request.sha256.len() != 64 || hf128_sha256_file(&request.installer)? != request.sha256 {
        anyhow::bail!("staged installer SHA256 mismatch");
    }
    let (version, status, signer) = hf128_installer_metadata(&request.installer)?;
    if version.is_empty() || compare_versions(&version, &request.version) != std::cmp::Ordering::Equal {
        anyhow::bail!("installer version mismatch: {version}");
    }
    hf128_validate_installer_signature(&status, &signer, request.allow_unsigned)?;
    Ok(held)
}

pub fn prepare(mut request: Request) -> Result<Handoff> {
    request.installer = fs::canonicalize(&request.installer)?;
    request.config = fs::canonicalize(&request.config)?;
    if compare_versions(&request.version, APP_VERSION) != std::cmp::Ordering::Greater {
        anyhow::bail!("update must be newer than this application");
    }
    let app = fs::canonicalize(std::env::current_exe()?)?;
    let root = app.parent().context("application directory missing")?;
    let run = root.join("data/updater").join(format!("hf134-{}-{}-{}", std::process::id(),
        hf85_now_ms(), HF128_UPDATE_TEMP_NONCE.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(run.parent().context("updater directory missing")?)?;
    fs::create_dir(&run)?;
    let plan = Plan { request, parent_pid: std::process::id(), app: app.clone(), run: run.clone(),
        marker: app_write_path(HF105_POST_UPDATE_RESTART_MARKER), result: app_write_path(UPDATE_INSTALL_RESULT_PATH) };
    let helper = run.join("qub-update-helper.exe");
    fs::copy(&app, &helper)?;
    if hf128_sha256_file(&app)? != hf128_sha256_file(&helper)? { anyhow::bail!("helper copy verification failed"); }
    let plan_path = run.join("plan.json");
    write_new(&plan_path, &serde_json::to_vec(&plan)?)?;
    let mut command = Command::new(&helper);
    command.arg(HELPER_FLAG).arg(&plan_path).current_dir(root)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    hide_command_window(&mut command);
    let child = command.spawn().context("start native update helper")?;
    let mut handoff = Handoff { child, run, armed: false };
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if fs::read(handoff.run.join("ready")).ok().as_deref() == Some(b"HF134\n") {
            return Ok(handoff);
        }
        if let Some(status) = handoff.child.try_wait()? {
            let detail = fs::read_to_string(handoff.run.join("error.txt")).unwrap_or_default();
            anyhow::bail!("update helper exited ({status}): {detail}");
        }
        if Instant::now() >= deadline { anyhow::bail!("update helper readiness timed out; GUI remains open"); }
        thread::sleep(Duration::from_millis(50));
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

struct ParentHandle(*mut std::ffi::c_void);
impl ParentHandle {
    fn open(pid: u32) -> Result<Self> {
        let raw = unsafe { OpenProcess(0x00100000, 0, pid) }; // SYNCHRONIZE only
        if raw.is_null() { return Err(std::io::Error::last_os_error().into()); }
        Ok(Self(raw))
    }
    fn exited(&self) -> Result<bool> {
        match unsafe { WaitForSingleObject(self.0, 0) } {
            0 => Ok(true), 258 => Ok(false),
            _ => Err(std::io::Error::last_os_error().into()),
        }
    }
}
impl Drop for ParentHandle { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }

fn phase(plan: &Plan, message: &str) -> Result<()> {
    let mut log = OpenOptions::new().create(true).append(true).open(plan.run.join("handoff.log"))?;
    writeln!(log, "{} {message}", hf85_now_ms())?;
    log.sync_all()?;
    Ok(())
}

fn inno_path(path: &Path) -> Result<String> {
    let text = path.to_str().context("installer path is not Unicode")?;
    // canonicalize() returns verbatim Windows paths. Inno expects normal
    // absolute drive/UNC paths; Command still quotes each argument itself.
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") { return Ok(format!(r"\\{unc}")); }
    Ok(text.strip_prefix(r"\\?\").unwrap_or(text).to_string())
}

fn installer_command(plan: &Plan) -> Result<Command> {
    let mut command = Command::new(&plan.request.installer);
    command.args(["/SP-", "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART",
        "/NOCLOSEAPPLICATIONS", "/NORESTARTAPPLICATIONS", "/RESTARTEXITCODE=3010"])
        .arg(format!("/DIR={}", inno_path(plan.app.parent().context("application directory missing")?)?))
        .arg(format!("/LOG={}", inno_path(&plan.run.join("setup.log"))?));
    hide_command_window(&mut command);
    Ok(command)
}

fn apply(plan: &Plan) -> Result<()> {
    let parent = ParentHandle::open(plan.parent_pid)?;
    if parent.exited()? { anyhow::bail!("GUI exited before helper was ready"); }
    let _installer_lease = verify_installer(&plan.request)?;
    phase(plan, "VERIFIED_WAITING_FOR_GUI_ACK")?;
    write_new(&plan.run.join("ready"), b"HF134\n")?;
    let deadline = Instant::now() + Duration::from_secs(120);
    while fs::read(plan.run.join("armed")).ok().as_deref() != Some(b"HF134\n") {
        if parent.exited()? {
            // The GUI can acknowledge and exit between our two observations.
            if fs::read(plan.run.join("armed")).ok().as_deref() == Some(b"HF134\n") { break; }
            anyhow::bail!("GUI exited without acknowledging update");
        }
        if Instant::now() >= deadline { anyhow::bail!("GUI did not acknowledge update"); }
        thread::sleep(Duration::from_millis(50));
    }
    phase(plan, "ARMED_WAITING_FOR_GUI_EXIT")?;
    while !parent.exited()? {
        if Instant::now() >= deadline { anyhow::bail!("GUI did not exit; installer was not started"); }
        thread::sleep(Duration::from_millis(50));
    }
    phase(plan, "GUI_EXITED_STARTING_INSTALLER")?;
    let status = installer_command(plan)?.status().context("run verified installer")?;
    phase(plan, &format!("INSTALLER_EXITED code={:?}", status.code()))?;
    if !status.success() {
        anyhow::bail!("installer exited with {:?}; inspect setup.log. Files may be partially updated; no automatic relaunch was attempted", status.code());
    }
    let (installed, _, _) = hf128_installer_metadata(&plan.app)?;
    if installed.is_empty() || compare_versions(&installed, &plan.request.version) != std::cmp::Ordering::Equal {
        anyhow::bail!("installed GUI version mismatch: {installed}; no automatic relaunch was attempted");
    }
    phase(plan, "INSTALLED_VERSION_VERIFIED")?;
    let _ = fs::remove_file(&plan.result);
    fs::write(&plan.marker, "post-update-restart\n")?;
    let mut launch = Command::new(&plan.app);
    launch.arg("--config").arg(&plan.request.config)
        .current_dir(plan.app.parent().context("application directory missing")?);
    hide_command_window(&mut launch);
    if let Err(error) = launch.spawn() {
        let _ = fs::remove_file(&plan.marker);
        return Err(error).context("relaunch installed GUI");
    }
    phase(plan, "RELAUNCH_STARTED")?;
    Ok(())
}

pub fn helper_main(path: &Path) -> Result<()> {
    if fs::metadata(path)?.len() > 64 * 1024 { anyhow::bail!("update plan is too large"); }
    let plan: Plan = serde_json::from_slice(&fs::read(path)?)?;
    if fs::canonicalize(path.parent().context("missing plan directory")?)? != fs::canonicalize(&plan.run)? {
        anyhow::bail!("update plan directory mismatch");
    }
    let result = apply(&plan);
    if let Err(error) = &result {
        let text = format!("QUB Core update failed: {error:#}. Details: {}", plan.run.display());
        let _ = fs::write(plan.run.join("error.txt"), &text);
        let _ = fs::write(&plan.result, &text);
        let _ = phase(&plan, &format!("FAILED {error:#}"));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hf134_updater_verification_pins_bytes_until_installer_exit() {
        let root = std::env::temp_dir().join(format!("qub-hf134-lease-{}-{}", std::process::id(), hf85_now_ms()));
        fs::create_dir(&root).unwrap();
        let path = root.join("verified.exe");
        fs::copy(std::env::current_exe().unwrap(), &path).unwrap();
        let mut request = Request { installer: path.clone(), sha256: hf128_sha256_file(&path).unwrap(),
            bytes: fs::metadata(&path).unwrap().len(), version: APP_VERSION.into(), allow_unsigned: true,
            config: PathBuf::new() };
        let lease = verify_installer(&request).unwrap();
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(fs::remove_file(&path).is_err());
        drop(lease);
        assert!(OpenOptions::new().write(true).open(&path).is_ok());
        request.sha256 = "0".repeat(64);
        assert!(verify_installer(&request).unwrap_err().to_string().contains("SHA256"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn hf134_updater_parent_handle_waits_for_real_exit() {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 1"]);
        hide_command_window(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let handle = ParentHandle::open(child.id()).unwrap();
        assert!(!handle.exited().unwrap());
        child.wait().unwrap();
        assert!(handle.exited().unwrap());
    }
    #[test]
    fn hf134_updater_arguments_preserve_special_paths() {
        let plan = Plan { request: Request { installer: PathBuf::from(r"C:\QUB %home% & Ελληνικά\setup.exe"),
            config: PathBuf::from(r"C:\QUB & coins\mainnet.toml"), bytes: 1, sha256: "0".repeat(64), version: "1.8.8".into(), allow_unsigned: false },
            parent_pid: 1, app: PathBuf::from(r"C:\QUB %home% & Ελληνικά\QUB-Core.exe"),
            run: PathBuf::from(r"C:\QUB %home% & Ελληνικά\data\updater\one"), marker: PathBuf::new(), result: PathBuf::new() };
        let command = installer_command(&plan).unwrap();
        assert_eq!(command.get_program(), plan.request.installer.as_os_str());
        let args: Vec<_> = command.get_args().map(|s| s.to_string_lossy().to_string()).collect();
        assert!(args.contains(&r"/DIR=C:\QUB %home% & Ελληνικά".to_string()));
        assert!(args.contains(&"/NOCLOSEAPPLICATIONS".into()));
        assert!(!args.iter().any(|a| a == "/FORCECLOSEAPPLICATIONS"));
    }
}
