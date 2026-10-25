//! hooks.recolor: fighters' recoloured armor (sheet: fighters.recolor), made on the player's PC from
//! their own Elden Ring files. Each recoloured piece is read from the game's archives, its albedo
//! textures are shifted to the fighter's colour (shading and detail kept), and it is written under a
//! new, unused model id into ModEngine2's mod folder (parts/<p>_<g>_91xx.partsbnd.dcx). The look
//! system points the worn armor rows at that model, so vanilla armor is never changed.
//! Nothing of Elden Ring is shipped; the files are made per player and stay on their PC.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ermario::{archive::Archives, bnd4, dcx, tpf};
use crate::log;
use crate::sheets;

/// Set once every recoloured model exists in the mod folder.
pub static READY: AtomicBool = AtomicBool::new(false);

/// Bump to remake every recoloured model after a change to the recolour itself.
const VERSION: u32 = 3;

const PREFIX: [&str; 4] = ["hd", "bd", "am", "lg"];

/// One piece to make: fighter index, piece 0..4, the source armor's model id, the target colour.
#[derive(Clone)]
pub struct Job {
    pub fighter: usize,
    pub piece: usize,
    pub src_model: u16,
    pub rgb: [u8; 3],
    /// A fan-made model shipped with the mod (skins/<name>.bnd) instead of a recolour.
    pub skin: Option<&'static str>,
    /// written as this model id instead of new_model (a second version of a piece, e.g. Asta's board)
    pub out: Option<u16>,
}

/// 1.2.0: Asta's glider board, his body with the blade under his feet (skins/asta_bd_m_1850_board.bnd,
/// work/rigport/asta_board.py): the model id of that version of a fighter's body. `foot`: the lab's version
/// bound to his right foot instead of his pelvis.
pub fn board_model(fighter: usize, foot: bool) -> u16 {
    9100 + (fighter * 10 + if foot { 6 } else { 5 }) as u16
}

/// 1.3.0 (the user: Gon's fishing rod "looks odd being drawn over his hand ... make the fishing rod ... a real world asset"):
/// the body a fighter swings with, his own with the rod in his right hand (Gon: the Nen Impact model's own rod,
/// work/rigport/merge_held.py), swapped in while he swings (look.rs SWING_PROP): the model id of that version of his body
pub fn swing_model(fighter: usize) -> u16 {
    9100 + (fighter * 10 + 5) as u16
}
pub fn swing_skin(fighter: &str) -> Option<&'static str> {
    match fighter {
        "gon" => Some("nigon_bd_m_1850_rod"),
        _ => None,
    }
}

/// 1.3.0: Luffy's Gear Fourth (Boundman) body (skins/luffy_bd_m_1850_gear4.bnd, work/rigport/luffy_gear4.py), swapped
/// in for his Awakening (look.rs GEAR4): the model id of that version of a fighter's body
pub fn gear4_model(fighter: usize) -> u16 {
    9100 + (fighter * 10 + 7) as u16
}

/// 1.3.0: the body a fighter's Awakening swaps in (look.rs GEAR4, Step::Gear4), no load: Luffy's Gear Fourth (Boundman); the
/// user, 2026-10-17: Gojo takes off his blindfold (Bluesands' unmasked Gojo) and Yuji becomes Sukuna (Sukuna's Vessel)
pub fn awakened_skin(fighter: &str) -> Option<&'static str> {
    match fighter {
        "luffy" => Some("luffy_bd_m_1850_gear4"),
        "gojo" => Some("gojou_bd_m_1850"),
        "yuji" => Some("sukuna_bd_m_1850"),
        // Gon's Adult Form; Killua's Godspeed hair
        "gon" => Some("adultgon_bd_m_1850"),
        "killua" => Some("nikilluagod_bd_m_1850"),
        _ => None,
    }
}

/// 1.3.0: the villains' own models (villains.rs; the user's downloads, work/rigport/build_rip.sh or port_parts.py): villain
/// id -> skin prefix (skins/<prefix>_{hd,bd,am,lg}_m_1850.bnd), worn through armour rows of their own
/// (villains::VILLAIN_ARMOR); and a second body for a villain who changes form mid-fight (Shigaraki -> All For One)
pub const VILLAIN_SKINS: &[(&str, &str)] = &[
    ("doflamingo", "doflamingo"), ("frieza", "frieza"), ("cell", "cell"), ("blackbeard", "blackbeard"), ("kars", "kars"),
    ("all_for_one", "afo"), ("madara", "madara"), ("hisoka", "hisoka"), ("meruem", "nimeruem"), ("aizen", "aizen"),
    ("pain", "pain"), ("shigaraki", "shigaraki"), ("sukuna", "sukuna"), ("patry", "patry"), ("dante", "dante"),
    ("muscular", "muscular"),
];
/// the bodies of villains' other forms, and the model piece each is built as (villains.rs FORMS): Shigaraki with All For One;
/// Pain's other five paths
pub const VILLAIN_FORMS: &[(&str, &str, usize)] = &[
    ("shigaraki", "shigarakiafo_bd_m_1850", 7),
    ("pain", "painasura_bd_m_1850", 4), ("pain", "painhuman_bd_m_1850", 5), ("pain", "painanimal_bd_m_1850", 6),
    ("pain", "painpreta_bd_m_1850", 8), ("pain", "painnaraka_bd_m_1850", 9),
    ("muscular", "muscularmf_bd_m_1850", 7),
];
/// 1.3.0 (the user: Blackbeard's download came with his flintlocks): a weapon from a villain's own download, held in the right
/// hand as an arms piece (work/rigport/pistol_held.py) and drawn only for the moves that use it (villains.rs HELD), and its piece
/// (Madara's gunbai, the war fan that came with lorisc93's port of him: always in his hand, villains.rs OWN_ARMS)
pub const VILLAIN_HELD: &[(&str, &str, usize)] = &[("blackbeard", "blackbeardgun_am_m_1850", 5), ("madara", "madarafan_am_m_1850", 5)];
pub fn villain_skin(id: &str) -> Option<&'static str> {
    // (2.0.1 crash hunt, lab: noskins.on beside the log folder: the villains wear their filler armour)
    #[cfg(feature = "lab")]
    {
        static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *OFF.get_or_init(|| crate::log::dir().join("noskins.on").exists()) {
            return None;
        }
    }
    VILLAIN_SKINS.iter().find(|(v, _)| *v == id).map(|(_, s)| *s)
}
/// 2.0.3 (the Nexus models: generated, hands cut at the wrist like Jotaro's): a villain whose shipped skin has no hands of its
/// own, marked by skins/<prefix>.erhands beside its pieces, wears Elden Ring's bare arms under it (real hands that animate)
pub fn villain_er_hands(id: &str) -> bool {
    villain_skin(id).zip(skins_dir()).is_some_and(|(p, d)| d.join(format!("{p}.erhands")).exists())
}
/// model id of villain `v`'s piece (7: the second form's body)
pub fn villain_model(v: usize, piece: usize) -> u16 {
    9500 + (v * 10 + piece) as u16
}

/// 1.3.0: the fighters' Stands (stands.rs; the user: "figure out how to use Stands for our characters"): fighter id ->
/// the Stand's body skin (a generated model, work/rigport/concepts_jojo.py), and its model id
pub const STAND_SKINS: &[(&str, &str)] = &[("jotaro", "star_platinum_bd_m_1850"), ("dio", "the_world_bd_m_1850")];
pub fn stand_model(i: usize) -> u16 {
    9700 + (i * 10 + 1) as u16
}

/// The new model id of a fighter's recoloured piece.
pub fn new_model(fighter: usize, piece: usize) -> u16 {
    9100 + (fighter * 10 + piece) as u16
}

/// ModEngine2's mod folder: <modengine2>/mod, next to our jumpring/ folder.
pub fn mod_dir() -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let module = {
        let mut h = std::ptr::null_mut();
        unsafe {
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleExW(
                windows_sys::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | windows_sys::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                mod_dir as *const u16,
                &mut h,
            )
        };
        h
    };
    let len = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let dll = PathBuf::from(String::from_utf16_lossy(&buf[..len]));
    Some(dll.parent()?.parent()?.join("mod"))
}

fn stamp(jobs: &[Job]) -> String {
    let mut s = format!("jumpring-recolor-v{VERSION}\n{}\n", game_build());
    for j in jobs {
        // 0.10.0: and the skin file's size, so a changed model (Asta's sword) is rebuilt on update;
        // 1.2.0: and when it was written (Asta's board versions are all the same size)
        let meta = j.skin.and_then(|n| skin_path(n).and_then(|p| std::fs::metadata(p).ok()));
        let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let when = meta.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        s += &format!("{} {} {} {:?} {:?} {len} {when}\n", j.fighter, j.piece, j.src_model, j.rgb, j.skin);
    }
    s
}

/// The game loads material/allmaterial.matbinbnd.dcx once, at startup, so the recoloured models'
/// materials only count when the file exists before that. When work is needed, every game thread that
/// opens one of the game's archives (.bhd/.bdt) waits at CreateFileW until the models are made.
static GATE: std::sync::OnceLock<retour::RawDetour> = std::sync::OnceLock::new();
static DONE: AtomicBool = AtomicBool::new(false);
thread_local! {
    /// The recolour worker itself reads the archives.
    static WORKER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

type CreateFileW = unsafe extern "system" fn(*const u16, u32, u32, *const std::ffi::c_void, u32, u32, isize) -> isize;

unsafe extern "system" fn hk_create_file(name: *const u16, access: u32, share: u32, sa: *const std::ffi::c_void, disp: u32, flags: u32, tmpl: isize) -> isize {
    // lab 0.14.3 diag: which part files the game opens, and when
    #[cfg(feature = "lab")]
    if !name.is_null() {
        let len = (0..).take_while(|&i| unsafe { *name.add(i) } != 0).count();
        let path = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(name, len) }).to_ascii_lowercase();
        if path.contains("partsbnd") && !WORKER.with(|w| w.get()) {
            log!("lab file: {}", path.rsplit(|c| c == '\\' || c == '/').next().unwrap_or(&path));
        }
        // 2.0.1 crash hunt: the behaviour scripts' own file loads (loadfile), by thread
        if path.ends_with(".lua") || path.ends_with(".hks") {
            static OPENS: std::sync::Mutex<Vec<(u32, u64)>> = std::sync::Mutex::new(Vec::new());
            let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
            if let Ok(mut o) = OPENS.lock() {
                let n = match o.iter_mut().find(|(t, _)| *t == tid) {
                    Some(e) => {
                        e.1 += 1;
                        e.1
                    }
                    None => {
                        o.push((tid, 1));
                        1
                    }
                };
                let total: u64 = o.iter().map(|e| e.1).sum();
                if n == 1 || total.is_power_of_two() || path.contains("jr_probe") {
                    let by: Vec<String> = o.iter().map(|(t, c)| format!("{t}:{c}")).collect();
                    log!("lab script file: {} on thread {tid} ({total} opens, by thread {})", path.rsplit(|c| c == '\\' || c == '/').next().unwrap_or(&path), by.join(" "));
                }
            }
        }
    }
    if !DONE.load(Ordering::Acquire) && !WORKER.with(|w| w.get()) && !name.is_null() {
        let len = (0..).take_while(|&i| unsafe { *name.add(i) } != 0).count();
        let path = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(name, len) }).to_ascii_lowercase();
        if path.ends_with(".bhd") || path.ends_with(".bdt") {
            let t0 = std::time::Instant::now();
            // 1.1.1: keep answering Windows while we wait (players saw "not responding" and a frozen
            // game here): PM_NOREMOVE handles messages sent to this thread's windows, leaves the rest
            while !DONE.load(Ordering::Acquire) && t0.elapsed() < std::time::Duration::from_secs(600) {
                unsafe {
                    let mut msg: windows_sys::Win32::UI::WindowsAndMessaging::MSG = std::mem::zeroed();
                    windows_sys::Win32::UI::WindowsAndMessaging::PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, windows_sys::Win32::UI::WindowsAndMessaging::PM_NOREMOVE);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    let orig: CreateFileW = unsafe { std::mem::transmute(GATE.get().unwrap().trampoline()) };
    unsafe { orig(name, access, share, sa, disp, flags, tmpl) }
}

/// The first launch's work is finished (or failed): the game may read its files.
pub fn done() -> bool {
    DONE.load(Ordering::Acquire)
}

fn install_gate() {
    unsafe {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
        let kb = GetModuleHandleA(c"kernelbase.dll".as_ptr() as _);
        if let Some(f) = GetProcAddress(kb, c"CreateFileW".as_ptr() as _) {
            if let Ok(d) = retour::RawDetour::new(f as *const (), hk_create_file as *const ()).and_then(|d| d.enable().map(|_| d)) {
                let _ = GATE.set(d);
            }
        }
    }
}

/// 1.2.0: the prepare step (elden_force_prepare.exe, before the game starts) makes the outfits too, so the
/// game never has to build them while it loads (a rebuild inside the game, on a first launch or after an
/// update changed a skin, once ended in a crash on the title screen in testing).
pub fn prepare() -> String {
    let jobs = jobs();
    let Some(dir) = mod_dir() else { return "mod folder not found".into() };
    let want = stamp(&jobs);
    if std::fs::read_to_string(dir.join("parts").join("jumpring-recolor.txt")).ok().as_deref() == Some(want.as_str())
        && dir.join(ER_SCRIPT).exists()
    {
        return "up to date".into();
    }
    make_all(jobs);
    "made".into()
}

/// From DllMain, before the game opens its archives: make the models now if they are missing or
/// out of date, holding the game's archive reads until they exist.
pub fn start_early() {
    let jobs = jobs();
    let Some(dir) = mod_dir() else { return };
    let want = stamp(&jobs);
    let anims_ok = crate::anims::up_to_date() && crate::title::up_to_date();
    if std::fs::read_to_string(dir.join("parts").join("jumpring-recolor.txt")).ok().as_deref() == Some(want.as_str())
        && dir.join(ER_SCRIPT).exists()
        && anims_ok
    {
        READY.store(true, Ordering::Relaxed);
        DONE.store(true, Ordering::Release);
        #[cfg(not(feature = "lab"))]
        return;
        #[cfg(feature = "lab")]
        {
            install_gate();
            return;
        }
    }
    install_gate();
    crate::splash::show();
    std::thread::spawn(move || {
        WORKER.with(|w| w.set(true));
        // the log opens on the boot thread; wait for it so these lines are kept
        std::thread::sleep(std::time::Duration::from_millis(200));
        log!("recolor: making the fighters' outfits before the game loads its files ({} pieces)", jobs.len());
        make_all(jobs);
        DONE.store(true, Ordering::Release);
    });
}

fn make_all(jobs: Vec<Job>) {
    let Some(dir) = mod_dir() else {
        log!("recolor: mod folder not found");
        return;
    };
    let parts = dir.join("parts");
    let stamp_path = parts.join("jumpring-recolor.txt");
    let want = stamp(&jobs);
    let t0 = std::time::Instant::now();
    let archives = match Archives::open() {
        Ok(a) => a,
        Err(e) => {
            log!("recolor: cannot read the game's archives: {e}");
            return;
        }
    };
    #[cfg(feature = "lab")]
    {
        skin_audit(&archives);
        // lab: JUMPRING_DUMP="/game/path=C:\out\file" copies one file out of the game's archives
        if let Some(spec) = std::env::var_os("JUMPRING_DUMP").and_then(|v| v.into_string().ok()) {
            for one in spec.split(';') {
                if let Some((src, dst)) = one.split_once('=') {
                    match archives.read(src) {
                        Ok(d) => { let _ = std::fs::write(dst, d); log!("lab: dumped {src}"); }
                        Err(e) => log!("lab: dump {src}: {e}"),
                    }
                }
            }
        }
    }
    if let Err(e) = copy_player_script(&archives, &dir) {
        log!("recolor: player script: {e}");
    }
    if !crate::anims::up_to_date() {
        crate::anims::make(&archives);
    }
    // 0.15.2: Elden Force's title screen
    if !crate::title::up_to_date() {
        crate::title::make(&archives);
    }
    if std::fs::read_to_string(&stamp_path).ok().as_deref() == Some(want.as_str()) {
        READY.store(true, Ordering::Relaxed);
        return;
    }
    let _ = std::fs::create_dir_all(&parts);
    // 1.1.1: one worker per core (was one after another: 100 s here, minutes on slower PCs)
    let made = std::sync::atomic::AtomicUsize::new(0);
    let next = std::sync::atomic::AtomicUsize::new(0);
    crate::splash::PROGRESS.1.store(jobs.len(), Ordering::Relaxed);
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 12);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                WORKER.with(|w| w.set(true));
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(j) = jobs.get(i) else { break };
                    match make(&archives, &parts, j) {
                        Ok(n) => {
                            made.fetch_add(n, Ordering::Relaxed);
                        }
                        Err(e) => log!("recolor: fighter {} piece {}: {e}", j.fighter, j.piece),
                    }
                    crate::splash::PROGRESS.0.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    let made = made.into_inner();
    // The new models' materials: the game looks materials up by name ("P[BD_M_9101]_Fabric") in
    // one global binder, so add renamed copies of the source pieces' materials to it.
    if let Err(e) = materials(&archives, &dir, &jobs) {
        log!("recolor: materials: {e}");
        return;
    }
    let _ = std::fs::write(&stamp_path, want);
    log!("recolor: made {made} files in {:.1}s", t0.elapsed().as_secs_f32());
    READY.store(true, Ordering::Relaxed);
}

/// Make one piece (all gender variants and LOD files the game has for it). Returns files written.
fn make(archives: &Archives, parts: &PathBuf, j: &Job) -> Result<usize, String> {
    let p = PREFIX[j.piece];
    let new = j.out.unwrap_or_else(|| new_model(j.fighter, j.piece));
    let mut written = 0;
    let skin = match j.skin {
        Some(name) => Some(load_skin(archives, name)?),
        None => None,
    };
    for g in ["m", "f", "a"] {
        // (a skin's LOD file is the same model under the same name: built once, written twice)
        let mut same: Option<Vec<u8>> = None;
        for lod in ["", "_l"] {
            if let Some(out) = same.as_ref() {
                std::fs::write(parts.join(format!("{p}_{g}_{new:04}{lod}.partsbnd.dcx")), out).map_err(|e| e.to_string())?;
                written += 1;
                continue;
            }
            let new_name = format!("{p}_{g}_{new:04}");
            let (src_name, bnd, mut files) = match &skin {
                // One shipped model serves every body type and LOD.
                Some((bnd, files)) => (
                    format!("{p}_m_{:04}", j.src_model),
                    bnd.clone(),
                    files.iter().map(|f| bnd4::File { id: f.id, name: f.name.clone(), data: f.data.clone() }).collect(),
                ),
                None => {
                    let src_name = format!("{p}_{g}_{:04}", j.src_model);
                    let Ok(raw) = archives.read(&format!("/parts/{src_name}{lod}.partsbnd.dcx")) else { continue };
                    let bnd = dcx::decompress(&raw)?;
                    let files = bnd4::read(&bnd)?;
                    (src_name, bnd, files)
                }
            };
            for f in files.iter_mut() {
                f.name = f.name.replace(&src_name, &new_name).replace(&src_name.to_uppercase(), &new_name.to_uppercase());
                for (old, newer) in [(src_name.clone(), new_name.clone()), (src_name.to_uppercase(), new_name.to_uppercase())] {
                    tpf::rename(&mut f.data, &old, &newer);
                }
                if f.name.to_lowercase().ends_with(".tpf") && j.skin.is_none() {
                    if std::env::var_os("JUMPRING_RECOLOR_IDENTITY").is_none() {
                        f.data = recolor_tpf(&f.data, j.rgb)?;
                    }
                }
            }
            let out = dcx::compress(&bnd4::build(&bnd, &files))?;
            std::fs::write(parts.join(format!("{new_name}{lod}.partsbnd.dcx")), &out).map_err(|e| e.to_string())?;
            written += 1;
            if skin.is_some() {
                same = Some(out);
            }
        }
    }
    if written == 0 {
        return Err(format!("no parts files for {p} model {}", j.src_model));
    }
    Ok(written)
}

/// Write material/allmaterial.matbinbnd.dcx: the game's own list plus, for every recoloured piece,
/// copies of its source materials renamed to the new model (their texture paths too).
fn materials(archives: &Archives, dir: &PathBuf, jobs: &[Job]) -> Result<(), String> {
    let path = "/material/allmaterial.matbinbnd.dcx";
    let src = dcx::decompress(&archives.read(path)?)?;
    let mut files = bnd4::read(&src)?;
    let mut next_id = files.iter().map(|f| f.id).max().unwrap_or(0) + 1;
    let mut added = Vec::new();
    for j in jobs {
        let p = PREFIX[j.piece].to_uppercase();
        for g in ["M", "F", "A"] {
            // a skin's materials are named after its one (male) model
            let old = format!("{p}_{}_{:04}", if j.skin.is_some() { "M" } else { g }, j.src_model);
            let new = format!("{p}_{g}_{:04}", j.out.unwrap_or_else(|| new_model(j.fighter, j.piece)));
            for f in files.iter().filter(|f| f.name.contains(&format!("[{old}]"))) {
                let mut data = f.data.clone();
                tpf::rename(&mut data, &old, &new);
                added.push(bnd4::File { id: next_id, name: f.name.replace(&old, &new), data });
                next_id += 1;
            }
        }
    }
    let count = added.len();
    files.extend(added);
    let out_dir = dir.join("material");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    std::fs::write(out_dir.join("allmaterial.matbinbnd.dcx"), dcx::compress(&bnd4::build(&src, &files))?).map_err(|e| e.to_string())?;
    log!("recolor: {count} materials added");
    Ok(())
}

/// DXGI format of a DDS (DX10 header) or the classic FourCC.
fn dds_format(dds: &[u8]) -> u32 {
    if &dds[84..88] == b"DX10" {
        u32::from_le_bytes(dds[128..132].try_into().unwrap())
    } else {
        match &dds[84..88] {
            b"DXT1" => 71,
            b"DXT5" => 77,
            _ => 0,
        }
    }
}

/// Recolour every albedo texture ("*_a", "*_a_l") in a TPF, all mip levels, keeping size and format.
fn recolor_tpf(t: &[u8], rgb: [u8; 3]) -> Result<Vec<u8>, String> {
    let mut out = t.to_vec();
    for tex in tpf::tpf_textures(t) {
        let base = tex.name.strip_suffix("_l").unwrap_or(&tex.name).to_lowercase();
        if !base.ends_with("_a") {
            continue;
        }
        let dds = &t[tex.off..tex.off + tex.size];
        let head = tpf::dds_header(dds);
        let (mut h, mut w) = (tpf::u32_at(dds, 12), tpf::u32_at(dds, 16));
        let fmt = dds_format(dds);
        let block = match fmt {
            71 | 72 => 8,
            77 | 78 | 98 | 99 => 16,
            _ => {
                log!("recolor: {} has format {fmt}, left as is", tex.name);
                continue;
            }
        };
        let mut at = head;
        for _ in 0..tex.mips.max(1) {
            let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
            let len = bw * bh * block;
            if at + len > dds.len() {
                break;
            }
            let src = &dds[at..at + len];
            let (pw, ph) = (bw * 4, bh * 4);
            let mut px = vec![0u32; pw * ph];
            let ok = match fmt {
                71 | 72 => texture2ddecoder::decode_bc1(src, pw, ph, &mut px),
                77 | 78 => texture2ddecoder::decode_bc3(src, pw, ph, &mut px),
                _ => texture2ddecoder::decode_bc7(src, pw, ph, &mut px),
            };
            if ok.is_err() {
                return Err(format!("{}: cannot decode", tex.name));
            }
            // texture2ddecoder gives BGRA packed as u32
            let mut rgba: Vec<u8> = px.iter().flat_map(|p| { let b = p.to_le_bytes(); [b[2], b[1], b[0], b[3]] }).collect();
            colorize(&mut rgba, rgb);
            let enc = match fmt {
                71 | 72 => encode_bc(texpresso::Format::Bc1, &rgba, pw, ph),
                77 | 78 => encode_bc(texpresso::Format::Bc3, &rgba, pw, ph),
                _ => tpf::bc7_mode6(&rgba, pw, ph),
            };
            if enc.len() != len {
                return Err(format!("{}: encoded size mismatch", tex.name));
            }
            out[tex.off + at..tex.off + at + len].copy_from_slice(&enc);
            at += len;
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
    }
    Ok(out)
}

fn encode_bc(fmt: texpresso::Format, rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut buf = vec![0u8; fmt.compressed_size(w, h)];
    fmt.compress(rgba, w, h, texpresso::Params::default(), &mut buf);
    buf
}

/// Shift a texture to `rgb`: each pixel's brightness relative to the texture's average brightness
/// scales the target colour, so folds, seams and wear survive while the cloth takes the new colour.
fn colorize(rgba: &mut [u8], rgb: [u8; 3]) {
    let lum = |p: &[u8]| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
    let n = rgba.len() / 4;
    let mut lums: Vec<f32> = rgba.chunks_exact(4).map(lum).collect();
    let mut sorted = lums.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[n / 2].max(8.0);
    let target = rgb.map(|c| c as f32);
    for (p, l) in rgba.chunks_exact_mut(4).zip(lums.iter_mut()) {
        // a narrow brightness range keeps highlights in the target hue instead of blowing out
        let k = (*l / median).clamp(0.35, 1.45);
        // keep a little of the original colour so metal and leather still read differently
        for c in 0..3 {
            let v = target[c] * k * 0.85 + p[c] as f32 * 0.15;
            p[c] = v.clamp(0.0, 255.0) as u8;
        }
    }
}

/// Model id of the armor rows the fighters' outfits use (EquipParamProtector.equipModelId), so the
/// jobs are known before the game has loaded its params.
fn src_model(protector: i32) -> Option<u16> {
    Some(match protector {
        100000 | 100300 => 1140,
        180100 | 180200 | 180300 => 1610,
        220100 | 220300 => 1680,
        240100 | 240200 | 240300 => 1700,
        60200 => 1100,
        700100 | 700200 | 700300 => 1320,
        812000 => 2145,
        _ => return None,
    })
}

/// The jobs for every fighter with recolours.
pub fn jobs() -> Vec<Job> {
    let mut jobs = Vec::new();
    for (fi, f) in sheets::FIGHTERS.iter().enumerate() {
        let Some(outfit) = f.outfit else { continue };
        for piece in 0..4 {
            if let Some(name) = f.skin.and_then(|s| s[piece]) {
                match name.rsplit('_').next().and_then(|n| n.parse().ok()) {
                    Some(src_model) => {
                        jobs.push(Job { fighter: fi, piece, src_model, rgb: [0; 3], skin: Some(name), out: None });
                        // Asta's board: his body again, the blade under his feet
                        if let (1, Some(alt)) = (piece, awakened_skin(f.id)) {
                            jobs.push(Job { fighter: fi, piece, src_model, rgb: [0; 3], skin: Some(alt), out: Some(gear4_model(fi)) });
                        }
                        if f.id == "gon" && piece == 1 && skin_exists("nigon_bd_m_1850_rod") {
                            jobs.push(Job { fighter: fi, piece, src_model, rgb: [0; 3], skin: Some("nigon_bd_m_1850_rod"), out: Some(swing_model(fi)) });
                        }
                        if f.id == "asta" && piece == 1 {
                            jobs.push(Job { fighter: fi, piece, src_model, rgb: [0; 3], skin: Some("asta_bd_m_1850_board"), out: Some(board_model(fi, false)) });
                            #[cfg(feature = "lab")]
                            jobs.push(Job { fighter: fi, piece, src_model, rgb: [0; 3], skin: Some("asta_bd_m_1850_boardfoot"), out: Some(board_model(fi, true)) });
                        }
                    }
                    None => log!("recolor: skin {name} has no model id"),
                }
                continue;
            }
            let Some(rgb) = f.recolor.and_then(|r| r[piece]) else { continue };
            match src_model(outfit[piece]) {
                Some(src_model) => jobs.push(Job { fighter: fi, piece, src_model, rgb, skin: None, out: None }),
                None => log!("recolor: no model id known for armor {} (add it to recolor::src_model)", outfit[piece]),
            }
        }
    }
    // the Stands' bodies
    for (i, (_, skin)) in STAND_SKINS.iter().enumerate() {
        if skin_exists(skin) {
            jobs.push(Job { fighter: 200 + i, piece: 1, src_model: 1850, rgb: [0; 3], skin: Some(skin), out: Some(stand_model(i)) });
        }
    }
    // the villains' models, when their skin files are shipped
    for (v, vil) in crate::villains::VILLAINS.iter().enumerate() {
        let Some(prefix) = villain_skin(vil.id) else { continue };
        if !skin_exists(&format!("{prefix}_bd_m_1850")) {
            continue;
        }
        for piece in 0..4 {
            let name: &'static str = Box::leak(format!("{prefix}_{}_m_1850", PREFIX[piece]).into_boxed_str());
            jobs.push(Job { fighter: 100 + v, piece, src_model: 1850, rgb: [0; 3], skin: Some(name), out: Some(villain_model(v, piece)) });
        }
        for (_, alt, piece) in VILLAIN_FORMS.iter().filter(|(id, alt, _)| *id == vil.id && skin_exists(alt)) {
            jobs.push(Job { fighter: 100 + v, piece: 1, src_model: 1850, rgb: [0; 3], skin: Some(alt), out: Some(villain_model(v, *piece)) });
        }
        for (_, held, piece) in VILLAIN_HELD.iter().filter(|(id, held, _)| *id == vil.id && skin_exists(held)) {
            jobs.push(Job { fighter: 100 + v, piece: 2, src_model: 1850, rgb: [0; 3], skin: Some(held), out: Some(villain_model(v, *piece)) });
        }
    }
    jobs
}

/// Whether a fighter's piece is worn as a model made by this module (a recolour or a skin).
pub fn has_model(f: &sheets::Fighter, piece: usize) -> bool {
    f.skin.and_then(|s| s[piece]).is_some() || f.recolor.and_then(|r| r[piece]).is_some()
}

/// 1.3.0: a shipped skin's file: skins/<name>.bnd.dcx (Kraken-compressed, as the package ships them: the raw binders
/// came to 1.7 GB) or skins/<name>.bnd (raw, the lab's)
pub fn skin_path(name: &str) -> Option<PathBuf> {
    let d = skins_dir()?;
    [d.join(format!("{name}.bnd.dcx")), d.join(format!("{name}.bnd"))].into_iter().find(|p| p.exists())
}

/// Whether a shipped skin file exists.
pub fn skin_exists(name: &str) -> bool {
    skin_path(name).is_some()
}

/// The shipped skins: <modengine2>/jumpring/skins.
fn skins_dir() -> Option<PathBuf> {
    Some(mod_dir()?.parent()?.join("jumpring").join("skins"))
}

const GAME_MARK: &[u8] = b"JRGAME:";

/// Elden Ring's own file an entry of a parts binder comes from, by the model folder in its path
/// ("...\FullBody\HD_M_1485\hkx\Skeleton.hkx" -> /parts/hd_m_1485.partsbnd.dcx).
fn vanilla_source(entry: &str) -> Option<String> {
    entry
        .split('\\')
        .find(|c| {
            let b = c.as_bytes();
            b.len() == 9 && b[2] == b'_' && b[4] == b'_' && b[5..].iter().all(u8::is_ascii_digit)
        })
        .map(|m| format!("/parts/{}.partsbnd.dcx", m.to_lowercase()))
}

/// A shipped skin with its Elden Ring entries (left empty in the shipped file) filled from the
/// player's own game files, so only the model author's work is in the mod.
fn load_skin(archives: &Archives, name: &str) -> Result<(Vec<u8>, Vec<bnd4::File>), String> {
    let path = skin_path(name).ok_or_else(|| format!("skin {name} not found"))?;
    let raw = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bnd = if path.extension().is_some_and(|e| e == "dcx") { dcx::decompress(&raw)? } else { raw };
    let mut files = bnd4::read(&bnd)?;
    let mut cache: Vec<(String, Vec<bnd4::File>)> = Vec::new();
    for f in files.iter_mut().filter(|f| f.data.is_empty() || f.data.starts_with(GAME_MARK)) {
        // empty: the game's entry of the same name; "JRGAME:<file>|<entry>": that entry of that file
        let (src, entry) = match std::str::from_utf8(&f.data).ok().and_then(|m| m.strip_prefix("JRGAME:")).and_then(|m| m.split_once('|')) {
            Some((src, entry)) => (src.to_string(), entry.to_string()),
            None => (vanilla_source(&f.name).ok_or_else(|| format!("{name}: no game file for {}", f.name))?, f.name.clone()),
        };
        if !cache.iter().any(|(p, _)| *p == src) {
            let v = bnd4::read(&dcx::decompress(&archives.read(&src)?)?)?;
            cache.push((src.clone(), v));
        }
        let (_, v) = cache.iter().find(|(p, _)| *p == src).unwrap();
        let data = v.iter().find(|x| x.name.eq_ignore_ascii_case(&entry)).ok_or_else(|| format!("{name}: {entry} not in {src}"))?;
        f.data = data.data.clone();
    }
    Ok((bnd, files))
}

/// Lab: strip a fan-made parts binder down to its author's work. For every <name>.bnd (raw BND4) in
/// JUMPRING_SKIN_AUDIT, entries byte-identical to Elden Ring's own file are emptied and the result is
/// written to <dir>/stripped/<name>.bnd, with a report of what was kept.
#[cfg(feature = "lab")]
pub fn skin_audit(archives: &Archives) {
    let Some(dir) = std::env::var_os("JUMPRING_SKIN_AUDIT").map(PathBuf::from) else { return };
    let out = dir.join("stripped");
    let _ = std::fs::create_dir_all(&out);
    let mut report = String::new();
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "bnd") {
            continue;
        }
        let Ok(bnd) = std::fs::read(&p) else { continue };
        let Ok(mut files) = bnd4::read(&bnd) else { continue };
        report += &format!("== {}\n", p.display());
        // other game models the binder mentions (an entry may be a copy of theirs under this name)
        let utf16: String = bnd.chunks_exact(2).map(|c| if c[1] == 0 && c[0].is_ascii_graphic() { c[0] as char } else { ' ' }).collect();
        let ascii: String = bnd.iter().map(|&b| if b.is_ascii_graphic() { b as char } else { ' ' }).collect();
        let mut others: Vec<String> = format!("{utf16} {ascii}")
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|t| {
                let b = t.as_bytes();
                b.len() == 9 && b[2] == b'_' && b[4] == b'_' && b[5..].iter().all(u8::is_ascii_digit)
            })
            .map(|t| t.to_uppercase())
            .collect();
        others.sort();
        others.dedup();
        for f in files.iter_mut() {
            let vanilla = vanilla_source(&f.name)
                .and_then(|src| archives.read(&src).ok())
                .and_then(|raw| dcx::decompress(&raw).ok())
                .and_then(|b| bnd4::read(&b).ok())
                .and_then(|v| v.into_iter().find(|x| x.name.eq_ignore_ascii_case(&f.name)));
            let same = vanilla.as_ref().is_some_and(|v| v.data == f.data);
            let own = f.name.split('\\').map(str::to_uppercase).find(|c| others.contains(c));
            let copy = if same {
                None
            } else {
                others.iter().filter(|o| Some(*o) != own.as_ref()).find_map(|o| {
                    let src = format!("/parts/{}.partsbnd.dcx", o.to_lowercase());
                    let entry = f.name.to_uppercase().replace(own.as_deref()?, o);
                    let v = bnd4::read(&dcx::decompress(&archives.read(&src).ok()?).ok()?).ok()?;
                    v.into_iter().find(|x| x.name.eq_ignore_ascii_case(&entry) && x.data == f.data).map(|x| (src, x.name))
                })
            };
            let tag = if same { "GAME" } else if copy.is_some() { "COPY" } else { "KEEP" };
            let from = copy.as_ref().map(|(s, e)| format!("  <- {s}|{e}")).unwrap_or_default();
            report += &format!("  {tag} {} {}{from}\n", f.data.len(), f.name);
            if same {
                f.data.clear();
            } else if let Some((src, entry)) = copy {
                f.data = format!("JRGAME:{src}|{entry}").into_bytes();
            }
        }
        let _ = std::fs::write(out.join(p.file_name().unwrap()), bnd4::build(&bnd, &files));
    }
    let _ = std::fs::write(out.join("report.txt"), report);
    log!("recolor: skin audit written to {}", out.display());
}

/// Where the game's own player script is copied for our c0000.hks to run (mod folder relative).
const ER_SCRIPT: &str = "action/script/c0000_er.hks";

/// The game build, from eldenring.exe's size and modified time.
pub fn game_build() -> String {
    crate::ermario::archive::GAME_DIR
        .get()
        .map(|d| d.join("eldenring.exe"))
        .or_else(|| std::env::current_exe().ok())
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| format!("{} {:?}", m.len(), m.modified().ok()))
        .unwrap_or_default()
}

/// Copy the player's own c0000.hks out of their game files, next to Elden Force's c0000.hks, which
/// runs it and adds to it (nothing of the game's script is shipped).
fn copy_player_script(archives: &Archives, dir: &PathBuf) -> Result<(), String> {
    #[allow(unused_mut)]
    let mut data = archives.read("/action/script/c0000.hks")?;
    // lab: another player script under ours (JR_LAB_ER_SCRIPT, e.g. Carian Combo Warriors')
    #[cfg(feature = "lab")]
    if let Some(p) = std::env::var_os("JR_LAB_ER_SCRIPT") {
        data = std::fs::read(&p).map_err(|e| e.to_string())?;
        log!("lab: player script from {}", std::path::Path::new(&p).display());
    }
    let out = dir.join(ER_SCRIPT);
    std::fs::create_dir_all(out.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&out, data).map_err(|e| e.to_string())?;
    log!("recolor: player script copied ({})", out.display());
    Ok(())
}
