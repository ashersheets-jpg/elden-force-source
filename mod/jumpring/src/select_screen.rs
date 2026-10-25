//! The fighter select in the manner of Elden Ring's own menus (0.8.0): a dark, vignetted backdrop,
//! carved gold capitals between thin gilded rules, every fighter's portrait card above their name, and
//! the highlighted fighter's details: build, passive, and each ability's type (melee, AOE, ranged),
//! damage at the player's level, Ki and cooldown. The boss difficulty is chosen here too. Type: Cinzel
//! and Cormorant Garamond (OFL); portraits: portraits.rs.

use hudhook::imgui::{DrawListMut, FontId, ImColor32, TextureId, Ui};

use crate::glyphs;
use crate::portraits::Portraits;
use crate::sheets::{self, Fighter, Slot};
use crate::ui::{Difficulty, Shared};

pub struct Fonts {
    /// Roboto: button prompts on every page, and the other menu pages
    pub body: FontId,
    pub title: FontId,
    pub huge: FontId,
    /// Cinzel: the fighter's name, the screen title, card names
    pub c_huge: FontId,
    pub c_title: FontId,
    pub c_name: FontId,
    /// Cormorant Garamond: text
    pub text: FontId,
    pub small: FontId,
}

/// Order of the roster tiles (and of the cursor), grouped by series: the first row (ROW tiles) Dragon Ball, Naruto,
/// One Piece, Bleach, Black Clover and Jujutsu Kaisen; the second My Hero Academia, Hunter x Hunter, JoJo's Bizarre
/// Adventure and Your Tarnished.
pub const ORDER: &[&str] = &[
    "goku", "naruto", "sasuke", "luffy", "zoro", "ichigo", "asta", "yami", "gojo", "yuji",
    "deku", "all_might", "bakugo", "todoroki", "gon", "killua", "jotaro", "dio", "custom",
];

pub fn index_of(id: &str) -> usize {
    ORDER.iter().position(|x| *x == id).unwrap_or(0)
}

pub(crate) const GOLD: [u8; 3] = [206, 170, 96];
pub(crate) const GOLD_HI: [u8; 3] = [240, 214, 150];
pub(crate) const BRONZE: [u8; 3] = [110, 92, 60];
pub(crate) const IVORY: [u8; 3] = [232, 224, 204];
pub(crate) const DIM: [u8; 3] = [150, 142, 128];
pub(crate) const EMBER: [u8; 3] = [232, 140, 60];
pub(crate) const KI: [u8; 3] = [130, 190, 240];

pub(crate) fn col(rgb: [u8; 3], a: u8) -> ImColor32 {
    ImColor32::from_rgba(rgb[0], rgb[1], rgb[2], a)
}

pub(crate) fn text(ui: &Ui, dl: &DrawListMut, font: FontId, at: [f32; 2], c: ImColor32, s: &str) -> [f32; 2] {
    let _f = ui.push_font(font);
    dl.add_text([at[0] + 1.5, at[1] + 1.5], ImColor32::from_rgba(0, 0, 0, 200), s);
    dl.add_text(at, c, s);
    ui.calc_text_size(s)
}

pub(crate) fn text_centered(ui: &Ui, dl: &DrawListMut, font: FontId, cx: f32, y: f32, c: ImColor32, s: &str) -> [f32; 2] {
    let size = {
        let _f = ui.push_font(font);
        ui.calc_text_size(s)
    };
    text(ui, dl, font, [cx - size[0] / 2.0, y], c, s)
}

/// A thin gilded rule fading out at both ends, with a small diamond in the middle.
pub(crate) fn rule(dl: &DrawListMut, cx: f32, y: f32, half: f32, a: u8) {
    let (clear, gold) = (col(GOLD, 0), col(GOLD, a));
    dl.add_rect_filled_multicolor([cx - half, y], [cx, y + 1.5], clear, gold, gold, clear);
    dl.add_rect_filled_multicolor([cx, y], [cx + half, y + 1.5], gold, clear, clear, gold);
    diamond(dl, cx, y + 0.75, 5.0, col(GOLD, a));
}

pub(crate) fn diamond(dl: &DrawListMut, cx: f32, cy: f32, r: f32, c: ImColor32) {
    dl.add_triangle([cx, cy - r], [cx + r, cy], [cx, cy + r], c).filled(true).build();
    dl.add_triangle([cx, cy - r], [cx - r, cy], [cx, cy + r], c).filled(true).build();
}

/// A frame with clipped corners, like Elden Ring's item slots.
pub(crate) fn frame(dl: &DrawListMut, min: [f32; 2], max: [f32; 2], cut: f32, c: ImColor32, thick: f32) {
    let p = [
        [min[0] + cut, min[1]],
        [max[0] - cut, min[1]],
        [max[0], min[1] + cut],
        [max[0], max[1] - cut],
        [max[0] - cut, max[1]],
        [min[0] + cut, max[1]],
        [min[0], max[1] - cut],
        [min[0], min[1] + cut],
    ];
    for i in 0..8 {
        dl.add_line(p[i], p[(i + 1) % 8], c).thickness(thick).build();
    }
}

fn profile_label(f: &Fighter) -> &'static str {
    match f.stat_profile {
        "brawler" => "Brawler",
        "speedster" => "Speedster",
        "tank" => "Juggernaut",
        "caster" => "Caster",
        _ => "Balanced",
    }
}

/// What the build means for play (stat_profiles).
fn profile_line(f: &Fighter) -> String {
    let p = sheets::profile(f.stat_profile);
    let tough = ((1.0 - p.damage_taken_mult) * 100.0).round() as i32;
    let tough = match tough {
        0 => "normal toughness".to_string(),
        t if t > 0 => format!("takes {t}% less damage"),
        t => format!("takes {}% more damage", -t),
    };
    format!("Health x{:.1}   Stamina x{:.1}   {tough}", p.hp_mult, p.stamina_mult)
}

/// The backdrop every menu page shares: nearly black, a faint warm light from the top, darker toward the
/// edges.
pub(crate) fn backdrop(ui: &Ui, w: f32, h: f32) {
    let dl = ui.get_background_draw_list();
    let top = ImColor32::from_rgba(26, 22, 17, 246);
    let bottom = ImColor32::from_rgba(5, 5, 6, 250);
    dl.add_rect_filled_multicolor([0.0, 0.0], [w, h], top, top, bottom, bottom);
    let edge = ImColor32::from_rgba(0, 0, 0, 170);
    let clear = ImColor32::from_rgba(0, 0, 0, 0);
    dl.add_rect_filled_multicolor([0.0, 0.0], [w * 0.18, h], edge, clear, clear, edge);
    dl.add_rect_filled_multicolor([w * 0.82, 0.0], [w, h], clear, edge, edge, clear);
}

/// 1.3.0 (the user: "every character you can play without having to scroll"): the roster as two rows of tiles, grouped
/// by series; the second row is centred under the first.
pub const ROW: usize = 10;

/// A roster slot's (row, column).
pub fn grid_pos(i: usize) -> (usize, usize) {
    (i / ROW, i % ROW)
}

/// The cursor one row up or down (same column, or the row's last tile).
pub fn grid_step(i: usize, down: bool) -> usize {
    let n = ORDER.len();
    let (row, c) = grid_pos(i);
    let last_row = (n - 1) / ROW;
    let to = if down { if row >= last_row { 0 } else { row + 1 } } else if row == 0 { last_row } else { row - 1 };
    let row_len = if to == last_row { n - last_row * ROW } else { ROW };
    to * ROW + c.min(row_len - 1)
}

/// Word-wrap `s` to `width` pixels in `font`.
pub(crate) fn wrap(ui: &Ui, font: FontId, s: &str, width: f32) -> Vec<String> {
    let _f = ui.push_font(font);
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let trial = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if ui.calc_text_size(&trial)[0] > width && !cur.is_empty() {
            lines.push(std::mem::replace(&mut cur, word.to_string()));
        } else {
            cur = trial;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

pub fn draw(ui: &Ui, s: &Shared, cursor_id: &str, fonts: &Fonts, portraits: &Portraits, w: f32, h: f32) {
    let k = h / 1080.0;
    // the 16:9 area in the middle of the screen (ultrawide: bars at the sides)
    let aw = (h * 16.0 / 9.0).min(w);
    let ox = (w - aw) / 2.0;
    let x = |v: f32| ox + v * k;
    let y = |v: f32| v * k;
    let level = s.hud.level.max(1);
    backdrop(ui, w, h);
    let cur = index_of(cursor_id);
    {
        let dl = ui.get_background_draw_list();
        let cx = w / 2.0;
        text_centered(ui, &dl, fonts.c_title, cx, y(22.0), col(GOLD_HI, 255), "CHOOSE YOUR FIGHTER");
        rule(&dl, cx, y(80.0), 600.0 * k, 220);

        // the roster: every fighter at once
        let (tw, th, gap) = (150.0 * k, 180.0 * k, 10.0 * k);
        let row_w = ROW as f32 * tw + (ROW - 1) as f32 * gap;
        let x0 = cx - row_w / 2.0;
        let y0 = y(100.0);
        for (slot, id) in ORDER.iter().enumerate() {
            let Some(f) = sheets::fighter(id) else { continue };
            let (row, c) = grid_pos(slot);
            let row_len = (ORDER.len() - row * ROW).min(ROW);
            let shift = (ROW - row_len) as f32 * (tw + gap) / 2.0;
            let px = x0 + shift + c as f32 * (tw + gap);
            let py = y0 + row as f32 * (th + 12.0 * k);
            let selected = slot == cur;
            let playing = *id == s.fighter;
            let (min, max) = ([px, py], [px + tw, py + th]);
            if let Some((card, _)) = portraits.by_id.get(id) {
                let tint = if selected { ImColor32::WHITE } else { ImColor32::from_rgba(150, 144, 136, 255) };
                dl.add_image(*card, min, max).col(tint).build();
            } else {
                dl.add_rect(min, max, ImColor32::from_rgba(20, 18, 16, 255)).filled(true).build();
            }
            // the name on the tile's dark foot
            let name = f.name.to_uppercase();
            let font = {
                let _f = ui.push_font(fonts.c_name);
                if ui.calc_text_size(&name)[0] > tw - 10.0 * k { fonts.small } else { fonts.c_name }
            };
            text_centered(ui, &dl, font, px + tw / 2.0, max[1] - 34.0 * k, if selected { col(GOLD_HI, 255) } else { col(IVORY, 215) }, &name);
            if playing {
                diamond(&dl, px + 14.0 * k, py + 14.0 * k, 6.0 * k, col(EMBER, 240));
            }
            if selected {
                frame(&dl, [min[0] - 5.0, min[1] - 5.0], [max[0] + 5.0, max[1] + 5.0], 12.0 * k, col(GOLD, 110), 4.0);
                frame(&dl, min, max, 10.0 * k, col(GOLD_HI, 255), 2.0);
            } else {
                frame(&dl, min, max, 10.0 * k, col(BRONZE, 200), 1.2);
            }
        }
    }

    // the highlighted fighter: the whole figure on the left, who they are, and their abilities
    let Some(f) = sheets::fighter(cursor_id) else { return };
    let dl = ui.get_background_draw_list();
    let (pl, pr, pt, pb) = (x(150.0), x(1770.0), y(500.0), y(930.0));
    dl.add_rect([pl, pt], [pr, pb], ImColor32::from_rgba(10, 9, 8, 200)).filled(true).build();
    rule(&dl, (pl + pr) / 2.0, pt, (pr - pl) / 2.0, 200);
    rule(&dl, (pl + pr) / 2.0, pb, (pr - pl) / 2.0, 200);
    // the figure stands in a warm light
    let (fw, fh) = (336.0 * k, 420.0 * k);
    let (fx, fy) = (pl + 20.0 * k, pb - fh - 4.0 * k);
    let glow_c = [fx + fw / 2.0, fy + fh * 0.45];
    for (r, a) in [(200.0, 16u8), (150.0, 20), (100.0, 24)] {
        dl.add_circle(glow_c, r * k, ImColor32::from_rgba(150, 108, 48, a)).filled(true).num_segments(48).build();
    }
    dl.add_rect_filled_multicolor([fx + 30.0 * k, pb - 22.0 * k], [fx + fw / 2.0, pb - 2.0 * k], col(GOLD, 0), col(GOLD, 50), col(GOLD, 50), col(GOLD, 0));
    dl.add_rect_filled_multicolor([fx + fw / 2.0, pb - 22.0 * k], [fx + fw - 30.0 * k, pb - 2.0 * k], col(GOLD, 50), col(GOLD, 0), col(GOLD, 0), col(GOLD, 50));
    if let Some(t) = portraits.full.get(f.id) {
        dl.add_image(*t, [fx, fy], [fx + fw, fy + fh]).build();
    }

    // who they are
    let lx = fx + fw + 24.0 * k;
    let lw = 430.0 * k;
    let mut ly = pt + 22.0 * k;
    let ns = text(ui, &dl, fonts.c_huge, [lx, ly], col(IVORY, 255), &f.name.to_uppercase());
    ly += ns[1] + 2.0 * k;
    text(ui, &dl, fonts.text, [lx, ly], col(GOLD, 255), &format!("{}   ·   {}", f.series, profile_label(f)));
    ly += 38.0 * k;
    for line in wrap(ui, fonts.small, &profile_line(f), lw) {
        text(ui, &dl, fonts.small, [lx, ly], col(DIM, 255), &line);
        ly += 26.0 * k;
    }
    ly += 12.0 * k;
    if let Some(aw) = f.awakening.filter(|a| *a != "Awakening") {
        for line in wrap(ui, fonts.small, &format!("Awakening: {aw}"), lw) {
            text(ui, &dl, fonts.small, [lx, ly], col(EMBER, 255), &line);
            ly += 26.0 * k;
        }
        ly += 8.0 * k;
    }
    if let Some(p) = f.passive {
        let (name, what) = sheets::passive_text(p);
        diamond(&dl, lx + 6.0 * k, ly + 14.0 * k, 5.0 * k, col(GOLD, 255));
        let ps = text(ui, &dl, fonts.text, [lx + 20.0 * k, ly], col(GOLD_HI, 255), &format!("Passive: {name}"));
        ly += ps[1] + 2.0 * k;
        for line in wrap(ui, fonts.small, what, lw - 20.0 * k) {
            text(ui, &dl, fonts.small, [lx + 20.0 * k, ly], col(IVORY, 220), &line);
            ly += 26.0 * k;
        }
    }
    if f.id == "custom" {
        text(ui, &dl, fonts.small, [lx, ly + 8.0 * k], col(KI, 255), "Build your own: choose any four abilities");
    }

    // the abilities
    let tx = lx + lw + 30.0 * k;
    let tr = pr - 30.0 * k;
    let cols = [tx, tx + 330.0 * k, tx + 450.0 * k, tx + 560.0 * k, tx + 630.0 * k];
    let mut ty = pt + 30.0 * k;
    for (i, label) in ["ABILITY", "TYPE", "DAMAGE", "KI", "COOLDOWN"].iter().enumerate() {
        text(ui, &dl, fonts.small, [cols[i], ty], col(DIM, 255), label);
    }
    ty += 34.0 * k;
    dl.add_line([tx, ty], [tr, ty], col(BRONZE, 160)).thickness(1.0).build();
    ty += 12.0 * k;
    for (slot, id) in f.abilities.iter().enumerate() {
        let id = if *id == "$loadout" { s.loadout[slot] } else { id };
        let Some(a) = sheets::ability(id) else { continue };
        let ult = a.slot == Slot::Ultimate;
        let name_c = if ult { col(EMBER, 255) } else { col(IVORY, 255) };
        // a long name wraps within its column
        let lines = wrap(ui, fonts.text, a.name, cols[1] - cols[0] - 14.0 * k);
        let mut ny = ty;
        for l in &lines {
            text(ui, &dl, fonts.text, [cols[0], ny], name_c, l);
            ny += 30.0 * k;
        }
        let mut row_h = (lines.len() as f32 * 30.0 + 18.0) * k;
        if a.combo {
            text(ui, &dl, fonts.small, [cols[0], ny], col(EMBER, 255), "COMBO");
            row_h += 22.0 * k;
        }
        text(ui, &dl, fonts.text, [cols[1], ty], col(GOLD, 255), a.kind.label());
        // (a move that deals no damage of its own: the Shadow Clone fights for him, Infinity guards)
        let dmg = crate::damage::total(a, level).round() as i32;
        text(ui, &dl, fonts.text, [cols[2], ty], col(IVORY, 255), &if dmg > 0 { dmg.to_string() } else { "-".to_string() });
        text(ui, &dl, fonts.text, [cols[3], ty], col(KI, 255), &a.fp_cost.to_string());
        let cd = if ult { "Awakening".to_string() } else { format!("{:.1} s", a.cooldown_s) };
        text(ui, &dl, fonts.text, [cols[4], ty], col(DIM, 255), &cd);
        ty += row_h;
        dl.add_line([tx, ty - 9.0 * k], [tr, ty - 9.0 * k], ImColor32::from_rgba(110, 92, 60, 60)).thickness(1.0).build();
    }
    for line in wrap(ui, fonts.small, &format!("Damage to every enemy it hits at your level ({level}). Bosses take less on higher difficulties; an Awakening takes at most a third of a boss."), tr - tx) {
        text(ui, &dl, fonts.small, [tx, ty], col(DIM, 220), &line);
        ty += 24.0 * k;
    }

    // 1.1.0: difficulty, boss warnings, Training and the volumes live in the Elden Force settings (its
    // button is below); here, what they are set to
    let dy = pb + 18.0 * k;
    let lab = text(ui, &dl, fonts.text, [pl, dy], col(DIM, 255), "Settings");
    let vol = |v: u8| if v == 0 { "off".to_string() } else { format!("{}%", v as u32 * 10) };
    let items = [
        ("Bosses", s.difficulty.label().to_string()),
        ("Warnings", s.warnings.label().to_string()),
        ("Training", if s.training { "On".to_string() } else { "Off".to_string() }),
        ("Music", vol(s.music_vol)),
        ("Voices", vol(s.voice_vol)),
    ];
    let mut sx = pl + lab[0] + 34.0 * k;
    for (name, value) in items {
        let a = text(ui, &dl, fonts.small, [sx, dy + 4.0 * k], col(DIM, 220), name);
        let b = text(ui, &dl, fonts.text, [sx + a[0] + 10.0 * k, dy], col(GOLD_HI, 255), &value);
        sx += a[0] + b[0] + 26.0 * k;
        diamond(&dl, sx - 10.0 * k, dy + 16.0 * k, 3.0 * k, col(BRONZE, 220));
        sx += 14.0 * k;
    }
    // the fighter being played (the ember diamond on its tile)
    let pw = {
        let _f = ui.push_font(fonts.small);
        ui.calc_text_size("playing now")[0]
    };
    diamond(&dl, pr - pw - 16.0 * k, dy + 14.0 * k, 6.0 * k, col(EMBER, 240));
    text(ui, &dl, fonts.small, [pr - pw, dy + 4.0 * k], col(DIM, 220), "playing now");
    let _ = Difficulty::Easy;
    drop(dl);

    // prompts (an ImGui window so the glyph helpers can lay out)
    let _f = ui.push_font(fonts.body);
    ui.window("##select_prompts")
        .position([pl, h - 58.0 * k], hudhook::imgui::Condition::Always)
        .flags(hudhook::imgui::WindowFlags::NO_DECORATION | hudhook::imgui::WindowFlags::ALWAYS_AUTO_RESIZE | hudhook::imgui::WindowFlags::NO_INPUTS | hudhook::imgui::WindowFlags::NO_BACKGROUND)
        .build(|| {
            ui.set_window_font_scale(k.clamp(0.75, 1.6));
            glyphs::hints(ui, &[("menu_left", ""), ("menu_right", "choose"), ("menu_up", ""), ("menu_down", "row"), ("menu_confirm", "fight"), ("menu_moves", "move list"), ("menu_build", "build"), ("menu_settings", "Elden Force settings"), ("menu_back", "close")])
        });
}

/// The fighter's face in Elden Ring's own HUD box (the slot left of the health bar), at 1080p
/// x 49..134, y 26..112 with clipped corners; scaled with the screen height like the game's HUD.
pub fn hud_face(ui: &Ui, face: TextureId, w: f32, h: f32) {
    let k = h / 1080.0;
    let ox = (w - h * 16.0 / 9.0).max(0.0) / 2.0;
    let (x0, y0, x1, y1) = (ox + 55.0 * k, 32.0 * k, ox + 128.0 * k, 106.0 * k);
    let dl = ui.get_background_draw_list();
    dl.add_image_rounded(face, [x0, y0], [x1, y1], 9.0 * k).build();
}
