// Carian Combo Warriors survey (1.2.0 lab tool, src/bin/ccw_survey.rs): which of its character files differ
// from the player's current game, entry by entry. Writes a report and the changed / new entries to disk.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::ermario::{archive, bnd4, dcx};

fn sum(d: &[u8]) -> u64 {
    d.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100_0000_01b3))
}

pub fn ccw_survey(ccw: &Path, out: &Path, game: PathBuf) -> Result<(), String> {
    let _ = archive::GAME_DIR.set(game);
    let archives = archive::Archives::open()?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut rep = std::fs::File::create(out.join("report.tsv")).map_err(|e| e.to_string())?;
    let _ = writeln!(rep, "binder\tentry\tstatus\tccw_size\tgame_size");
    for f in std::fs::read_dir(ccw.join("chr")).map_err(|e| e.to_string())? {
        let p = f.map_err(|e| e.to_string())?.path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let mine = bnd4::read(&dcx::decompress(&std::fs::read(&p).map_err(|e| e.to_string())?)?)?;
        let theirs = match archives.read(&format!("/chr/{name}")).and_then(|d| dcx::decompress(&d)).and_then(|d| bnd4::read(&d)) {
            Ok(t) => t,
            Err(e) => {
                let _ = writeln!(rep, "{name}\t-\tgame binder unreadable: {e}\t\t");
                Vec::new()
            }
        };
        let dump = out.join(name.trim_end_matches(".dcx"));
        let (mut new, mut changed, mut same) = (0, 0, 0);
        for e in &mine {
            let short = e.name.rsplit('\\').next().unwrap_or(&e.name).to_string();
            // the same path first (a behaviour binder holds three c0000.hkx), else the same file name
            let g = theirs.iter().find(|t| t.name.eq_ignore_ascii_case(&e.name)).or_else(|| theirs.iter().find(|t| t.name.rsplit('\\').next() == Some(short.as_str())));
            let short = if mine.iter().filter(|m| m.name.rsplit('\\').next() == Some(short.as_str())).count() > 1 { e.name.clone() } else { short };
            let status = match g {
                None => { new += 1; "new" }
                Some(g) if sum(&g.data) != sum(&e.data) => { changed += 1; "changed" }
                Some(_) => { same += 1; "same" }
            };
            let _ = writeln!(rep, "{name}\t{short}\t{status}\t{}\t{}", e.data.len(), g.map(|g| g.data.len()).unwrap_or(0));
            if status != "same" {
                let _ = std::fs::create_dir_all(&dump);
                let _ = std::fs::write(dump.join(&short), &e.data);
                if let Some(g) = g {
                    let _ = std::fs::create_dir_all(dump.join("game"));
                    let _ = std::fs::write(dump.join("game").join(&short), &g.data);
                }
            }
        }
        let missing = theirs.iter().filter(|t| !mine.iter().any(|m| m.name.rsplit('\\').next() == t.name.rsplit('\\').next())).count();
        println!("{name}: {} entries: {new} new, {changed} changed, {same} same; {missing} of the game's not in it", mine.len());
    }
    Ok(())
}

/// One file from the game's archives, decompressed, and a binder's entries beside it.
/// A local .dcx file decompressed (and, for a binder, its files split out): ccw_survey dcx <in> <out> <game folder>.
pub fn dcx_file(input: &Path, out: &Path, game: PathBuf) -> Result<(), String> {
    let _ = archive::GAME_DIR.set(game);
    let raw = dcx::decompress(&std::fs::read(input).map_err(|e| e.to_string())?)?;
    std::fs::write(out, &raw).map_err(|e| e.to_string())?;
    if let Ok(files) = bnd4::read(&raw) {
        for (i, f) in files.iter().enumerate() {
            let _ = std::fs::write(out.with_extension(format!("{i}")), &f.data);
            println!("{i}: {} ({} bytes)", f.name, f.data.len());
        }
    }
    Ok(())
}

pub fn ccw_dump(path: &str, out: &Path, game: PathBuf) -> Result<(), String> {
    let _ = archive::GAME_DIR.set(game);
    let archives = archive::Archives::open()?;
    let raw = dcx::decompress(&archives.read(path)?)?;
    std::fs::write(out, &raw).map_err(|e| e.to_string())?;
    if let Ok(files) = bnd4::read(&raw) {
        for (i, f) in files.iter().enumerate() {
            let _ = std::fs::write(out.with_extension(format!("{i}")), &f.data);
            println!("{i}: {} ({} bytes)", f.name, f.data.len());
        }
    }
    Ok(())
}

/// 1.3.0: every path in `list` (one a line) that exists in the game's archives, decompressed into `out_dir`
/// (named after the path's file name without .dcx); the found paths printed.
pub fn ccw_dump_many(list: &Path, out_dir: &Path, game: PathBuf) -> Result<(), String> {
    let _ = archive::GAME_DIR.set(game);
    let archives = archive::Archives::open()?;
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let names = std::fs::read_to_string(list).map_err(|e| e.to_string())?;
    let mut found = 0;
    for path in names.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Ok(bytes) = archives.read(path) else { continue };
        let Ok(raw) = dcx::decompress(&bytes) else { continue };
        let file = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".dcx");
        std::fs::write(out_dir.join(file), &raw).map_err(|e| e.to_string())?;
        println!("{path}");
        found += 1;
    }
    eprintln!("{found} found");
    Ok(())
}

/// Elden Ring's regulation.bin key (AES-256-CBC; the first 16 bytes are the IV).
const REG_KEY: [u8; 32] = [
    0x99, 0xBF, 0xFC, 0x36, 0x6A, 0x6B, 0xC8, 0xC6, 0xF5, 0x82, 0x7D, 0x09, 0x36, 0x02, 0xD6, 0x76,
    0xC4, 0x28, 0x92, 0xA0, 0x1C, 0x20, 0x7F, 0xB0, 0x24, 0xD3, 0xAF, 0x4E, 0x49, 0x3F, 0xEF, 0x99,
];

/// regulation.bin, decrypted (still DCX-compressed).
pub fn reg_decrypt(enc: &[u8]) -> Vec<u8> {
    use aes::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};
    let c = aes::Aes256::new(GenericArray::from_slice(&REG_KEY));
    let mut prev: [u8; 16] = enc[..16].try_into().unwrap();
    let mut out = Vec::with_capacity(enc.len());
    for ch in enc[16..].chunks_exact(16) {
        let mut b = GenericArray::clone_from_slice(ch);
        c.decrypt_block(&mut b);
        for i in 0..16 {
            out.push(b[i] ^ prev[i]);
        }
        prev = ch.try_into().unwrap();
    }
    out
}

/// regulation.bin, encrypted again (a fresh IV).
pub fn reg_encrypt(plain: &[u8]) -> Vec<u8> {
    use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
    let c = aes::Aes256::new(GenericArray::from_slice(&REG_KEY));
    let mut data = plain.to_vec();
    while data.len() % 16 != 0 {
        data.push(0);
    }
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(7);
    let iv: [u8; 16] = std::array::from_fn(|i| (seed >> ((i % 8) * 8)) as u8 ^ (i as u8).wrapping_mul(37));
    let mut out = iv.to_vec();
    let mut prev = iv;
    for ch in data.chunks_exact(16) {
        let mut b = GenericArray::clone_from_slice(ch);
        for i in 0..16 {
            b[i] ^= prev[i];
        }
        c.encrypt_block(&mut b);
        prev = b.as_slice().try_into().unwrap();
        out.extend_from_slice(&prev);
    }
    out
}

/// Lab: a regulation.bin decrypted (and decompressed when it's Kraken) to `out`.
pub fn ccw_reg(path: &Path, out: &Path) -> Result<(), String> {
    let enc = std::fs::read(path).map_err(|e| e.to_string())?;
    let dec = reg_decrypt(&enc);
    let be = |o: usize| u32::from_be_bytes(dec[o..o + 4].try_into().unwrap()) as usize;
    let raw = match dec.get(0x28..0x2C) {
        Some(b"KRAK") => dcx::decompress(&dec)?,
        Some(b"ZSTD") => crate::ermario::zstd::decompress(&dec[0x4C..0x4C + be(0x20)], be(0x1C))?,
        _ => dec,
    };
    std::fs::write(out, raw).map_err(|e| e.to_string())
}
