//! 0.15.2: Elden Force's own title screen.
//!
//! Elden Ring's title screen is one picture: the ELDEN RING logo and sigil on black, a 2532 x 1532 panel
//! in the menu atlas SB_Title_01 (menu/<hi|low>/01_common.tpf.dcx, at x 0, y 60 of the 4096 x 2048
//! texture), drawn at the top centre of a black screen with "PRESS ANY BUTTON" and the menu below it.
//! At first launch (with the outfits, recolor.rs) the panel is replaced by Elden Force's key art
//! (jumpring/title.png, edges fading to black) in a copy of the player's own atlas, written to the
//! ModEngine2 mod folder. Every other sprite in the atlas is left as it was (only the panel's blocks are
//! re-encoded, in every mip level).

use std::path::PathBuf;

use crate::ermario::{archive::Archives, dcx, tpf};
use crate::log;

const VERSION: u32 = 1;
const TEXTURE: &str = "SB_Title_01";
/// The panel in the full-size (4096 wide) atlas.
const PANEL: (usize, usize, usize, usize) = (0, 60, 2532, 1532);
/// The title movie (Scaleform) that shows the panel: it places it at half scale at (330, 16) of its
/// 1920 x 1080 stage, a box in the upper middle of the screen. The same placement (PlaceObject3, the
/// image at depth 2) with the panel scaled to the stage's width and centred fills the whole screen
/// (the panel's top and bottom 54 rows fall outside). Same matrix size: patched in place.
const LOGO_GFX: &str = "/menu/05_001_title_logo.gfx";
const PLACE_HALF: [u8; 16] = [0x06, 0x10, 0x02, 0x00, 0x04, 0x00, 0xc5, 0x00, 0x00, 0x80, 0x00, 0x39, 0x9c, 0x30, 0x50, 0x40];
const MATRIX_FULL: [u8; 10] = [0xc5, 0x84, 0x40, 0xc2, 0x20, 0x38, 0x00, 0x0f, 0x33, 0xc0];
const QUALITIES: [&str; 2] = ["hi", "low"];

fn art_path() -> Option<PathBuf> {
    Some(crate::recolor::mod_dir()?.parent()?.join("jumpring").join("title.png"))
}

fn out_path(q: &str) -> Option<PathBuf> {
    Some(crate::recolor::mod_dir()?.join("menu").join(q).join("01_common.tpf.dcx"))
}

fn gfx_path() -> Option<PathBuf> {
    Some(crate::recolor::mod_dir()?.join("menu").join("05_001_title_logo.gfx"))
}

/// The title movie with the panel filling the screen.
fn make_gfx(archives: &Archives) -> Result<Vec<u8>, String> {
    let mut g = archives.read(LOGO_GFX)?;
    if !g.starts_with(b"GFX") {
        return Err("the title movie is compressed or not Scaleform".into());
    }
    let at = g.windows(PLACE_HALF.len()).position(|w| w == PLACE_HALF).ok_or("the title panel's placement isn't where it was")?;
    g[at + 6..at + 16].copy_from_slice(&MATRIX_FULL);
    Ok(g)
}

fn stamp() -> String {
    let art = art_path().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0);
    format!("jumpring-title-v{VERSION}-letters\n{}\n{art}\n", crate::recolor::game_build())
}

fn stamp_path() -> Option<PathBuf> {
    Some(crate::recolor::mod_dir()?.join("menu").join("jumpring-title.txt"))
}

/// The title files exist and were made from this art and this game version.
pub fn up_to_date() -> bool {
    if art_path().is_none_or(|p| !p.exists()) {
        return true;
    }
    stamp_path().and_then(|p| std::fs::read_to_string(p).ok()).as_deref() == Some(stamp().as_str())
        && QUALITIES.iter().all(|q| out_path(q).is_some_and(|p| p.exists()))
        && gfx_path().is_some_and(|p| p.exists())
}

fn decode_png(data: &[u8]) -> Option<(Vec<u8>, usize, usize)> {
    let mut dec = png::Decoder::new(data);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let px = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        _ => return None,
    };
    Some((rgba, info.width as usize, info.height as usize))
}

/// `src` (sw x sh RGBA) resized to w x h (area average when shrinking, nearest when growing).
fn resize(src: &[u8], sw: usize, sh: usize, w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let (y0, y1) = (y * sh / h, ((y + 1) * sh / h).max(y * sh / h + 1).min(sh));
        for x in 0..w {
            let (x0, x1) = (x * sw / w, ((x + 1) * sw / w).max(x * sw / w + 1).min(sw));
            let mut acc = [0u32; 4];
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let i = (yy * sw + xx) * 4;
                    for c in 0..4 {
                        acc[c] += src[i + c] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            for c in 0..4 {
                out[(y * w + x) * 4 + c] = (acc[c] / n) as u8;
            }
        }
    }
    out
}

/// Put the art into one quality's atlas. Returns the new tpf.dcx.
fn make_one(archives: &Archives, q: &str, art: &[u8], aw: usize, ah: usize) -> Result<Vec<u8>, String> {
    let src = archives.read(&format!("/menu/{q}/01_common.tpf.dcx"))?;
    let mut t = dcx::decompress(&src)?;
    let tex = tpf::tpf_textures(&t).into_iter().find(|x| x.name == TEXTURE).ok_or(format!("{TEXTURE} not in the {q} atlas"))?;
    let dds = &t[tex.off..tex.off + tex.size];
    let head = tpf::dds_header(dds);
    let (h0, w0) = (u32::from_le_bytes(dds[12..16].try_into().unwrap()) as usize, u32::from_le_bytes(dds[16..20].try_into().unwrap()) as usize);
    if head != 148 || u32::from_le_bytes(dds[128..132].try_into().unwrap()) != 98 {
        return Err(format!("{TEXTURE} ({q}) is not BC7"));
    }
    let mips = tex.mips.max(1);
    let s = w0 as f32 / 4096.0;
    let mut data = dds.to_vec();
    let (mut w, mut h, mut at) = (w0, h0, head);
    for m in 0..mips {
        let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
        let len = bw * bh * 16;
        if at + len > data.len() {
            break;
        }
        let k = s / (1u32 << m) as f32;
        let (px, py, pw, ph) = ((PANEL.0 as f32 * k) as usize, (PANEL.1 as f32 * k).round() as usize, (PANEL.2 as f32 * k) as usize, (PANEL.3 as f32 * k) as usize);
        if pw < 4 || ph < 4 {
            break;
        }
        // the block rows and columns the panel touches
        let (bx0, by0, bx1, by1) = (px / 4, py / 4, (px + pw).div_ceil(4).min(bw), (py + ph).div_ceil(4).min(bh));
        let mut decoded = vec![0u32; bw * 4 * bh * 4];
        texture2ddecoder::decode_bc7(&data[at..at + len], bw * 4, bh * 4, &mut decoded).map_err(|e| format!("{TEXTURE} mip {m}: {e}"))?;
        let scaled = resize(art, aw, ah, pw, ph);
        let (rw, rh) = ((bx1 - bx0) * 4, (by1 - by0) * 4);
        let mut region = vec![0u8; rw * rh * 4];
        for y in 0..rh {
            for x in 0..rw {
                let (gx, gy) = (bx0 * 4 + x, by0 * 4 + y);
                let o = (y * rw + x) * 4;
                if gx >= px && gx < px + pw && gy >= py && gy < py + ph {
                    let i = ((gy - py) * pw + (gx - px)) * 4;
                    region[o..o + 3].copy_from_slice(&scaled[i..i + 3]);
                    region[o + 3] = 255;
                } else {
                    let b = decoded[gy * bw * 4 + gx].to_le_bytes();
                    region[o..o + 4].copy_from_slice(&[b[2], b[1], b[0], b[3]]);
                }
            }
        }
        let enc = tpf::bc7_mode6(&region, rw, rh);
        let row = (bx1 - bx0) * 16;
        for r in 0..(by1 - by0) {
            let dst = at + ((by0 + r) * bw + bx0) * 16;
            data[dst..dst + row].copy_from_slice(&enc[r * row..(r + 1) * row]);
        }
        at += len;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    t[tex.off..tex.off + tex.size].copy_from_slice(&data);
    dcx::compress(&t)
}

/// Make the title files (with the outfits, before the game opens its archives).
pub fn make(archives: &Archives) {
    let Some(art_file) = art_path() else { return };
    let Ok(png) = std::fs::read(&art_file) else {
        log!("title: no title art ({}); Elden Ring's own title stays", art_file.display());
        return;
    };
    let Some((art, aw, ah)) = decode_png(&png) else {
        log!("title: the title art can't be read");
        return;
    };
    let t0 = std::time::Instant::now();
    let (pw, ph) = (PANEL.2, PANEL.3);
    let mut art = if (aw, ah) == (pw, ph) { art } else { resize(&art, aw, ah, pw, ph) };
    // ELDEN FORCE in Elden Ring's own lettering, from the player's own title panel
    match vanilla_panel(archives) {
        Ok(er) => letter(&mut art, &er),
        Err(e) => log!("title: no lettering: {e}"),
    }
    let (aw, ah) = (pw, ph);
    for q in QUALITIES {
        let Some(out) = out_path(q) else { return };
        match make_one(archives, q, &art, aw, ah) {
            Ok(d) => {
                let _ = std::fs::create_dir_all(out.parent().unwrap());
                if let Err(e) = std::fs::write(&out, d) {
                    log!("title: can't write {}: {e}", out.display());
                    return;
                }
            }
            Err(e) => {
                log!("title: {q}: {e}");
                return;
            }
        }
    }
    match (make_gfx(archives), gfx_path()) {
        (Ok(d), Some(p)) => {
            let _ = std::fs::write(p, d);
        }
        // the panel still shows, in its box
        (Err(e), _) => log!("title: full screen: {e}"),
        _ => {}
    }
    if let Some(p) = stamp_path() {
        let _ = std::fs::write(p, stamp());
    }
    log!("title: Elden Force's title screen made in {:.1}s", t0.elapsed().as_secs_f32());
}

// ---- ELDEN FORCE from Elden Ring's own logo -------------------------------------------------------
// The vanilla panel holds ELDEN RING in its cracked gold lettering. ELDEN comes as it is; F is the big E
// without its bottom arm, R is RING's R, O is G's left bowl beside its mirror, C is G's upper half above
// its mirror, and the last E is ELDEN's second one; O and C are brought down to the small capitals'
// height. Coordinates are the panel's (Elden Ring 2.7.1). Letters are taken by brightness, so the dim
// sigil behind them stays behind.

/// An RGBA image.
struct Img {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Img {
    fn blank(w: usize, h: usize) -> Img {
        Img { w, h, px: vec![0; w * h * 4] }
    }
    fn crop(src: &Img, x0: usize, x1: usize, y0: usize, y1: usize) -> Img {
        let (w, h) = (x1 - x0, y1 - y0);
        let mut px = Vec::with_capacity(w * h * 4);
        for y in y0..y1 {
            px.extend_from_slice(&src.px[(y * src.w + x0) * 4..(y * src.w + x1) * 4]);
        }
        Img { w, h, px }
    }
    fn flip_h(&self) -> Img {
        let mut o = Img::blank(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                let (a, b) = ((y * self.w + x) * 4, (y * self.w + self.w - 1 - x) * 4);
                o.px[a..a + 4].copy_from_slice(&self.px[b..b + 4]);
            }
        }
        o
    }
    fn flip_v(&self) -> Img {
        let mut o = Img::blank(self.w, self.h);
        for y in 0..self.h {
            let (a, b) = (y * self.w * 4, (self.h - 1 - y) * self.w * 4);
            o.px[a..a + self.w * 4].copy_from_slice(&self.px[b..b + self.w * 4]);
        }
        o
    }
    fn hcat(parts: &[&Img]) -> Img {
        let h = parts.iter().map(|p| p.h).max().unwrap_or(0);
        let w = parts.iter().map(|p| p.w).sum();
        let mut o = Img::blank(w, h);
        let mut x0 = 0;
        for p in parts {
            for y in 0..p.h {
                let d = (y * w + x0) * 4;
                o.px[d..d + p.w * 4].copy_from_slice(&p.px[y * p.w * 4..(y + 1) * p.w * 4]);
            }
            x0 += p.w;
        }
        o
    }
    fn vcat(a: &Img, b: &Img) -> Img {
        let mut px = a.px.clone();
        px.extend_from_slice(&b.px);
        Img { w: a.w, h: a.h + b.h, px }
    }
    fn resized(&self, w: usize, h: usize) -> Img {
        Img { w, h, px: resize(&self.px, self.w, self.h, w, h) }
    }
    fn clear(&mut self, x0: usize, x1: usize, y0: usize, y1: usize) {
        for y in y0..y1.min(self.h) {
            for x in x0..x1.min(self.w) {
                let i = (y * self.w + x) * 4;
                self.px[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
}

/// The vanilla title panel (RGBA, PANEL size) from the player's own high-quality atlas.
fn vanilla_panel(archives: &Archives) -> Result<Img, String> {
    let src = archives.read("/menu/hi/01_common.tpf.dcx")?;
    let t = dcx::decompress(&src)?;
    let tex = tpf::tpf_textures(&t).into_iter().find(|x| x.name == TEXTURE).ok_or("no title atlas")?;
    let dds = &t[tex.off..tex.off + tex.size];
    let head = tpf::dds_header(dds);
    let (h, w) = (u32::from_le_bytes(dds[12..16].try_into().unwrap()) as usize, u32::from_le_bytes(dds[16..20].try_into().unwrap()) as usize);
    if w != 4096 || h != 2048 {
        return Err(format!("title atlas is {w} x {h}"));
    }
    let len = (w / 4) * (h / 4) * 16;
    let mut px = vec![0u32; w * h];
    texture2ddecoder::decode_bc7(&dds[head..head + len], w, h, &mut px).map_err(|e| e.to_string())?;
    let full = Img { w, h, px: px.iter().flat_map(|p| { let b = p.to_le_bytes(); [b[2], b[1], b[0], 255] }).collect() };
    Ok(Img::crop(&full, PANEL.0, PANEL.0 + PANEL.2, PANEL.1, PANEL.1 + PANEL.3))
}

/// Letter ELDEN FORCE onto `art` (PANEL size) from the vanilla panel `er`.
fn letter(art: &mut [u8], er: &Img) {
    const Y0: usize = 505;
    const Y1: usize = 885;
    // small capitals' top and bottom (L, R, N)
    const S0: usize = 578;
    const S1: usize = 827;
    let mut f = Img::crop(er, 39, 330, Y0, Y1);
    f.clear(200 - 39, f.w, 835 - Y0, f.h);
    let elden = Img::crop(er, 39, 1341, Y0, Y1);
    let r = Img::crop(er, 1446, 1686, Y0, Y1);
    let e = Img::crop(er, 824, 1022, Y0, Y1);
    let g = Img::crop(er, 2152, 2476, 514, 868);
    let left = Img::crop(&g, 0, g.w / 2, 0, g.h);
    let o = Img::hcat(&[&left, &left.flip_h()]);
    let top = Img::crop(&g, 0, g.w, 0, g.h / 2);
    let c = Img::vcat(&top, &top.flip_v());
    let small = |img: &Img| {
        let h = S1 - S0;
        let w = (img.w as f32 * h as f32 / img.h as f32).round() as usize;
        let s = img.resized(w, h);
        let mut out = Img::blank(w, Y1 - Y0);
        for y in 0..h {
            let d = ((S0 - Y0 + y) * w) * 4;
            out.px[d..d + w * 4].copy_from_slice(&s.px[y * w * 4..(y + 1) * w * 4]);
        }
        out
    };
    let (o, c) = (small(&o), small(&c));
    let gap = Img::blank(18, Y1 - Y0);
    let space = Img::blank(110, Y1 - Y0);
    let word = Img::hcat(&[&elden, &space, &f, &gap, &o, &gap, &r, &gap, &c, &gap, &e]);
    // scaled to about two thirds of the screen's width, near the top (the game shows the panel from row 54)
    let tw = 1750usize;
    let th = (word.h as f32 * tw as f32 / word.w as f32).round() as usize;
    let word = word.resized(tw, th);
    let (x0, y0) = ((PANEL.2 - tw) / 2, 140usize);
    let alpha: Vec<f32> = word.px.chunks(4).map(|p| ((p[0].max(p[1]).max(p[2]) as f32 - 85.0) / 75.0).clamp(0.0, 1.0)).collect();
    // a soft dark halo under the letters (three box blurs of the letters' shape)
    let mut halo = alpha.clone();
    for _ in 0..3 {
        halo = box_blur(&halo, tw, th, 9);
    }
    for y in 0..th {
        for x in 0..tw {
            let (ax, ay) = (x0 + x, y0 + y);
            if ax >= PANEL.2 || ay >= PANEL.3 {
                continue;
            }
            let i = (ay * PANEL.2 + ax) * 4;
            let a = alpha[y * tw + x];
            let dark = 1.0 - 0.75 * (halo[y * tw + x] * 1.5).min(1.0);
            for ch in 0..3 {
                let under = art[i + ch] as f32 * dark;
                art[i + ch] = (under * (1.0 - a) + word.px[(y * tw + x) * 4 + ch] as f32 * a).clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// A box blur of radius `r` (horizontal then vertical).
fn box_blur(v: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut t = vec![0f32; w * h];
    let n = (2 * r + 1) as f32;
    for y in 0..h {
        let row = &v[y * w..(y + 1) * w];
        let mut acc: f32 = (0..=r.min(w - 1)).map(|i| row[i]).sum();
        for x in 0..w {
            t[y * w + x] = acc / n;
            if x + r + 1 < w {
                acc += row[x + r + 1];
            }
            if x >= r {
                acc -= row[x - r];
            }
        }
    }
    let mut o = vec![0f32; w * h];
    for x in 0..w {
        let mut acc: f32 = (0..=r.min(h - 1)).map(|i| t[i * w + x]).sum();
        for y in 0..h {
            o[y * w + x] = acc / n;
            if y + r + 1 < h {
                acc += t[(y + r + 1) * w + x];
            }
            if y >= r {
                acc -= t[(y - r) * w + x];
            }
        }
    }
    o
}
