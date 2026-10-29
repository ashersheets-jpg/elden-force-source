// Vendored from ER Mario (https://github.com/deltarooo/er-mario), MIT License, Copyright (c) the ER Mario authors.
//! DCX containers with Kraken compression, through the game's own oo2core_6_win64.dll.

use std::ffi::c_void;
use std::sync::OnceLock;

use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{HSTRING, s};

type Decompress = unsafe extern "C" fn(
    *const u8, isize, *mut u8, isize, i32, i32, i32, *mut c_void, isize, *mut c_void, *mut c_void, *mut c_void, isize, i32,
) -> isize;
type Compress =
    unsafe extern "C" fn(i32, *const u8, isize, *mut u8, i32, *const c_void, *const c_void, *const c_void, *mut c_void, isize) -> isize;
type GetDefault = unsafe extern "C" fn(i32, i32) -> *const u8;

struct Oodle {
    decompress: Decompress,
    compress: Compress,
    defaults: GetDefault,
}

const KRAKEN: i32 = 8;
/// Optimal2 like the game's own files; big files (the 200 MB menu textures) get Normal, which is
/// many times faster to compress (the game reads either)
// 1.3.0 (optimising: the first launch builds ~900 outfit files): Kraken "Normal". The game decompresses every level
// equally fast; 6 ("Optimal2") made files a few percent smaller at many times the time.
const LEVEL: i32 = 4;
const LEVEL_BIG: i32 = 4;

fn oodle() -> Result<&'static Oodle, String> {
    static OODLE: OnceLock<Option<Oodle>> = OnceLock::new();
    OODLE
        .get_or_init(|| unsafe {
            let path = super::archive::game_dir().join("oo2core_6_win64.dll");
            let dll = LoadLibraryW(&HSTRING::from(path.as_os_str())).ok()?;
            Some(Oodle {
                decompress: std::mem::transmute(GetProcAddress(dll, s!("OodleLZ_Decompress"))?),
                compress: std::mem::transmute(GetProcAddress(dll, s!("OodleLZ_Compress"))?),
                defaults: std::mem::transmute(GetProcAddress(dll, s!("OodleLZ_CompressOptions_GetDefault"))?),
            })
        })
        .as_ref()
        .ok_or_else(|| "oo2core_6_win64.dll not found next to eldenring.exe".to_string())
}

fn be32(d: &[u8], o: usize) -> usize {
    u32::from_be_bytes(d[o..o + 4].try_into().unwrap()) as usize
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    if !data.starts_with(b"DCX\0") {
        return Ok(data.to_vec());
    }
    if data.len() < 0x4C || &data[0x28..0x2C] != b"KRAK" {
        return Err("unsupported DCX format".into());
    }
    let (raw, comp) = (be32(data, 0x1C), be32(data, 0x20));
    let body = data.get(0x4C..0x4C + comp).ok_or("truncated DCX")?;
    let o = oodle()?;
    let mut out = vec![0u8; raw];
    let n = unsafe {
        (o.decompress)(
            body.as_ptr(), body.len() as isize, out.as_mut_ptr(), raw as isize, 1, 0, 0,
            std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0, 3,
        )
    };
    if n != raw as isize {
        return Err(format!("Kraken decompression failed ({n})"));
    }
    Ok(out)
}

/// A DCX_KRAK container like the game's own (level 6, independent 256 KB chunks, 16-byte padded).
pub fn compress(raw: &[u8]) -> Result<Vec<u8>, String> {
    let o = oodle()?;
    let level = if raw.len() > 32 << 20 { LEVEL_BIG } else { LEVEL };
    let mut opts = [0u8; 128];
    unsafe { std::ptr::copy_nonoverlapping((o.defaults)(KRAKEN, level), opts.as_mut_ptr(), 128) };
    opts[8..12].copy_from_slice(&1u32.to_le_bytes()); // seekChunkReset
    opts[12..16].copy_from_slice(&0x40000i32.to_le_bytes()); // seekChunkLen
    // (2.0.7: compressed straight in after the header, in a buffer only as big as Kraken can need (its worst case is a
    // few hundred bytes per 256 KB chunk over the input); it was 1.5x the input plus a second copy, 1 GB more at the peak
    // for the 357 MB effects binder. Same bytes out.)
    let mut out = vec![0u8; 0x4C + raw.len() + raw.len() / 64 + 65536];
    let n = unsafe {
        (o.compress)(
            KRAKEN, raw.as_ptr(), raw.len() as isize, out.as_mut_ptr().add(0x4C), level,
            opts.as_ptr() as *const c_void, std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), 0,
        )
    };
    if n <= 0 {
        return Err(format!("Kraken compression failed ({n})"));
    }
    out.truncate(0x4C + n as usize);
    let comp_len = n as usize;
    let mut head = Vec::with_capacity(0x4C);
    let be = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_be_bytes());
    head.extend_from_slice(b"DCX\0");
    for v in [0x11000, 0x18, 0x24, 0x44, 0x4C] {
        be(&mut head, v);
    }
    head.extend_from_slice(b"DCS\0");
    be(&mut head, raw.len() as u32);
    be(&mut head, comp_len as u32);
    head.extend_from_slice(b"DCP\0KRAK");
    be(&mut head, 0x20);
    be(&mut head, 0x0600_0000);
    head.extend_from_slice(&[0u8; 12]);
    be(&mut head, 0x0001_0100);
    head.extend_from_slice(b"DCA\0");
    be(&mut head, 8);
    debug_assert_eq!(head.len(), 0x4C);
    out[..0x4C].copy_from_slice(&head);
    out.resize(out.len().div_ceil(16) * 16, 0);
    Ok(out)
}
