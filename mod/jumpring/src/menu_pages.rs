//! 1.1.0: every page of the Elden Force menu in the fighter select's look (user: "make all screens inside
//! of our custom UI match the UI that we most recently built for the character selection"): the same
//! dark vignetted backdrop, carved gold capitals between gilded rules, a dark panel of rows with the
//! chosen row lit in gold, Cinzel and Cormorant Garamond, and the new button glyphs.
//! Pages: Settings (volumes, difficulty, boss warnings, Training, Controls), Controls, Build, and Your
//! Tarnished's loadout.

use hudhook::imgui::{Condition, DrawListMut, ImColor32, Ui, WindowFlags};

use crate::glyphs;
use crate::portraits::Portraits;
use crate::select_screen::{backdrop, col, diamond, frame, rule, text, text_centered, Fonts, BRONZE, DIM, EMBER, GOLD, GOLD_HI, IVORY, KI};
use crate::sheets::{self, Slot};
use crate::ui::{Difficulty, Menu, Page, SettingRow, Shared, Warnings};

/// The page's layout on the 1080p grid, scaled to the screen.
struct Grid {
    k: f32,
    ox: f32,
}

impl Grid {
    fn x(&self, v: f32) -> f32 {
        self.ox + v * self.k
    }
    fn y(&self, v: f32) -> f32 {
        v * self.k
    }
}

const PANEL_L: f32 = 330.0;
const PANEL_R: f32 = 1590.0;
const VALUE_X: f32 = 860.0;

/// The title, the line under it and the panel's frame; returns the panel's top.
fn chrome(ui: &Ui, dl: &DrawListMut, fonts: &Fonts, g: &Grid, w: f32, title: &str, sub: &[&str], rows_h: f32) -> f32 {
    let cx = w / 2.0;
    text_centered(ui, dl, fonts.c_title, cx, g.y(34.0), col(GOLD_HI, 255), title);
    rule(dl, cx, g.y(92.0), 560.0 * g.k, 220);
    let mut y = g.y(108.0);
    for line in sub {
        text_centered(ui, dl, fonts.small, cx, y, col(DIM, 235), line);
        y += 30.0 * g.k;
    }
    let top = y + 22.0 * g.k;
    let (l, r) = (g.x(PANEL_L), g.x(PANEL_R));
    dl.add_rect([l, top], [r, top + rows_h], ImColor32::from_rgba(10, 9, 8, 205)).filled(true).build();
    rule(dl, (l + r) / 2.0, top, (r - l) / 2.0, 200);
    rule(dl, (l + r) / 2.0, top + rows_h, (r - l) / 2.0, 200);
    top
}

/// One row's backing: the chosen row has a gold wash, a clipped-corner frame and a diamond at its left.
fn row_back(dl: &DrawListMut, g: &Grid, y: f32, h: f32, selected: bool, dim: bool) {
    let (l, r) = (g.x(PANEL_L + 18.0), g.x(PANEL_R - 18.0));
    if selected {
        let (a, z) = (col(GOLD, 40), col(GOLD, 0));
        dl.add_rect_filled_multicolor([l, y], [r, y + h], a, z, z, a);
        frame(dl, [l, y], [r, y + h], 9.0 * g.k, col(GOLD, 170), 1.3);
        diamond(dl, l + 22.0 * g.k, y + h / 2.0, 6.0 * g.k, col(GOLD_HI, 255));
    } else if !dim {
        dl.add_line([l + 40.0 * g.k, y + h], [r - 40.0 * g.k, y + h], col(BRONZE, 55)).thickness(1.0).build();
    }
}

/// A gilded slider: track, gold fill, a tick each tenth, a diamond knob, and the value as a percentage.
fn slider(ui: &Ui, dl: &DrawListMut, fonts: &Fonts, g: &Grid, x0: f32, x1: f32, cy: f32, v: u8, selected: bool) {
    let frac = v as f32 / 10.0;
    let fx = x0 + (x1 - x0) * frac;
    dl.add_line([x0, cy], [x1, cy], col(BRONZE, 230)).thickness(2.0 * g.k.max(0.7)).build();
    if frac > 0.0 {
        dl.add_rect_filled_multicolor([x0, cy - 2.0 * g.k], [fx, cy + 2.0 * g.k], col(EMBER, 230), col(GOLD_HI, 255), col(GOLD_HI, 255), col(EMBER, 230));
    }
    for i in 0..=10 {
        let tx = x0 + (x1 - x0) * i as f32 / 10.0;
        let tall = if i % 5 == 0 { 7.0 } else { 4.0 } * g.k;
        dl.add_line([tx, cy - tall], [tx, cy + tall], col(if (i as f32) <= frac * 10.0 { GOLD } else { BRONZE }, 200)).thickness(1.0).build();
    }
    diamond(dl, fx, cy, if selected { 9.0 } else { 7.0 } * g.k, col(if selected { GOLD_HI } else { GOLD }, 255));
    let label = if v == 0 { "Off".to_string() } else { format!("{}%", v as u32 * 10) };
    text(ui, dl, fonts.text, [x1 + 26.0 * g.k, cy - 17.0 * g.k], col(if selected { GOLD_HI } else { IVORY }, 255), &label);
}

/// Choices in a row (the chosen one in gold with a diamond), like the fighter select's.
fn choices(ui: &Ui, dl: &DrawListMut, fonts: &Fonts, g: &Grid, x: f32, y: f32, items: &[&str], on: usize) -> f32 {
    let mut x = x;
    for (i, label) in items.iter().enumerate() {
        let sel = i == on;
        let sz = text(ui, dl, fonts.text, [x, y], if sel { col(GOLD_HI, 255) } else { col(DIM, 150) }, label);
        if sel {
            diamond(dl, x - 13.0 * g.k, y + sz[1] / 2.0, 4.5 * g.k, col(GOLD, 255));
        }
        x += sz[0] + 44.0 * g.k;
    }
    x
}

/// Small arrows either side of a value that left / right changes.
fn arrows(dl: &DrawListMut, g: &Grid, x0: f32, x1: f32, cy: f32) {
    let r = 6.0 * g.k;
    let c = col(GOLD, 230);
    dl.add_triangle([x0 - r, cy], [x0 + r * 0.6, cy - r], [x0 + r * 0.6, cy + r], c).filled(true).build();
    dl.add_triangle([x1 + r, cy], [x1 - r * 0.6, cy - r], [x1 - r * 0.6, cy + r], c).filled(true).build();
}

/// Button glyphs at given places (an ImGui window over the page: the glyph helpers lay out with it).
fn glyph_layer(ui: &Ui, fonts: &Fonts, w: f32, h: f32, k: f32, draw: impl FnOnce()) {
    let _f = ui.push_font(fonts.body);
    ui.window("##page_glyphs")
        .position([0.0, 0.0], Condition::Always)
        .size([w, h], Condition::Always)
        .flags(WindowFlags::NO_DECORATION | WindowFlags::NO_INPUTS | WindowFlags::NO_BACKGROUND | WindowFlags::NO_SCROLLBAR | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS)
        .build(|| {
            ui.set_window_font_scale(k.clamp(0.75, 1.6));
            draw();
        });
}

fn prompts(ui: &Ui, g: &Grid, h: f32, items: &[(&str, &str)]) {
    ui.set_cursor_screen_pos([g.x(PANEL_L), h - 62.0 * g.k]);
    glyphs::hints(ui, items);
}

pub fn draw(ui: &Ui, s: &Shared, menu: &Menu, fonts: &Fonts, portraits: &Portraits, w: f32, h: f32) {
    let k = h / 1080.0;
    let g = Grid { k, ox: (w - (h * 16.0 / 9.0).min(w)) / 2.0 };
    backdrop(ui, w, h);
    match menu.page {
        Page::Settings => settings(ui, s, menu, fonts, &g, w, h),
        Page::Controls => controls(ui, menu, fonts, &g, w, h),
        Page::Build => build(ui, s, menu, fonts, &g, w, h),
        Page::Loadout => loadout(ui, s, menu, fonts, portraits, &g, w, h),
        Page::Moves => moves(ui, s, menu, fonts, &g, w, h),
        Page::Credits => credits(ui, menu, fonts, &g, w, h),
        Page::Roster => {}
    }
}

fn settings(ui: &Ui, s: &Shared, menu: &Menu, fonts: &Fonts, g: &Grid, w: f32, h: f32) {
    let row_h = 84.0 * g.k;
    let rows = crate::ui::setting_rows(s);
    let dl = ui.get_background_draw_list();
    let top = chrome(ui, &dl, fonts, g, w, "ELDEN FORCE SETTINGS", &["Saved for your next session too. Training stays on only until you leave the game."], row_h * rows.len() as f32 + 24.0 * g.k);
    let cur = menu.cursor.min(rows.len() - 1);
    let mut controls_y = 0.0;
    for (i, row) in rows.iter().enumerate() {
        let y = top + 12.0 * g.k + row_h * i as f32;
        let sel = i == cur;
        row_back(&dl, g, y, row_h, sel, false);
        let lx = g.x(PANEL_L + 58.0);
        let (name, what) = match row {
            SettingRow::Music => ("Music", "Elden Force's own soundtrack: menus, exploring, boss themes"),
            SettingRow::Voices => ("Voice quips", "the fighters' battle shouts"),
            SettingRow::Difficulty => ("Boss difficulty", match s.difficulty {
                Difficulty::Easy => "bosses have 1.5x their health",
                Difficulty::Normal => "bosses have 3x their health",
                Difficulty::Hard => "bosses have 5x their health",
            }),
            SettingRow::Warnings => ("Boss warnings", match s.warnings {
                Warnings::Off => "no tells: read the bosses yourself",
                Warnings::Text => "the attack's name above the boss bar",
                Warnings::Visual => "where it lands, outlined on the ground",
                Warnings::All => "the attack's name and its outline on the ground",
            }),
            SettingRow::Opacity => ("Warning opacity", "how solid the boss attacks' seals on the ground look"),
            SettingRow::Training => ("Training", "the enemy you lock onto shows every hit's damage"),
            SettingRow::Moves => ("Move list", "every fighter's moveset and how they move"),
            SettingRow::Controls => ("Controls", "change the buttons for abilities, swinging and the menu"),
            SettingRow::Credits => ("Credits", "the people whose work Elden Force is built with"),
        };
        text(ui, &dl, fonts.text, [lx, y + 10.0 * g.k], col(if sel { GOLD_HI } else { IVORY }, 255), name);
        text(ui, &dl, fonts.small, [lx, y + 44.0 * g.k], col(DIM, 220), what);
        let vx = g.x(VALUE_X + 70.0);
        let cy = y + row_h / 2.0;
        match row {
            SettingRow::Music | SettingRow::Voices | SettingRow::Opacity => {
                let v = match row { SettingRow::Music => s.music_vol, SettingRow::Voices => s.voice_vol, _ => s.warn_opacity };
                let x1 = g.x(PANEL_R - 190.0);
                slider(ui, &dl, fonts, g, vx, x1, cy, v, sel);
                if sel {
                    arrows(&dl, g, vx - 26.0 * g.k, x1 + 120.0 * g.k, cy);
                }
            }
            SettingRow::Difficulty => {
                let on = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard].iter().position(|d| *d == s.difficulty).unwrap_or(1);
                choices(ui, &dl, fonts, g, vx, cy - 17.0 * g.k, &["Easy", "Normal", "Hard"], on);
            }
            SettingRow::Warnings => {
                let on = Warnings::ALL.iter().position(|x| *x == s.warnings).unwrap_or(3);
                choices(ui, &dl, fonts, g, vx, cy - 17.0 * g.k, &["Off", "Text", "Visual", "All"], on);
            }
            SettingRow::Training => {
                choices(ui, &dl, fonts, g, vx, cy - 17.0 * g.k, &["Off", "On"], s.training as usize);
            }
            SettingRow::Controls | SettingRow::Moves | SettingRow::Credits => {
                controls_y = cy;
                let t = text(ui, &dl, fonts.text, [vx, cy - 17.0 * g.k], col(if sel { GOLD_HI } else { IVORY }, 255), "Open");
                let ax = vx + t[0] + 18.0 * g.k;
                dl.add_triangle([ax + 7.0 * g.k, cy], [ax - 3.0 * g.k, cy - 7.0 * g.k], [ax - 3.0 * g.k, cy + 7.0 * g.k], col(GOLD, 255)).filled(true).build();
            }
        }
    }
    let _ = controls_y;
    drop(dl);
    glyph_layer(ui, fonts, w, h, g.k, || {
        let row = rows[cur];
        let change = if matches!(row, SettingRow::Controls | SettingRow::Moves | SettingRow::Credits) { ("menu_confirm", "open") } else { ("menu_right", "change") };
        prompts(ui, g, h, &[("menu_up", ""), ("menu_down", "choose"), ("menu_left", ""), change, ("menu_back", "back")]);
    });
}

/// 1.3.0: the Credits page: headings in gold, their entries under them, scrolled a line at a time.
fn credits(ui: &Ui, menu: &Menu, fonts: &Fonts, g: &Grid, w: f32, h: f32) {
    let lines = crate::credits::LINES;
    let shown = crate::credits::PAGE_LINES.min(lines.len());
    let line_h = 34.0 * g.k;
    let dl = ui.get_background_draw_list();
    let top = chrome(ui, &dl, fonts, g, w, "CREDITS", &["Elden Force is built with the work of these people. Thank you."], line_h * shown as f32 + 36.0 * g.k);
    let first = menu.cursor.min(lines.len().saturating_sub(shown));
    for (i, (heading, line)) in lines.iter().skip(first).take(shown).enumerate() {
        let y = top + 18.0 * g.k + line_h * i as f32;
        if *heading {
            diamond(&dl, g.x(PANEL_L + 40.0), y + line_h / 2.0, 5.0 * g.k, col(GOLD_HI, 255));
            text(ui, &dl, fonts.text, [g.x(PANEL_L + 58.0), y], col(GOLD_HI, 255), line);
        } else {
            text(ui, &dl, fonts.small, [g.x(PANEL_L + 58.0), y + 4.0 * g.k], col(IVORY, 235), line);
        }
    }
    // more above / below
    let cx = w / 2.0;
    if first > 0 {
        dl.add_triangle([cx, top + 4.0 * g.k], [cx - 8.0 * g.k, top + 14.0 * g.k], [cx + 8.0 * g.k, top + 14.0 * g.k], col(GOLD, 220)).filled(true).build();
    }
    let bottom = top + line_h * shown as f32 + 36.0 * g.k;
    if first + shown < lines.len() {
        dl.add_triangle([cx, bottom - 4.0 * g.k], [cx - 8.0 * g.k, bottom - 14.0 * g.k], [cx + 8.0 * g.k, bottom - 14.0 * g.k], col(GOLD, 220)).filled(true).build();
    }
    drop(dl);
    glyph_layer(ui, fonts, w, h, g.k, || {
        prompts(ui, g, h, &[("menu_up", ""), ("menu_down", "scroll"), ("menu_back", "back")]);
    });
}

fn controls(ui: &Ui, menu: &Menu, fonts: &Fonts, g: &Grid, w: f32, h: f32) {
    let rows = crate::ui::control_rows();
    let n = rows.len() + 1;
    let row_h = 70.0 * g.k;
    let capturing = crate::binds::CAPTURE.load(std::sync::atomic::Ordering::Relaxed);
    let dl = ui.get_background_draw_list();
    let top = chrome(
        ui,
        &dl,
        fonts,
        g,
        w,
        "CONTROLS",
        &[
            "Pick an action, then hold the buttons you want together and let go (or press a key or mouse button).",
            "The game never sees the buttons of a combo you are holding, so pick ones that are free to give up while held.",
        ],
        row_h * n as f32 + 64.0 * g.k,
    );
    let cur = menu.cursor.min(n - 1);
    let (pad_x, key_x) = (g.x(VALUE_X + 60.0), g.x(VALUE_X + 420.0));
    text(ui, &dl, fonts.small, [pad_x, top + 14.0 * g.k], col(DIM, 255), "CONTROLLER");
    text(ui, &dl, fonts.small, [key_x, top + 14.0 * g.k], col(DIM, 255), "KEYBOARD AND MOUSE");
    let y0 = top + 52.0 * g.k;
    for (i, inp) in rows.iter().enumerate() {
        let y = y0 + row_h * i as f32;
        let sel = i == cur;
        row_back(&dl, g, y, row_h, sel, false);
        let r = &sheets::INPUTS[*inp];
        text(ui, &dl, fonts.text, [g.x(PANEL_L + 58.0), y + row_h / 2.0 - 17.0 * g.k], col(if sel { GOLD_HI } else { IVORY }, 255), crate::ui::control_label(r.id));
        if capturing == *inp as i32 {
            let pulse = (0.6 + 0.4 * (ui.time() as f32 * 5.0).sin()) * 255.0;
            text(ui, &dl, fonts.text, [pad_x, y + row_h / 2.0 - 17.0 * g.k], col(EMBER, pulse as u8), "hold the new buttons now...   (Esc cancels)");
        }
    }
    let ry = y0 + row_h * rows.len() as f32;
    row_back(&dl, g, ry, row_h, cur == rows.len(), true);
    text(ui, &dl, fonts.text, [g.x(PANEL_L + 58.0), ry + row_h / 2.0 - 17.0 * g.k], col(if cur == rows.len() { GOLD_HI } else { DIM }, 255), "Reset to defaults");
    drop(dl);
    glyph_layer(ui, fonts, w, h, g.k, || {
        for (i, inp) in rows.iter().enumerate() {
            if capturing == *inp as i32 {
                continue;
            }
            let y = y0 + row_h * i as f32 + row_h / 2.0 - 19.0 * g.k;
            let id = sheets::INPUTS[*inp].id;
            ui.set_cursor_screen_pos([pad_x, y]);
            glyphs::prompt_for(ui, id, crate::binds::Device::Controller);
            ui.set_cursor_screen_pos([key_x, y]);
            glyphs::prompt_for(ui, id, crate::binds::Device::KeyboardMouse);
        }
        prompts(ui, g, h, &[("menu_up", ""), ("menu_down", "choose"), ("menu_confirm", if cur == rows.len() { "reset" } else { "change" }), ("menu_back", "back")]);
    });
}

fn build(ui: &Ui, s: &Shared, menu: &Menu, fonts: &Fonts, g: &Grid, w: f32, h: f32) {
    let b = &s.build;
    let row_h = 58.0 * g.k;
    let n = if b.tarnished { 8 } else { 3 };
    let dl = ui.get_background_draw_list();
    let (title, sub): (&str, [&str; 2]) = if b.tarnished {
        ("YOUR TARNISHED'S BUILD", ["Your Tarnished's own attributes: put free points anywhere. Abilities follow your level.", "Levels gained on the fighters' build arrive here as free points. Rune level is shared."])
    } else {
        ("THE FIGHTERS' BUILD", ["Shared by every fighter. Their moves and punches follow your level, so points go into Vigor, Mind and Endurance.", "Levels gained on Your Tarnished's build arrive here as free points. Rune level is shared."])
    };
    let top = chrome(ui, &dl, fonts, g, w, title, &sub, row_h * 8.0 + 96.0 * g.k);
    // level and free points
    let hx = g.x(PANEL_L + 58.0);
    let ls = text(ui, &dl, fonts.text, [hx, top + 22.0 * g.k], col(IVORY, 255), &format!("Level {}", b.level));
    let fc = if b.free > 0 { col(EMBER, 255) } else { col(DIM, 255) };
    text(ui, &dl, fonts.text, [hx + ls[0] + 60.0 * g.k, top + 22.0 * g.k], fc, &format!("Free points: {}", b.free));
    let y0 = top + 82.0 * g.k;
    let cur = menu.cursor.min(n - 1);
    for i in 0..8 {
        let y = y0 + row_h * i as f32;
        let open = i < n;
        let sel = open && i == cur;
        row_back(&dl, g, y, row_h, sel, !open);
        let c = if sel { col(GOLD_HI, 255) } else if open { col(IVORY, 255) } else { col(DIM, 110) };
        let cy = y + row_h / 2.0;
        text(ui, &dl, fonts.text, [hx, cy - 17.0 * g.k], c, crate::builds::NAMES[i]);
        let v = b.attrs[i];
        let vs = text(ui, &dl, fonts.text, [g.x(VALUE_X), cy - 17.0 * g.k], c, &format!("{v:>2}"));
        // a thin gilded bar: the attribute out of 99
        let (bx0, bx1) = (g.x(VALUE_X) + vs[0] + 30.0 * g.k, g.x(PANEL_R - 300.0));
        dl.add_line([bx0, cy], [bx1, cy], col(BRONZE, if open { 200 } else { 70 })).thickness(2.0).build();
        let fx = bx0 + (bx1 - bx0) * (v as f32 / 99.0).clamp(0.0, 1.0);
        if open {
            dl.add_rect_filled_multicolor([bx0, cy - 2.0 * g.k], [fx, cy + 2.0 * g.k], col(EMBER, 220), col(GOLD_HI, 255), col(GOLD_HI, 255), col(EMBER, 220));
        }
        if !open {
            text(ui, &dl, fonts.small, [bx1 + 24.0 * g.k, cy - 14.0 * g.k], col(DIM, 120), "fixed for fighters");
        } else if sel && b.free > 0 {
            text(ui, &dl, fonts.small, [bx1 + 24.0 * g.k, cy - 14.0 * g.k], col(EMBER, 255), "+1 with confirm");
        }
    }
    drop(dl);
    glyph_layer(ui, fonts, w, h, g.k, || {
        prompts(ui, g, h, &[("menu_up", ""), ("menu_down", "attribute"), ("menu_confirm", "add a point"), ("menu_back", "back")]);
    });
}

fn loadout(ui: &Ui, s: &Shared, menu: &Menu, fonts: &Fonts, portraits: &Portraits, g: &Grid, w: f32, h: f32) {
    let row_h = 118.0 * g.k;
    let level = s.hud.level.max(1);
    let dl = ui.get_background_draw_list();
    let top = chrome(ui, &dl, fonts, g, w, "YOUR TARNISHED", &["Choose any four abilities: three skills (combos go on the third) and an Awakening."], row_h * 4.0 + 24.0 * g.k);
    let cur = menu.cursor.min(3);
    let binds = ["ability_1", "ability_2", "ability_3", "ability_4"];
    let mut glyph_at = Vec::new();
    for slot in 0..4 {
        let y = top + 12.0 * g.k + row_h * slot as f32;
        let sel = slot == cur;
        row_back(&dl, g, y, row_h, sel, false);
        let lx = g.x(PANEL_L + 58.0);
        let kind = if slot == 3 { "AWAKENING" } else if slot == 2 { "SKILL  ·  COMBOS" } else { "SKILL" };
        text(ui, &dl, fonts.small, [lx, y + 16.0 * g.k], col(if slot == 3 { EMBER } else { DIM }, 255), &format!("ABILITY {}   {kind}", slot + 1));
        glyph_at.push((binds[slot], [lx, y + 56.0 * g.k]));
        let Some(a) = sheets::ability(s.loadout[slot]) else { continue };
        let owner = sheets::fighter(a.owner);
        // the owner's face, then the ability
        let px = g.x(VALUE_X - 160.0);
        let ps = 86.0 * g.k;
        let py = y + (row_h - ps) / 2.0;
        // (Your Tarnished's own abilities, owner "avatar": the Tarnished's emblem)
        let face_id = if sheets::fighter(a.owner).is_some() { a.owner } else { "custom" };
        if let Some((_, face)) = portraits.by_id.get(face_id) {
            dl.add_image(*face, [px, py], [px + ps, py + ps]).build();
        } else {
            dl.add_rect([px, py], [px + ps, py + ps], ImColor32::from_rgba(20, 18, 16, 255)).filled(true).build();
        }
        frame(&dl, [px, py], [px + ps, py + ps], 8.0 * g.k, col(if sel { GOLD_HI } else { BRONZE }, 230), 1.2);
        let nx = px + ps + 28.0 * g.k;
        let name_c = if a.slot == Slot::Ultimate { col(EMBER, 255) } else if sel { col(GOLD_HI, 255) } else { col(IVORY, 255) };
        let ns = text(ui, &dl, fonts.text, [nx, y + 18.0 * g.k], name_c, a.name);
        if a.combo {
            text(ui, &dl, fonts.small, [nx + ns[0] + 12.0 * g.k, y + 22.0 * g.k], col(EMBER, 255), "COMBO");
        }
        let from = owner.map(|f| f.name).unwrap_or("Your Tarnished");
        let dmg = crate::damage::total(a, level).round() as i32;
        let cd = if a.slot == Slot::Ultimate { "Awakening".to_string() } else { format!("{:.1} s", a.cooldown_s) };
        let dmg = if dmg > 0 { format!("{dmg} damage") } else { "no damage of its own".to_string() };
        let line = format!("{from}   ·   {}   ·   {dmg}   ·   {} Ki   ·   {cd}", a.kind.label(), a.fp_cost);
        text(ui, &dl, fonts.small, [nx, y + 62.0 * g.k], col(DIM, 235), &line);
        let _ = KI;
        if sel {
            arrows(&dl, g, px - 30.0 * g.k, g.x(PANEL_R - 40.0), y + row_h / 2.0);
        }
    }
    drop(dl);
    glyph_layer(ui, fonts, w, h, g.k, || {
        for (id, at) in &glyph_at {
            ui.set_cursor_screen_pos(*at);
            glyphs::prompt(ui, id);
        }
        prompts(ui, g, h, &[("menu_up", ""), ("menu_down", "slot"), ("menu_left", ""), ("menu_right", "ability"), ("menu_confirm", "play"), ("menu_back", "back")]);
    });
}

/// 1.2.x: a Move List note in the words of the device in use (the notes name the pad's buttons).
fn for_device(note: &str, pad: bool) -> String {
    if pad {
        return note.to_string();
    }
    let mut out = String::new();
    let words: Vec<&str> = note.split(' ').collect();
    let mut i = 0;
    while i < words.len() {
        if i + 2 < words.len() && words[i] == "Y" && words[i + 1] == "+" && words[i + 2].trim_end_matches([',', ':', ';', ')']) == "RB" {
            out.push_str("Two-Hand");
            out.push_str(&words[i + 2][2..]);
            i += 3;
        } else {
            let w = words[i];
            let core = w.trim_end_matches([',', ':', ';', ')', '.']);
            let rest = &w[core.len()..];
            let name = match core {
                "RT" => "Strong Attack",
                "RB" => "Attack",
                "LB" => "Guard",
                "LT" => "Skill",
                other => other,
            };
            out.push_str(name);
            out.push_str(rest);
            i += 1;
        }
        if i < words.len() {
            out.push(' ');
        }
    }
    out
}

/// 1.2.x: how far the Move List can scroll (1080p pixels), for the input handler (lib.rs).
pub static MOVES_MAX_SCROLL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// 1.2.x: each Move List line's buttons as drawn last frame, in 1080p pixels (keyboard keys are wider than pad
/// buttons: the words start after the widest, so they never run into each other).
static BTN_W: std::sync::Mutex<Option<std::collections::HashMap<String, f32>>> = std::sync::Mutex::new(None);

/// 1.2.x (user: "The move list screen is very confusing and cluttered ... the names of the inputs cover up what
/// the move actually is"): two tabs, the moveset (Carian Combo Warriors, by fighter or by weapon, each grip a
/// section) and movement (Elden Force's own, by fighter); one wide column, the buttons in a column as wide as the
/// widest drawn, the move's name and a note beside them; up / down scrolls a long list.
fn moves(ui: &Ui, s: &Shared, menu: &Menu, fonts: &Fonts, g: &Grid, w: f32, h: f32) {
    use crate::movelists::{self, Section, Weapon};
    let list = crate::ui::combo_fighters();
    if list.is_empty() {
        return;
    }
    let v = s.moves;
    let fighter = list[menu.cursor.min(list.len() - 1)];
    let fname = sheets::fighter(fighter).map(|f| f.name).unwrap_or(fighter);
    let by_weapon = v.tab == 0 && v.by_weapon;
    let weapon = if by_weapon { movelists::WEAPONS[v.weapon.min(movelists::WEAPONS.len() - 1)] } else { Weapon::of_fighter(fighter) };
    let (page_name, about, sections): (String, String, Vec<Section>) = if v.tab == 0 {
        let name = if by_weapon { weapon.name().to_uppercase() } else { format!("{}: {}", fname.to_uppercase(), weapon.name().to_uppercase()) };
        let mut secs = movelists::sections(weapon);
        // (1.3.0) the sword fighters' left hand: Zoro's second katana, the others' empty hand
        if !by_weapon && weapon != Weapon::Fist {
            use crate::combos::Btn;
            let lines = if fighter == "zoro" {
                vec![crate::combos::Line { buttons: vec![Btn::Left], name: "Paired slashes".to_string(), note: "a katana in each hand: both blades strike together" }]
            } else {
                vec![crate::combos::Line { buttons: vec![Btn::Left], name: "Guard".to_string(), note: "two-handed only: one-handed, the left hand is empty and LB does nothing" }]
            };
            secs.insert(0, Section { title: if fighter == "zoro" { "TWO KATANAS" } else { "LEFT HAND" }, lines });
        }
        (name, weapon.about().to_string(), secs)
    } else {
        let all = crate::combos::movement_lines(fighter);
        let shared_n = 3.min(all.len());
        let (own, common) = all.split_at(all.len() - shared_n);
        let take = |l: &[crate::combos::Line]| l.iter().map(|x| crate::combos::Line { buttons: x.buttons.clone(), name: x.name.clone(), note: x.note }).collect::<Vec<_>>();
        let mut secs = Vec::new();
        if !own.is_empty() {
            secs.push(Section { title: "THEIR OWN", lines: take(own) });
        }
        secs.push(Section { title: "EVERY FIGHTER", lines: take(common) });
        (fname.to_uppercase(), "Elden Force's movement: how this fighter gets around".to_string(), secs)
    };

    let wrap = |font, s: &str, width: f32, sep: &str| -> Vec<String> {
        let _f = ui.push_font(font);
        let mut out: Vec<String> = Vec::new();
        let mut cur = String::new();
        for part in s.split(sep) {
            let next = if cur.is_empty() { part.to_string() } else { format!("{cur}{sep}{part}") };
            if !cur.is_empty() && ui.calc_text_size(&next)[0] > width {
                out.push(std::mem::take(&mut cur));
                cur = part.to_string();
            } else {
                cur = next;
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    };
    let device = crate::binds::device() == crate::binds::Device::Controller;
    let key = |l: &crate::combos::Line| format!("{}|{}|{:?}", device, l.name, l.buttons);
    let measured = BTN_W.lock().unwrap().clone().unwrap_or_default();
    // the button column: as wide as the widest row drawn (last frame), within limits
    let (l0, r0) = (g.x(PANEL_L + 40.0), g.x(PANEL_R - 40.0));
    let widest = sections
        .iter()
        .flat_map(|s| s.lines.iter())
        .map(|l| measured.get(&key(l)).copied().unwrap_or(if l.buttons.is_empty() { 0.0 } else { 160.0 }))
        .fold(0.0f32, f32::max);
    let col_w = ((widest + 28.0) * g.k).clamp(150.0 * g.k, (r0 - l0) * 0.42);
    let words_x = l0 + col_w;
    let words_w = r0 - words_x;
    let (line_n, line_s, head_h) = (30.0 * g.k, 24.0 * g.k, 44.0 * g.k);

    enum Row {
        Head(&'static str),
        Move { idx: (usize, usize), name: Vec<String>, note: Vec<String>, below: bool },
    }
    // each row, its y from the body's top and its height
    let mut rows: Vec<(Row, f32, f32)> = Vec::new();
    let mut y = 0.0;
    for (si, sec) in sections.iter().enumerate() {
        rows.push((Row::Head(sec.title), y, head_h));
        y += head_h;
        for (li, l) in sec.lines.iter().enumerate() {
            let bw = measured.get(&key(l)).copied().unwrap_or(0.0) * g.k;
            // buttons wider than their column go on a line of their own, the words under them
            let below = bw > col_w - 12.0 * g.k;
            let w_w = if below { r0 - l0 } else { words_w };
            let name = wrap(fonts.text, &l.name, w_w, "  >  ");
            let note = if l.note.is_empty() { Vec::new() } else { wrap(fonts.small, &for_device(l.note, device), w_w, " ") };
            let mut hh = name.len() as f32 * line_n + note.len() as f32 * line_s + 16.0 * g.k;
            if below {
                hh += 44.0 * g.k;
            }
            let hh = hh.max(56.0 * g.k);
            rows.push((Row::Move { idx: (si, li), name, note, below }, y, hh));
            y += hh;
        }
        y += 10.0 * g.k;
    }
    let content_h = y;

    let dl = ui.get_background_draw_list();
    // the panel: as tall as the screen allows
    let panel_h = (h - g.y(260.0)).max(200.0 * g.k);
    let top = chrome(ui, &dl, fonts, g, w, "MOVE LIST", &[], panel_h);
    // tabs
    let tabs = ["MOVESET  (Carian Combo Warriors)", "MOVEMENT  (Elden Force)"];
    let tab_y = top + 16.0 * g.k;
    let mut tx = g.x(PANEL_L + 110.0);
    let mut tab_x = [0.0f32; 2];
    let mut tab_end = 0.0f32;
    for (i, t) in tabs.iter().enumerate() {
        let on = v.tab as usize == i;
        let sz = text(ui, &dl, fonts.text, [tx, tab_y], col(if on { GOLD_HI } else { DIM }, if on { 255 } else { 170 }), t);
        if on {
            dl.add_line([tx, tab_y + sz[1] + 4.0 * g.k], [tx + sz[0], tab_y + sz[1] + 4.0 * g.k], col(GOLD, 230)).thickness(2.0 * g.k.max(0.7)).build();
            diamond(&dl, tx - 14.0 * g.k, tab_y + sz[1] / 2.0, 5.0 * g.k, col(GOLD_HI, 255));
        }
        tab_x[i] = tx;
        tab_end = tx + sz[0];
        tx += sz[0] + 90.0 * g.k;
    }
    // the page: the fighter or the weapon between arrows, what it covers under it
    let sel_y = tab_y + 52.0 * g.k;
    let sz = text_centered(ui, &dl, fonts.c_title, w / 2.0, sel_y, col(GOLD_HI, 255), &page_name);
    arrows(&dl, g, w / 2.0 - sz[0] / 2.0 - 30.0 * g.k, w / 2.0 + sz[0] / 2.0 + 30.0 * g.k, sel_y + sz[1] / 2.0);
    let mode = if by_weapon { "by weapon" } else { "by fighter" };
    let pager = if by_weapon {
        format!("{} of {}  ·  {}", v.weapon.min(movelists::WEAPONS.len() - 1) + 1, movelists::WEAPONS.len(), mode)
    } else {
        format!("{} of {}  ·  {}", menu.cursor.min(list.len() - 1) + 1, list.len(), mode)
    };
    text_centered(ui, &dl, fonts.small, w / 2.0, sel_y + sz[1] + 6.0 * g.k, col(DIM, 220), &pager);
    let about_lines = wrap(fonts.small, &about, r0 - l0, " ");
    let mut ay = sel_y + sz[1] + 36.0 * g.k;
    for a in &about_lines {
        text_centered(ui, &dl, fonts.small, w / 2.0, ay, col(IVORY, 200), a);
        ay += line_s;
    }
    let body_top = ay + 14.0 * g.k;
    rule(&dl, w / 2.0, body_top - 6.0 * g.k, (r0 - l0) / 2.0, 140);
    // (a strip at the bottom for "more below")
    let body_bottom = top + panel_h - 40.0 * g.k;
    let view_h = (body_bottom - body_top).max(1.0);
    let max_scroll = (content_h - view_h).max(0.0);
    MOVES_MAX_SCROLL.store((max_scroll / g.k).ceil() as u32, std::sync::atomic::Ordering::Relaxed);
    let scroll = (v.scroll * g.k).min(max_scroll);
    // whole rows only: a row that doesn't fit the view waits for the scroll
    let visible = |ry: f32, rh: f32| ry - scroll >= -0.5 && ry - scroll + rh <= view_h + 0.5;
    for (row, ry, rh) in &rows {
        if !visible(*ry, *rh) {
            continue;
        }
        let y = body_top + ry - scroll;
        match row {
            Row::Head(t) => {
                text(ui, &dl, fonts.small, [l0, y + 12.0 * g.k], col(GOLD, 255), t);
            }
            Row::Move { idx, name, note, below } => {
                let l = &sections[idx.0].lines[idx.1];
                let x = if *below { l0 } else { words_x };
                let mut yy = y + if *below { 44.0 * g.k } else { 0.0 };
                let gold = l.buttons.is_empty();
                for (i, n) in name.iter().enumerate() {
                    text(ui, &dl, fonts.text, [x, yy], col(if gold && i == 0 { GOLD_HI } else { IVORY }, 255), n);
                    yy += line_n;
                }
                for n in note {
                    text(ui, &dl, fonts.small, [x, yy], col(DIM, 230), n);
                    yy += line_s;
                }
                dl.add_line([l0, y + rh - 6.0 * g.k], [r0, y + rh - 6.0 * g.k], col(BRONZE, 40)).thickness(1.0).build();
            }
        }
    }
    if scroll > 0.5 {
        text_centered(ui, &dl, fonts.small, w / 2.0, body_top + 2.0 * g.k, col(GOLD, 200), "more above");
    }
    // 1.3.0: the "sort by" dropdown at the tab row's right end (X or a click opens it)
    let mut sort_rects: Vec<([f32; 2], [f32; 2], u8)> = Vec::new();
    if v.tab == 0 {
        let label = format!("SORT BY:  {}", if by_weapon { "WEAPON" } else { "FIGHTER" });
        let ts = {
            let _f = ui.push_font(fonts.small);
            ui.calc_text_size(&label)
        };
        let (bw, bh) = (ts[0] + 44.0 * g.k, ts[1] + 14.0 * g.k);
        let (bx, by) = (r0 - bw, tab_y - 6.0 * g.k);
        dl.add_rect([bx, by], [bx + bw, by + bh], col([16, 14, 11], 235)).filled(true).build();
        dl.add_rect([bx, by], [bx + bw, by + bh], col(if v.sort_open { GOLD_HI } else { GOLD }, 220)).thickness(1.5 * g.k.max(0.7)).build();
        text(ui, &dl, fonts.small, [bx + 12.0 * g.k, by + 7.0 * g.k], col(IVORY, 255), &label);
        let (cx, cy, r) = (bx + bw - 16.0 * g.k, by + bh / 2.0, 5.0 * g.k);
        let tri = if v.sort_open { [[cx - r, cy + r * 0.5], [cx + r, cy + r * 0.5], [cx, cy - r * 0.6]] } else { [[cx - r, cy - r * 0.5], [cx + r, cy - r * 0.5], [cx, cy + r * 0.6]] };
        dl.add_triangle(tri[0], tri[1], tri[2], col(GOLD_HI, 255)).filled(true).build();
        sort_rects.push(([bx, by], [bw, bh], 1));
        if v.sort_open {
            for (i, name) in ["FIGHTER", "WEAPON"].iter().enumerate() {
                let oy = by + bh + i as f32 * bh;
                let on = v.sort_pick as usize == i;
                dl.add_rect([bx, oy], [bx + bw, oy + bh], col([24, 21, 16], 245)).filled(true).build();
                if on {
                    dl.add_rect([bx, oy], [bx + bw, oy + bh], col(GOLD, 70)).filled(true).build();
                    diamond(&dl, bx + 12.0 * g.k, oy + bh / 2.0, 4.0 * g.k, col(GOLD_HI, 255));
                }
                text(ui, &dl, fonts.small, [bx + 26.0 * g.k, oy + 7.0 * g.k], col(if on { GOLD_HI } else { IVORY }, 255), name);
                sort_rects.push(([bx, oy], [bw, bh], 2 + i as u8));
            }
            dl.add_rect([bx, by + bh], [bx + bw, by + 3.0 * bh], col(GOLD, 160)).thickness(1.0).build();
        }
    }
    if scroll < max_scroll - 0.5 {
        text_centered(ui, &dl, fonts.small, w / 2.0, body_bottom + 6.0 * g.k, col(GOLD, 200), "more below");
    }
    drop(dl);
    // the dropdown takes mouse clicks (the page's own layer takes none)
    for (at, size, what) in &sort_rects {
        ui.window(format!("##movesort{what}"))
            .position(*at, Condition::Always)
            .size(*size, Condition::Always)
            .flags(WindowFlags::NO_DECORATION | WindowFlags::NO_BACKGROUND | WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SAVED_SETTINGS)
            .build(|| {
                ui.set_cursor_screen_pos(*at);
                if ui.invisible_button(format!("##sortbtn{what}"), *size) {
                    crate::ui::SORT_CLICK.store(*what, std::sync::atomic::Ordering::Relaxed);
                }
            });
    }
    glyph_layer(ui, fonts, w, h, g.k, || {
        // the tabs' buttons: LB before the first, RB after the last
        ui.set_cursor_screen_pos([tab_x[0] - 72.0 * g.k, tab_y - 2.0 * g.k]);
        glyphs::prompt(ui, "menu_build");
        ui.set_cursor_screen_pos([tab_end + 24.0 * g.k, tab_y - 2.0 * g.k]);
        glyphs::prompt(ui, "menu_warnings");
        let mut guard = BTN_W.lock().unwrap();
        let widths = guard.get_or_insert_with(Default::default);
        for (row, ry, rh) in &rows {
            let Row::Move { idx, .. } = row else { continue };
            let l = &sections[idx.0].lines[idx.1];
            if l.buttons.is_empty() {
                widths.insert(key(l), 0.0);
                continue;
            }
            if !visible(*ry, *rh) {
                continue;
            }
            ui.set_cursor_screen_pos([l0, body_top + ry - scroll + 2.0 * g.k]);
            let mut right = l0;
            let mut j = 0;
            let mut first = true;
            while j < l.buttons.len() {
                let b = l.buttons[j];
                let mut n = 1;
                while j + n < l.buttons.len() && l.buttons[j + n] == b {
                    n += 1;
                }
                let reps = if n >= 3 { 1 } else { n };
                for _ in 0..reps {
                    if !first {
                        ui.same_line_with_spacing(0.0, 5.0 * g.k);
                    }
                    glyphs::game_btn(ui, b);
                    right = right.max(ui.item_rect_max()[0]);
                    first = false;
                }
                if n >= 3 {
                    ui.same_line_with_spacing(0.0, 6.0 * g.k);
                    let p = ui.cursor_screen_pos();
                    ui.set_cursor_screen_pos([p[0], p[1] + 4.0 * g.k]);
                    ui.text_colored([0.93, 0.78, 0.38, 1.0], format!("x{n}"));
                    right = right.max(ui.item_rect_max()[0]);
                }
                j += n;
            }
            widths.insert(key(l), (right - l0) / g.k);
        }
        drop(guard);
        let side = if by_weapon { "weapon" } else { "fighter" };
        let mut hints = vec![("menu_left", ""), ("menu_right", side), ("menu_up", ""), ("menu_down", "scroll")];
        if v.tab == 0 {
            hints.push(("menu_difficulty", "sort by"));
        }
        hints.push(("menu_back", "back"));
        prompts(ui, g, h, &hints);
    });
}
