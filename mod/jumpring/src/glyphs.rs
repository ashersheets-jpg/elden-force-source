//! Button prompts drawn for the device the player used last (hooks.input). Bindings come from the inputs
//! sheet or the player's own (Controls page).
//!
//! 1.1.0 (user: nicer than flat grey / green / red / blue / yellow buttons, in the game's look): every
//! glyph is drawn like Elden Ring's own menus, a dark medallion or plate with a thin gold rim and ivory
//! type. Face buttons keep a muted hint of their colour as an inner ring; the shoulder buttons are
//! angled plates, the triggers taller ones, the sticks a ringed thumbstick, the d-pad a small cross with
//! the pressed arm lit. On keyboard and mouse: dark keycaps with a bronze bevel, and a mouse with the
//! pressed button lit.

use hudhook::imgui::{DrawListMut, ImColor32, Ui};

use crate::binds::{self, Device};

/// What a glyph looks like.
#[derive(Clone, Copy)]
enum Look {
    /// a face button: round medallion with an inner ring in this (muted) colour
    Face([u8; 3]),
    /// LB / RB: an angled plate
    Bumper,
    /// LT / RT: a taller plate, rounded on top
    Trigger,
    /// L3 / R3: a ringed thumbstick
    Stick,
    /// a d-pad arm: 0 up, 1 down, 2 left, 3 right
    Dpad(u8),
    /// View / Menu: a small pill
    Pill,
}

const PAD: [(u16, &str, Look); 16] = [
    // modifiers first, so a chord reads "L3 + A"
    (0x0040, "L", Look::Stick),
    (0x0100, "LB", Look::Bumper),
    (0x0200, "RB", Look::Bumper),
    (crate::binds::LT, "LT", Look::Trigger),
    (crate::binds::RT, "RT", Look::Trigger),
    (0x1000, "A", Look::Face([92, 150, 84])),
    (0x2000, "B", Look::Face([170, 70, 62])),
    (0x4000, "X", Look::Face([78, 118, 176])),
    (0x8000, "Y", Look::Face([196, 160, 72])),
    (0x0080, "R", Look::Stick),
    (0x0020, "View", Look::Pill),
    (0x0010, "Menu", Look::Pill),
    (0x0001, "", Look::Dpad(0)),
    (0x0002, "", Look::Dpad(1)),
    (0x0004, "", Look::Dpad(2)),
    (0x0008, "", Look::Dpad(3)),
];

const GOLD: [u8; 3] = [206, 170, 96];
const GOLD_HI: [u8; 3] = [240, 214, 150];
const BRONZE: [u8; 3] = [120, 98, 62];
const IVORY: [u8; 3] = [236, 228, 208];
const INK: [u8; 3] = [16, 14, 12];

fn c(rgb: [u8; 3], a: u8) -> ImColor32 {
    ImColor32::from_rgba(rgb[0], rgb[1], rgb[2], a)
}

pub fn key_name(vk: i32) -> String {
    match vk {
        0x01 => "Mouse 1".into(),
        0x02 => "Mouse 2".into(),
        0x04 => "Mouse 3".into(),
        0x05 => "Mouse 4".into(),
        0x06 => "Mouse 5".into(),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x14 => "Caps".into(),
        0x20 => "Space".into(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0xA0 => "L Shift".into(),
        0xA1 => "R Shift".into(),
        0xA2 => "L Ctrl".into(),
        0xA3 => "R Ctrl".into(),
        0xA4 => "L Alt".into(),
        0xA5 => "R Alt".into(),
        0xC0 => "`".into(),
        0xBA => ";".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBD => "-".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        0xDB => "[".into(),
        0xDC => "\\".into(),
        0xDD => "]".into(),
        0xDE => "'".into(),
        0x0D => "Enter".into(),
        0x1B => "Esc".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x30..=0x39 | 0x41..=0x5A => ((vk as u8) as char).to_string(),
        _ => format!("{vk:#x}"),
    }
}

/// A small filled triangle pointing `dir` (0 up, 1 down, 2 left, 3 right) centred on `m`.
fn arrow(dl: &DrawListMut, m: [f32; 2], r: f32, dir: u8, col: ImColor32) {
    let (a, b, cc) = match dir {
        0 => ([m[0], m[1] - r], [m[0] + r, m[1] + r * 0.6], [m[0] - r, m[1] + r * 0.6]),
        1 => ([m[0], m[1] + r], [m[0] - r, m[1] - r * 0.6], [m[0] + r, m[1] - r * 0.6]),
        2 => ([m[0] - r, m[1]], [m[0] + r * 0.6, m[1] - r], [m[0] + r * 0.6, m[1] + r]),
        _ => ([m[0] + r, m[1]], [m[0] - r * 0.6, m[1] + r], [m[0] - r * 0.6, m[1] - r]),
    };
    dl.add_triangle(a, b, cc, col).filled(true).build();
}

/// A keycap: a dark key with a bronze bevel, a lit top edge and ivory type. Arrow keys show an arrow.
fn keycap(ui: &Ui, label: &str) {
    let [tw, th] = ui.calc_text_size(label);
    let p = ui.cursor_screen_pos();
    let h = th + 8.0;
    let arrow_key = matches!(label, "Up" | "Down" | "Left" | "Right");
    let w = if arrow_key { h } else { (tw + 14.0).max(h) };
    let dl = ui.get_window_draw_list();
    let (a, b) = (p, [p[0] + w, p[1] + h]);
    // depth: a darker base a little lower, then the key face
    dl.add_rect([a[0], a[1] + 2.0], [b[0], b[1] + 2.0], c([6, 5, 4], 230)).rounding(5.0).filled(true).build();
    dl.add_rect_filled_multicolor([a[0] + 1.0, a[1] + 1.0], [b[0] - 1.0, b[1] - 1.0], c([46, 41, 34], 240), c([46, 41, 34], 240), c([22, 20, 17], 240), c([22, 20, 17], 240));
    dl.add_rect(a, b, c(BRONZE, 255)).rounding(5.0).thickness(1.2).build();
    dl.add_line([a[0] + 5.0, a[1] + 1.6], [b[0] - 5.0, a[1] + 1.6], c(GOLD_HI, 90)).thickness(1.0).build();
    if arrow_key {
        let dir = match label { "Up" => 0, "Down" => 1, "Left" => 2, _ => 3 };
        arrow(&dl, [a[0] + w / 2.0, a[1] + h / 2.0], h * 0.2, dir, c(IVORY, 255));
    } else {
        dl.add_text([a[0] + (w - tw) / 2.0, a[1] + 4.0], c(IVORY, 255), label);
    }
    ui.dummy([w, h + 2.0]);
}

/// A mouse: a dark rounded body with a gold rim, the pressed button lit (side buttons as a numbered
/// notch on its flank).
fn mouse(ui: &Ui, button: u8) {
    let [_, th] = ui.calc_text_size("M");
    let p = ui.cursor_screen_pos();
    let (w, h) = ((th + 8.0) * 0.72, th + 8.0);
    let dl = ui.get_window_draw_list();
    let (a, b) = (p, [p[0] + w, p[1] + h]);
    dl.add_rect(a, b, c([28, 25, 21], 240)).rounding(w * 0.48).filled(true).build();
    let mid = a[0] + w / 2.0;
    let split = a[1] + h * 0.42;
    match button {
        1 | 2 => {
            let (x0, x1) = if button == 1 { (a[0] + 1.5, mid) } else { (mid, b[0] - 1.5) };
            dl.add_rect([x0, a[1] + 1.5], [x1, split], c(GOLD, 220)).rounding(w * 0.4).filled(true).build();
        }
        3 => {
            dl.add_rect([mid - 2.0, a[1] + h * 0.12], [mid + 2.0, a[1] + h * 0.34], c(GOLD_HI, 255)).rounding(2.0).filled(true).build();
        }
        _ => {}
    }
    dl.add_line([mid, a[1] + 2.0], [mid, split], c(BRONZE, 255)).thickness(1.0).build();
    dl.add_line([a[0] + 2.0, split], [b[0] - 2.0, split], c(BRONZE, 255)).thickness(1.0).build();
    dl.add_rect(a, b, c(GOLD, 255)).rounding(w * 0.48).thickness(1.2).build();
    if button >= 4 {
        let n = (button - 1).to_string();
        let [nw, _] = ui.calc_text_size(&n);
        dl.add_rect([a[0] - 3.0, a[1] + h * 0.5], [a[0] + 2.0, a[1] + h * 0.72], c(GOLD_HI, 255)).filled(true).build();
        drop(dl);
        ui.dummy([w, h]);
        ui.same_line_with_spacing(0.0, 3.0);
        let q = ui.cursor_screen_pos();
        ui.get_window_draw_list().add_text([q[0], q[1] + 4.0], c(GOLD_HI, 255), &n);
        ui.dummy([nw, h]);
        return;
    }
    ui.dummy([w, h]);
}

/// One controller glyph.
fn pad_glyph(ui: &Ui, label: &str, look: Look) {
    let [tw, th] = ui.calc_text_size(label);
    let p = ui.cursor_screen_pos();
    let d = th + 8.0;
    let dl = ui.get_window_draw_list();
    let rim = c(GOLD, 255);
    let size = match look {
        Look::Face(hue) => {
            let (cx, cy, r) = (p[0] + d / 2.0, p[1] + d / 2.0, d / 2.0);
            dl.add_circle([cx, cy + 1.5], r, c([4, 4, 3], 200)).filled(true).num_segments(32).build();
            dl.add_circle([cx, cy], r, c([30, 27, 23], 245)).filled(true).num_segments(32).build();
            dl.add_circle([cx, cy - r * 0.18], r * 0.72, c([48, 43, 36], 120)).filled(true).num_segments(32).build();
            dl.add_circle([cx, cy], r - 3.2, c(hue, 230)).num_segments(32).thickness(2.0).build();
            dl.add_circle([cx, cy], r, rim).num_segments(32).thickness(1.4).build();
            dl.add_text([cx - tw / 2.0, cy - th / 2.0], c(IVORY, 255), label);
            [d, d]
        }
        Look::Bumper => {
            let w = tw + 18.0;
            let h = d * 0.82;
            let y0 = p[1] + (d - h) / 2.0;
            let lean = h * 0.35;
            let left = label.starts_with('L');
            let pts = if left {
                [[p[0] + lean, y0], [p[0] + w, y0], [p[0] + w, y0 + h], [p[0], y0 + h]]
            } else {
                [[p[0], y0], [p[0] + w - lean, y0], [p[0] + w, y0 + h], [p[0], y0 + h]]
            };
            dl.add_polyline(pts.to_vec(), c([30, 27, 23], 245)).filled(true).build();
            dl.add_polyline(vec![pts[0], pts[1], pts[2], pts[3], pts[0]], rim).thickness(1.4).build();
            dl.add_text([p[0] + (w - tw) / 2.0, p[1] + (d - th) / 2.0], c(IVORY, 255), label);
            [w, d]
        }
        Look::Trigger => {
            let w = tw + 14.0;
            let (a, b) = ([p[0], p[1] - 1.0], [p[0] + w, p[1] + d + 1.0]);
            dl.add_rect(a, b, c([30, 27, 23], 245)).rounding(w * 0.42).round_bot_left(false).round_bot_right(false).filled(true).build();
            dl.add_rect(a, b, rim).rounding(w * 0.42).round_bot_left(false).round_bot_right(false).thickness(1.4).build();
            dl.add_line([a[0] + 3.0, b[1] - 3.0], [b[0] - 3.0, b[1] - 3.0], c(BRONZE, 255)).thickness(1.0).build();
            dl.add_text([p[0] + (w - tw) / 2.0, p[1] + (d - th) / 2.0 - 1.0], c(IVORY, 255), label);
            [w, d]
        }
        Look::Stick => {
            let (cx, cy, r) = (p[0] + d / 2.0, p[1] + d / 2.0, d / 2.0);
            dl.add_circle([cx, cy], r, c([30, 27, 23], 245)).filled(true).num_segments(32).build();
            dl.add_circle([cx, cy], r, rim).num_segments(32).thickness(1.4).build();
            dl.add_circle([cx, cy], r * 0.62, c(BRONZE, 255)).num_segments(28).thickness(1.2).build();
            // the press: a small notch under the cap
            arrow(&dl, [cx, cy + r * 0.82], r * 0.16, 1, c(GOLD_HI, 255));
            dl.add_text([cx - tw / 2.0, cy - th / 2.0 - 1.0], c(IVORY, 255), label);
            [d, d]
        }
        Look::Dpad(arm) => {
            let (cx, cy) = (p[0] + d / 2.0, p[1] + d / 2.0);
            let (l, t) = (d * 0.5, d * 0.17);
            for (k, (x0, y0, x1, y1)) in [(cx - t, cy - l, cx + t, cy - t), (cx - t, cy + t, cx + t, cy + l), (cx - l, cy - t, cx - t, cy + t), (cx + t, cy - t, cx + l, cy + t)].iter().enumerate() {
                let lit = k as u8 == arm;
                dl.add_rect([*x0, *y0], [*x1, *y1], if lit { c(GOLD, 255) } else { c([30, 27, 23], 245) }).filled(true).build();
            }
            dl.add_rect([cx - t, cy - t], [cx + t, cy + t], c([30, 27, 23], 245)).filled(true).build();
            for (x0, y0, x1, y1) in [(cx - t, cy - l, cx + t, cy + l), (cx - l, cy - t, cx + l, cy + t)] {
                dl.add_rect([x0, y0], [x1, y1], c(BRONZE, 255)).thickness(1.0).build();
            }
            [d, d]
        }
        Look::Pill => {
            let w = tw + 16.0;
            let h = d * 0.8;
            let y0 = p[1] + (d - h) / 2.0;
            dl.add_rect([p[0], y0], [p[0] + w, y0 + h], c([30, 27, 23], 245)).rounding(h / 2.0).filled(true).build();
            dl.add_rect([p[0], y0], [p[0] + w, y0 + h], rim).rounding(h / 2.0).thickness(1.2).build();
            dl.add_text([p[0] + (w - tw) / 2.0, p[1] + (d - th) / 2.0], c(IVORY, 255), label);
            [w, d]
        }
    };
    let _ = INK;
    ui.dummy(size);
}

/// Draw the prompt for binding `id` (sheet: inputs, as currently bound) inline, for the current device.
pub fn prompt(ui: &Ui, id: &str) {
    prompt_for(ui, id, binds::device());
}

/// The chord joiner: a small gold plus.
fn plus(ui: &Ui) {
    ui.same_line_with_spacing(0.0, 4.0);
    let [_, th] = ui.calc_text_size("+");
    let p = ui.cursor_screen_pos();
    let (cx, cy, r) = (p[0] + 5.0, p[1] + (th + 8.0) / 2.0, 4.0);
    let dl = ui.get_window_draw_list();
    dl.add_line([cx - r, cy], [cx + r, cy], c(GOLD, 220)).thickness(1.6).build();
    dl.add_line([cx, cy - r], [cx, cy + r], c(GOLD, 220)).thickness(1.6).build();
    drop(dl);
    ui.dummy([10.0, th + 8.0]);
    ui.same_line_with_spacing(0.0, 4.0);
}

/// The same, for a given device (the Controls page shows both).
pub fn prompt_for(ui: &Ui, id: &str, device: Device) {
    let i = binds::row(id);
    ui.group(|| match device {
        Device::KeyboardMouse => {
            let vk = binds::key(i);
            match vk {
                0x01 => mouse(ui, 1),
                0x02 => mouse(ui, 2),
                0x04 => mouse(ui, 3),
                0x05 => mouse(ui, 4),
                0x06 => mouse(ui, 5),
                _ => keycap(ui, &key_name(vk)),
            }
        }
        Device::Controller => {
            let mut first = true;
            let chord = binds::pad(i);
            for (bit, name, look) in PAD {
                if chord & bit != 0 {
                    if !first {
                        plus(ui);
                    }
                    pad_glyph(ui, name, look);
                    first = false;
                }
            }
        }
    });
}

/// A row of prompts with labels: [(binding, "label"), ...].
pub fn hints(ui: &Ui, items: &[(&str, &str)]) {
    for (i, (id, label)) in items.iter().enumerate() {
        if i > 0 {
            ui.same_line_with_spacing(0.0, if label.is_empty() || items[i - 1].1.is_empty() { 6.0 } else { 26.0 });
        }
        prompt(ui, id);
        if !label.is_empty() {
            ui.same_line_with_spacing(0.0, 8.0);
            let p = ui.cursor_screen_pos();
            let [_, th] = ui.calc_text_size(label);
            ui.set_cursor_screen_pos([p[0], p[1] + 4.0]);
            ui.text_colored([0.91, 0.88, 0.80, 1.0], label);
            let _ = th;
        }
    }
}

/// 1.2.0: one button of the Move List (combos::Btn): the game's own attack, jump and dodge buttons as
/// the pad glyphs (or, on keyboard and mouse, a keycap with the game's name for the action: the player
/// may have moved it), "hold" before a held one, and the swing as its own binding.
pub fn game_btn(ui: &Ui, b: crate::combos::Btn) {
    use crate::combos::Btn;
    let pad = binds::device() == Device::Controller;
    ui.group(|| match b {
        Btn::Hold(inner) => {
            let p = ui.cursor_screen_pos();
            ui.set_cursor_screen_pos([p[0], p[1] + 4.0]);
            ui.text_colored([0.72, 0.66, 0.52, 1.0], "hold");
            ui.same_line_with_spacing(0.0, 6.0);
            game_btn(ui, *inner);
        }
        Btn::Swing => prompt_for(ui, "special", binds::device()),
        _ if pad => {
            let (name, look) = match b {
                Btn::Light => ("RB", Look::Bumper),
                Btn::Heavy => ("RT", Look::Trigger),
                Btn::Jump => ("A", Look::Face([92, 150, 84])),
                Btn::Dodge => ("B", Look::Face([170, 70, 62])),
                Btn::Left => ("LB", Look::Bumper),
                Btn::TwoHand => ("Y", Look::Face([200, 170, 60])),
                Btn::Skill => ("LT", Look::Trigger),
                _ => ("L", Look::Stick),
            };
            pad_glyph(ui, name, look);
        }
        _ => {
            let name = match b {
                Btn::Light => "Attack",
                Btn::Heavy => "Strong Attack",
                Btn::Jump => "Jump",
                Btn::Dodge => "Dodge",
                Btn::Left => "Guard",
                Btn::TwoHand => "Two-Hand",
                Btn::Skill => "Skill",
                _ => "Crouch",
            };
            keycap(ui, name);
        }
    });
}
