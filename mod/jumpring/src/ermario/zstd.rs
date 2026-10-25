//! A Zstandard decoder (RFC 8878), for the game's regulation.bin (DCX "ZSTD" since Shadow of the Erdtree):
//! Elden Force 1.2.0 adds Carian Combo Warriors' param rows to the player's own regulation at prepare
//! time (src/regulation.rs). Decoding only; follows the RFC's reference ("educational") decoder.

fn err<T>(m: &str) -> Result<T, String> {
    Err(format!("zstd: {m}"))
}

fn highest_bit(v: u32) -> i32 {
    31 - v.leading_zeros() as i32
}

/// Bits read forwards (little-endian bit order), for FSE table headers.
struct Fwd<'a> {
    d: &'a [u8],
    bit: usize,
}

impl Fwd<'_> {
    fn read(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        for i in 0..n {
            let b = self.bit + i as usize;
            let byte = self.d.get(b / 8).copied().unwrap_or(0);
            v |= (((byte >> (b % 8)) & 1) as u32) << i;
        }
        self.bit += n as usize;
        v
    }
    fn bytes_used(&self) -> usize {
        self.bit.div_ceil(8)
    }
}

/// Bits read backwards from the end (the stream's last byte holds a 1 padding marker), most significant
/// first; reading past the start yields zeros and leaves `left` negative.
struct Back<'a> {
    d: &'a [u8],
    left: i64,
}

impl<'a> Back<'a> {
    fn new(d: &'a [u8]) -> Result<Self, String> {
        let last = *d.last().ok_or("zstd: empty bitstream")?;
        if last == 0 {
            return err("bitstream without its end marker");
        }
        Ok(Back { d, left: d.len() as i64 * 8 - 8 + highest_bit(last as u32) as i64 })
    }
    fn read(&mut self, n: u32) -> u64 {
        if n == 0 {
            return 0;
        }
        self.left -= n as i64;
        let mut v = 0u64;
        for i in 0..n as i64 {
            let b = self.left + i;
            if b >= 0 {
                let byte = self.d[(b / 8) as usize];
                v |= (((byte >> (b % 8)) & 1) as u64) << i;
            }
        }
        v
    }
}

#[derive(Clone, Default)]
struct Fse {
    log: u32,
    sym: Vec<u8>,
    bits: Vec<u8>,
    base: Vec<u16>,
}

impl Fse {
    fn from_counts(counts: &[i32], log: u32) -> Result<Fse, String> {
        let size = 1usize << log;
        let mut sym = vec![0u8; size];
        let mut next = vec![0u32; counts.len()];
        let mut high = size;
        for (s, &c) in counts.iter().enumerate() {
            if c == -1 {
                high -= 1;
                sym[high] = s as u8;
                next[s] = 1;
            } else {
                next[s] = c.max(0) as u32;
            }
        }
        let (step, mask) = ((size >> 1) + (size >> 3) + 3, size - 1);
        let mut pos = 0usize;
        for (s, &c) in counts.iter().enumerate() {
            for _ in 0..c.max(0) {
                sym[pos] = s as u8;
                loop {
                    pos = (pos + step) & mask;
                    if pos < high {
                        break;
                    }
                }
            }
        }
        if pos != 0 {
            return err("FSE table spread");
        }
        let (mut bits, mut base) = (vec![0u8; size], vec![0u16; size]);
        for i in 0..size {
            let s = sym[i] as usize;
            let n = next[s];
            next[s] += 1;
            let nb = log as i32 - highest_bit(n);
            bits[i] = nb as u8;
            base[i] = ((n << nb) as usize - size) as u16;
        }
        Ok(Fse { log, sym, bits, base })
    }

    fn rle(s: u8) -> Fse {
        Fse { log: 0, sym: vec![s], bits: vec![0], base: vec![0] }
    }

    /// A table description (normalized counts): the table and the bytes it took.
    fn read(d: &[u8], max_log: u32, max_sym: usize) -> Result<(Fse, usize), String> {
        let mut f = Fwd { d, bit: 0 };
        let log = f.read(4) + 5;
        if log > max_log {
            return err("FSE accuracy too high");
        }
        let mut remaining: i32 = 1 << log;
        let mut counts = Vec::new();
        while remaining > 0 && counts.len() < max_sym {
            let bits = highest_bit((remaining + 1) as u32) as u32 + 1;
            let mut val = f.read(bits) as i32;
            let lower = (1 << (bits - 1)) - 1;
            let threshold = (1 << bits) - 1 - (remaining + 1);
            if (val & lower) < threshold {
                f.bit -= 1;
                val &= lower;
            } else if val > lower {
                val -= threshold;
            }
            let p = val - 1;
            remaining -= p.abs();
            counts.push(p);
            if p == 0 {
                let mut rep = f.read(2);
                loop {
                    for _ in 0..rep {
                        if counts.len() < max_sym {
                            counts.push(0);
                        }
                    }
                    if rep == 3 {
                        rep = f.read(2);
                    } else {
                        break;
                    }
                }
            }
        }
        if remaining != 0 {
            return err("FSE counts");
        }
        Ok((Fse::from_counts(&counts, log)?, f.bytes_used()))
    }
}

struct State<'t> {
    t: &'t Fse,
    s: usize,
}

impl<'t> State<'t> {
    fn init(t: &'t Fse, b: &mut Back) -> Self {
        State { t, s: b.read(t.log) as usize }
    }
    fn sym(&self) -> u8 {
        self.t.sym[self.s]
    }
    fn update(&mut self, b: &mut Back) {
        self.s = self.t.base[self.s] as usize + b.read(self.t.bits[self.s] as u32) as usize;
    }
}

#[derive(Clone, Default)]
struct Huf {
    max: u32,
    sym: Vec<u8>,
    bits: Vec<u8>,
}

impl Huf {
    fn from_weights(mut w: Vec<u8>) -> Result<Huf, String> {
        let sum: u32 = w.iter().filter(|&&x| x > 0).map(|&x| 1u32 << (x - 1)).sum();
        if sum == 0 {
            return err("Huffman weights");
        }
        let max = highest_bit(sum) as u32 + 1;
        let left = (1u32 << max) - sum;
        if !left.is_power_of_two() {
            return err("Huffman last weight");
        }
        w.push(highest_bit(left) as u8 + 1);
        let size = 1usize << max;
        let (mut sym, mut bits) = (vec![0u8; size], vec![0u8; size]);
        let mut pos = 0usize;
        for weight in 1..=max as u8 {
            for (s, &x) in w.iter().enumerate() {
                if x == weight {
                    let n = 1usize << (weight - 1);
                    for e in &mut sym[pos..pos + n] {
                        *e = s as u8;
                    }
                    for e in &mut bits[pos..pos + n] {
                        *e = max as u8 + 1 - weight;
                    }
                    pos += n;
                }
            }
        }
        if pos != size {
            return err("Huffman table");
        }
        Ok(Huf { max, sym, bits })
    }

    /// A tree description: the table and its size in bytes.
    fn read(d: &[u8]) -> Result<(Huf, usize), String> {
        let h = *d.first().ok_or("zstd: no Huffman header")? as usize;
        if h >= 128 {
            let n = h - 127;
            let bytes = n.div_ceil(2);
            let src = d.get(1..1 + bytes).ok_or("zstd: Huffman weights truncated")?;
            let w = (0..n).map(|i| if i % 2 == 0 { src[i / 2] >> 4 } else { src[i / 2] & 15 }).collect();
            return Ok((Huf::from_weights(w)?, 1 + bytes));
        }
        let src = d.get(1..1 + h).ok_or("zstd: Huffman weights truncated")?;
        let (t, used) = Fse::read(src, 6, 256)?;
        let mut b = Back::new(&src[used..])?;
        let (mut s1, mut s2) = (State::init(&t, &mut b), State::init(&t, &mut b));
        let mut w = Vec::new();
        loop {
            w.push(s1.sym());
            s1.update(&mut b);
            if b.left < 0 {
                w.push(s2.sym());
                break;
            }
            w.push(s2.sym());
            s2.update(&mut b);
            if b.left < 0 {
                w.push(s1.sym());
                break;
            }
            if w.len() > 255 {
                return err("too many Huffman weights");
            }
        }
        Ok((Huf::from_weights(w)?, 1 + h))
    }

    fn stream(&self, d: &[u8], n: usize, out: &mut Vec<u8>) -> Result<(), String> {
        let mut b = Back::new(d)?;
        let mask = (1usize << self.max) - 1;
        let mut s = b.read(self.max) as usize;
        for _ in 0..n {
            out.push(self.sym[s]);
            let nb = self.bits[s] as u32;
            s = ((s << nb) + b.read(nb) as usize) & mask;
        }
        Ok(())
    }
}

const LL_BASE: [u32; 36] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 24, 28, 32, 40, 48, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536];
const LL_BITS: [u32; 36] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
const ML_BASE: [u32; 53] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 37, 39, 41, 43, 47, 51, 59, 67, 83, 99, 131, 259, 515, 1027, 2051, 4099, 8195, 16387, 32771, 65539,
];
const ML_BITS: [u32; 53] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 4, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
];
const LL_DEFAULT: [i32; 36] = [4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 1, 1, 1, 1, 1, -1, -1, -1, -1];
const ML_DEFAULT: [i32; 53] = [
    1, 4, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1, -1, -1,
];
const OF_DEFAULT: [i32; 29] = [1, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1];

#[derive(Default)]
struct Frame {
    huf: Option<Huf>,
    ll: Option<Fse>,
    of: Option<Fse>,
    ml: Option<Fse>,
    rep: [usize; 3],
}

impl Frame {
    fn literals(&mut self, d: &[u8]) -> Result<(Vec<u8>, usize), String> {
        let b0 = d[0] as usize;
        let kind = b0 & 3;
        let fmt = (b0 >> 2) & 3;
        if kind < 2 {
            let (size, hdr) = match fmt {
                0 | 2 => (b0 >> 3, 1),
                1 => ((b0 >> 4) + ((d[1] as usize) << 4), 2),
                _ => ((b0 >> 4) + ((d[1] as usize) << 4) + ((d[2] as usize) << 12), 3),
            };
            return if kind == 0 {
                Ok((d.get(hdr..hdr + size).ok_or("zstd: raw literals truncated")?.to_vec(), hdr + size))
            } else {
                Ok((vec![d[hdr]; size], hdr + 1))
            };
        }
        let (hdr, bits) = match fmt {
            0 | 1 => (3, 10),
            2 => (4, 14),
            _ => (5, 18),
        };
        let mut h = 0u64;
        for i in 0..hdr {
            h |= (d[i] as u64) << (8 * i);
        }
        let mask = (1u64 << bits) - 1;
        let (regen, comp) = (((h >> 4) & mask) as usize, ((h >> (4 + bits)) & mask) as usize);
        let mut src = d.get(hdr..hdr + comp).ok_or("zstd: compressed literals truncated")?;
        if kind == 2 {
            let (t, used) = Huf::read(src)?;
            self.huf = Some(t);
            src = &src[used..];
        }
        let huf = self.huf.as_ref().ok_or("zstd: treeless literals without a tree")?;
        let mut out = Vec::with_capacity(regen);
        if fmt == 0 {
            huf.stream(src, regen, &mut out)?;
        } else {
            let s1 = u16::from_le_bytes([src[0], src[1]]) as usize;
            let s2 = u16::from_le_bytes([src[2], src[3]]) as usize;
            let s3 = u16::from_le_bytes([src[4], src[5]]) as usize;
            let each = regen.div_ceil(4);
            let mut at = 6;
            for (k, len) in [s1, s2, s3, src.len() - 6 - s1 - s2 - s3].into_iter().enumerate() {
                let n = if k < 3 { each } else { regen - 3 * each };
                huf.stream(src.get(at..at + len).ok_or("zstd: literal stream truncated")?, n, &mut out)?;
                at += len;
            }
        }
        Ok((out, hdr + comp))
    }

    fn table(slot: &mut Option<Fse>, mode: u8, d: &[u8], default: &[i32], default_log: u32, max_log: u32, max_sym: usize) -> Result<usize, String> {
        match mode {
            0 => {
                *slot = Some(Fse::from_counts(default, default_log)?);
                Ok(0)
            }
            1 => {
                *slot = Some(Fse::rle(d[0]));
                Ok(1)
            }
            2 => {
                let (t, used) = Fse::read(d, max_log, max_sym)?;
                *slot = Some(t);
                Ok(used)
            }
            _ => {
                if slot.is_none() {
                    return err("repeat table without one");
                }
                Ok(0)
            }
        }
    }

    fn block(&mut self, d: &[u8], out: &mut Vec<u8>) -> Result<(), String> {
        let (lits, used) = self.literals(d)?;
        let mut p = used;
        let b0 = *d.get(p).ok_or("zstd: no sequences header")? as usize;
        let nseq = if b0 < 128 {
            p += 1;
            b0
        } else if b0 < 255 {
            p += 2;
            ((b0 - 128) << 8) + d[p - 1] as usize
        } else {
            p += 3;
            d[p - 2] as usize + ((d[p - 1] as usize) << 8) + 0x7F00
        };
        if nseq == 0 {
            out.extend_from_slice(&lits);
            return Ok(());
        }
        let modes = d[p];
        p += 1;
        p += Self::table(&mut self.ll, modes >> 6, &d[p..], &LL_DEFAULT, 6, 9, 36)?;
        p += Self::table(&mut self.of, (modes >> 4) & 3, &d[p..], &OF_DEFAULT, 5, 8, 32)?;
        p += Self::table(&mut self.ml, (modes >> 2) & 3, &d[p..], &ML_DEFAULT, 6, 9, 53)?;
        let (ll, of, ml) = (self.ll.as_ref().unwrap(), self.of.as_ref().unwrap(), self.ml.as_ref().unwrap());
        let mut b = Back::new(&d[p..])?;
        let mut sl = State::init(ll, &mut b);
        let mut so = State::init(of, &mut b);
        let mut sm = State::init(ml, &mut b);
        let mut li = 0usize;
        for k in 0..nseq {
            let (oc, mc, lc) = (so.sym() as u32, sm.sym() as usize, sl.sym() as usize);
            if mc >= 53 || lc >= 36 || oc > 31 {
                return err("sequence code");
            }
            let ov = (1usize << oc) + b.read(oc) as usize;
            let mlen = ML_BASE[mc] as usize + b.read(ML_BITS[mc]) as usize;
            let llen = LL_BASE[lc] as usize + b.read(LL_BITS[lc]) as usize;
            if k + 1 < nseq {
                sl.update(&mut b);
                sm.update(&mut b);
                so.update(&mut b);
            }
            let r = &mut self.rep;
            let off = if ov > 3 {
                let o = ov - 3;
                *r = [o, r[0], r[1]];
                o
            } else {
                let idx = ov - 1 + usize::from(llen == 0);
                match idx {
                    0 => r[0],
                    1 => {
                        let o = r[1];
                        *r = [o, r[0], r[2]];
                        o
                    }
                    2 => {
                        let o = r[2];
                        *r = [o, r[0], r[1]];
                        o
                    }
                    _ => {
                        let o = r[0].wrapping_sub(1);
                        *r = [o, r[0], r[1]];
                        o
                    }
                }
            };
            out.extend_from_slice(lits.get(li..li + llen).ok_or("zstd: literals overrun")?);
            li += llen;
            if off == 0 || off > out.len() {
                return err("match offset");
            }
            let start = out.len() - off;
            for i in 0..mlen {
                let c = out[start + i];
                out.push(c);
            }
        }
        out.extend_from_slice(&lits[li..]);
        Ok(())
    }
}

/// One Zstandard frame (the whole of `d`), decoded.
pub fn decompress(d: &[u8], size_hint: usize) -> Result<Vec<u8>, String> {
    if d.get(..4) != Some(&[0x28, 0xB5, 0x2F, 0xFD]) {
        return err("not a frame");
    }
    let fhd = d[4];
    let single = fhd & 0x20 != 0;
    let mut p = 5 + usize::from(!single);
    p += [0, 1, 2, 4][(fhd & 3) as usize];
    p += match fhd >> 6 {
        0 => usize::from(single),
        1 => 2,
        2 => 4,
        _ => 8,
    };
    let mut out = Vec::with_capacity(size_hint);
    let mut fr = Frame { rep: [1, 4, 8], ..Default::default() };
    loop {
        let h = d.get(p..p + 3).ok_or("zstd: block header truncated")?;
        let h = h[0] as usize | (h[1] as usize) << 8 | (h[2] as usize) << 16;
        p += 3;
        let (last, kind, size) = (h & 1 != 0, (h >> 1) & 3, h >> 3);
        match kind {
            0 => {
                out.extend_from_slice(d.get(p..p + size).ok_or("zstd: raw block truncated")?);
                p += size;
            }
            1 => {
                let b = *d.get(p).ok_or("zstd: RLE block truncated")?;
                out.resize(out.len() + size, b);
                p += 1;
            }
            2 => {
                fr.block(d.get(p..p + size).ok_or("zstd: block truncated")?, &mut out)?;
                p += size;
            }
            _ => return err("reserved block type"),
        }
        if last {
            break;
        }
    }
    Ok(out)
}
