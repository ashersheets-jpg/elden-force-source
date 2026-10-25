// Vendored from ER Mario (https://github.com/deltarooo/er-mario), MIT License, Copyright (c) the ER Mario authors.

//! TPF texture containers, renaming UTF-16 names in binders, and a minimal BC7 encoder.

fn u16s(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Replaces every UTF-16 occurrence of `old` with `new` (same length).
pub fn rename(blob: &mut [u8], old: &str, new: &str) {
    let (o, n) = (u16s(old), u16s(new));
    debug_assert_eq!(o.len(), n.len());
    let mut i = 0;
    while i + o.len() <= blob.len() {
        if blob[i..i + o.len()] == o[..] {
            blob[i..i + o.len()].copy_from_slice(&n);
            i += o.len();
        } else {
            i += 1;
        }
    }
}

fn i32_at(d: &[u8], o: usize) -> usize {
    i32::from_le_bytes(d[o..o + 4].try_into().unwrap()) as usize
}

pub fn u32_at(d: &[u8], o: usize) -> usize {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap()) as usize
}

pub struct TpfTexture {
    pub name: String,
    pub off: usize,
    pub size: usize,
    pub mips: usize,
}

pub fn tpf_textures(t: &[u8]) -> Vec<TpfTexture> {
    (0..i32_at(t, 8))
        .map(|i| {
            let a = 0x10 + i * 0x14;
            TpfTexture {
                off: i32_at(t, a),
                size: i32_at(t, a + 4),
                mips: t[a + 10] as usize,
                name: super::bnd4::wstr(t, i32_at(t, a + 12)).unwrap_or_default(),
            }
        })
        .collect()
}

pub fn dds_header(dds: &[u8]) -> usize {
    if &dds[84..88] == b"DX10" { 148 } else { 128 }
}


// ---- BC7 ---------------------------------------------------------------------------------------

const BC7_WEIGHTS: [f64; 16] = [0., 4., 9., 13., 17., 21., 26., 30., 34., 38., 43., 47., 51., 55., 60., 64.];

/// Minimal BC7 encoder (mode 6 only: RGBA endpoints 7 bits + p-bit, 16 index levels).
pub fn bc7_mode6(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h);
    for by in 0..h / 4 {
        for bx in 0..w / 4 {
            let px: [[f64; 4]; 16] =
                std::array::from_fn(|i| std::array::from_fn(|c| rgba[((by * 4 + i / 4) * w + bx * 4 + i % 4) * 4 + c] as f64));
            let lo: [f64; 4] = std::array::from_fn(|c| px.iter().map(|p| p[c]).fold(f64::MAX, f64::min));
            let hi: [f64; 4] = std::array::from_fn(|c| px.iter().map(|p| p[c]).fold(f64::MIN, f64::max));
            let q = |v: f64| (v / 2.0).round_ties_even().clamp(0.0, 127.0) as u128;
            let (mut e0, mut e1) = (lo.map(q), hi.map(q));
            let (f0, f1) = (e0.map(|v| v as f64 * 2.0), e1.map(|v| v as f64 * 2.0));
            let d: [f64; 4] = std::array::from_fn(|c| f1[c] - f0[c]);
            let dd: f64 = d.iter().map(|v| v * v).sum();
            let mut idx: [u128; 16] = std::array::from_fn(|i| {
                let t = if dd > 0.0 { (0..4).map(|c| (px[i][c] - f0[c]) * d[c]).sum::<f64>() / dd.max(1.0) } else { 0.0 };
                let t = t.clamp(0.0, 1.0);
                let mut best = 0;
                for k in 1..16 {
                    if (t - BC7_WEIGHTS[k] / 64.0).abs() < (t - BC7_WEIGHTS[best] / 64.0).abs() {
                        best = k;
                    }
                }
                best as u128
            });
            // the first pixel's index has only 3 bits: swap the endpoints where it would need the 4th
            if idx[0] >= 8 {
                std::mem::swap(&mut e0, &mut e1);
                idx = idx.map(|i| 15 - i);
            }
            let mut bits: u128 = 1 << 6;
            for c in 0..4 {
                bits |= e0[c] << (7 + 14 * c) | e1[c] << (14 + 14 * c);
            }
            bits |= idx[0] << 65;
            for (k, &i) in idx.iter().enumerate().skip(1) {
                bits |= i << (64 + 4 + 4 * (k - 1));
            }
            out.extend_from_slice(&bits.to_le_bytes());
        }
    }
    out
}
