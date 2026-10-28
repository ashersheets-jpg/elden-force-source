//! Lab only (2.0.6, the DX12 "device removed" crash hunt: c0000005 after DXGI_ERROR_DEVICE_REMOVED, INVALID_CALL): with
//! d3d12debug.on beside the log, Direct3D 12's debug layer and DRED (device removed breadcrumbs, page faults) are turned on
//! at load, before the game makes its device (Windows' Graphics Tools must be installed: d3d12SDKLayers.dll). The layer's
//! messages go out through OutputDebugString; veh.rs writes them into the log ("d3d12: ..."). d3d12gbv.on as well adds
//! GPU-based validation (much slower; it catches bad descriptors and resource states on the GPU itself).

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::core::GUID;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

/// the layer is on: veh.rs logs debugger prints
pub static ON: AtomicBool = AtomicBool::new(false);

const IID_ID3D12_DEBUG1: GUID = GUID::from_u128(0xaffaa4ca_63fe_4d8e_b8ad_159000af4304);
const IID_ID3D12_DEBUG: GUID = GUID::from_u128(0x344488b7_6846_474b_b989_f027448245e0);
const IID_DRED_SETTINGS: GUID = GUID::from_u128(0x82bc481c_6b9b_4030_aedb_7ee3d1df1e63);

type GetDebugInterface = unsafe extern "system" fn(*const GUID, *mut *mut c_void) -> i32;

/// the COM object's method `i` (its vtable's entry)
unsafe fn method<T: Copy>(obj: *mut c_void, i: usize) -> T {
    let vt = unsafe { *(obj as *const *const usize) };
    unsafe { std::mem::transmute_copy(&*vt.add(i)) }
}

unsafe fn release(obj: *mut c_void) {
    let f: unsafe extern "system" fn(*mut c_void) -> u32 = unsafe { method(obj, 2) };
    unsafe { f(obj) };
}

pub fn enable() {
    let dir = crate::log::dir();
    if !dir.join("d3d12debug.on").exists() {
        return;
    }
    let gbv = dir.join("d3d12gbv.on").exists();
    let lib = unsafe { LoadLibraryW(windows_sys::w!("d3d12.dll")) };
    if lib.is_null() {
        crate::log!("d3d12 debug: d3d12.dll didn't load");
        return;
    }
    let Some(get) = (unsafe { GetProcAddress(lib, windows_sys::s!("D3D12GetDebugInterface")) }) else {
        crate::log!("d3d12 debug: no D3D12GetDebugInterface");
        return;
    };
    let get: GetDebugInterface = unsafe { std::mem::transmute(get) };
    ON.store(true, Ordering::Relaxed);
    // the debug layer (ID3D12Debug::EnableDebugLayer, then ID3D12Debug1::SetEnableGPUBasedValidation)
    let mut dbg: *mut c_void = std::ptr::null_mut();
    let hr = unsafe { get(&IID_ID3D12_DEBUG, &mut dbg) };
    if hr >= 0 && !dbg.is_null() {
        let enable: unsafe extern "system" fn(*mut c_void) = unsafe { method(dbg, 3) };
        unsafe { enable(dbg) };
        let mut dbg1: *mut c_void = std::ptr::null_mut();
        if gbv && unsafe { get(&IID_ID3D12_DEBUG1, &mut dbg1) } >= 0 && !dbg1.is_null() {
            let set_gbv: unsafe extern "system" fn(*mut c_void, i32) = unsafe { method(dbg1, 4) };
            unsafe { set_gbv(dbg1, 1) };
            unsafe { release(dbg1) };
        }
        unsafe { release(dbg) };
        crate::log!("d3d12 debug: debug layer on (GPU-based validation {gbv})");
    } else {
        crate::log!("d3d12 debug: no debug layer ({hr:#x}): Graphics Tools not installed?");
    }
    // DRED: breadcrumbs and page faults, forced on (D3D12_DRED_ENABLEMENT_FORCED_ON = 2)
    let mut dred: *mut c_void = std::ptr::null_mut();
    let hr = unsafe { get(&IID_DRED_SETTINGS, &mut dred) };
    if hr >= 0 && !dred.is_null() {
        for i in [3, 4, 5] {
            let set: unsafe extern "system" fn(*mut c_void, i32) = unsafe { method(dred, i) };
            unsafe { set(dred, 2) };
        }
        unsafe { release(dred) };
        crate::log!("d3d12 debug: DRED breadcrumbs, page faults and Watson dumps on");
    } else {
        crate::log!("d3d12 debug: no DRED settings ({hr:#x})");
    }
}

/// veh.rs: a debugger print (DBG_PRINTEXCEPTION_C: [length, ANSI text]) while the layer is on: into the log, each distinct
/// message a few times only
pub fn print(params: &[usize], sp: usize) {
    if params.len() < 2 || params[1] == 0 {
        return;
    }
    let len = params[0].min(4096);
    let bytes = unsafe { std::slice::from_raw_parts(params[1] as *const u8, len) };
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_end_matches(['\0', '\n', '\r']);
    if text.is_empty() {
        return;
    }
    static SEEN: std::sync::Mutex<Vec<(String, u32)>> = std::sync::Mutex::new(Vec::new());
    static TOTAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let Ok(mut seen) = SEEN.try_lock() else { return };
    // (the same message differs only in its object addresses: compared on its first 120 characters)
    let key: String = text.chars().take(120).collect();
    let n = match seen.iter().position(|e| e.0 == key) {
        Some(i) => {
            seen[i].1 += 1;
            seen[i].1
        }
        None if seen.len() < 400 => {
            seen.push((key.clone(), 1));
            1
        }
        None => return,
    };
    // (a message that keeps coming: how often, at 10, 100, 1000... times)
    if n > 3 {
        if [10, 100, 1000, 10_000, 100_000].contains(&n) {
            crate::log::write_now(&format!("d3d12: (seen {n} times) {key}"));
        }
        return;
    }
    if TOTAL.fetch_add(1, Ordering::Relaxed) >= 2000 {
        return;
    }
    // an error's caller: the stack's words that point into code we know (the game, our DLL, D3D12 itself)
    let mut msg = format!("d3d12: {text}");
    if text.contains("ERROR") || text.contains("CORRUPTION") {
        let top: usize;
        unsafe { std::arch::asm!("mov {}, gs:[0x8]", out(reg) top) };
        let end = if sp < top && top - sp < 0x100_0000 { (sp + 0x10000).min(top) } else { sp };
        msg += "
  stack:";
        let mut found = 0;
        for k in 0..(end - sp) / 8 {
            let v = unsafe { ((sp + k * 8) as *const usize).read_volatile() };
            // (the game's and our DLL's frames only: the print's own frames (ntdll, KernelBase, the layer) fill the first part)
            if let Some(m) = crate::veh::module_of(v).filter(|m| m.starts_with("er+") || m.starts_with("jr+")) {
                msg += &format!(" {m}");
                found += 1;
                if found >= 24 {
                    break;
                }
            }
        }
    }
    crate::log::write_now(&msg);
}
