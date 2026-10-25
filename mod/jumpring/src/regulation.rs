//! 1.2.0 (the user: "use carian combo warriors"): Carian Combo Warriors' moves need its param rows (attacks,
//! behaviours, bullets, special effects; by its author on Nexus Mods, used with permission and credit),
//! about 280, over a hundred of them rows the game's own regulation doesn't have.
//!
//! They go into the params in memory once the game has loaded them: rows the game has are overwritten in
//! place, and each param that gets new rows is copied into a bigger block (its rows keep their indices, the
//! new ones come after, the id lookup table is made again) that the game's param holder then points at.
//! The game's regulation.bin is left alone: a rebuilt one (tried first) has to fit the copy every save
//! keeps of it, and ours didn't ("Save data is corrupted").
//! Rows: jumpring/ccw_params.bin (tools_scripts/ccw/param_closure.py).

use std::collections::BTreeMap;

use eldenring::cs::SoloParamRepository;
use fromsoftware_shared::FromStatic;

use crate::log;

fn u32le(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}
fn u64le(d: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(d[o..o + 8].try_into().unwrap())
}

/// The rows: param name -> id -> bytes.
fn load_rows() -> Option<BTreeMap<String, BTreeMap<i32, Vec<u8>>>> {
    let d = std::fs::read(crate::recolor::mod_dir()?.parent()?.join("jumpring").join("ccw_params.bin")).ok()?;
    if !d.starts_with(b"JRCP") || u32le(&d, 4) != 1 {
        return None;
    }
    let mut out: BTreeMap<String, BTreeMap<i32, Vec<u8>>> = BTreeMap::new();
    let mut p = 12;
    for _ in 0..u32le(&d, 8) {
        let n = u16::from_le_bytes([d[p], d[p + 1]]) as usize;
        let name = String::from_utf8_lossy(&d[p + 2..p + 2 + n]).to_string();
        p += 2 + n;
        let (id, size) = (u32le(&d, p) as i32, u32le(&d, p + 4) as usize);
        out.entry(name).or_default().insert(id, d[p + 8..p + 8 + size].to_vec());
        p += 8 + size;
    }
    Some(out)
}

/// A param file (as loaded: header, row descriptors, row data, strings) with the new `added` rows: the
/// old rows keep their descriptors' places (indices), the new ones' come after them, their data after the
/// old data, the strings after that. Returns the file and its (id, index) lookup, sorted by id.
fn expand(p: &[u8], added: &[(i32, &[u8])], size: usize) -> Result<(Vec<u8>, Vec<(u32, u32)>), String> {
    let count = u16::from_le_bytes([p[0x0A], p[0x0B]]) as usize;
    let strings = u32le(p, 0) as usize;
    let data_start = u64le(p, 0x30) as usize;
    if p[0x2D] & 0x80 == 0 || data_start != 0x40 + 24 * count || strings > p.len() {
        return Err("param layout".into());
    }
    let m = added.len();
    if count + m > u16::MAX as usize {
        return Err("too many rows".into());
    }
    let shift = 24 * m as u64;
    let names_shift = shift + (size * m) as u64;
    let mut out = p[..0x40].to_vec();
    let mut lookup = Vec::with_capacity(count + m);
    let mut empty_name = strings as u64;
    for i in 0..count {
        let o = 0x40 + 24 * i;
        let (id, d, n) = (u32le(p, o), u64le(p, o + 8), u64le(p, o + 16));
        empty_name = n;
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(d + shift).to_le_bytes());
        out.extend_from_slice(&(n + names_shift).to_le_bytes());
        lookup.push((id, i as u32));
    }
    for (k, (id, _)) in added.iter().enumerate() {
        out.extend_from_slice(&(*id as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(strings as u64 + shift + (k * size) as u64).to_le_bytes());
        out.extend_from_slice(&(empty_name + names_shift).to_le_bytes());
        lookup.push((*id as u32, (count + k) as u32));
    }
    out.extend_from_slice(&p[data_start..strings]);
    for (_, row) in added {
        out.extend_from_slice(row);
    }
    out.extend_from_slice(&p[strings..]);
    out[0x0A..0x0C].copy_from_slice(&((count + m) as u16).to_le_bytes());
    out[0..4].copy_from_slice(&((strings as u64 + names_shift) as u32).to_le_bytes());
    out[0x10..0x18].copy_from_slice(&(u64le(p, 0x10) + names_shift).to_le_bytes());
    out[0x30..0x38].copy_from_slice(&(0x40 + 24 * (count + m) as u64).to_le_bytes());
    lookup.sort();
    Ok((out, lookup))
}

/// The game's own allocator lookup: which of its allocators owns an address (eldenring.exe 2.7.2.0 +0x1ec9340, called only
/// when its first bytes match)
const FIND_ALLOCATOR_RVA: usize = 0x1ec9340;
const FIND_ALLOCATOR_START: [u8; 13] = [0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0x1d];
/// DLKR::DLAllocator's allocate_aligned (vtable slot 10; deallocate, slot 13, is what the game frees its params with)
const ALLOCATE_ALIGNED_SLOT: usize = 10;

/// `total` bytes (zeroed, 16-aligned) made by the allocator that made the game's block at `owner`: the game's own free of
/// them, as it closes, then goes back where they came from.
fn game_alloc(owner: *const u8, total: usize) -> Option<*mut u8> {
    let base = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) } as usize;
    let find = (base + FIND_ALLOCATOR_RVA) as *const u8;
    if unsafe { std::slice::from_raw_parts(find, FIND_ALLOCATOR_START.len()) } != FIND_ALLOCATOR_START {
        log!("params: the game's allocator lookup is not where 2.7.2.0 has it");
        return None;
    }
    let find: unsafe extern "C" fn(*const u8) -> *mut *const usize = unsafe { std::mem::transmute(find) };
    let alloc = unsafe { find(owner) };
    if alloc.is_null() {
        log!("params: no allocator of the game's owns its block {owner:?}");
        return None;
    }
    let slot = unsafe { *(*alloc).add(ALLOCATE_ALIGNED_SLOT) };
    let allocate_aligned: unsafe extern "C" fn(*mut *const usize, usize, usize) -> *mut u8 = unsafe { std::mem::transmute(slot) };
    let p = unsafe { allocate_aligned(alloc, total, 0x10) };
    if p.is_null() {
        log!("params: the game's allocator {alloc:?} gave nothing for {total} bytes");
        return None;
    }
    unsafe { std::ptr::write_bytes(p, 0, total) };
    Some(p)
}

/// Put the rows in; false while the params aren't loaded yet (try again later), true once done (or there
/// is nothing to do).
pub fn inject() -> bool {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
    let Some(rows) = load_rows() else { return true };
    let mut done = (0, 0);
    for holder in repo.solo_param_holders.iter_mut() {
        let Some(rc) = holder.get_res_cap_mut(0) else { continue };
        let Some((name, want)) = rows.iter().find(|(n, _)| rc.res_cap.name == n.as_str()) else { continue };
        let frc = rc.param_res_cap.as_mut();
        let file = frc.data.as_ptr() as *mut u8;
        // the runtime header before the file: the end of its strings, and its row count
        let after_name = unsafe { *(file.sub(0x10) as *const u32) } as usize;
        let p = unsafe { std::slice::from_raw_parts_mut(file, after_name) };
        let count = u16::from_le_bytes([p[0x0A], p[0x0B]]) as usize;
        let mut offs: Vec<u64> = (0..count).map(|i| u64le(p, 0x40 + 24 * i + 8)).collect();
        offs.sort();
        let size = offs.windows(2).map(|w| w[1] - w[0]).filter(|&d| d > 0).min().unwrap_or(0) as usize;
        let index: BTreeMap<u32, usize> = (0..count).map(|i| (u32le(p, 0x40 + 24 * i), u64le(p, 0x40 + 24 * i + 8) as usize)).collect();
        let mut added: Vec<(i32, &[u8])> = Vec::new();
        for (&id, row) in want {
            if row.len() != size {
                log!("params: {name} {id}: {} bytes, the game's rows are {size}", row.len());
                continue;
            }
            match index.get(&(id as u32)) {
                Some(&off) => {
                    p[off..off + size].copy_from_slice(row);
                    done.0 += 1;
                }
                None => added.push((id, row.as_slice())),
            }
        }
        if added.is_empty() {
            continue;
        }
        let (new_file, lookup) = match expand(p, &added, size) {
            Ok(x) => x,
            Err(e) => {
                log!("params: {name}: {e}");
                continue;
            }
        };
        // [runtime header 0x10][file][to 0x10][lookup] (the old block stays too: anything that kept a pointer to a row
        // keeps a good one)
        let file_len = new_file.len();
        let table = file_len.next_multiple_of(0x10);
        let total = 0x10 + table + 8 * lookup.len();
        // 2.0.1: made by the allocator that made the game's own block, so the game freeing it as it closes is a free of its
        // own (made by ours, no allocator of the game's owned it and the game handed it to its Windows heap's free: heap
        // corruption, c0000374, every time Elden Ring closed: players' most common crash, lab 2026-10-21)
        let base = match game_alloc(unsafe { file.sub(0x10) }, total) {
            Some(b) => b,
            None => {
                log!("params: {name}: made apart (the game's close may crash)");
                let layout = std::alloc::Layout::from_size_align(total, 0x10).unwrap();
                unsafe { std::alloc::alloc_zeroed(layout) }
            }
        };
        if base.is_null() {
            continue;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(file.sub(0x10), base, 0x10);
            std::ptr::copy_nonoverlapping(new_file.as_ptr(), base.add(0x10), file_len);
            *(base as *mut u32) = file_len as u32;
            *(base.add(4) as *mut u32) = lookup.len() as u32;
            let t = base.add(0x10 + table) as *mut u32;
            for (i, (id, ix)) in lookup.iter().enumerate() {
                *t.add(2 * i) = *id;
                *t.add(2 * i + 1) = *ix;
            }
        }
        let old_size = frc.size;
        frc.size = (table + 8 * lookup.len()) as u64;
        unsafe { *(&mut frc.data as *mut _ as *mut *mut u8) = base.add(0x10) };
        log!("params: {name}: {} rows added (block {old_size} -> {} bytes; file {after_name} -> {file_len})", added.len(), frc.size);
        done.1 += added.len();
    }
    log!("params: Carian Combo Warriors: {} rows changed, {} added", done.0, done.1);
    true
}
