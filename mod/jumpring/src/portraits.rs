//! Fighter portraits for the fighter select (roster tiles, and the posed whole figure beside the details) and the HUD
//! (faces), read from jumpring\assets\portraits (2.0.9; embedded in the DLL before). 1.3.0: the tiles and figures are posed renders (work/portraits/posed.py, compose_posed.py). They are
//! renders of Elden Force's own fighter models (the six made for the mod, and Bluesands' Goku and Naruto,
//! shared under the author's permissions) made offline by work/portraits; Your Tarnished's card is an
//! emblem drawn for Elden Force (the player's own character has no fixed look).

use std::collections::HashMap;

use hudhook::imgui::TextureId;

use crate::log;

/// (2.0.9) The fighters whose pictures are in jumpring\assets\portraits: <id>.png (the card), <id>_face.png (the HUD),
/// <id>_full.png (the posed figure).
static IDS: [&str; 19] = [
    // 1.3.0: the new fighters' cards are renders of their models (work/portraits, outline shells left out)
    "ichigo", "sasuke", "zoro", "yami", "gojo", "yuji", "gon", "killua", "jotaro", "dio",
    "goku", "naruto", "luffy", "deku", "all_might", "bakugo", "todoroki", "asta", "custom",
];

/// (2.0.9) One of Elden Force's pictures, read from the assets folder beside the DLL (jumpring\assets). 2.0.8 and before
/// carried 8 MB of them inside the DLL: a small program holding a big compressed blob is what antivirus heuristics call
/// a dropper (VirusTotal's "BehavesLike.Win64.Dropper"), and likely why Nexus Mods quarantined the uploads.
pub(crate) fn asset(rel: &str) -> Option<Vec<u8>> {
    let path = crate::recolor::mod_dir()?.parent()?.join("jumpring").join("assets").join(rel);
    match std::fs::read(&path) {
        Ok(b) => Some(b),
        Err(e) => {
            log!("assets: {} not read ({e})", path.display());
            None
        }
    }
}

#[derive(Default)]
pub struct Portraits {
    /// fighter id -> (card, face)
    pub by_id: HashMap<&'static str, (TextureId, TextureId)>,
    /// 1.3.0: fighter id -> the whole figure, posed, transparent (the fighter select's big picture; work/portraits/posed.py)
    pub full: HashMap<&'static str, TextureId>,
}

pub(crate) fn decode(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut dec = png::Decoder::new(data);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    let px = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        _ => return None,
    };
    Some((rgba, w, h))
}

impl Portraits {
    /// Upload every portrait to the GPU (overlay initialize).
    pub fn load(rc: &mut dyn hudhook::RenderContext) -> Self {
        let mut by_id = HashMap::new();
        let mut full = HashMap::new();
        for id in IDS {
            let up = |rc: &mut dyn hudhook::RenderContext, name: String| {
                let (px, w, h) = decode(&asset(&format!("portraits/{name}.png"))?)?;
                rc.load_texture(&px, w, h).ok()
            };
            if let Some(t) = up(rc, format!("{id}_full")) {
                full.insert(id, t);
            }
            match (up(rc, id.to_string()), up(rc, format!("{id}_face"))) {
                (Some(c), Some(f)) => {
                    by_id.insert(id, (c, f));
                }
                _ => log!("portraits: {id} could not be loaded"),
            }
        }
        log!("portraits: {} loaded", by_id.len());
        Self { by_id, full }
    }
}
