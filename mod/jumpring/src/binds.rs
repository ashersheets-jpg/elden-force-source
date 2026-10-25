//! hooks.input: the mod's buttons (sheet: inputs) on keyboard and controller, without the game also
//! reacting. The game reads keyboard/mouse with DirectInput GetDeviceState and pads with XInputGetState;
//! both are detoured. While our menu is open the game sees nothing; in the world, the buttons of a held
//! ability chord and the keys bound to our actions are removed. Our own keys are read with
//! GetAsyncKeyState. Chords are hidden inside the XInput hook itself, from the same pad state the game
//! is about to get, so a chord's face button never reaches the game, even on the frame it goes down.
//! Bindings start from the sheet and can be changed in the Controls page (saved to controls.txt).

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU16, Ordering};

use retour::RawDetour;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetModuleHandleW, GetProcAddress};
use windows_sys::core::GUID;

use crate::log;
use crate::sheets::{self, When};

/// Our menu is open: the game gets no keyboard, mouse or pad input.
pub static BLOCK_ALL: AtomicBool = AtomicBool::new(false);
/// Our menu just closed: keep the game's input empty until every key and button is let go, so the
/// key that closed it (Esc / B) doesn't also open the game's menu or dodge.
static RELEASE_WAIT: AtomicBool = AtomicBool::new(false);
/// 0.15.0: when the wait began. It used to end only in the keyboard hook, and after a fighter change's
/// reload the game stopped reading the keyboard until the window lost focus: the wait never ended and
/// the game got no buttons at all (only the mod's chords worked). Now it ends when everything is let
/// go as either hook sees it, and after 1.2 s whatever happens.
static RELEASE_SINCE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Called every frame with whether our menu is open.
pub fn set_menu_open(open: bool) {
    if BLOCK_ALL.swap(open, Ordering::Relaxed) && !open {
        RELEASE_WAIT.store(true, Ordering::Relaxed);
        *RELEASE_SINCE.lock().unwrap() = Some(std::time::Instant::now());
    }
    if RELEASE_WAIT.load(Ordering::Relaxed) && RELEASE_SINCE.lock().unwrap().is_none_or(|t| t.elapsed() > std::time::Duration::from_millis(1200)) {
        end_release_wait();
    }
}

fn end_release_wait() {
    RELEASE_WAIT.store(false, Ordering::Relaxed);
    *RELEASE_SINCE.lock().unwrap() = None;
}

/// Nothing on the keyboard is held, as Windows sees it (works without the game reading the keyboard).
fn keyboard_idle() -> bool {
    (0x08..0xFF).all(|vk| !key_down(vk))
}
/// Virtual pad bits for the analog triggers (unused by XInput's button mask).
pub const LT: u16 = 0x0400;
pub const RT: u16 = 0x0800;
const TRIGGER_DOWN: u8 = 100;
/// Latest real pad buttons per XInput slot (before hiding; triggers as LT/RT), for our chords.
static PAD_RAW: [AtomicU16; 4] = [const { AtomicU16::new(0) }; 4];
/// The device the player touched last (hooks.input): 0 keyboard/mouse, 1 controller.
static LAST_DEVICE: AtomicU8 = AtomicU8::new(0);
/// Where the frame task is: 0 in the world, 1 resting at a grace, 2 in our menu.
static CTX: AtomicU8 = AtomicU8::new(0);
/// Current bindings, one per inputs-sheet row (u16::MAX / -1: the sheet's default).
static PAD: [AtomicU16; 32] = [const { AtomicU16::new(u16::MAX) }; 32];
static KEY: [AtomicI32; 32] = [const { AtomicI32::new(-1) }; 32];
/// Row being rebound in the Controls page (-1: none).
pub static CAPTURE: AtomicI32 = AtomicI32::new(-1);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Device {
    KeyboardMouse,
    Controller,
}

pub fn device() -> Device {
    if FORCE_PAD.load(Ordering::Relaxed) || LAST_DEVICE.load(Ordering::Relaxed) == 1 { Device::Controller } else { Device::KeyboardMouse }
}

/// Lab only: show controller prompts regardless of the last device (End toggles).
static FORCE_PAD: AtomicBool = AtomicBool::new(false);
/// Lab: controller (true) or keyboard (false) prompts, as if the player had just used it.
#[cfg(feature = "lab")]
pub fn lab_device(pad: bool) {
    FORCE_PAD.store(pad, Ordering::Relaxed);
    LAST_DEVICE.store(pad as u8, Ordering::Relaxed);
}

#[cfg(feature = "lab")]
pub fn toggle_device() {
    FORCE_PAD.fetch_xor(true, Ordering::Relaxed);
}

/// The left stick click.
pub const L3: u16 = 0x0040;
/// XInput's left shoulder (LB)
const LB_BIT: u16 = 0x0100;
/// 1.3.0 (the user: "every character with a sword is using sword moves"): a sword fighter holding the sword in one hand
/// (look.rs): LB and the mouse's right button (the left hand's attack) are kept from the game, so the empty left hand
/// throws no punch. Two-handed (Y), LB is the sword's guard and reaches the game.
pub static HIDE_LEFT_HAND: AtomicBool = AtomicBool::new(false);

/// 0.15.2: these pad buttons are held now (as the player presses them, before anything is hidden).
pub fn pad_held(bits: u16) -> bool {
    pad_raw() & bits == bits
}

fn pad_raw() -> u16 {
    PAD_RAW.iter().fold(0, |m, p| m | p.load(Ordering::Relaxed))
}

/// Index of an inputs-sheet row.
pub fn row(id: &str) -> usize {
    sheets::INPUTS.iter().position(|r| r.id == id).unwrap_or_else(|| panic!("inputs sheet has no row {id}"))
}

/// Controller chord of row `i`.
pub fn pad(i: usize) -> u16 {
    match PAD[i].load(Ordering::Relaxed) {
        u16::MAX => sheets::INPUTS[i].controller,
        m => m,
    }
}

/// Keyboard / mouse virtual key of row `i`.
pub fn key(i: usize) -> i32 {
    match KEY[i].load(Ordering::Relaxed) {
        -1 => sheets::INPUTS[i].keyboard,
        k => k,
    }
}

/// Rows the player may rebind: everything used outside our menu.
pub fn rebindable(i: usize) -> bool {
    sheets::INPUTS[i].when != When::Menu
}

fn controls_path() -> std::path::PathBuf {
    crate::log::dir().join("controls.txt")
}

/// Read controls.txt ("ability_1=0x1040,0x31" per line).
pub fn load() {
    let Ok(text) = std::fs::read_to_string(controls_path()) else { return };
    for line in text.lines() {
        let Some((id, v)) = line.split_once('=') else { continue };
        let Some(i) = sheets::INPUTS.iter().position(|r| r.id == id.trim()) else { continue };
        if !rebindable(i) {
            continue;
        }
        let mut parts = v.split(',').map(|x| x.trim().trim_start_matches("0x"));
        if let (Some(Ok(p)), Some(Ok(k))) = (parts.next().map(|p| u16::from_str_radix(p, 16)), parts.next().map(|k| i32::from_str_radix(k, 16))) {
            if p != 0 {
                PAD[i].store(p, Ordering::Relaxed);
            }
            if k > 0 {
                KEY[i].store(k, Ordering::Relaxed);
            }
        }
    }
    log!("input: controls loaded from controls.txt");
}

fn save() {
    let mut out = String::new();
    for (i, r) in sheets::INPUTS.iter().enumerate() {
        if rebindable(i) {
            out += &format!("{}={:#06x},{:#04x}\n", r.id, pad(i), key(i));
        }
    }
    let _ = std::fs::write(controls_path(), out);
}

pub fn reset() {
    for i in 0..sheets::INPUTS.len() {
        PAD[i].store(u16::MAX, Ordering::Relaxed);
        KEY[i].store(-1, Ordering::Relaxed);
    }
    let _ = std::fs::remove_file(controls_path());
    log!("input: controls reset to defaults");
}

/// Pad buttons to keep from the game for this pad state. A fully held chord is hidden. While a chord
/// is one button short, only its modifiers (buttons shared by two or more active chords, L3 by default)
/// are hidden: the game plays as vanilla until the modifier is held, then the face buttons are ours.
fn hide_for(raw: u16) -> u16 {
    let ctx = CTX.load(Ordering::Relaxed);
    if ctx == 2 {
        return 0;
    }
    let chords: Vec<u16> = sheets::INPUTS
        .iter()
        .enumerate()
        .filter(|(_, r)| r.when == When::InWorld || (r.when == When::Grace && (ctx == 1 || cfg!(feature = "lab"))))
        .map(|(i, _)| pad(i))
        .filter(|c| *c != 0)
        .collect();
    let mut modifiers = 0u16;
    for bit in (0..16).map(|b| 1u16 << b) {
        if chords.iter().filter(|c| *c & bit != 0).count() >= 2 {
            modifiers |= bit;
        }
    }
    let mut hide = 0;
    let mut full = 0;
    for chord in chords {
        let missing = (chord & !raw).count_ones();
        if missing == 0 {
            hide |= chord & raw;
            if chord.count_ones() > 1 {
                full |= chord;
            }
        } else if missing == 1 && chord.count_ones() > 1 {
            hide |= chord & raw & modifiers;
        }
    }
    // 1.2.0 (Asta's glider on L3 + RB): a chord's buttons stay hidden until each is let go, so an RB still
    // held after L3 is no attack (it knocked him off the board)
    let sticky = (CHORD_STICKY.load(Ordering::Relaxed) & raw) | full;
    CHORD_STICKY.store(sticky, Ordering::Relaxed);
    hide | (sticky & raw)
}

/// Buttons of a chord that fired, still held (hide_for)
static CHORD_STICKY: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

/// A quick tap of a modifier (press and release with no chord) still reaches the game, so L3 crouches.
const TAP_MAX: std::time::Duration = std::time::Duration::from_millis(250);
const TAP_PASS: std::time::Duration = std::time::Duration::from_millis(80);
struct Tap {
    down: u16,
    since: Option<std::time::Instant>,
    chorded: bool,
    pass: u16,
    pass_until: Option<std::time::Instant>,
}
static TAP: std::sync::Mutex<Tap> = std::sync::Mutex::new(Tap { down: 0, since: None, chorded: false, pass: 0, pass_until: None });

/// Pad bits to give back to the game this poll: a modifier tapped and released on its own.
fn tap_pass(raw: u16, hide: u16) -> u16 {
    let Ok(mut t) = TAP.lock() else { return 0 };
    let now = std::time::Instant::now();
    let held_mod = hide & raw & !(LT | RT);
    if held_mod != 0 && t.since.is_none() {
        t.down = held_mod;
        t.since = Some(now);
        t.chorded = false;
    }
    if t.since.is_some() && raw & !t.down & !(LT | RT) != 0 && hide & !t.down != 0 {
        t.chorded = true; // a chord's face button went down with it
    }
    if let Some(since) = t.since
        && raw & t.down == 0
    {
        if !t.chorded && now - since < TAP_MAX {
            t.pass = t.down;
            t.pass_until = Some(now + TAP_PASS);
        }
        t.since = None;
    }
    match t.pass_until {
        Some(u) if now < u => t.pass,
        _ => 0,
    }
}

static DI_DETOUR: OnceLock<RawDetour> = OnceLock::new();
static XI_DETOUR: OnceLock<RawDetour> = OnceLock::new();

type GetDeviceState = unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> i32;
type XInputGetState = unsafe extern "system" fn(u32, *mut XInputState) -> u32;

#[repr(C)]
pub struct XInputState {
    packet: u32,
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    thumb_lx: i16,
    thumb_ly: i16,
    thumb_rx: i16,
    thumb_ry: i16,
}

/// Lab scripts (lab/script.txt): keyboard scan codes held down for the game, even when its window
/// is in the background (lab runs while someone else uses the PC).
#[cfg(feature = "lab")]
pub static LAB_KEYS: [AtomicU8; 8] = [const { AtomicU8::new(0) }; 8];
/// Lab scripts: mouse buttons held down (bit 0 left, 1 right), like LAB_KEYS
#[cfg(feature = "lab")]
pub static LAB_MOUSE: AtomicU8 = AtomicU8::new(0);
/// Lab scripts: controller held down (script "pad rt|lt|a|b|x|y|rb|lb <ms>"): XInput button bits, plus
/// RT 0x10000 / LT 0x20000; the game reads the controller with its window in the background too
#[cfg(feature = "lab")]
pub static LAB_PAD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Lab scripts: mouse movement to add each poll (x), for this many more polls (the camera turns)
#[cfg(feature = "lab")]
pub static LAB_LOOK: [std::sync::atomic::AtomicI32; 2] = [const { std::sync::atomic::AtomicI32::new(0) }; 2];

/// 0.10.0: the movement stick as the player holds it, read before anything is hidden from the game:
/// the controller's left stick (x, y as i16) and the W A S D keys (bits 1 2 4 8).
pub static STICK_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
pub static STICK_Y: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
pub static WASD: AtomicU8 = AtomicU8::new(0);

unsafe extern "system" fn hk_get_device_state(this: *mut c_void, cb: u32, data: *mut c_void) -> i32 {
    let orig: GetDeviceState = unsafe { std::mem::transmute(DI_DETOUR.get().unwrap().trampoline()) };
    #[allow(unused_mut)]
    let mut r = unsafe { orig(this, cb, data) };
    #[cfg(feature = "lab")]
    if cb == 256 && !data.is_null() && LAB_KEYS.iter().any(|k| k.load(Ordering::Relaxed) != 0) {
        if r < 0 {
            unsafe { std::ptr::write_bytes(data as *mut u8, 0, 256) };
            r = 0;
        }
        let bytes = unsafe { std::slice::from_raw_parts_mut(data as *mut u8, 256) };
        for k in &LAB_KEYS {
            let sc = k.load(Ordering::Relaxed) as usize;
            if sc != 0 {
                bytes[sc] = 0x80;
            }
        }
    }
    #[cfg(feature = "lab")]
    if (cb == 16 || cb == 20) && !data.is_null() && LAB_LOOK[1].load(Ordering::Relaxed) > 0 {
        if r < 0 {
            unsafe { std::ptr::write_bytes(data as *mut u8, 0, cb as usize) };
            r = 0;
        }
        LAB_LOOK[1].fetch_sub(1, Ordering::Relaxed);
        let bytes = unsafe { std::slice::from_raw_parts_mut(data as *mut u8, cb as usize) };
        let dx = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) + LAB_LOOK[0].load(Ordering::Relaxed);
        bytes[0..4].copy_from_slice(&dx.to_le_bytes());
    }
    #[cfg(feature = "lab")]
    if (cb == 16 || cb == 20) && !data.is_null() && LAB_MOUSE.load(Ordering::Relaxed) != 0 {
        if r < 0 {
            unsafe { std::ptr::write_bytes(data as *mut u8, 0, cb as usize) };
            r = 0;
        }
        let bytes = unsafe { std::slice::from_raw_parts_mut(data as *mut u8, cb as usize) };
        let m = LAB_MOUSE.load(Ordering::Relaxed);
        for b in 0..2 {
            if m & (1 << b) != 0 {
                bytes[12 + b] = 0x80;
            }
        }
    }
    // (the left hand's attack on a mouse: its right button, DIMOUSESTATE rgbButtons[1])
    if r >= 0 && (cb == 16 || cb == 20) && !data.is_null() && HIDE_LEFT_HAND.load(Ordering::Relaxed) {
        let bytes = unsafe { std::slice::from_raw_parts_mut(data as *mut u8, cb as usize) };
        bytes[13] = 0;
    }
    if r >= 0 && cb == 256 && !data.is_null() {
        let bytes = unsafe { std::slice::from_raw_parts(data as *const u8, 256) };
        let k = |sc: usize, bit: u8| if bytes[sc] & 0x80 != 0 { bit } else { 0 };
        WASD.store(k(0x11, 1) | k(0x1E, 2) | k(0x1F, 4) | k(0x20, 8), Ordering::Relaxed);
    }
    // Keyboard (256-byte state) key down, or mouse (DIMOUSESTATE/2) moved or clicked: keyboard and mouse.
    if r >= 0 && !data.is_null() {
        let bytes = unsafe { std::slice::from_raw_parts(data as *const u8, cb as usize) };
        let used = match cb {
            256 => bytes.iter().any(|b| b & 0x80 != 0),
            16 | 20 => {
                let dx = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                let dy = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                dx.abs() > 2 || dy.abs() > 2 || bytes[12..].iter().any(|b| b & 0x80 != 0)
            }
            _ => false,
        };
        if used {
            LAST_DEVICE.store(0, Ordering::Relaxed);
        }
    }
    if r >= 0 && cb == 256 && !data.is_null() && RELEASE_WAIT.load(Ordering::Relaxed) {
        let bytes = unsafe { std::slice::from_raw_parts(data as *const u8, 256) };
        if bytes.iter().any(|b| b & 0x80 != 0) {
            unsafe { std::ptr::write_bytes(data as *mut u8, 0, 256) };
            return r;
        }
        if pad_raw() == 0 {
            end_release_wait();
        }
    }
    if r >= 0 && BLOCK_ALL.load(Ordering::Relaxed) && !data.is_null() {
        unsafe { std::ptr::write_bytes(data as *mut u8, 0, cb as usize) };
    } else if r >= 0 && cb == 256 && !data.is_null() && CTX.load(Ordering::Relaxed) != 2 {
        // Keys bound to our actions don't reach the game (DirectInput keyboard codes are scan codes).
        let bytes = unsafe { std::slice::from_raw_parts_mut(data as *mut u8, 256) };
        for i in 0..sheets::INPUTS.len() {
            let vk = key(i);
            if !rebindable(i) || vk < 0x08 {
                continue; // menu keys, mouse buttons
            }
            let sc = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::MapVirtualKeyW(vk as u32, 0) } as usize;
            if sc > 0 && sc < 256 {
                bytes[sc] = 0;
            }
        }
    }
    r
}

unsafe extern "system" fn hk_xinput_get_state(user: u32, state: *mut XInputState) -> u32 {
    let orig: XInputGetState = unsafe { std::mem::transmute(XI_DETOUR.get().unwrap().trampoline()) };
    #[allow(unused_mut)]
    let mut r = unsafe { orig(user, state) };
    #[cfg(feature = "lab")]
    let lab_pad = LAB_PAD.load(Ordering::Relaxed);
    #[cfg(feature = "lab")]
    // (lab: slot 0 always reads as a connected pad, so the game keeps polling it even with no controller on
    // and a short scripted press is never missed)
    #[cfg(feature = "lab")]
    if user == 0 && r != 0 && !state.is_null() {
        unsafe { std::ptr::write_bytes(state as *mut u8, 0, std::mem::size_of::<XInputState>()) };
        r = 0;
    }
    #[cfg(feature = "lab")]
    if lab_pad != 0 && !state.is_null() {
        if r != 0 {
            unsafe { std::ptr::write_bytes(state as *mut u8, 0, std::mem::size_of::<XInputState>()) };
            r = 0;
        }
        let s = unsafe { &mut *state };
        s.packet = s.packet.wrapping_add(1);
        s.buttons |= (lab_pad & 0xFFFF) as u16;
        if lab_pad & 0x10000 != 0 {
            s.right_trigger = 255;
        }
        if lab_pad & 0x20000 != 0 {
            s.left_trigger = 255;
        }
    }
    if r == 0 && !state.is_null() {
        let s = unsafe { &mut *state };
        if user == 0 || (s.thumb_lx as i32).abs() + (s.thumb_ly as i32).abs() > 8000 {
            STICK_X.store(s.thumb_lx as i32, Ordering::Relaxed);
            STICK_Y.store(s.thumb_ly as i32, Ordering::Relaxed);
        }
        let raw = s.buttons | if s.left_trigger > TRIGGER_DOWN { LT } else { 0 } | if s.right_trigger > TRIGGER_DOWN { RT } else { 0 };
        if let Some(slot) = PAD_RAW.get(user as usize) {
            slot.store(raw, Ordering::Relaxed);
        }
        let stick = |v: i16| (v as i32).abs() > 9000;
        if s.buttons != 0 || s.left_trigger > 40 || s.right_trigger > 40 || stick(s.thumb_lx) || stick(s.thumb_ly) || stick(s.thumb_rx) || stick(s.thumb_ry) {
            LAST_DEVICE.store(1, Ordering::Relaxed);
        }
        if RELEASE_WAIT.load(Ordering::Relaxed) && pad_raw() == 0 && keyboard_idle() {
            end_release_wait();
        }
        if BLOCK_ALL.load(Ordering::Relaxed) || (RELEASE_WAIT.load(Ordering::Relaxed) && raw != 0) {
            s.buttons = 0;
            s.left_trigger = 0;
            s.right_trigger = 0;
            s.thumb_lx = 0;
            s.thumb_ly = 0;
            s.thumb_rx = 0;
            s.thumb_ry = 0;
        } else {
            let hide = hide_for(raw);
            s.buttons &= !(hide & !(LT | RT));
            if HIDE_LEFT_HAND.load(Ordering::Relaxed) {
                s.buttons &= !LB_BIT;
            }
            s.buttons |= tap_pass(raw, hide);
            if hide & LT != 0 {
                s.left_trigger = 0;
            }
            if hide & RT != 0 {
                s.right_trigger = 0;
            }
        }
    }
    r
}

const IID_IDIRECTINPUT8W: GUID = GUID::from_u128(0xBF798031_483A_4DA2_AA99_5D64ED369700);
const GUID_SYS_KEYBOARD: GUID = GUID::from_u128(0x6F1D2B61_D5A0_11CF_BFC7_444553540000);

#[link(name = "dinput8")]
unsafe extern "system" {
    fn DirectInput8Create(hinst: HMODULE, version: u32, riid: *const GUID, out: *mut *mut c_void, outer: *mut c_void) -> i32;
}

/// Read slot `i` of a COM object's vtable.
unsafe fn vfn(obj: *mut c_void, i: usize) -> *const () {
    unsafe { *(*(obj as *const *const *const ())).add(i) }
}

pub fn install() {
    load();
    unsafe {
        // DirectInput: every device shares one GetDeviceState, so a throwaway keyboard device finds it.
        let mut di: *mut c_void = std::ptr::null_mut();
        if DirectInput8Create(GetModuleHandleW(std::ptr::null()), 0x0800, &IID_IDIRECTINPUT8W, &mut di, std::ptr::null_mut()) >= 0 {
            type CreateDevice = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void, *mut c_void) -> i32;
            let create: CreateDevice = std::mem::transmute(vfn(di, 3));
            let mut dev: *mut c_void = std::ptr::null_mut();
            if create(di, &GUID_SYS_KEYBOARD, &mut dev, std::ptr::null_mut()) >= 0 {
                let target = vfn(dev, 9);
                match RawDetour::new(target, hk_get_device_state as *const ()).and_then(|d| d.enable().map(|_| d)) {
                    Ok(d) => {
                        let _ = DI_DETOUR.set(d);
                        log!("input: DirectInput hooked");
                    }
                    Err(e) => log!("input: DirectInput hook failed: {e}"),
                }
            }
        } else {
            log!("input: DirectInput8Create failed");
        }

        // XInput: hook whichever XInput the game loaded.
        for name in [c"xinput1_4.dll", c"xinput1_3.dll", c"xinput9_1_0.dll"] {
            let m = GetModuleHandleA(name.as_ptr() as _);
            if m.is_null() {
                continue;
            }
            if let Some(f) = GetProcAddress(m, c"XInputGetState".as_ptr() as _) {
                match RawDetour::new(f as *const (), hk_xinput_get_state as *const ()).and_then(|d| d.enable().map(|_| d)) {
                    Ok(d) => {
                        let _ = XI_DETOUR.set(d);
                        log!("input: XInput hooked ({})", name.to_string_lossy());
                    }
                    Err(e) => log!("input: XInput hook failed: {e}"),
                }
                break;
            }
        }
        if XI_DETOUR.get().is_none() {
            log!("input: no XInput module loaded yet; controller bindings unavailable");
        }
    }
}

fn key_down(vk: i32) -> bool {
    unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

/// Keys a rebind may take: mouse middle/side buttons and the keyboard (not Esc, not bare modifiers).
fn capturable() -> impl Iterator<Item = i32> {
    [0x04, 0x05, 0x06].into_iter().chain(0x08..0xFF).filter(|k| !matches!(*k, 0x1B | 0x10 | 0x11 | 0x12 | 0x5B | 0x5C))
}

/// Edge-detects every binding once per frame, and runs a rebind capture.
#[derive(Default)]
pub struct Bindings {
    held: Vec<bool>,
    /// capture in progress: buttons seen together, keys already held when it began, when it began
    seen: u16,
    keys_at_start: Vec<i32>,
    started: Option<std::time::Instant>,
}

impl Bindings {
    /// Ids of bindings for `ctx` that went down this frame.
    pub fn poll(&mut self, ctx: When) -> Vec<&'static str> {
        if self.held.len() != sheets::INPUTS.len() {
            self.held = vec![false; sheets::INPUTS.len()];
        }
        CTX.store(
            match ctx {
                When::InWorld => 0,
                When::Grace => 1,
                When::Menu => 2,
            },
            Ordering::Relaxed,
        );
        let pad_now = pad_raw();
        if let Ok(i) = usize::try_from(CAPTURE.load(Ordering::Relaxed)) {
            self.capture(i, pad_now);
            // nothing fires while capturing, and what is held then must be let go before it counts
            self.held.iter_mut().for_each(|h| *h = true);
            return Vec::new();
        }
        let mut fired = Vec::new();
        for (i, b) in sheets::INPUTS.iter().enumerate() {
            let mut active = b.when == ctx || (b.when == When::InWorld && ctx == When::Grace);
            // Lab builds: the menu opens anywhere, so tests need no Site of Grace.
            if cfg!(feature = "lab") && b.when == When::Grace && ctx == When::InWorld {
                active = true;
            }
            let chord = pad(i);
            let down = active && (key_down(key(i)) || (chord != 0 && pad_now & chord == chord));
            if down && !self.held[i] {
                fired.push(b.id);
            }
            self.held[i] = down;
        }
        fired
    }

    /// Binding `id` is held down right now (as of the last poll).
    pub fn held(&self, id: &str) -> bool {
        sheets::INPUTS.iter().position(|b| b.id == id).is_some_and(|i| self.held.get(i).copied().unwrap_or(false))
    }

    /// Rebind row `i`: the first new key, or every pad button held together (taken on release).
    fn capture(&mut self, i: usize, pad_now: u16) {
        let started = match self.started {
            Some(t) => t,
            None => {
                self.seen = 0;
                self.keys_at_start = capturable().filter(|k| key_down(*k)).collect();
                *self.started.insert(std::time::Instant::now())
            }
        };
        let id = sheets::INPUTS[i].id;
        let mut finish = |me: &mut Self, what: Option<String>| {
            match what {
                Some(w) => {
                    save();
                    log!("input: {id} bound to {w}");
                }
                None => log!("input: rebinding {id} cancelled"),
            }
            me.started = None;
            CAPTURE.store(-1, Ordering::Relaxed);
        };
        if started.elapsed() > std::time::Duration::from_secs(8) || key_down(0x1B) {
            finish(self, None);
            return;
        }
        if let Some(k) = capturable().find(|k| key_down(*k) && !self.keys_at_start.contains(k)) {
            KEY[i].store(k, Ordering::Relaxed);
            finish(self, Some(format!("key {k:#x}")));
            return;
        }
        self.keys_at_start.retain(|k| key_down(*k));
        // ignore the button that chose the row: only a press that starts after a short pause counts
        if started.elapsed() < std::time::Duration::from_millis(300) {
            if pad_now != 0 {
                self.started = Some(std::time::Instant::now());
            }
            return;
        }
        if pad_now != 0 {
            self.seen |= pad_now;
        } else if self.seen != 0 {
            PAD[i].store(self.seen, Ordering::Relaxed);
            let seen = self.seen;
            finish(self, Some(format!("pad {seen:#06x}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L3: u16 = 0x0040;
    const A: u16 = 0x1000;
    const B: u16 = 0x2000;

    #[test]
    fn face_buttons_alone_reach_the_game() {
        for b in [A, B, 0x4000, 0x8000] {
            assert_eq!(hide_for(b), 0, "button {b:#x} alone was hidden");
        }
    }

    #[test]
    fn modifier_alone_is_hidden_and_full_chord_is_hidden() {
        assert_eq!(hide_for(L3), L3);
        assert_eq!(hide_for(L3 | A), L3 | A);
        assert_eq!(hide_for(L3 | B), L3 | B);
    }

    #[test]
    fn a_quick_modifier_tap_is_passed_on_but_a_chord_is_not() {
        // tap L3 alone: held, then released
        assert_eq!(tap_pass(L3, hide_for(L3)), 0);
        assert_eq!(tap_pass(0, hide_for(0)), L3);
        std::thread::sleep(TAP_PASS + std::time::Duration::from_millis(20));
        assert_eq!(tap_pass(0, 0), 0);
        // L3 + A (an ability): no crouch afterwards
        assert_eq!(tap_pass(L3, hide_for(L3)), 0);
        assert_eq!(tap_pass(L3 | A, hide_for(L3 | A)), 0);
        assert_eq!(tap_pass(0, hide_for(0)), 0);
    }
}
