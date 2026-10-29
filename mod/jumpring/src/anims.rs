//! Elden Force's own animations (sheet: anims.json, baked by tools_scripts/bake_anims.py into
//! jumpring/anims.bin): hand-keyed moves on Elden Ring's player skeleton, made into the player's own
//! animation binders on their PC at first launch, next to the outfits (recolor.rs gates the game's
//! archive reads until both are done).
//!
//! Each move gets a new animation id in the event range (a000_06xxxx), which the game plays when the
//! DLL sets ChrIns event.request_animation_id (play()). Two files are written into ModEngine2's mod
//! folder: chr/c0000_a00_hi.anibnd.dcx (the player's own binder plus one .hkx per move, built from the
//! game's own a000_080000.hkx as a container template) and chr/c0000.anibnd.dcx (the player's own,
//! with a00.tae given one entry per move, each a copy of a game entry's events). Nothing of Elden Ring
//! is shipped: only Elden Force's own pose data.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use eldenring::cs::PlayerIns;

use crate::ermario::{archive::Archives, bnd4, dcx};
use crate::log;

/// Bump to rebuild the binders after a change to the builder itself.
const VERSION: u32 = 1;
/// The game animation whose .hkx is the container for ours (a gesture: full body, 72 tracks).
const TEMPLATE: u32 = 80000;
const A00_HI: &str = "/chr/c0000_a00_hi.anibnd.dcx";
/// 1.2.0: where other sections' clips are (Ashes of War in a6x and a9x, Shadow of the Erdtree's in dlc01 / dlc02; 1.3.0:
/// a6x for the base game's a6xx Ashes of War, Repeating Thrust and Lifesteal Fist among them)
const OTHER_BINDERS: [&str; 4] = ["/chr/c0000_a6x.anibnd.dcx", "/chr/c0000_a9x.anibnd.dcx", "/chr/c0000_dlc01.anibnd.dcx", "/chr/c0000_dlc02.anibnd.dcx"];
const ANIBND: &str = "/chr/c0000.anibnd.dcx";

/// Set when the binders with our animations are in the mod folder (play() does nothing otherwise).
pub static READY: AtomicBool = AtomicBool::new(false);

pub struct Anim {
    pub id: u32,
    pub events_from: u32,
    /// 1.2.0: not hand-keyed: Elden Ring's own animation a000_<copy> (motion-captured), taken from the
    /// player's binder and played in this slot with the events of `events_from` (none of its own: an
    /// item throw's events would use up the player's items). Over 1_000_000: another section's clip,
    /// a<copy / 1_000_000>_<copy % 1_000_000> (an Ash of War's), from whichever of the player's binders
    /// holds it; `fallback` the same when that one isn't there (Shadow of the Erdtree's moves)
    pub copy: u32,
    pub fallback: u32,
    pub frames: usize,
    pub bones: Vec<i16>,
    /// frames * bones * (pos xyz, quat xyzw, scale xyz)
    pub data: Vec<f32>,
}

fn dll_dir() -> Option<PathBuf> {
    Some(crate::recolor::mod_dir()?.parent()?.join("jumpring"))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32, String> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| "truncated".to_string())
}

fn u64_at(d: &[u8], o: usize) -> Result<u64, String> {
    d.get(o..o + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| "truncated".to_string())
}

pub fn load(path: &Path) -> Result<Vec<Anim>, String> {
    let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let version = u32_at(&d, 4)?;
    if !d.starts_with(b"JRAN") || !(2..=4).contains(&version) {
        return Err("anims.bin: bad header".into());
    }
    let count = u32_at(&d, 8)? as usize;
    let mut p = 12;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let (id, events_from, frames, tracks) = (u32_at(&d, p)?, u32_at(&d, p + 4)?, u32_at(&d, p + 8)? as usize, u32_at(&d, p + 12)? as usize);
        p += 16;
        let copy = if version >= 3 {
            p += 4;
            u32_at(&d, p - 4)?
        } else {
            0
        };
        let fallback = if version >= 4 {
            p += 4;
            u32_at(&d, p - 4)?
        } else {
            0
        };
        let bones: Vec<i16> = (0..tracks).map(|i| i16::from_le_bytes([d[p + 2 * i], d[p + 2 * i + 1]])).collect();
        p += (2 * tracks).next_multiple_of(4);
        let n = frames * tracks * STRIDE;
        let data: Vec<f32> = d.get(p..p + 4 * n).ok_or("anims.bin truncated")?.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        p += 4 * n;
        out.push(Anim { id, events_from, copy, fallback, frames, bones, data });
    }
    Ok(out)
}

// ---------------------------------------------------------------- enemies

/// 1.2.0 round 5 (the user: the launched enemy needs "the Goku animation"): Elden Force's own hit
/// reactions for enemies (tools_scripts/anims/make_enemy_pose.py -> enemy_anims.bin). Enemies can't be
/// asked to play an animation, but the common soldiers share Elden Ring's soldier set (c4310), whose
/// binders ModEngine2 loads from our mod folder like the player's: the reaction our launcher's blow
/// causes (dmgLevel 7, a00x_<target>; <target + 1> from behind) is rebuilt there with ours, in the game's
/// own clip as the container, on the player's PC from their own game files.
pub struct EnemyAnim {
    pub family: String,
    pub target: u32,
    pub anim: Anim,
}

/// The binders of each enemy animation set.
fn enemy_binders(family: &str) -> &'static [&'static str] {
    match family {
        "c4310" => &["/chr/c4310_div00.anibnd.dcx", "/chr/c4310_div01.anibnd.dcx", "/chr/c4310_div02.anibnd.dcx", "/chr/c4310_div03.anibnd.dcx"],
        _ => &[],
    }
}

pub fn load_enemy(path: &Path) -> Result<Vec<EnemyAnim>, String> {
    let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !d.starts_with(b"JREN") || u32_at(&d, 4)? != 1 {
        return Err("enemy_anims.bin: bad header".into());
    }
    let count = u32_at(&d, 8)? as usize;
    let mut p = 12;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let family = String::from_utf8_lossy(d.get(p..p + 8).ok_or("enemy_anims.bin truncated")?).trim_end_matches('\0').to_string();
        p += 8;
        let (target, frames, tracks) = (u32_at(&d, p)?, u32_at(&d, p + 4)? as usize, u32_at(&d, p + 8)? as usize);
        p += 16;
        let bones: Vec<i16> = (0..tracks).map(|i| i16::from_le_bytes([d[p + 2 * i], d[p + 2 * i + 1]])).collect();
        p += (2 * tracks).next_multiple_of(4);
        let n = frames * tracks * STRIDE;
        let data: Vec<f32> = d.get(p..p + 4 * n).ok_or("enemy_anims.bin truncated")?.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        p += 4 * n;
        out.push(EnemyAnim { family, target, anim: Anim { id: target, events_from: 0, copy: 0, fallback: 0, frames, bones, data } });
    }
    Ok(out)
}

/// The enemy binders' file names in our mod folder (written by make_enemies). 1.2.0: none: the
/// launcher's enemy reaction was scrapped (a leftover enemy_anims.bin must not make the binders count as
/// missing, or they would be rebuilt inside the game at every launch).
fn enemy_outputs() -> Vec<String> {
    Vec::new()
}

#[allow(dead_code)]
fn enemy_outputs_of_bin() -> Vec<String> {
    let Some(dir) = dll_dir() else { return Vec::new() };
    let Ok(list) = load_enemy(&dir.join("enemy_anims.bin")) else { return Vec::new() };
    let mut out = Vec::new();
    for e in &list {
        for b in enemy_binders(&e.family) {
            let name = b.trim_start_matches("/chr/").to_string();
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// Rebuild the enemy sets' reactions with ours (each in the game's own clip as the container).
fn make_enemies(archives: &Archives, dir: &Path) -> Result<usize, String> {
    let Some(dll) = dll_dir() else { return Ok(0) };
    let path = dll.join("enemy_anims.bin");
    if !path.exists() {
        return Ok(0);
    }
    let list = load_enemy(&path)?;
    let mut made = 0;
    let mut binders: Vec<&str> = list.iter().flat_map(|e| enemy_binders(&e.family).iter().copied()).collect();
    binders.dedup();
    for bnd in binders {
        let src = dcx::decompress(&archives.read(bnd)?)?;
        let mut files = bnd4::read(&src)?;
        let mut changed = 0;
        for f in files.iter_mut() {
            for e in list.iter().filter(|e| enemy_binders(&e.family).contains(&bnd)) {
                if f.name.ends_with(&format!("_{:06}.hkx", e.target)) || f.name.ends_with(&format!("_{:06}.hkx", e.target + 1)) {
                    f.data = build_hkx(&f.data, &e.anim).map_err(|x| format!("{}: {x}", f.name))?;
                    changed += 1;
                }
            }
        }
        std::fs::write(dir.join(bnd.trim_start_matches("/chr/")), dcx::compress(&bnd4::build(&src, &files))?).map_err(|e| e.to_string())?;
        made += changed;
    }
    Ok(made)
}

// ---------------------------------------------------------------- Carian Combo Warriors

/// 1.2.0 (the user: "use carian combo warriors"): the fighters' movesets are Carian Combo Warriors' (by its
/// author on Nexus Mods, used with permission and credit). Its animations and move tables that Elden Force
/// uses are in jumpring/ccw_anims.bin (tools_scripts/ccw/make_pack.py) and go over the game's own, in the
/// player's binders, here; its player script runs beside the game's (hks/jr_dispatch.lua).
struct CcwEntry {
    binder: String,
    name: String,
    data: Vec<u8>,
}

fn load_ccw() -> Vec<CcwEntry> {
    let Some(dir) = dll_dir() else { return Vec::new() };
    let Ok(d) = std::fs::read(dir.join("ccw_anims.bin")) else { return Vec::new() };
    let mut out = Vec::new();
    if !d.starts_with(b"JRCW") || u32_at(&d, 4).ok() != Some(1) {
        return out;
    }
    let count = u32_at(&d, 8).unwrap_or(0) as usize;
    let mut p = 12;
    let s16 = |p: &mut usize| -> Option<String> {
        let n = u16::from_le_bytes(d.get(*p..*p + 2)?.try_into().ok()?) as usize;
        let s = String::from_utf8_lossy(d.get(*p + 2..*p + 2 + n)?).to_string();
        *p += 2 + n;
        Some(s)
    };
    for _ in 0..count {
        let (Some(binder), Some(name)) = (s16(&mut p), s16(&mut p)) else { break };
        let Ok(n) = u32_at(&d, p) else { break };
        let Some(data) = d.get(p + 4..p + 4 + n as usize) else { break };
        p += 4 + n as usize;
        out.push(CcwEntry { binder, name, data: data.to_vec() });
    }
    out
}

/// The binders besides c0000.anibnd that Carian Combo Warriors' animations go into (mod folder names).
fn ccw_outputs() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in load_ccw() {
        // (relative to the chr folder: "../sfx/..." for their effects)
        let n = match e.binder.strip_prefix("/chr/") {
            Some(n) => n.to_string(),
            None => format!("..{}", e.binder),
        };
        if e.binder != ANIBND && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// The game's id and full name for an entry of theirs the binder doesn't have yet (their stance set
/// a060-a062, their table a06): the binder's own numbering (an animation: 1_000_000_000 + its id; a move
/// table: 5_000_000 + its number) in the folder its kind already uses there.
fn ccw_new_entry(name: &str, files: &[bnd4::File]) -> Option<(i32, String)> {
    // anything else (their effects, textures): after the last of its kind, in that kind's folder
    if !(name.ends_with(".hkx") || name.ends_with(".tae")) {
        let ext = name.rsplit('.').next()?.to_ascii_lowercase();
        let kind: Vec<&bnd4::File> = files.iter().filter(|f| f.name.to_ascii_lowercase().ends_with(&format!(".{ext}"))).collect();
        let like = kind.last()?;
        let folder = &like.name[..like.name.rfind('\\')? + 1];
        let id = kind.iter().map(|f| f.id).max()? + 1;
        return Some((id, format!("{folder}{name}")));
    }
    let stem = name.strip_suffix(".hkx").or_else(|| name.strip_suffix(".tae"))?;
    let digits: String = stem.chars().filter(|c| c.is_ascii_digit()).collect();
    let (id, ext) = if name.ends_with(".tae") {
        (5_000_000 + digits.parse::<i32>().ok()?, ".tae")
    } else {
        let (sec, num) = stem.trim_start_matches('a').split_once('_')?;
        (1_000_000_000 + sec.parse::<i32>().ok()? * 1_000_000 + num.parse::<i32>().ok()?, ".compendium")
    };
    let like = files.iter().find(|f| f.name.to_ascii_lowercase().ends_with(if ext == ".tae" { ".tae" } else { ".hkx" }))
        .or_else(|| files.iter().find(|f| f.name.to_ascii_lowercase().ends_with(ext)))?;
    let folder = &like.name[..like.name.rfind('\\')? + 1];
    Some((id, format!("{folder}{name}")))
}

/// Put their entries over the game's in `files` (a binder of `binder`), or in when it has none: how
/// many went in.
fn ccw_into(list: &[CcwEntry], binder: &str, files: &mut Vec<bnd4::File>) -> usize {
    let mut n = 0;
    for e in list.iter().filter(|e| e.binder == binder) {
        let tail = format!("\\{}", e.name).to_ascii_lowercase();
        if !files.iter().any(|f| f.name.to_ascii_lowercase().ends_with(&tail)) {
            if let Some((id, name)) = ccw_new_entry(&e.name, files) {
                files.push(bnd4::File { id, name, data: e.data.clone() });
                n += 1;
            }
            continue;
        }
        let tail = format!("\\{}", e.name).to_ascii_lowercase();
        if let Some(f) = files.iter_mut().find(|f| f.name.to_ascii_lowercase().ends_with(&tail)) {
            // (their tables lack the game's newer entries, 1.12's second backstep attacks: only the game's
            // own player script asks for those, and it only plays Shadow of the Erdtree's weapon types now;
            // copying them over crashed the game, their events point at data elsewhere in the game's table)
            f.data = e.data.clone();
            n += 1;
        }
    }
    n
}

/// 1.2.0 (the user: Asta's red glintstone seal never showed): the effects binder keeps each kind of file
/// (effects, their resource lists, textures) in name order, numbered in that order from the kind's first
/// id, and the game looks effects up by halving that list: added effects appended at the end, out of
/// order, were never found (Carian Combo Warriors' own new ones neither). Each kind is put back in name
/// order, numbered from its first id again, the whole binder in id order.
fn in_name_order(files: &mut Vec<bnd4::File>) {
    let kind = |f: &bnd4::File| f.name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let mut kinds: Vec<String> = files.iter().map(kind).collect();
    kinds.sort();
    kinds.dedup();
    for k in kinds {
        let mut idx: Vec<usize> = (0..files.len()).filter(|&i| kind(&files[i]) == k).collect();
        let base = idx.iter().map(|&i| files[i].id).min().unwrap_or(0);
        let key = |f: &bnd4::File| f.name.rsplit('\\').next().unwrap_or("").to_ascii_lowercase();
        idx.sort_by(|&a, &b| key(&files[a]).cmp(&key(&files[b])));
        for (rank, &i) in idx.iter().enumerate() {
            files[i].id = base + rank as i32;
        }
    }
    files.sort_by_key(|f| f.id);
}

/// The other binders with their animations (c0000_a4x: the fists'), into `dir`.
fn make_ccw(archives: &Archives, list: &[CcwEntry], dir: &Path) -> Result<usize, String> {
    let mut made = 0;
    for name in ccw_outputs() {
        let bnd = match name.strip_prefix("..") {
            Some(path) => path.to_string(),
            None => format!("/chr/{name}"),
        };
        let src = dcx::decompress(&archives.read(&bnd)?)?;
        let mut files = bnd4::read(&src)?;
        made += ccw_into(list, &bnd, &mut files);
        // (an animation binder in id order)
        if bnd.starts_with("/chr/") {
            files.sort_by_key(|f| f.id);
        } else {
            in_name_order(&mut files);
        }
        let out = dir.join(&name);
        if let Some(parent) = out.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // (2.0.7: the source and its files freed before compressing: the effects binder is 357 MB, held three times over)
        let built = bnd4::build(&src, &files);
        let size = src.len() >> 20;
        drop(files);
        drop(src);
        let peak = crate::recolor::private_mb();
        std::fs::write(out, dcx::compress(&built)?).map_err(|e| e.to_string())?;
        log!("anims: {name} written ({size} MB of the binder, memory {peak} MB)");
    }
    Ok(made)
}

// ---------------------------------------------------------------- spline encoding (Havok)

const F40: f32 = 0.000345436;
const STRIDE: usize = 10;

/// THREECOMP40: the three smallest components in 12 bits each, the largest's index in 2 bits.
fn enc40(q: [f32; 4]) -> [u8; 5] {
    let l = (q.iter().map(|x| x * x).sum::<f32>()).sqrt().max(1e-9);
    let mut q = q.map(|x| x / l);
    let k = (0..4).max_by(|&a, &b| q[a].abs().total_cmp(&q[b].abs())).unwrap();
    if q[k] < 0.0 {
        q = q.map(|x| -x);
    }
    let mut v: u64 = 0;
    let mut j = 0;
    for (i, c) in q.iter().enumerate() {
        if i == k {
            continue;
        }
        let r = ((c / F40 + 2047.0).round() as i64).clamp(0, 4095) as u64;
        v |= r << (12 * j);
        j += 1;
    }
    v |= (k as u64) << 36;
    let b = v.to_le_bytes();
    [b[0], b[1], b[2], b[3], b[4]]
}

fn pad4(b: &mut Vec<u8>) {
    while b.len() % 4 != 0 {
        b.push(0);
    }
}

fn knots(body: &mut Vec<u8>, n: usize) {
    body.extend_from_slice(&(n as u16).to_le_bytes());
    body.push(1);
    body.push(0);
    body.extend((0..=n).map(|k| k as u8));
    body.push(n as u8);
}

/// Which components of a position/scale track vary (spline), stay put (static) or keep the default.
fn vec_type(v: &[[f32; 3]], default: f32, n: usize) -> (u8, [Option<(f32, f32)>; 3]) {
    let mut ty = 0u8;
    let mut comp: [Option<(f32, f32)>; 3] = [None; 3];
    for c in 0..3 {
        let lo = v.iter().map(|p| p[c]).fold(f32::MAX, f32::min);
        let hi = v.iter().map(|p| p[c]).fold(f32::MIN, f32::max);
        if hi - lo > 1e-5 && n > 0 {
            ty |= 0x10 << c;
            comp[c] = Some((lo, hi));
        } else if (lo - default).abs() > 1e-6 {
            ty |= 1 << c;
            comp[c] = Some((lo, lo));
        }
    }
    (ty, comp)
}

/// A position or scale track: knots, ranges/values, 16-bit control points.
fn vec_track(body: &mut Vec<u8>, v: &[[f32; 3]], ty: u8, comp: &[Option<(f32, f32)>; 3], n: usize) {
    if ty & 0x70 != 0 {
        knots(body, n);
        pad4(body);
    }
    for c in 0..3 {
        match comp[c] {
            Some((lo, hi)) if ty & (0x10 << c) != 0 => {
                body.extend_from_slice(&lo.to_le_bytes());
                body.extend_from_slice(&hi.to_le_bytes());
            }
            Some((x, _)) => body.extend_from_slice(&x.to_le_bytes()),
            None => {}
        }
    }
    if ty & 0x70 != 0 {
        for p in v {
            for c in 0..3 {
                if let Some((lo, hi)) = comp[c] {
                    if ty & (0x10 << c) != 0 {
                        let r = if hi > lo { ((p[c] - lo) / (hi - lo) * 65535.0).round() as u16 } else { 0 };
                        body.extend_from_slice(&r.to_le_bytes());
                    }
                }
            }
        }
        pad4(body);
    }
}

/// Degree-1 splines, one control point per frame; positions 16-bit, rotations THREECOMP40, scale
/// 16-bit. Returns (data, block offsets, mask size).
fn encode(a: &Anim) -> (Vec<u8>, Vec<u32>, u32) {
    let nt = a.bones.len();
    let at = |f: usize, t: usize| &a.data[(f * nt + t) * STRIDE..(f * nt + t) * STRIDE + STRIDE];
    let mut blob = Vec::new();
    let mut offs = Vec::new();
    let mut b = 0;
    loop {
        let f0 = b * 255;
        let f1 = (f0 + 256).min(a.frames);
        let n = f1 - f0 - 1;
        offs.push(blob.len() as u32);
        let mut masks = Vec::with_capacity(4 * nt);
        let mut body = Vec::new();
        for t in 0..nt {
            let pos: Vec<[f32; 3]> = (f0..f1).map(|f| { let v = at(f, t); [v[0], v[1], v[2]] }).collect();
            let scl: Vec<[f32; 3]> = (f0..f1).map(|f| { let v = at(f, t); [v[7], v[8], v[9]] }).collect();
            let mut rot: Vec<[f32; 4]> = (f0..f1).map(|f| { let v = at(f, t); [v[3], v[4], v[5], v[6]] }).collect();
            for i in 1..rot.len() {
                if (0..4).map(|k| rot[i][k] * rot[i - 1][k]).sum::<f32>() < 0.0 {
                    rot[i] = rot[i].map(|x| -x);
                }
            }
            let (ptype, pcomp) = vec_type(&pos, 0.0, n);
            let (stype, scomp) = vec_type(&scl, 1.0, n);
            let rvar = n > 0 && rot.iter().any(|r| (0..4).any(|k| (r[k] - rot[0][k]).abs() > 1e-4));
            masks.extend_from_slice(&[0x01 | (1 << 2) | (1 << 6), ptype, if rvar { 0xF0 } else { 0x0F }, stype]);
            vec_track(&mut body, &pos, ptype, &pcomp, n);
            if rvar {
                knots(&mut body, n);
                for r in &rot {
                    body.extend_from_slice(&enc40(*r));
                }
            } else {
                body.extend_from_slice(&enc40(rot[0]));
            }
            pad4(&mut body);
            vec_track(&mut body, &scl, stype, &scomp, n);
        }
        blob.extend_from_slice(&masks);
        blob.extend_from_slice(&body);
        while blob.len() % 16 != 0 {
            blob.push(0);
        }
        b += 1;
        if f1 >= a.frames {
            break;
        }
    }
    (blob, offs, 4 * nt as u32)
}

// ---------------------------------------------------------------- Havok tagfile container

struct Section {
    tag: [u8; 4],
    leaf: bool,
    body: Vec<u8>,
    kids: Vec<Section>,
}

fn parse_sections(d: &[u8]) -> Result<Vec<Section>, String> {
    let mut out = Vec::new();
    let mut o = 0;
    while o + 8 <= d.len() {
        let h = u32::from_be_bytes(d[o..o + 4].try_into().unwrap());
        let size = (h & 0x3FFF_FFFF) as usize;
        let leaf = h & 0x4000_0000 != 0;
        if size < 8 || o + size > d.len() {
            return Err("bad hkx section".into());
        }
        let tag: [u8; 4] = d[o + 4..o + 8].try_into().unwrap();
        let inner = &d[o + 8..o + size];
        out.push(if leaf { Section { tag, leaf, body: inner.to_vec(), kids: vec![] } } else { Section { tag, leaf, body: vec![], kids: parse_sections(inner)? } });
        o += size;
    }
    Ok(out)
}

fn write_sections(s: &[Section]) -> Vec<u8> {
    let mut out = Vec::new();
    for x in s {
        let leaf = x.leaf;
        let inner = if leaf { x.body.clone() } else { write_sections(&x.kids) };
        let h = (inner.len() as u32 + 8) | if leaf { 0x4000_0000 } else { 0 };
        out.extend_from_slice(&h.to_be_bytes());
        out.extend_from_slice(&x.tag);
        out.extend_from_slice(&inner);
    }
    out
}

fn find<'a>(s: &'a mut [Section], tag: &[u8; 4]) -> Option<&'a mut Section> {
    for x in s.iter_mut() {
        if &x.tag == tag {
            return Some(x);
        }
        if let Some(f) = find(&mut x.kids, tag) {
            return Some(f);
        }
    }
    None
}

/// Our animation in the template's container. The template's own arrays stay where they are,
/// unused; the new ones go at the end of DATA and their items point there.
fn build_hkx(tpl: &[u8], a: &Anim) -> Result<Vec<u8>, String> {
    let mut root = parse_sections(tpl)?;
    let items: Vec<(u32, u32, u32)> = {
        let it = find(&mut root, b"ITEM").ok_or("no ITEM")?;
        it.body.chunks_exact(12).map(|c| (u32_at(c, 0).unwrap(), u32_at(c, 4).unwrap(), u32_at(c, 8).unwrap())).collect()
    };
    let data = &mut find(&mut root, b"DATA").ok_or("no DATA")?.body;
    // The template's layout (a000_080000 in 2.7.1.x): the spline animation is the only object whose
    // fields point at three uint arrays and a byte array; check before writing into it.
    let anim_item = 8usize;
    let bind_item = 9usize;
    let ref_item = 10usize;
    let obj = |k: usize| items.get(k).map(|i| i.1 as usize).ok_or("template layout");
    let (ao, bo) = (obj(anim_item)?, obj(bind_item)?);
    // 1.2.0 round 5: an enemy's clip has no reference frame (no root motion): item 10 is then an array
    let ro = items.get(ref_item).filter(|i| i.0 >> 28 == 1).map(|i| i.1 as usize);
    let ptr = |d: &[u8], o: usize| u64_at(d, o).map(|v| v as usize);
    let (blocks_i, fblocks_i, blob_i) = (ptr(data, ao + 96)?, ptr(data, ao + 112)?, ptr(data, ao + 160)?);
    let tracks_i = ptr(data, bo + 40)?;
    let samples_i = match ro {
        Some(ro) => Some(ptr(data, ro + 72)?),
        None => None,
    };
    let ntr_tpl = u32_at(data, ao + 32)? as usize;
    if [blocks_i, fblocks_i, blob_i, tracks_i].iter().chain(samples_i.iter()).any(|&i| i == 0 || i >= items.len()) || items[tracks_i].2 as usize != ntr_tpl {
        return Err("template animation has an unexpected layout".into());
    }
    let (blob, boffs, msize) = encode(a);
    let nb = boffs.len() as u32;
    let fps = 30.0f32;
    let dur = (a.frames.max(2) - 1) as f32 / fps;
    let bd = 255.0 / fps;
    data[ao + 28..ao + 32].copy_from_slice(&dur.to_le_bytes());
    data[ao + 32..ao + 36].copy_from_slice(&(a.bones.len() as u32).to_le_bytes());
    for (k, v) in [(64, a.frames as u32), (68, nb), (72, 256), (76, msize)] {
        data[ao + k..ao + k + 4].copy_from_slice(&v.to_le_bytes());
    }
    for (k, v) in [(80, bd), (84, 1.0 / bd), (88, 1.0 / fps)] {
        data[ao + k..ao + k + 4].copy_from_slice(&v.to_le_bytes());
    }
    data[bo + 88] = 0; // blend hint: normal
    if let Some(ro) = ro {
        data[ro + 64..ro + 68].copy_from_slice(&dur.to_le_bytes());
    }
    let mut new_items = items.clone();
    let mut put = |data: &mut Vec<u8>, item: usize, bytes: &[u8], count: usize| {
        while data.len() % 16 != 0 {
            data.push(0);
        }
        new_items[item].1 = data.len() as u32;
        new_items[item].2 = count as u32;
        data.extend_from_slice(bytes);
    };
    put(data, blocks_i, &boffs.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>(), boffs.len());
    put(data, fblocks_i, &vec![0u8; 4 * boffs.len()], boffs.len());
    put(data, blob_i, &blob, blob.len());
    put(data, tracks_i, &a.bones.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>(), a.bones.len());
    if let Some(samples_i) = samples_i {
        put(data, samples_i, &vec![0u8; 16 * a.frames], a.frames);
    }
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let it = find(&mut root, b"ITEM").ok_or("no ITEM")?;
    it.body = new_items.iter().flat_map(|(a, b, c)| [a.to_le_bytes(), b.to_le_bytes(), c.to_le_bytes()].concat()).collect();
    Ok(write_sections(&root))
}

// ---------------------------------------------------------------- TAE (0x1000D)

/// New entries `copies` (new id -> game id): each gets its own copy of the game entry's events and
/// anim file header (the game turns offsets into pointers in place, so nothing can be shared). The
/// old table stays (unused); a new sorted table and its id groups go at the end.
fn tae_add(t: &[u8], copies: &[(u32, u32)]) -> Result<Vec<u8>, String> {
    let mut t = t.to_vec();
    if !t.starts_with(b"TAE ") {
        return Err("a00.tae: bad header".into());
    }
    let count = u32_at(&t, 84)? as usize;
    let table = u64_at(&t, 88)? as usize;
    let mut ents: std::collections::BTreeMap<i64, u64> = std::collections::BTreeMap::new();
    for i in 0..count {
        ents.insert(u64_at(&t, table + 16 * i)? as i64, u64_at(&t, table + 16 * i + 8)?);
    }
    let mut starts: Vec<u64> = ents.values().map(|b| u64_at(&t, *b as usize + 24)).collect::<Result<_, _>>()?;
    starts.sort();
    starts.dedup();
    let align16 = |t: &mut Vec<u8>| while t.len() % 16 != 0 { t.push(0) };
    for &(new, src) in copies {
        let body = *ents.get(&(src as i64)).ok_or_else(|| format!("a00.tae has no {src}"))? as usize;
        let a = u64_at(&t, body + 24)?;
        let k = starts.iter().position(|s| *s == a).ok_or("a00.tae layout")?;
        let b = starts.get(k + 1).copied().unwrap_or(t.len() as u64);
        align16(&mut t);
        let delta = t.len() as u64 - a;
        let mut reg = t[a as usize..b as usize].to_vec();
        for o in (0..reg.len().saturating_sub(7)).step_by(8) {
            let v = u64_at(&reg, o)?;
            if v >= a && v < b {
                reg[o..o + 8].copy_from_slice(&(v + delta).to_le_bytes());
            }
        }
        let old: Vec<u8> = format!("a000_{src:06}").encode_utf16().flat_map(u16::to_le_bytes).collect();
        let newn: Vec<u8> = format!("a000_{new:06}").encode_utf16().flat_map(u16::to_le_bytes).collect();
        if let Some(i) = reg.windows(old.len()).position(|w| w == old.as_slice()) {
            reg[i..i + newn.len()].copy_from_slice(&newn);
        }
        t.extend_from_slice(&reg);
        align16(&mut t);
        let nb = t.len() as u64;
        let mut bd = t[body..body + 48].to_vec();
        for o in (0..32).step_by(8) {
            let v = u64_at(&bd, o)?;
            if v >= a && v < b {
                bd[o..o + 8].copy_from_slice(&(v + delta).to_le_bytes());
            }
        }
        t.extend_from_slice(&bd);
        ents.insert(new as i64, nb);
    }
    align16(&mut t);
    let table = t.len();
    let ids: Vec<i64> = ents.keys().copied().collect();
    for id in &ids {
        t.extend_from_slice(&id.to_le_bytes());
        t.extend_from_slice(&ents[id].to_le_bytes());
    }
    let groups_hdr = t.len();
    let mut groups = Vec::new();
    let mut k = 0;
    while k < ids.len() {
        let first = k;
        while k + 1 < ids.len() && ids[k + 1] == ids[k] + 1 {
            k += 1;
        }
        groups.push((ids[first] as i32, ids[k] as i32, (table + 16 * first) as u64));
        k += 1;
    }
    t.extend_from_slice(&(groups.len() as u64).to_le_bytes());
    t.extend_from_slice(&(groups_hdr as u64 + 16).to_le_bytes());
    for (g0, g1, o) in groups {
        t.extend_from_slice(&g0.to_le_bytes());
        t.extend_from_slice(&g1.to_le_bytes());
        t.extend_from_slice(&o.to_le_bytes());
    }
    t[84..88].copy_from_slice(&(ids.len() as u32).to_le_bytes());
    t[88..96].copy_from_slice(&(table as u64).to_le_bytes());
    t[96..104].copy_from_slice(&(groups_hdr as u64).to_le_bytes());
    t[112..120].copy_from_slice(&(ids.len() as u64).to_le_bytes());
    let len = t.len() as u32;
    t[12..16].copy_from_slice(&len.to_le_bytes());
    Ok(t)
}

// ---------------------------------------------------------------- build

fn stamp() -> Option<String> {
    let mut bin = std::fs::read(dll_dir()?.join("anims.bin")).ok()?;
    // 1.2.0 round 5: and the enemies' (when there are any), and Carian Combo Warriors'
    for extra in ["ccw_anims.bin"] {
        if let Ok(e) = std::fs::read(dll_dir()?.join(extra)) {
            bin.extend_from_slice(&e);
        }
    }
    let sum = bin.iter().fold(0u64, |h, b| h.wrapping_mul(1_000_003).wrapping_add(*b as u64));
    Some(format!("jumpring-anims-v{VERSION}\n{}\n{} {sum:x}\n", crate::recolor::game_build(), bin.len()))
}

fn out_dir() -> Option<PathBuf> {
    Some(crate::recolor::mod_dir()?.join("chr"))
}

/// The binders are there and match this game and these animations.
pub fn up_to_date() -> bool {
    // lab: other clips in our slots this run (JR_LAB_SLOTS): always made again
    #[cfg(feature = "lab")]
    if std::env::var_os("JR_LAB_SLOTS").is_some() {
        return false;
    }
    let (Some(want), Some(dir)) = (stamp(), out_dir()) else { return false };
    let ok = std::fs::read_to_string(dir.join("jumpring-anims.txt")).ok().as_deref() == Some(want.as_str())
        && dir.join("c0000.anibnd.dcx").exists()
        && dir.join("c0000_a00_hi.anibnd.dcx").exists()
        && enemy_outputs().iter().all(|n| dir.join(n).exists())
        && ccw_outputs().iter().all(|n| dir.join(n).exists());
    if ok {
        READY.store(true, Ordering::Relaxed);
    }
    ok
}

/// 1.2.0 (the user: could the players "load the new animations before launching the game"): the
/// starter's prepare step builds everything here before Elden Ring starts (src/bin/prepare.rs), so the
/// game never builds them while it loads (a launch right after building them twice froze loading into
/// the world). The game itself still builds them when they are missing (an older starter).
pub fn prepare() -> Result<&'static str, String> {
    if up_to_date() {
        return Ok("already up to date");
    }
    let archives = Archives::open()?;
    make(&archives);
    if up_to_date() { Ok("made") } else { Err("not made (see the log)".into()) }
}

/// Make both binders (called by the recolour worker, which already holds the game's archives).
pub fn make(archives: &Archives) {
    let t0 = std::time::Instant::now();
    match make_inner(archives) {
        Ok(n) => {
            READY.store(true, Ordering::Relaxed);
            log!("anims: {n} animations in the player's binders ({:.1}s)", t0.elapsed().as_secs_f32());
        }
        Err(e) => {
            // without our binders the moves fall back to Elden Ring's own animations
            if let Some(dir) = out_dir() {
                let _ = std::fs::remove_file(dir.join("c0000.anibnd.dcx"));
                let _ = std::fs::remove_file(dir.join("c0000_a00_hi.anibnd.dcx"));
                let _ = std::fs::remove_file(dir.join("jumpring-anims.txt"));
                for n in enemy_outputs().into_iter().chain(ccw_outputs()) {
                    let _ = std::fs::remove_file(dir.join(n));
                }
            }
            log!("anims: not made ({e}); moves use Elden Ring's animations");
        }
    }
}

fn make_inner(archives: &Archives) -> Result<usize, String> {
    let dir = out_dir().ok_or("mod folder not found")?;
    let want = stamp().ok_or("anims.bin missing")?;
    let anims = load(&dll_dir().ok_or("no dll dir")?.join("anims.bin"))?;
    // the animations
    let hi_src = dcx::decompress(&archives.read(A00_HI)?)?;
    let mut hi = bnd4::read(&hi_src)?;
    let tpl_name = format!("a000_{TEMPLATE:06}.hkx");
    let (tpl_path, tpl) = hi.iter().find(|f| f.name.ends_with(&tpl_name)).map(|f| (f.name.clone(), f.data.clone())).ok_or("template animation missing")?;
    let folder = tpl_path[..tpl_path.len() - tpl_name.len()].to_string();
    let (commit, phys) = crate::recolor::memory_free_mb();
    log!("anims: making the animations ({commit} MB free to commit, {phys} MB of memory free)");
    let mut skipped: Vec<u32> = Vec::new();
    // (2.0.7: the binders below are freed as soon as the animations are written, before the next ones are read: all of
    // them held at once took 2.2 GB, and a PC short of memory crashed here)
    {
    // other sections' clips (Ashes of War) are read from the binders that hold them, once each
    let mut others: Vec<(String, Vec<bnd4::File>)> = Vec::new();
    let mut find_clip = |id: u32, hi: &[bnd4::File]| -> Option<Vec<u8>> {
        let (sec, num) = if id >= 1_000_000 { (id / 1_000_000, id % 1_000_000) } else { (0, id) };
        let want = format!("a{sec:03}_{num:06}.hkx");
        if let Some(f) = hi.iter().find(|f| f.name.ends_with(&want)) {
            return Some(f.data.clone());
        }
        for bnd in OTHER_BINDERS {
            if !others.iter().any(|(n, _)| n == bnd) {
                let files = archives.read(bnd).ok().and_then(|d| dcx::decompress(&d).ok()).and_then(|d| bnd4::read(&d).ok()).unwrap_or_default();
                others.push((bnd.to_string(), files));
            }
            let files = &others.iter().find(|(n, _)| n == bnd).unwrap().1;
            if let Some(f) = files.iter().find(|f| f.name.ends_with(&want)) {
                return Some(f.data.clone());
            }
        }
        None
    };
    // lab (Carian Combo Warriors audition): JR_LAB_SLOTS = clip ids, one per slot in order, read from the
    // .hkx files in JR_LAB_CLIPS (a<sec>_<num>.hkx)
    #[cfg(feature = "lab")]
    let lab_slots: Vec<u32> = std::env::var("JR_LAB_SLOTS").unwrap_or_default().split(',').filter_map(|s| s.trim().parse().ok()).collect();
    for (_k, a) in anims.iter().enumerate() {
        #[cfg(feature = "lab")]
        if let (Some(&clip), Ok(dir)) = (lab_slots.get(_k), std::env::var("JR_LAB_CLIPS")) {
            let p = std::path::Path::new(&dir).join(format!("a{:03}_{:06}.hkx", clip / 1_000_000, clip % 1_000_000));
            match std::fs::read(&p) {
                Ok(data) => {
                    let id = 1_000_000_000 + a.id as i32;
                    hi.retain(|f| f.id != id);
                    hi.push(bnd4::File { id, name: format!("{folder}a000_{:06}.hkx", a.id), data });
                    log!("lab slots: {} plays {}", a.id, p.display());
                    continue;
                }
                Err(e) => log!("lab slots: {}: {e}", p.display()),
            }
        }
        let data = if a.copy != 0 {
            // the game's own clip, unchanged, under our id (or its fallback)
            match find_clip(a.copy, &hi).or_else(|| (a.fallback != 0).then(|| find_clip(a.fallback, &hi)).flatten()) {
                Some(d) => d,
                // (1.3.0: one missing clip leaves only its own slot empty, not every animation unmade)
                None => {
                    log!("anims: game animation {} missing: slot {} left empty", a.copy, a.id);
                    skipped.push(a.id);
                    continue;
                }
            }
        } else {
            build_hkx(&tpl, a)?
        };
        let id = 1_000_000_000 + a.id as i32;
        hi.retain(|f| f.id != id);
        hi.push(bnd4::File { id, name: format!("{folder}a000_{:06}.hkx", a.id), data });
    }
    // lab: a binder-wide Havok type compendium to go with the auditioned clips (JR_LAB_COMPENDIUM: Carian
    // Combo Warriors' own, which their 1.10 clips were made with)
    #[cfg(feature = "lab")]
    if let Some(p) = std::env::var_os("JR_LAB_COMPENDIUM") {
        if let (Ok(data), Some(f)) = (std::fs::read(&p), hi.iter_mut().find(|f| f.name.to_ascii_lowercase().ends_with(".compendium"))) {
            f.data = data;
            log!("lab slots: compendium from {}", std::path::Path::new(&p).display());
        }
    }
    hi.sort_by_key(|f| f.id);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("c0000_a00_hi.anibnd.dcx"), dcx::compress(&bnd4::build(&hi_src, &hi))?).map_err(|e| e.to_string())?;
    let peak_hi = crate::recolor::private_mb();
    drop(hi);
    drop(hi_src);
    // their events
    let ani_src = dcx::decompress(&archives.read(ANIBND)?)?;
    let mut ani = bnd4::read(&ani_src)?;
    let tae = ani.iter_mut().find(|f| f.name.to_ascii_lowercase().ends_with("\\a00.tae")).ok_or("a00.tae missing")?;
    let copies: Vec<(u32, u32)> = anims.iter().filter(|a| !skipped.contains(&a.id)).map(|a| (a.id, a.events_from)).collect();
    tae.data = tae_add(&tae.data, &copies)?;
    let peak_ani = crate::recolor::private_mb();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // 1.2.0: Carian Combo Warriors' animations, then their move tables (only when the animations went in:
    // their tables with the game's clips would be half a moveset)
    let ccw = load_ccw();
    if !ccw.is_empty() {
        match make_ccw(archives, &ccw, &dir) {
            Ok(n) => {
                let t = ccw_into(&ccw, ANIBND, &mut ani);
                ani.sort_by_key(|f| f.id);
                log!("anims: Carian Combo Warriors: {n} animations, {t} move tables");
                log!("anims: memory {peak_hi} MB with the hi binder, {peak_ani} MB with the events, {} MB now", crate::recolor::private_mb());
            }
            Err(e) => {
                for n in ccw_outputs() {
                    let _ = std::fs::remove_file(dir.join(n));
                }
                log!("anims: Carian Combo Warriors not made ({e}); the game's own moves stay");
            }
        }
    }
    std::fs::write(dir.join("c0000.anibnd.dcx"), dcx::compress(&bnd4::build(&ani_src, &ani))?).map_err(|e| e.to_string())?;
    // 1.2.0: the launcher's enemy reaction was scrapped with the combo trees (the user saw it again after a
    // parry's burst): the soldiers keep their own. Binders an earlier build made are taken away.
    for b in enemy_binders("c4310") {
        let f = dir.join(b.trim_start_matches("/chr/"));
        if f.exists() && std::fs::remove_file(&f).is_ok() {
            log!("anims: removed the old enemy reaction binder {}", f.display());
        }
    }
    std::fs::write(dir.join("jumpring-anims.txt"), want).map_err(|e| e.to_string())?;
    Ok(anims.len())
}

// ---------------------------------------------------------------- playing

/// Play one of our animations (its id from the sheet) on the player now.
pub fn play(player: &mut PlayerIns, id: u32) -> bool {
    if !READY.load(Ordering::Relaxed) {
        return false;
    }
    player.chr_ins.modules.event.request_animation_id = id as i32;
    true
}
