//! OpenCL GPU mining alpha for QUB Core.
//!
//! Safety design:
//! - Loaded dynamically from OpenCL.dll on Windows; no link-time OpenCL requirement.
//! - Used by the GUI miner when the GPU power slider is above 0.
//! - GPU scans candidate headers with full double-SHA256 and target comparison.
//! - CPU still verifies any found nonce with consensus `verify_header_pow` before submit.
//! - If OpenCL/device/kernel fails, CPU mining continues.

use anyhow::{anyhow, bail, Result};
use std::ffi::{c_char, c_void};
use std::ptr;

#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;

#[cfg(target_os = "windows")]
unsafe extern "system" {
    fn LoadLibraryW(lp_lib_file_name: *const u16) -> *mut c_void;
    fn GetProcAddress(h_module: *mut c_void, lp_proc_name: *const c_char) -> *mut c_void;
}

type ClInt = i32;
type ClUInt = u32;
type ClULong = u64;
type SizeT = usize;
type ClPlatformId = *mut c_void;
type ClDeviceId = *mut c_void;
type ClContext = *mut c_void;
type ClCommandQueue = *mut c_void;
type ClProgram = *mut c_void;
type ClKernel = *mut c_void;
type ClMem = *mut c_void;
type ClEvent = *mut c_void;

const CL_SUCCESS: ClInt = 0;
const CL_TRUE: ClUInt = 1;
const CL_DEVICE_TYPE_GPU: ClULong = 1 << 2;
const CL_MEM_READ_WRITE: ClULong = 1 << 0;
const CL_MEM_READ_ONLY: ClULong = 1 << 2;
const CL_DEVICE_NAME: ClUInt = 0x102B;
const CL_DEVICE_VENDOR: ClUInt = 0x102C;
const CL_DEVICE_MAX_COMPUTE_UNITS: ClUInt = 0x1002;
const CL_DEVICE_GLOBAL_MEM_SIZE: ClUInt = 0x101F;
const CL_PROGRAM_BUILD_LOG: ClUInt = 0x1183;
pub const GPU_DEVICE_ALL: &str = "All";
pub const GPU_DEVICE_ALL_DETECTED: &str = "AllDetected";
const MAX_GPU_WORK_ITEMS: usize = 2_097_152;
const HF76_FAST_WORK_ITEMS: usize = 524_288;
const MIN_GPU_WORK_ITEMS: usize = 4_096;
const GPU_WORKGROUP_ALIGN: usize = 256;

pub struct GpuScanResult {
    pub nonce: Option<u32>,
    pub hashes: u64,
}

pub fn recommended_work_items(gpu_percent: u8) -> usize {
    let pct = gpu_percent.clamp(1, 100) as usize;
    // HF78/v1.6.0: start from the proven HF76 fast lane size and let the GUI
    // miner auto-tune upward/downward per device. Returning an oversized fixed
    // batch made some laptop OpenCL drivers throttle hard, even on RTX GPUs.
    let min = MIN_GPU_WORK_ITEMS;
    let fast = HF76_FAST_WORK_ITEMS;
    let max = MAX_GPU_WORK_ITEMS;
    let work = if pct <= 50 {
        fast.saturating_mul(pct).saturating_div(50)
    } else {
        let hi_pct = pct.saturating_sub(50);
        fast.saturating_add(fast.saturating_mul(hi_pct).saturating_div(50))
    };
    align_work_items(work.max(min).min(max))
}

pub fn initial_work_items(gpu_percent: u8) -> usize {
    recommended_work_items(gpu_percent).min(HF76_FAST_WORK_ITEMS).max(MIN_GPU_WORK_ITEMS)
}

pub fn tuning_work_item_candidates(gpu_percent: u8) -> Vec<usize> {
    let pct = gpu_percent.clamp(1, 100) as usize;
    let mut candidates = vec![
        HF76_FAST_WORK_ITEMS,
        HF76_FAST_WORK_ITEMS / 2,
        HF76_FAST_WORK_ITEMS.saturating_mul(3) / 2,
        HF76_FAST_WORK_ITEMS.saturating_mul(2),
    ];
    if pct >= 90 {
        candidates.push(HF76_FAST_WORK_ITEMS.saturating_mul(3));
        candidates.push(MAX_GPU_WORK_ITEMS);
    }
    let requested = recommended_work_items(gpu_percent);
    candidates.push(requested);
    let mut out = Vec::new();
    for candidate in candidates {
        let aligned = align_work_items(candidate.max(MIN_GPU_WORK_ITEMS).min(MAX_GPU_WORK_ITEMS));
        if aligned >= MIN_GPU_WORK_ITEMS && !out.contains(&aligned) {
            out.push(aligned);
        }
    }
    out
}

pub fn align_work_items(work_items: usize) -> usize {
    if work_items < GPU_WORKGROUP_ALIGN { return work_items.max(1); }
    let aligned = work_items / GPU_WORKGROUP_ALIGN * GPU_WORKGROUP_ALIGN;
    aligned.max(GPU_WORKGROUP_ALIGN).min(MAX_GPU_WORK_ITEMS)
}

#[cfg(not(target_os = "windows"))]
pub fn available_gpu_device_labels() -> Result<Vec<String>> { Ok(Vec::new()) }

#[cfg(not(target_os = "windows"))]
pub fn preferred_gpu_device_labels() -> Result<Vec<String>> { Ok(Vec::new()) }

#[cfg(not(target_os = "windows"))]
pub struct OpenClGpuMiner;

#[cfg(not(target_os = "windows"))]
impl OpenClGpuMiner {
    pub fn new() -> Result<Self> { Self::new_for_selector(GPU_DEVICE_ALL) }
    pub fn new_for_selector(_selector: &str) -> Result<Self> { bail!("OpenCL GPU alpha is currently implemented for Windows builds only") }
    pub fn device_name(&self) -> &str { "unavailable" }
    pub fn scan_nonce_range(&mut self, _prefix: &[u8; 76], _target: &[u8; 32], _start_nonce: u64, _work_items: usize) -> Result<GpuScanResult> {
        bail!("OpenCL GPU alpha is currently implemented for Windows builds only")
    }
}

#[cfg(target_os = "windows")]
pub struct OpenClGpuMiner {
    api: OpenClApi,
    context: ClContext,
    queue: ClCommandQueue,
    program: ClProgram,
    kernel: ClKernel,
    midstate_mem: ClMem,
    tail_mem: ClMem,
    target_mem: ClMem,
    result_mem: ClMem,
    device_name: String,
    result_host: [ClUInt; 2],
    cached_prefix: Option<[u8; 76]>,
    cached_target: Option<[u8; 32]>,
    kernel_args_bound: bool,
}

#[cfg(target_os = "windows")]
pub fn available_gpu_device_labels() -> Result<Vec<String>> {
    let api = OpenClApi::load()?;
    unsafe {
        let devices = enumerate_gpu_devices(&api)?;
        Ok(devices.into_iter().map(|d| d.display_name).collect())
    }
}

#[cfg(target_os = "windows")]
pub fn preferred_gpu_device_labels() -> Result<Vec<String>> {
    let api = OpenClApi::load()?;
    unsafe {
        let devices = enumerate_gpu_devices(&api)?;
        Ok(preferred_device_labels(&devices))
    }
}

#[cfg(target_os = "windows")]
impl OpenClGpuMiner {
    pub fn new() -> Result<Self> { Self::new_for_selector(GPU_DEVICE_ALL) }

    pub fn new_for_selector(selector: &str) -> Result<Self> {
        let api = OpenClApi::load()?;
        unsafe {
            let selected = select_gpu_device(&api, selector)?;
            let selected_device = selected.device;
            let display_name = selected.display_name;

            let mut err: ClInt = 0;
            let context = (api.cl_create_context)(ptr::null(), 1, &selected_device, None, ptr::null_mut(), &mut err);
            check(err, "clCreateContext")?;
            if context.is_null() { bail!("clCreateContext returned null"); }

            let queue = (api.cl_create_command_queue)(context, selected_device, 0, &mut err);
            check(err, "clCreateCommandQueue")?;
            if queue.is_null() { bail!("clCreateCommandQueue returned null"); }

            let src = OPENCL_KERNEL.as_bytes();
            let src_ptr = src.as_ptr() as *const c_char;
            let src_len = src.len();
            let program = (api.cl_create_program_with_source)(context, 1, &src_ptr, &src_len, &mut err);
            check(err, "clCreateProgramWithSource")?;
            if program.is_null() { bail!("clCreateProgramWithSource returned null"); }

            let build_opts = b"-cl-std=CL1.2\0";
            let build_rc = (api.cl_build_program)(program, 1, &selected_device, build_opts.as_ptr() as *const c_char, None, ptr::null_mut());
            if build_rc != CL_SUCCESS {
                let log = program_build_log(&api, program, selected_device).unwrap_or_else(|_| "<no OpenCL build log>".to_string());
                bail!("OpenCL kernel build failed ({build_rc}): {log}");
            }

            let kernel_name = b"scan_qub_fullsha\0";
            let kernel = (api.cl_create_kernel)(program, kernel_name.as_ptr() as *const c_char, &mut err);
            check(err, "clCreateKernel")?;
            if kernel.is_null() { bail!("clCreateKernel returned null"); }

            let midstate_mem = create_buffer(&api, context, CL_MEM_READ_ONLY, 8 * std::mem::size_of::<ClUInt>())?;
            let tail_mem = create_buffer(&api, context, CL_MEM_READ_ONLY, 12)?;
            let target_mem = create_buffer(&api, context, CL_MEM_READ_ONLY, 32)?;
            let result_mem = create_buffer(&api, context, CL_MEM_READ_WRITE, 2 * std::mem::size_of::<ClUInt>())?;

            Ok(Self {
                api,
                context,
                queue,
                program,
                kernel,
                midstate_mem,
                tail_mem,
                target_mem,
                result_mem,
                device_name: display_name,
                result_host: [0u32; 2],
                cached_prefix: None,
                cached_target: None,
                kernel_args_bound: false,
            })
        }
    }

    pub fn device_name(&self) -> &str { &self.device_name }

    pub fn scan_nonce_range(&mut self, prefix: &[u8; 76], target: &[u8; 32], start_nonce: u64, work_items: usize) -> Result<GpuScanResult> {
        if start_nonce > u32::MAX as u64 {
            return Ok(GpuScanResult { nonce: None, hashes: 0 });
        }
        let remaining = (u32::MAX as u64 + 1).saturating_sub(start_nonce);
        let work_items = align_work_items(work_items.max(1).min(MAX_GPU_WORK_ITEMS).min(remaining as usize).max(1));
        if work_items == 0 {
            return Ok(GpuScanResult { nonce: None, hashes: 0 });
        }

        unsafe {
            if self.cached_prefix.as_ref() != Some(prefix) {
                let midstate = sha256_midstate_first64(prefix);
                let mut tail = [0u8; 12];
                tail.copy_from_slice(&prefix[64..76]);
                check((self.api.cl_enqueue_write_buffer)(self.queue, self.midstate_mem, CL_TRUE, 0, midstate.len() * std::mem::size_of::<ClUInt>(), midstate.as_ptr() as *const c_void, 0, ptr::null(), ptr::null_mut()), "clEnqueueWriteBuffer(midstate)")?;
                check((self.api.cl_enqueue_write_buffer)(self.queue, self.tail_mem, CL_TRUE, 0, tail.len(), tail.as_ptr() as *const c_void, 0, ptr::null(), ptr::null_mut()), "clEnqueueWriteBuffer(tail)")?;
                self.cached_prefix = Some(*prefix);
                self.kernel_args_bound = false;
            }
            if self.cached_target.as_ref() != Some(target) {
                check((self.api.cl_enqueue_write_buffer)(self.queue, self.target_mem, CL_TRUE, 0, target.len(), target.as_ptr() as *const c_void, 0, ptr::null(), ptr::null_mut()), "clEnqueueWriteBuffer(target)")?;
                self.cached_target = Some(*target);
                self.kernel_args_bound = false;
            }

            let zero_result = [0u32; 2];
            check((self.api.cl_enqueue_write_buffer)(self.queue, self.result_mem, CL_TRUE, 0, zero_result.len() * std::mem::size_of::<ClUInt>(), zero_result.as_ptr() as *const c_void, 0, ptr::null(), ptr::null_mut()), "clEnqueueWriteBuffer(result-reset)")?;

            if !self.kernel_args_bound {
                set_kernel_arg(&self.api, self.kernel, 0, &self.midstate_mem)?;
                set_kernel_arg(&self.api, self.kernel, 1, &self.tail_mem)?;
                set_kernel_arg(&self.api, self.kernel, 2, &self.target_mem)?;
                set_kernel_arg(&self.api, self.kernel, 4, &self.result_mem)?;
                self.kernel_args_bound = true;
            }
            let start: ClUInt = start_nonce as ClUInt;
            set_kernel_arg(&self.api, self.kernel, 3, &start)?;

            let global = work_items;
            check((self.api.cl_enqueue_nd_range_kernel)(self.queue, self.kernel, 1, ptr::null(), &global, ptr::null(), 0, ptr::null(), ptr::null_mut()), "clEnqueueNDRangeKernel")?;
            check((self.api.cl_finish)(self.queue), "clFinish")?;

            check((self.api.cl_enqueue_read_buffer)(self.queue, self.result_mem, CL_TRUE, 0, self.result_host.len() * std::mem::size_of::<ClUInt>(), self.result_host.as_mut_ptr() as *mut c_void, 0, ptr::null(), ptr::null_mut()), "clEnqueueReadBuffer(result)")?;
        }

        let nonce = if self.result_host[0] != 0 {
            let candidate = self.result_host[1] as u64;
            if candidate >= start_nonce && candidate < start_nonce.saturating_add(work_items as u64) {
                Some(self.result_host[1])
            } else {
                None
            }
        } else {
            None
        };
        Ok(GpuScanResult { nonce, hashes: work_items as u64 })
    }
}

#[cfg(target_os = "windows")]
impl Drop for OpenClGpuMiner {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.api.cl_release_mem_object)(self.midstate_mem);
            let _ = (self.api.cl_release_mem_object)(self.tail_mem);
            let _ = (self.api.cl_release_mem_object)(self.target_mem);
            let _ = (self.api.cl_release_mem_object)(self.result_mem);
            let _ = (self.api.cl_release_kernel)(self.kernel);
            let _ = (self.api.cl_release_program)(self.program);
            let _ = (self.api.cl_release_command_queue)(self.queue);
            let _ = (self.api.cl_release_context)(self.context);
        }
    }
}

#[cfg(target_os = "windows")]
struct OpenClApi {
    _lib: *mut c_void,
    cl_get_platform_ids: unsafe extern "system" fn(ClUInt, *mut ClPlatformId, *mut ClUInt) -> ClInt,
    cl_get_device_ids: unsafe extern "system" fn(ClPlatformId, ClULong, ClUInt, *mut ClDeviceId, *mut ClUInt) -> ClInt,
    cl_get_device_info: unsafe extern "system" fn(ClDeviceId, ClUInt, SizeT, *mut c_void, *mut SizeT) -> ClInt,
    cl_create_context: unsafe extern "system" fn(*const isize, ClUInt, *const ClDeviceId, Option<unsafe extern "system" fn(*const c_char, *const c_void, SizeT, *mut c_void)>, *mut c_void, *mut ClInt) -> ClContext,
    cl_create_command_queue: unsafe extern "system" fn(ClContext, ClDeviceId, ClULong, *mut ClInt) -> ClCommandQueue,
    cl_create_program_with_source: unsafe extern "system" fn(ClContext, ClUInt, *const *const c_char, *const SizeT, *mut ClInt) -> ClProgram,
    cl_build_program: unsafe extern "system" fn(ClProgram, ClUInt, *const ClDeviceId, *const c_char, Option<unsafe extern "system" fn(ClProgram, *mut c_void)>, *mut c_void) -> ClInt,
    cl_get_program_build_info: unsafe extern "system" fn(ClProgram, ClDeviceId, ClUInt, SizeT, *mut c_void, *mut SizeT) -> ClInt,
    cl_create_kernel: unsafe extern "system" fn(ClProgram, *const c_char, *mut ClInt) -> ClKernel,
    cl_create_buffer: unsafe extern "system" fn(ClContext, ClULong, SizeT, *mut c_void, *mut ClInt) -> ClMem,
    cl_set_kernel_arg: unsafe extern "system" fn(ClKernel, ClUInt, SizeT, *const c_void) -> ClInt,
    cl_enqueue_write_buffer: unsafe extern "system" fn(ClCommandQueue, ClMem, ClUInt, SizeT, SizeT, *const c_void, ClUInt, *const ClEvent, *mut ClEvent) -> ClInt,
    cl_enqueue_read_buffer: unsafe extern "system" fn(ClCommandQueue, ClMem, ClUInt, SizeT, SizeT, *mut c_void, ClUInt, *const ClEvent, *mut ClEvent) -> ClInt,
    cl_enqueue_nd_range_kernel: unsafe extern "system" fn(ClCommandQueue, ClKernel, ClUInt, *const SizeT, *const SizeT, *const SizeT, ClUInt, *const ClEvent, *mut ClEvent) -> ClInt,
    cl_finish: unsafe extern "system" fn(ClCommandQueue) -> ClInt,
    cl_release_mem_object: unsafe extern "system" fn(ClMem) -> ClInt,
    cl_release_kernel: unsafe extern "system" fn(ClKernel) -> ClInt,
    cl_release_program: unsafe extern "system" fn(ClProgram) -> ClInt,
    cl_release_command_queue: unsafe extern "system" fn(ClCommandQueue) -> ClInt,
    cl_release_context: unsafe extern "system" fn(ClContext) -> ClInt,
}

#[cfg(target_os = "windows")]
impl OpenClApi {
    fn load() -> Result<Self> {
        unsafe {
            let wide: Vec<u16> = std::ffi::OsStr::new("OpenCL.dll").encode_wide().chain(Some(0)).collect();
            let lib = LoadLibraryW(wide.as_ptr());
            if lib.is_null() { bail!("OpenCL.dll not found. Install/update AMD/Nvidia GPU drivers with OpenCL support."); }
            Ok(Self {
                _lib: lib,
                cl_get_platform_ids: load_fn(lib, b"clGetPlatformIDs\0")?,
                cl_get_device_ids: load_fn(lib, b"clGetDeviceIDs\0")?,
                cl_get_device_info: load_fn(lib, b"clGetDeviceInfo\0")?,
                cl_create_context: load_fn(lib, b"clCreateContext\0")?,
                cl_create_command_queue: load_fn(lib, b"clCreateCommandQueue\0")?,
                cl_create_program_with_source: load_fn(lib, b"clCreateProgramWithSource\0")?,
                cl_build_program: load_fn(lib, b"clBuildProgram\0")?,
                cl_get_program_build_info: load_fn(lib, b"clGetProgramBuildInfo\0")?,
                cl_create_kernel: load_fn(lib, b"clCreateKernel\0")?,
                cl_create_buffer: load_fn(lib, b"clCreateBuffer\0")?,
                cl_set_kernel_arg: load_fn(lib, b"clSetKernelArg\0")?,
                cl_enqueue_write_buffer: load_fn(lib, b"clEnqueueWriteBuffer\0")?,
                cl_enqueue_read_buffer: load_fn(lib, b"clEnqueueReadBuffer\0")?,
                cl_enqueue_nd_range_kernel: load_fn(lib, b"clEnqueueNDRangeKernel\0")?,
                cl_finish: load_fn(lib, b"clFinish\0")?,
                cl_release_mem_object: load_fn(lib, b"clReleaseMemObject\0")?,
                cl_release_kernel: load_fn(lib, b"clReleaseKernel\0")?,
                cl_release_program: load_fn(lib, b"clReleaseProgram\0")?,
                cl_release_command_queue: load_fn(lib, b"clReleaseCommandQueue\0")?,
                cl_release_context: load_fn(lib, b"clReleaseContext\0")?,
            })
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
struct CandidateDevice {
    device: ClDeviceId,
    display_name: String,
    score: u128,
}

#[cfg(target_os = "windows")]
unsafe fn enumerate_gpu_devices(api: &OpenClApi) -> Result<Vec<CandidateDevice>> {
    let mut platform_count: ClUInt = 0;
    check((api.cl_get_platform_ids)(0, ptr::null_mut(), &mut platform_count), "clGetPlatformIDs(count)")?;
    if platform_count == 0 { bail!("No OpenCL platforms found"); }
    let mut platforms = vec![ptr::null_mut(); platform_count as usize];
    check((api.cl_get_platform_ids)(platform_count, platforms.as_mut_ptr(), ptr::null_mut()), "clGetPlatformIDs(list)")?;

    let mut out: Vec<CandidateDevice> = Vec::new();
    for platform in platforms {
        let mut device_count: ClUInt = 0;
        let rc = (api.cl_get_device_ids)(platform, CL_DEVICE_TYPE_GPU, 0, ptr::null_mut(), &mut device_count);
        if rc != CL_SUCCESS || device_count == 0 { continue; }
        let mut devices = vec![ptr::null_mut(); device_count as usize];
        check((api.cl_get_device_ids)(platform, CL_DEVICE_TYPE_GPU, device_count, devices.as_mut_ptr(), ptr::null_mut()), "clGetDeviceIDs(list)")?;
        for device in devices {
            let name = device_info_string(api, device, CL_DEVICE_NAME).unwrap_or_else(|_| "OpenCL GPU".to_string());
            let vendor = device_info_string(api, device, CL_DEVICE_VENDOR).unwrap_or_default();
            let cu = device_info_u32(api, device, CL_DEVICE_MAX_COMPUTE_UNITS).unwrap_or(1).max(1);
            let mem = device_info_u64(api, device, CL_DEVICE_GLOBAL_MEM_SIZE).unwrap_or(0);
            let vendor_l = vendor.to_ascii_lowercase();
            let name_l = name.to_ascii_lowercase();
            let mut score = (cu as u128) * 100_000u128 + (mem as u128 / (16 * 1024 * 1024) as u128);
            if vendor_l.contains("nvidia") || name_l.contains("nvidia") || name_l.contains("rtx") || name_l.contains("gtx") {
                score += 10_000_000_000u128;
            }
            if vendor_l.contains("advanced micro devices") || vendor_l.contains("amd") || name_l.contains("radeon") {
                score += 1_000_000_000u128;
            }
            let mem_gb = (mem as f64) / (1024.0 * 1024.0 * 1024.0);
            let detail = if vendor.is_empty() {
                format!("{} ({} CU, {:.1} GB)", name, cu, mem_gb)
            } else {
                format!("{} ({}, {} CU, {:.1} GB)", name, vendor, cu, mem_gb)
            };
            out.push(CandidateDevice { device, display_name: detail, score });
        }
    }
    if out.is_empty() { bail!("No OpenCL GPU device found"); }
    out.sort_by(|a, b| b.score.cmp(&a.score).then(a.display_name.cmp(&b.display_name)));
    for (idx, cand) in out.iter_mut().enumerate() {
        let detail = cand.display_name.clone();
        let tag = if idx == 0 { " (fastest)" } else { "" };
        cand.display_name = format!("GPU #{}{}: {}", idx + 1, tag, detail);
    }
    Ok(out)
}

#[cfg(target_os = "windows")]
fn preferred_device_labels(devices: &[CandidateDevice]) -> Vec<String> {
    let has_nvidia = devices.iter().any(|d| {
        let label = d.display_name.to_ascii_lowercase();
        label.contains("nvidia") || label.contains("rtx") || label.contains("gtx")
    });
    let preferred: Vec<String> = if has_nvidia {
        devices.iter()
            .filter(|d| {
                let label = d.display_name.to_ascii_lowercase();
                label.contains("nvidia") || label.contains("rtx") || label.contains("gtx")
            })
            .map(|d| d.display_name.clone())
            .collect()
    } else {
        devices.iter().map(|d| d.display_name.clone()).collect()
    };
    if preferred.is_empty() {
        devices.iter().map(|d| d.display_name.clone()).collect()
    } else {
        preferred
    }
}

#[cfg(target_os = "windows")]
fn normalize_gpu_label(label: &str) -> String {
    let lower = label.to_ascii_lowercase();
    let detail = lower.split_once(':').map(|(_, rhs)| rhs).unwrap_or(lower.as_str());
    detail.replace("(fastest)", "").split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(target_os = "windows")]
unsafe fn select_gpu_device(api: &OpenClApi, selector: &str) -> Result<CandidateDevice> {
    let selector = selector.trim();
    let mut devices = enumerate_gpu_devices(api)?;
    if !selector.is_empty() && !selector.eq_ignore_ascii_case(GPU_DEVICE_ALL) {
        if let Some(pos) = devices.iter().position(|d| d.display_name == selector) {
            return Ok(devices.remove(pos));
        }
        let selector_l = selector.to_ascii_lowercase();
        let selector_key = normalize_gpu_label(selector);
        if let Some(pos) = devices.iter().position(|d| normalize_gpu_label(&d.display_name) == selector_key) {
            return Ok(devices.remove(pos));
        }
        if let Some(pos) = devices.iter().position(|d| {
            let label_l = d.display_name.to_ascii_lowercase();
            let key = normalize_gpu_label(&d.display_name);
            label_l.contains(&selector_l) || selector_l.contains(&key)
        }) {
            return Ok(devices.remove(pos));
        }
    }
    devices.into_iter().next().ok_or_else(|| anyhow!("No OpenCL GPU device found"))
}

#[cfg(target_os = "windows")]
unsafe fn load_fn<T>(lib: *mut c_void, name: &'static [u8]) -> Result<T> {
    let ptr = GetProcAddress(lib, name.as_ptr() as *const c_char);
    if ptr.is_null() {
        bail!("OpenCL function not found: {}", String::from_utf8_lossy(&name[..name.len().saturating_sub(1)]));
    }
    Ok(std::mem::transmute_copy(&ptr))
}

#[cfg(target_os = "windows")]
fn check(code: ClInt, name: &str) -> Result<()> {
    if code == CL_SUCCESS { Ok(()) } else { Err(anyhow!("{name} failed with OpenCL error {code}")) }
}

#[cfg(target_os = "windows")]
unsafe fn create_buffer(api: &OpenClApi, context: ClContext, flags: ClULong, size: usize) -> Result<ClMem> {
    let mut err = 0;
    let mem = (api.cl_create_buffer)(context, flags, size, ptr::null_mut(), &mut err);
    check(err, "clCreateBuffer")?;
    if mem.is_null() { bail!("clCreateBuffer returned null"); }
    Ok(mem)
}

#[cfg(target_os = "windows")]
unsafe fn set_kernel_arg<T>(api: &OpenClApi, kernel: ClKernel, idx: ClUInt, value: &T) -> Result<()> {
    check((api.cl_set_kernel_arg)(kernel, idx, std::mem::size_of::<T>(), value as *const _ as *const c_void), "clSetKernelArg")
}

#[cfg(target_os = "windows")]
unsafe fn device_info_string(api: &OpenClApi, device: ClDeviceId, param: ClUInt) -> Result<String> {
    let mut len = 0usize;
    check((api.cl_get_device_info)(device, param, 0, ptr::null_mut(), &mut len), "clGetDeviceInfo(size)")?;
    if len == 0 { return Ok(String::new()); }
    let mut buf = vec![0u8; len];
    check((api.cl_get_device_info)(device, param, len, buf.as_mut_ptr() as *mut c_void, ptr::null_mut()), "clGetDeviceInfo(value)")?;
    while buf.last() == Some(&0) { buf.pop(); }
    Ok(String::from_utf8_lossy(&buf).trim().to_string())
}

#[cfg(target_os = "windows")]
unsafe fn device_info_u32(api: &OpenClApi, device: ClDeviceId, param: ClUInt) -> Result<u32> {
    let mut value = 0u32;
    check((api.cl_get_device_info)(device, param, std::mem::size_of::<u32>(), &mut value as *mut _ as *mut c_void, ptr::null_mut()), "clGetDeviceInfo(u32)")?;
    Ok(value)
}

#[cfg(target_os = "windows")]
unsafe fn device_info_u64(api: &OpenClApi, device: ClDeviceId, param: ClUInt) -> Result<u64> {
    let mut value = 0u64;
    check((api.cl_get_device_info)(device, param, std::mem::size_of::<u64>(), &mut value as *mut _ as *mut c_void, ptr::null_mut()), "clGetDeviceInfo(u64)")?;
    Ok(value)
}

#[cfg(target_os = "windows")]
unsafe fn program_build_log(api: &OpenClApi, program: ClProgram, device: ClDeviceId) -> Result<String> {
    let mut len = 0usize;
    let _ = (api.cl_get_program_build_info)(program, device, CL_PROGRAM_BUILD_LOG, 0, ptr::null_mut(), &mut len);
    if len == 0 { return Ok(String::new()); }
    let mut buf = vec![0u8; len];
    let _ = (api.cl_get_program_build_info)(program, device, CL_PROGRAM_BUILD_LOG, len, buf.as_mut_ptr() as *mut c_void, ptr::null_mut());
    while buf.last() == Some(&0) { buf.pop(); }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

#[cfg(target_os = "windows")]
const SHA256_K_CPU: [u32; 64] = [
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
    0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
    0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
    0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
    0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2,
];

#[cfg(target_os = "windows")]
fn sha256_midstate_first64(prefix: &[u8; 76]) -> [u32; 8] {
    let mut state = [
        0x6a09e667u32, 0xbb67ae85u32, 0x3c6ef372u32, 0xa54ff53au32,
        0x510e527fu32, 0x9b05688cu32, 0x1f83d9abu32, 0x5be0cd19u32,
    ];
    let mut w = [0u32; 64];
    for i in 0..16 {
        let off = i * 4;
        w[i] = u32::from_be_bytes([prefix[off], prefix[off+1], prefix[off+2], prefix[off+3]]);
    }
    for i in 16..64 {
        let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
        let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
        w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
    }
    let mut a=state[0]; let mut b=state[1]; let mut c=state[2]; let mut d=state[3];
    let mut e=state[4]; let mut f=state[5]; let mut g=state[6]; let mut h=state[7];
    for i in 0..64 {
        let ch = (e & f) ^ ((!e) & g);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA256_K_CPU[i]).wrapping_add(w[i]);
        let t2 = s0.wrapping_add(maj);
        h=g; g=f; f=e; e=d.wrapping_add(t1); d=c; c=b; b=a; a=t1.wrapping_add(t2);
    }
    state[0]=state[0].wrapping_add(a); state[1]=state[1].wrapping_add(b); state[2]=state[2].wrapping_add(c); state[3]=state[3].wrapping_add(d);
    state[4]=state[4].wrapping_add(e); state[5]=state[5].wrapping_add(f); state[6]=state[6].wrapping_add(g); state[7]=state[7].wrapping_add(h);
    state
}

#[cfg(target_os = "windows")]
const OPENCL_KERNEL: &str = r#"
// QUB OpenCL GPU kernel HF78/v1.6.0 / HF80 v1.6.2
// GPU performs full double-SHA256(header) and compact-target comparison.
__constant uint K[64] = {
  0x428a2f98U,0x71374491U,0xb5c0fbcfU,0xe9b5dba5U,0x3956c25bU,0x59f111f1U,0x923f82a4U,0xab1c5ed5U,
  0xd807aa98U,0x12835b01U,0x243185beU,0x550c7dc3U,0x72be5d74U,0x80deb1feU,0x9bdc06a7U,0xc19bf174U,
  0xe49b69c1U,0xefbe4786U,0x0fc19dc6U,0x240ca1ccU,0x2de92c6fU,0x4a7484aaU,0x5cb0a9dcU,0x76f988daU,
  0x983e5152U,0xa831c66dU,0xb00327c8U,0xbf597fc7U,0xc6e00bf3U,0xd5a79147U,0x06ca6351U,0x14292967U,
  0x27b70a85U,0x2e1b2138U,0x4d2c6dfcU,0x53380d13U,0x650a7354U,0x766a0abbU,0x81c2c92eU,0x92722c85U,
  0xa2bfe8a1U,0xa81a664bU,0xc24b8b70U,0xc76c51a3U,0xd192e819U,0xd6990624U,0xf40e3585U,0x106aa070U,
  0x19a4c116U,0x1e376c08U,0x2748774cU,0x34b0bcb5U,0x391c0cb3U,0x4ed8aa4aU,0x5b9cca4fU,0x682e6ff3U,
  0x748f82eeU,0x78a5636fU,0x84c87814U,0x8cc70208U,0x90befffaU,0xa4506cebU,0xbef9a3f7U,0xc67178f2U
};
uint rotr(uint x, uint n) { return (x >> n) | (x << (32U - n)); }
uint ch(uint x, uint y, uint z) { return (x & y) ^ (~x & z); }
uint maj(uint x, uint y, uint z) { return (x & y) ^ (x & z) ^ (y & z); }
uint bsig0(uint x) { return rotr(x, 2U) ^ rotr(x, 13U) ^ rotr(x, 22U); }
uint bsig1(uint x) { return rotr(x, 6U) ^ rotr(x, 11U) ^ rotr(x, 25U); }
uint ssig0(uint x) { return rotr(x, 7U) ^ rotr(x, 18U) ^ (x >> 3U); }
uint ssig1(uint x) { return rotr(x, 17U) ^ rotr(x, 19U) ^ (x >> 10U); }
uint load_be(__global const uchar *p, int off) { return ((uint)p[off] << 24) | ((uint)p[off+1] << 16) | ((uint)p[off+2] << 8) | (uint)p[off+3]; }
uint nonce_header_word(uint nonce) {
  return ((nonce & 0xffU) << 24) | (((nonce >> 8) & 0xffU) << 16) | (((nonce >> 16) & 0xffU) << 8) | ((nonce >> 24) & 0xffU);
}
void sha256_init(__private uint s[8]) {
  s[0]=0x6a09e667U; s[1]=0xbb67ae85U; s[2]=0x3c6ef372U; s[3]=0xa54ff53aU;
  s[4]=0x510e527fU; s[5]=0x9b05688cU; s[6]=0x1f83d9abU; s[7]=0x5be0cd19U;
}
void sha256_compress16(__private uint s[8], __private uint w[16]) {
  uint a=s[0], b=s[1], c=s[2], d=s[3], e=s[4], f=s[5], g=s[6], h=s[7];
  for (uint i=0U; i<64U; i++) {
    uint wi;
    if (i < 16U) {
      wi = w[i];
    } else {
      uint idx = i & 15U;
      wi = w[idx] + ssig0(w[(i + 1U) & 15U]) + w[(i + 9U) & 15U] + ssig1(w[(i + 14U) & 15U]);
      w[idx] = wi;
    }
    uint t1 = h + bsig1(e) + ch(e,f,g) + K[i] + wi;
    uint t2 = bsig0(a) + maj(a,b,c);
    h = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2;
  }
  s[0]+=a; s[1]+=b; s[2]+=c; s[3]+=d; s[4]+=e; s[5]+=f; s[6]+=g; s[7]+=h;
}
void sha256_second_from_first_state(__private uint first[8], __private uint out[8]) {
  sha256_init(out);
  uint w[16];
  for (int i=0; i<16; i++) w[i] = 0U;
  for (int i=0; i<8; i++) w[i] = first[i];
  w[8] = 0x80000000U;
  w[15] = 256U;
  sha256_compress16(out, w);
}
uchar digest_reversed_byte(__private uint digest[8], int target_index) {
  int be_index = 31 - target_index;
  int word = be_index / 4;
  int off = be_index - word * 4;
  return (uchar)((digest[word] >> (24 - off * 8)) & 0xffU);
}
int digest_meets_target(__private uint digest[8], __global const uchar *target) {
  for (int i=0; i<32; i++) {
    uchar h = digest_reversed_byte(digest, i);
    uchar t = target[i];
    if (h < t) return 1;
    if (h > t) return 0;
  }
  return 1;
}
uint load_mid(__global const uint *midstate, int i) { return midstate[i]; }
__kernel void scan_qub_fullsha(__global const uint *midstate,
                               __global const uchar *tail,
                               __global const uchar *target,
                               uint start_nonce,
                               __global uint *result) {
  uint gid = (uint)get_global_id(0);
  uint nonce = start_nonce + gid;
  uint s[8];
  for (int i=0; i<8; i++) s[i] = load_mid(midstate, i);
  uint w[16];
  for (int i=0; i<16; i++) w[i] = 0U;
  w[0] = load_be(tail, 0);
  w[1] = load_be(tail, 4);
  w[2] = load_be(tail, 8);
  w[3] = nonce_header_word(nonce);
  w[4] = 0x80000000U;
  w[15] = 640U;
  sha256_compress16(s, w);
  uint second[8];
  sha256_second_from_first_state(s, second);
  if (digest_meets_target(second, target)) {
    result[1] = nonce;
    result[0] = 1U;
  }
}
"#;
