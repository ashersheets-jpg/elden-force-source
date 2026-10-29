//! hooks.overlay: fighter select, Your Tarnished loadout, grace prompt and the Ki/Awakening HUD,
//! drawn with Dear ImGui into Elden Ring's DX12 frame (hudhook). Reads ui::UI; never touches game state.


use hudhook::ImguiRenderLoop;
use hudhook::hooks::dx12::ImguiDx12Hooks;
use hudhook::imgui::{self, Condition, FontId, ImColor32, StyleColor, StyleVar, Ui, WindowFlags};

use crate::combat::AWAKENING_FULL;
use crate::glyphs;
use crate::log;
use crate::sheets;
use crate::ui::{self, Page, UI};

const GOLD: [f32; 4] = [0.93, 0.78, 0.38, 1.0];
const DIM: [f32; 4] = [0.65, 0.65, 0.65, 1.0];
const KI: [f32; 4] = [0.35, 0.75, 1.0, 1.0];
const AWAKE: [f32; 4] = [1.0, 0.55, 0.15, 1.0];

struct Overlay {
    fonts: Option<crate::select_screen::Fonts>,
    hud: Option<HudFonts>,
    portraits: crate::portraits::Portraits,
    /// the wind streaks' current strength (eased)
    wind: f32,
}

/// Elden Ring-style type for the in-world HUD: Cinzel (titles) and Cormorant Garamond (text), both OFL.
struct HudFonts {
    name: FontId,
    text: FontId,
    small: FontId,
}

// FontId wraps a pointer into ImGui's font atlas; hudhook only uses the overlay from its render thread.
unsafe impl Send for Overlay {}
unsafe impl Sync for Overlay {}

static BODY_TTF: &[u8] = include_bytes!("../assets/Roboto-BoldCondensed.ttf");
static TITLE_TTF: &[u8] = include_bytes!("../assets/Roboto-BlackItalic.ttf");
static CINZEL_TTF: &[u8] = include_bytes!("../assets/Cinzel-SemiBold.ttf");
static CORMORANT_TTF: &[u8] = include_bytes!("../assets/CormorantGaramond-SemiBold.ttf");

/// 2.0.3 cinematic mode (the user's trailer: clean, film-like shots): the Home key hides everything of ours drawn over the
/// world (the fighter box, ability list, combo counter, villain bars, banners and notices); the effects that belong to the
/// shot stay (Gon's line, the swing cords, the lightning). Elden Ring's own HUD is turned off in its settings.
pub static CINEMATIC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl ImguiRenderLoop for Overlay {
    fn initialize<'a>(&'a mut self, ctx: &mut imgui::Context, rc: &'a mut dyn hudhook::RenderContext) {
        // (lab, 2.0.7 title freeze: "notextures.on" beside the logs: no texture uploads (hudhook's own upload queue))
        #[cfg(feature = "lab")]
        let skip = crate::log::dir().join("notextures.on").exists();
        #[cfg(not(feature = "lab"))]
        let skip = false;
        if skip {
            log!("overlay: (lab) no textures");
        } else {
            self.portraits = crate::portraits::Portraits::load(rc);
            crate::world_fx::load_textures(rc);
        }
        ctx.set_ini_filename(None);
        // Roboto (Apache-2.0): condensed bold for text, black italic for JUMP FORCE-style titles.
        let font = |ctx: &mut imgui::Context, data: &'static [u8], px: f32| {
            ctx.fonts().add_font(&[imgui::FontSource::TtfData { data, size_pixels: px, config: None }])
        };
        let body = font(ctx, BODY_TTF, 26.0);
        let title = font(ctx, TITLE_TTF, 40.0);
        let huge = font(ctx, TITLE_TTF, 96.0);
        self.fonts = Some(crate::select_screen::Fonts {
            body,
            title,
            huge,
            c_huge: font(ctx, CINZEL_TTF, 64.0),
            c_title: font(ctx, CINZEL_TTF, 46.0),
            c_name: font(ctx, CINZEL_TTF, 23.0),
            text: font(ctx, CORMORANT_TTF, 29.0),
            small: font(ctx, CORMORANT_TTF, 23.0),
        });
        self.hud = Some(HudFonts { name: font(ctx, CINZEL_TTF, 30.0), text: font(ctx, CORMORANT_TTF, 25.0), small: font(ctx, CORMORANT_TTF, 21.0) });
        let style = ctx.style_mut();
        style.window_rounding = 4.0;
        style.window_border_size = 1.0;
        style[StyleColor::WindowBg] = [0.04, 0.04, 0.05, 0.86];
        style[StyleColor::Border] = [0.93, 0.78, 0.38, 0.45];
        READY.store(true, std::sync::atomic::Ordering::Relaxed);
        log!("overlay: ready");
        // (lab, 2.0.7 title freeze: the GPU never finished the game's frame 7; which game frame is the overlay's first?)
        #[cfg(feature = "lab")]
        unsafe {
            let base = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) as usize;
            let gfx = *((base + 0x47f6400) as *const usize);
            if gfx != 0 {
                log!("lab overlay: first drawn in game frame {}", *((gfx + 0xe80) as *const u64));
            }
        }
        log_hookers();
    }

    fn render(&mut self, ui: &mut Ui) {
        let _p = crate::perf::span("overlay");
        let Ok(s) = UI.lock() else { return };
        let [w, h] = ui.io().display_size;
        let _body = self.fonts.as_ref().map(|f| ui.push_font(f.body));
        if let Some(menu) = &s.menu {
            match (menu.page, &self.fonts) {
                (Page::Roster, Some(fonts)) => {
                    let id = sheets::FIGHTERS[menu.cursor.min(sheets::FIGHTERS.len() - 1)].id;
                    crate::select_screen::draw(ui, &s, id, fonts, &self.portraits, w, h);
                }
                (_, Some(fonts)) => crate::menu_pages::draw(ui, &s, menu, fonts, &self.portraits, w, h),
                _ => {}
            }
        } else if s.hud.in_world && s.hud.game_menu && !s.hud.at_grace {
            // 1.1.0 (user): Elden Ring's pause menu or map is open: nothing of ours over it, except (1.3.0, user: "a
            // lot of people did not see the move set screen ... make that a clickable option on our pause screen")
            // a Move List button in the pause menu's corner
            if crate::pause_menu_open() {
                if let Some(f) = &self.hud {
                    draw_pause_moves(ui, f, w, h);
                }
            }
        } else if s.hud.in_world && CINEMATIC.load(std::sync::atomic::Ordering::Relaxed) {
            crate::world_fx::draw(ui, w, h, self.hud.as_ref().map(|f| f.name), crate::world_fx::Show { text: false, visual: s.warnings.visual(), opacity: s.warn_opacity as f32 / 10.0 });
            draw_fx(ui, &s.fx, &mut self.wind, w, h);
            return;
        } else if s.hud.in_world {
            crate::world_fx::draw(ui, w, h, self.hud.as_ref().map(|f| f.name), crate::world_fx::Show { text: s.warnings.text(), visual: s.warnings.visual(), opacity: s.warn_opacity as f32 / 10.0 });
            draw_fx(ui, &s.fx, &mut self.wind, w, h);
            if let (Some(f), false) = (&self.hud, s.hud.at_grace) {
                draw_combo(ui, f, &s.fx, w, h);
                draw_villain_bars(ui, f, w, h);
            }
            // 0.15.0: resting at a grace, the HUD steps aside so the grace's menus and Melina's words
            // can be read: no fighter box or ability list, and the JUMP FORCE prompt up at the right
            if !s.hud.at_grace {
                if let Some((_, face)) = self.portraits.by_id.get(s.hud.fighter_id) {
                    crate::select_screen::hud_face(ui, *face, w, h);
                }
            }
            if let Some(f) = &self.hud {
                if !s.hud.at_grace {
                    draw_hud(ui, &s, w, h, f);
                }
                if s.hud.at_grace {
                    let _t = ui.push_font(f.text);
                    ui.window("##grace")
                        .position([w - 40.0 * (h / 1080.0), 40.0 * (h / 1080.0)], Condition::Always)
                        .position_pivot([1.0, 0.0])
                        .bg_alpha(0.0)
                        .flags(WindowFlags::NO_DECORATION | WindowFlags::ALWAYS_AUTO_RESIZE | WindowFlags::NO_INPUTS)
                        .build(|| glyphs::hints(ui, &[("menu_open", "Elden Force: fighter and build")]));
                }
            }
        }
        if s.menu.is_none() && s.hud.in_world && !s.hud.game_menu {
            if let Some(f) = &self.hud {
                draw_training(ui, f, w, h);
            }
        }
        if let Some((text, at)) = &s.notice {
            let t = at.elapsed().as_secs_f32();
            if t < 6.0 {
                if let Some(f) = &self.hud {
                    draw_notice(ui, f, text, t, w, h);
                }
            }
        }
    }
}

/// Screen effects: the parry (a white flash, a ring of blue light racing out from the centre, and dark
/// blue edges while the game is slowed) and faint wind streaks along the edges when the fighter
/// sprints, strikes or chains a combo. All drawn behind the HUD, never over the middle of the screen.
fn draw_fx(ui: &Ui, fx: &ui::Fx, wind: &mut f32, w: f32, h: f32) {
    let dl = ui.get_background_draw_list();
    let (cx, cy) = (w * 0.5, h * 0.5);
    let diag = (w * w + h * h).sqrt() * 0.5;
    let now = std::time::Instant::now();
    if let Some(until) = fx.slow_until {
        if now < until {
            let left = until.duration_since(now).as_secs_f32();
            let a = (left / 0.4).min(1.0) * 0.35;
            let edge = rgba([0.05, 0.12, 0.30, 1.0], a);
            let clear = rgba([0.05, 0.12, 0.30, 1.0], 0.0);
            let band = w.min(h) * 0.22;
            dl.add_rect_filled_multicolor([0.0, 0.0], [w, band], edge, edge, clear, clear);
            dl.add_rect_filled_multicolor([0.0, h - band], [w, h], clear, clear, edge, edge);
            dl.add_rect_filled_multicolor([0.0, 0.0], [band, h], edge, clear, clear, edge);
            dl.add_rect_filled_multicolor([w - band, 0.0], [w, h], clear, edge, edge, clear);
        }
    }
    if let Some((at, secs, c)) = fx.time_stop {
        // 1.3.0: The World's stopped time (the anime drains the world's colour and floods it with one cold tone the
        // moment time stops, then gives it back when time moves again): a flash of the Stand's colour as the sphere
        // bursts, then the whole screen washed in a dim blue-grey with the Stand's colour gathered at its edges,
        // fading out over the last third of a second
        let t = now.duration_since(at).as_secs_f32();
        if t < secs + 0.35 {
            let fade_in = (t / 0.25).min(1.0);
            let fade_out = ((secs + 0.35 - t) / 0.35).min(1.0);
            let a = fade_in * fade_out;
            if t < 0.3 {
                let f = 0.4 * (1.0 - t / 0.3);
                let fc = rgba([c[0] * 0.5 + 0.5, c[1] * 0.5 + 0.5, c[2] * 0.5 + 0.5, 1.0], f);
                dl.add_rect_filled_multicolor([0.0, 0.0], [w, h], fc, fc, fc, fc);
            }
            let wash = rgba([0.16, 0.2, 0.3, 1.0], 0.34 * a);
            dl.add_rect_filled_multicolor([0.0, 0.0], [w, h], wash, wash, wash, wash);
            let edge = rgba([c[0] * 0.45, c[1] * 0.45, c[2] * 0.45, 1.0], 0.45 * a);
            let clear = rgba([c[0] * 0.45, c[1] * 0.45, c[2] * 0.45, 1.0], 0.0);
            let band = w.min(h) * 0.3;
            dl.add_rect_filled_multicolor([0.0, 0.0], [w, band], edge, edge, clear, clear);
            dl.add_rect_filled_multicolor([0.0, h - band], [w, h], clear, clear, edge, edge);
            dl.add_rect_filled_multicolor([0.0, 0.0], [band, h], edge, clear, clear, edge);
            dl.add_rect_filled_multicolor([w - band, 0.0], [w, h], clear, edge, edge, clear);
        }
    }
    if let Some(at) = fx.parry_at {
        // 0.11.0: a short soft flash and one clean ring of light (no line sparks)
        let t = now.duration_since(at).as_secs_f32();
        if t < 0.12 {
            let a = 0.32 * (1.0 - t / 0.12);
            let c = rgba([1.0; 4], a);
            dl.add_rect_filled_multicolor([0.0, 0.0], [w, h], c, c, c, c);
        }
        if t < 0.5 {
            let u = t / 0.5;
            let r = diag * (0.06 + 0.9 * (1.0 - (1.0 - u) * (1.0 - u)));
            let a = (1.0 - u) * (1.0 - u);
            dl.add_circle([cx, cy], r, rgba([0.70, 0.90, 1.0, 1.0], 0.35 * a)).num_segments(128).thickness(14.0 * (1.0 - u) + 2.0).build();
            dl.add_circle([cx, cy], r, rgba([1.0, 1.0, 1.0, 1.0], 0.7 * a)).num_segments(128).thickness(2.0).build();
        }
    }
    // 0.11.0: the screen-edge wind streaks are gone (the dash ribbon in world_fx.rs replaces them)
    let _ = wind;
}

fn rgba(c: [f32; 4], a: f32) -> ImColor32 {
    ImColor32::from_rgba_f32s(c[0], c[1], c[2], c[3] * a)
}

const IVORY: [f32; 4] = [0.93, 0.89, 0.80, 1.0];
const FRAME: [f32; 4] = [0.62, 0.55, 0.38, 1.0];
const SHADOW: [f32; 4] = [0.0, 0.0, 0.0, 0.75];

/// Text with a soft shadow, as Elden Ring draws its HUD type. Returns its size.
fn shadowed(ui: &Ui, dl: &imgui::DrawListMut, font: FontId, at: [f32; 2], c: ImColor32, text: &str) -> [f32; 2] {
    let _f = ui.push_font(font);
    dl.add_text([at[0] + 1.5, at[1] + 1.5], rgba(SHADOW, 1.0), text);
    dl.add_text(at, c, text);
    ui.calc_text_size(text)
}

/// The in-world HUD, under Elden Ring's own HP / FP / stamina bars (and its buff icons) and in their
/// style: the fighter's name in carved capitals, a thin gilded Awakening bar, and the four abilities as
/// a short list (button, name, Ki cost or cooldown). Nothing at the bottom of the screen, where
/// subtitles and item messages go.
/// 1.3.0: the pause menu's Move List button, bottom left, in the HUD's manner (click it, or R3 / M).
fn draw_pause_moves(ui: &Ui, f: &HudFonts, w: f32, h: f32) {
    let k = h / 1080.0;
    let x0 = (w - h * 16.0 / 9.0).max(0.0) / 2.0 + 52.0 * k;
    ui.window("##pause_moves")
        // (lab: at h - 150 it covered the menu's OK / Close prompts; this is between "System" and them)
        .position([x0, h - 212.0 * k], Condition::Always)
        .bg_alpha(0.0)
        .flags(WindowFlags::NO_DECORATION | WindowFlags::ALWAYS_AUTO_RESIZE | WindowFlags::NO_SAVED_SETTINGS)
        .build(|| {
            ui.set_window_font_scale(k.clamp(0.6, 2.0));
            let _t = ui.push_font(f.text);
            let _c1 = ui.push_style_color(StyleColor::Button, [0.10, 0.08, 0.05, 0.88]);
            let _c2 = ui.push_style_color(StyleColor::ButtonHovered, [0.36, 0.27, 0.10, 0.95]);
            let _c3 = ui.push_style_color(StyleColor::ButtonActive, [0.55, 0.42, 0.15, 1.0]);
            let _c4 = ui.push_style_color(StyleColor::Text, [0.95, 0.85, 0.58, 1.0]);
            let _c5 = ui.push_style_color(StyleColor::Border, [0.72, 0.58, 0.30, 0.9]);
            let _v = ui.push_style_var(StyleVar::FrameBorderSize(1.2));
            let _p = ui.push_style_var(StyleVar::FramePadding([18.0 * k, 10.0 * k]));
            // (no key hint: the user asked for the button alone, no R3 / M shortcut)
            if ui.button("ELDEN FORCE  MOVE LIST") {
                ui::OPEN_MOVES.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
}

fn draw_hud(ui: &Ui, s: &ui::Shared, w: f32, h: f32, f: &HudFonts) {
    // Elden Ring lays its HUD out on a 16:9 grid scaled to the screen height.
    let k = h / 1080.0;
    let x0 = (w - h * 16.0 / 9.0).max(0.0) / 2.0 + 52.0 * k;
    let y0 = 150.0 * k;
    let bar_w = 300.0 * k;
    let full = s.hud.awakening >= AWAKENING_FULL;
    let pulse = if full { 0.75 + 0.25 * (ui.time() as f32 * 4.0).sin() } else { 1.0 };

    ui.window("##hud")
        .position([x0, y0], Condition::Always)
        .size([bar_w + 260.0 * k, 470.0 * k], Condition::Always)
        .flags(WindowFlags::NO_DECORATION | WindowFlags::NO_INPUTS | WindowFlags::NO_BACKGROUND | WindowFlags::NO_SCROLLBAR)
        .build(|| {
            ui.set_window_font_scale(k.clamp(0.6, 2.0));
            let o = ui.cursor_screen_pos();
            // imgui allows one draw list handle at a time, and the button prompts take their own: every
            // drawing step below gets the list for itself and lets it go.
            let ns = {
                let dl = ui.get_window_draw_list();
                // name and series
                let ns = shadowed(ui, &dl, f.name, o, rgba(IVORY, 1.0), &s.hud.fighter.to_uppercase());
                shadowed(ui, &dl, f.small, [o[0] + ns[0] + 12.0 * k, o[1] + 8.0 * k], rgba(DIM, 1.0), s.hud.series);
                ns
            };

            // Awakening bar: dark track, thin gilded frame, a small diamond cap on the left
            let by = o[1] + ns[1] + 8.0 * k;
            let bh = 7.0 * k;
            {
                let dl = ui.get_window_draw_list();
                let (bx0, bx1) = (o[0] + 14.0 * k, o[0] + bar_w);
                dl.add_rect([bx0, by], [bx1, by + bh], rgba([0.05, 0.04, 0.03, 1.0], 0.85)).filled(true).build();
                let frac = (s.hud.awakening / AWAKENING_FULL).clamp(0.0, 1.0);
                if frac > 0.0 {
                    let top = if full { rgba([1.0, 0.86, 0.45, 1.0], pulse) } else { rgba([0.95, 0.62, 0.22, 1.0], 1.0) };
                    let bot = if full { rgba([0.80, 0.55, 0.15, 1.0], pulse) } else { rgba([0.62, 0.30, 0.08, 1.0], 1.0) };
                    let fx = bx0 + (bx1 - bx0) * frac;
                    dl.add_rect_filled_multicolor([bx0 + 1.0, by + 1.0], [fx - 1.0, by + bh - 1.0], top, top, bot, bot);
                }
                dl.add_rect([bx0, by], [bx1, by + bh], rgba(FRAME, 1.0)).build();
                let (cx, cy, r) = (o[0] + 6.0 * k, by + bh / 2.0, 6.0 * k);
                dl.add_triangle([cx, cy - r], [cx + r, cy], [cx, cy + r], rgba(FRAME, 1.0)).filled(true).build();
                dl.add_triangle([cx, cy - r], [cx - r, cy], [cx, cy + r], rgba(FRAME, 1.0)).filled(true).build();
                let label = if full { "Awakening ready".to_string() } else { format!("Awakening {:.0}%", s.hud.awakening) };
                shadowed(ui, &dl, f.small, [bx1 + 10.0 * k, by - 9.0 * k], if full { rgba(GOLD, pulse) } else { rgba(DIM, 1.0) }, &label);
            }

            // abilities
            let binds = ["ability_1", "ability_2", "ability_3", "ability_4"];
            let mut y = by + bh + 12.0 * k;
            let _t = ui.push_font(f.small);
            for (i, a) in s.hud.abilities.iter().enumerate() {
                let ready = a.cooldown <= 0.0 && s.hud.fp >= a.cost && (!a.ultimate || full);
                ui.set_cursor_screen_pos([o[0] + 2.0 * k, y]);
                glyphs::prompt(ui, binds[i]);
                let gx = ui.item_rect_max()[0] + 10.0 * k;
                let dl = ui.get_window_draw_list();
                let name_c = if a.ultimate && ready { rgba(GOLD, pulse) } else if ready { rgba(IVORY, 1.0) } else { rgba([0.55, 0.53, 0.50, 1.0], 1.0) };
                let mut ns = shadowed(ui, &dl, f.text, [gx, y], name_c, a.name);
                if a.combo {
                    let cs = shadowed(ui, &dl, f.small, [gx + ns[0] + 8.0 * k, y + 3.0 * k], rgba(AWAKE, if ready { 0.95 } else { 0.5 }), "COMBO");
                    ns[0] += cs[0] + 8.0 * k;
                }
                let status = if a.cooldown > 0.0 { format!("{:.0}s", a.cooldown.ceil()) } else { format!("{} Ki", a.cost) };
                let sc = if a.cooldown > 0.0 { rgba(DIM, 1.0) } else if a.ultimate { rgba(AWAKE, 1.0) } else { rgba(KI, 0.9) };
                shadowed(ui, &dl, f.small, [gx + ns[0] + 10.0 * k, y + 3.0 * k], sc, &status);
                y += 36.0 * k;
            }
            // 1.3.0 (user: "add the movement abilities onto the moveset box in the top left during gameplay ... black
            // whip, stretchy arms, the jumps for the characters who have those, the demon sword glider for Asta"):
            // the fighter's own ways of moving, each with its buttons
            let own = crate::combos::own_movement_lines(s.hud.fighter_id);
            if !own.is_empty() {
                y += 4.0 * k;
                {
                    let dl = ui.get_window_draw_list();
                    shadowed(ui, &dl, f.small, [o[0] + 4.0 * k, y], rgba(GOLD, 0.9), "MOVEMENT");
                }
                y += 24.0 * k;
                for l in &own {
                    ui.set_cursor_screen_pos([o[0] + 2.0 * k, y]);
                    let mut first = true;
                    for b in &l.buttons {
                        if !first {
                            ui.same_line_with_spacing(0.0, 4.0 * k);
                        }
                        glyphs::game_btn(ui, *b);
                        first = false;
                    }
                    let gx = if l.buttons.is_empty() { o[0] + 2.0 * k } else { ui.item_rect_max()[0] + 10.0 * k };
                    let dl = ui.get_window_draw_list();
                    shadowed(ui, &dl, f.small, [gx, y + 3.0 * k], rgba(IVORY, 0.92), &l.name);
                    y += 30.0 * k;
                }
            }
            // 1.2.0: the combo trees' finisher (RT to end a string): ready, or its cooldown
            if let Some(cd) = s.fx.finisher_cd {
                let dl = ui.get_window_draw_list();
                let ready = cd <= 0.0;
                let ns = shadowed(ui, &dl, f.small, [o[0] + 4.0 * k, y + 2.0 * k], if ready { rgba(GOLD, 0.95) } else { rgba(DIM, 0.9) }, "FINISHER");
                let st = if ready { "ready: RT to end a string".to_string() } else { format!("{:.0}s", cd.ceil()) };
                shadowed(ui, &dl, f.small, [o[0] + ns[0] + 14.0 * k, y + 2.0 * k], if ready { rgba(IVORY, 0.85) } else { rgba(DIM, 0.9) }, &st);
            }
        });
}

/// 1.3.0 (the user: the villains should "have their own boss health bars"): Elden Ring's boss bar for each villain in the
/// fight (villains::BARS), the way the game shows a second boss: the name over a long thin bar, stacked one row above the game's
/// own bars (at 1080p the first is at y 867, x 459..1462). Crimson health, a pale stretch for what it just lost, a thin gilt
/// frame, a small diamond in the villain's colour by its name.
fn draw_villain_bars(ui: &Ui, f: &HudFonts, w: f32, h: f32) {
    let bars = crate::villains::BARS.lock().unwrap().clone();
    if bars.is_empty() {
        return;
    }
    let k = h / 1080.0;
    let ox = (w - h * 16.0 / 9.0).max(0.0) / 2.0;
    use fromsoftware_shared::FromStatic;
    let game = unsafe { eldenring::cs::CSFeManImp::instance() }.map(|fe| fe.boss_health_displays.iter().filter(|b| b.fmg_id > 0 && !b.field_ins_handle.is_empty()).count()).unwrap_or(0);
    let dl = ui.get_background_draw_list();
    let (x0, x1) = (ox + 459.0 * k, ox + 1462.0 * k);
    for (i, (name, frac, lag, c)) in bars.iter().enumerate() {
        let y = (867.0 - 54.0 * (game + i) as f32) * k;
        let bh = 9.0 * k;
        // the name, a diamond in the villain's colour before it
        let (dx, dy, r) = (x0 + 6.0 * k, y - 16.0 * k, 5.0 * k);
        dl.add_triangle([dx, dy - r], [dx + r, dy], [dx, dy + r], rgba(*c, 1.0)).filled(true).build();
        dl.add_triangle([dx, dy - r], [dx - r, dy], [dx, dy + r], rgba(*c, 1.0)).filled(true).build();
        shadowed(ui, &dl, f.text, [x0 + 18.0 * k, y - 32.0 * k], rgba(IVORY, 1.0), name);
        // track, lag, health, frame
        dl.add_rect([x0, y], [x1, y + bh], rgba([0.03, 0.02, 0.02, 1.0], 0.78)).filled(true).build();
        let lx = x0 + (x1 - x0) * lag.clamp(0.0, 1.0);
        let hx = x0 + (x1 - x0) * frac.clamp(0.0, 1.0);
        if lx > hx {
            dl.add_rect([hx, y + 1.0], [lx, y + bh - 1.0], rgba([0.86, 0.74, 0.42, 1.0], 0.85)).filled(true).build();
        }
        if hx > x0 + 1.0 {
            let top = rgba([0.66, 0.10, 0.08, 1.0], 1.0);
            let bot = rgba([0.40, 0.04, 0.04, 1.0], 1.0);
            dl.add_rect_filled_multicolor([x0 + 1.0, y + 1.0], [hx, y + bh - 1.0], top, top, bot, bot);
        }
        dl.add_rect([x0, y], [x1, y + bh], rgba(FRAME, 0.9)).thickness(1.0).build();
    }
}

/// 1.2.0: the combo counter at the right of the screen, in the HUD's manner: the hit count in tall ivory
/// figures that pop on every hit, HITS beside it, a gold rule fading out to the left and the move's name
/// under it in small capitals; a finisher turns it gold and holds it a moment longer. Fades out 1.6 s
/// after the last hit (2.4 s after a finisher).
fn draw_combo(ui: &Ui, f: &HudFonts, fx: &ui::Fx, w: f32, h: f32) {
    let Some((hits, name, fin, at)) = fx.combo else { return };
    let t = at.elapsed().as_secs_f32();
    let life = if fin { 2.4 } else { 1.6 };
    if (hits < 2 && !fin) || t > life {
        return;
    }
    let k = h / 1080.0;
    let a = ((life - t) / 0.5).clamp(0.0, 1.0);
    // a pop on each new hit
    let pop = 1.0 + 0.35 * (1.0 - (t / 0.14).min(1.0)).powi(2);
    // at the right, under the Training panel's place, clear of Elden Ring's own HUD
    let right = w - (w - h * 16.0 / 9.0).max(0.0) / 2.0 - 90.0 * k;
    let y = h * 0.56;
    let main = if fin { GOLD } else { IVORY };
    let (wx, wy) = (right - 520.0 * k, y - 190.0 * k);
    ui.window("##combo")
        .position([wx, wy], Condition::Always)
        .size([600.0 * k, 280.0 * k], Condition::Always)
        .flags(WindowFlags::NO_DECORATION | WindowFlags::NO_INPUTS | WindowFlags::NO_BACKGROUND | WindowFlags::NO_SCROLLBAR)
        .build(|| {
            let base = k.clamp(0.6, 2.0);
            let label = "HITS";
            ui.set_window_font_scale(base);
            let lw = {
                let _sm = ui.push_font(f.small);
                ui.calc_text_size(label)[0]
            };
            // the figures: real text widgets at the window's scale (the draw list keeps the font's own size)
            ui.set_window_font_scale(base * 3.0 * pop);
            {
                let _big = ui.push_font(f.name);
                let num = format!("{hits}");
                let sz = ui.calc_text_size(&num);
                let (nx, ny) = (right - lw - 14.0 * k - sz[0], y - sz[1] + 14.0 * k);
                ui.set_cursor_screen_pos([nx + 3.0 * k, ny + 3.0 * k]);
                ui.text_colored(shadow_a(0.9 * a), &num);
                ui.set_cursor_screen_pos([nx, ny]);
                ui.text_colored([main[0], main[1], main[2], a], &num);
            }
            ui.set_window_font_scale(base);
            let dl = ui.get_window_draw_list();
            shadowed(ui, &dl, f.small, [right - lw, y - 24.0 * k], rgba(if fin { GOLD } else { DIM }, a), label);
            // a gold rule fading out to the left, the move's name under it in small capitals
            let rl = 260.0 * k;
            let (gold, clear) = (rgba(GOLD, 0.85 * a), rgba(GOLD, 0.0));
            dl.add_rect_filled_multicolor([right - rl, y + 8.0 * k], [right, y + 9.5 * k], clear, gold, gold, clear);
            let caps = name.to_uppercase();
            let cw = {
                let _sm = ui.push_font(f.small);
                ui.calc_text_size(&caps)[0]
            };
            shadowed(ui, &dl, f.small, [right - cw, y + 16.0 * k], rgba(if fin { GOLD } else { IVORY }, 0.9 * a), &caps);
        });
}

fn shadow_a(a: f32) -> [f32; 4] {
    [0.0, 0.0, 0.0, 0.75 * a]
}

/// A message across the top of the screen in Elden Ring's manner: centred capitals between two fading
/// gold rules, fading in and out.
/// 0.16.0: Training: the punching bag's damage panel at the right edge (training.rs).
fn draw_training(ui: &Ui, f: &HudFonts, w: f32, h: f32) {
    let p = crate::training::PANEL.lock().unwrap();
    if !p.on {
        return;
    }
    let k = h / 1080.0;
    let x = w - 470.0 * k;
    let y = 260.0 * k;
    let dl = ui.get_background_draw_list();
    dl.add_rect([x - 16.0 * k, y - 14.0 * k], [w - 30.0 * k, y + (60.0 + 34.0 * p.rows.len() as f32) * k], rgba([0.0, 0.0, 0.0, 0.55], 1.0)).filled(true).build();
    let head = match &p.target {
        Some(t) => format!("TRAINING  punching bag: {t}"),
        None => "TRAINING  lock onto an enemy".to_string(),
    };
    shadowed(ui, &dl, f.text, [x, y], rgba(FRAME, 1.0), &head);
    for (i, r) in p.rows.iter().enumerate() {
        let ry = y + (44.0 + 34.0 * i as f32) * k;
        let c = rgba(IVORY, if i == 0 { 1.0 } else { 0.7 });
        shadowed(ui, &dl, f.text, [x, ry], c, &r.what);
        let n = format!("{}  ({} hit{})", r.damage, r.hits, if r.hits == 1 { "" } else { "s" });
        let _t = ui.push_font(f.text);
        let nw = ui.calc_text_size(&n)[0];
        drop(_t);
        shadowed(ui, &dl, f.text, [w - 46.0 * k - nw, ry], c, &n);
    }
}

fn draw_notice(ui: &Ui, f: &HudFonts, text: &str, t: f32, w: f32, h: f32) {
    let a = (t / 0.6).min((6.0 - t) / 1.0).clamp(0.0, 1.0);
    let k = h / 1080.0;
    let dl = ui.get_foreground_draw_list();
    let _f = ui.push_font(f.name);
    let text = text.to_uppercase();
    let size = ui.calc_text_size(&text);
    let (cx, y) = (w * 0.5, h * 0.16);
    let half = size[0] / 2.0 + 120.0 * k;
    for yy in [y - 12.0 * k, y + size[1] + 12.0 * k] {
        let (clear, gold) = (rgba(GOLD, 0.0), rgba(GOLD, 0.8 * a));
        dl.add_rect_filled_multicolor([cx - half, yy], [cx, yy + 1.5], clear, gold, gold, clear);
        dl.add_rect_filled_multicolor([cx, yy], [cx + half, yy + 1.5], gold, clear, clear, gold);
    }
    dl.add_text([cx - size[0] / 2.0 + 2.0, y + 2.0], rgba(SHADOW, a), &text);
    dl.add_text([cx - size[0] / 2.0, y], rgba(IVORY, a), &text);
}

/// The overlay has drawn its first frame (its fonts and portraits are made).
pub static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 1.3.1 round 2 (the user: "the HUDs never showed up. every bit of our UI was gone"; their log had no "overlay: ready", the
/// hooks in without an error): hudhook says why it can't draw through `tracing`, which went nowhere. Its warnings and errors
/// (and the DX12 setup's debug lines) go into our log, each distinct one at most a few times (some repeat every frame).
struct HudhookLog;

struct Msg(String);
impl tracing::field::Visit for Msg {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        if field.name() == "message" {
            self.0 += &format!("{value:?}");
        } else {
            self.0 += &format!("{}={value:?}", field.name());
        }
    }
}

impl tracing::Subscriber for HudhookLog {
    fn enabled(&self, m: &tracing::Metadata<'_>) -> bool {
        m.target().starts_with("hudhook") && (*m.level() <= tracing::Level::WARN || (*m.level() == tracing::Level::DEBUG && m.target().contains("dx12")))
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        static SEEN: std::sync::Mutex<Vec<(String, u32)>> = std::sync::Mutex::new(Vec::new());
        let mut msg = Msg(String::new());
        e.record(&mut msg);
        let mut seen = SEEN.lock().unwrap();
        let n = match seen.iter().position(|(m, _)| *m == msg.0) {
            Some(i) => {
                seen[i].1 += 1;
                seen[i].1
            }
            None if seen.len() < 200 => {
                seen.push((msg.0.clone(), 1));
                1
            }
            None => return,
        };
        if n <= 3 || n.is_power_of_two() {
            log!("overlay (hudhook {}): {}{}", e.metadata().level(), msg.0, if n > 1 { format!(" (x{n})") } else { String::new() });
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Install the overlay into the game's DX12 swap chain. `hmodule` is this DLL.
pub fn install(hmodule: usize) {
    let _ = tracing::subscriber::set_global_default(HudhookLog);
    // (1.3.1 lab, the startup crash hunt: boots without the overlay)
    #[cfg(feature = "lab")]
    // (or a file "nooverlay.on" beside the log folder: the game is started through Steam, which can drop the variable)
    if std::env::var_os("JUMPRING_NO_OVERLAY").is_some() || crate::log::dir().join("nooverlay.on").exists() {
        log!("overlay: off (JUMPRING_NO_OVERLAY)");
        return;
    }
    // 2.0.7: the overlay starts once the game is well past its start. Started in the game's first frames (its textures
    // uploaded, then its first draws), the GPU never finished the game's 7th frame on 3 of 7 boots after a force-closed
    // game (the title froze; on players' PCs, Windows reset the driver and the game closed or crashed); started 20 s
    // later, 10 of 10 were clean. It now waits for 600 of the game's frames and 20 s from its first (nothing of ours is
    // drawn on the title screen anyway).
    hudhook::jr_state::GATE.store(false, std::sync::atomic::Ordering::SeqCst);
    // (lab: "overlayfallback.on": Present hooked the old way only, from a dummy swap chain, as when the game's was missed)
    #[cfg(feature = "lab")]
    if crate::log::dir().join("overlayfallback.on").exists() {
        hudhook::jr_state::NO_LATE.store(true, std::sync::atomic::Ordering::SeqCst);
        log!("overlay: (lab) hooking from the game's swap chain off");
    }
    #[cfg_attr(not(feature = "lab"), allow(unused_mut))]
    let (mut frames, mut secs, mut from_first) = (600u64, 20u64, true);
    // (lab: "overlaynow.on" beside the logs: at once, as up to 2.0.6; "overlaydelay.on" holding N: N s after now)
    #[cfg(feature = "lab")]
    if crate::log::dir().join("overlaynow.on").exists() {
        (frames, secs, from_first) = (0, 0, false);
        log!("overlay: (lab) at once");
    } else if let Ok(t) = std::fs::read_to_string(crate::log::dir().join("overlaydelay.on")) {
        (frames, secs, from_first) = (0, t.trim().parse().unwrap_or(15), false);
        log!("overlay: (lab) held back {secs} s");
    }
    std::thread::spawn(move || {
        use hudhook::jr_state::{GATE, PRESENTS};
        use std::sync::atomic::Ordering::SeqCst;
        // (hudhook hooks Present when the game makes its swap chain; if that was never seen, once the game has run 600
        // frames: hooked the old way, from a dummy swap chain)
        while PRESENTS.load(SeqCst) == 0 && (from_first || !hudhook::jr_state::LATE_HOOKED.load(SeqCst)) {
            if !hudhook::jr_state::LATE_HOOKED.load(SeqCst) && crate::watchdog::beats() >= 600 {
                log!("overlay: the game's swap chain was never seen being made; hooking it from a dummy one");
                hudhook::hooks::dx12::late_hook_fallback();
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let t0 = std::time::Instant::now();
        while PRESENTS.load(SeqCst) < frames || t0.elapsed() < std::time::Duration::from_secs(secs) {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        GATE.store(true, SeqCst);
        log!("overlay: starting ({} game frames, {:.1} s after the first)", PRESENTS.load(SeqCst), t0.elapsed().as_secs_f32());
    });
    std::thread::spawn(move || {
        let r = hudhook::Hudhook::builder()
            .with::<ImguiDx12Hooks>(Overlay { fonts: None, hud: None, portraits: Default::default(), wind: 0.0 })
            .with_hmodule(hudhook::windows::Win32::Foundation::HINSTANCE(hmodule as _))
            .build()
            .apply();
        if let Err(e) = r {
            log!("overlay: could not hook DX12: {e:?}");
        }
    });
    // (said once if nothing has been drawn a minute and a half on (2.0.7: it starts 20 s in): hudhook's apply holds its thread while hooked)
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(90));
        if !READY.load(std::sync::atomic::Ordering::Relaxed) {
            log!("overlay: nothing drawn 90 s after starting");
        }
    });
}

/// 2.0.7: the other programs drawing into the game or hooking it, once (players' early crashes didn't come back in the
/// lab, where none run; with the crash log, a report says which were there): DLLs loaded in the game whose names match
/// known overlays and hooks (names only)
fn log_hookers() {
    use windows_sys::Win32::System::ProcessStatus::{K32EnumProcessModules, K32GetModuleBaseNameW};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    const KNOWN: [&str; 27] = [
        "gameoverlayrenderer", "discordhook", "discord", "rtsshooks", "rtss", "msiafterburner", "nvspcap", "nvcamera",
        "nvidia", "amdxx", "amdow", "aticfx", "igo", "overwolf", "obs-", "graphics-hook", "reshade", "dxgi.dll",
        "d3d12.dll", "gamebar", "xboxgame", "medal", "fraps", "questpath",
        // (the graphics drivers' Direct3D 12 parts: which maker's GPU)
        "nvwgf2", "amdxc", "igd12",
    ];
    let mut mods = vec![std::ptr::null_mut(); 1024];
    let mut needed = 0u32;
    let ok = unsafe { K32EnumProcessModules(GetCurrentProcess(), mods.as_mut_ptr(), (mods.len() * 8) as u32, &mut needed) } != 0;
    if !ok {
        return;
    }
    let n = (needed as usize / 8).min(mods.len());
    let mut found = Vec::new();
    for m in &mods[..n] {
        let mut buf = [0u16; 260];
        let len = unsafe { K32GetModuleBaseNameW(GetCurrentProcess(), *m, buf.as_mut_ptr(), buf.len() as u32) } as usize;
        let name = String::from_utf16_lossy(&buf[..len]);
        let lower = name.to_ascii_lowercase();
        // (dxgi.dll / d3d12.dll: the system's own are expected; a copy beside the game is ReShade or another wrapper)
        if KNOWN.iter().any(|k| lower.contains(k)) && lower != "dxgi.dll" && lower != "d3d12.dll" {
            found.push(name);
        }
    }
    let wrapper = ["dxgi.dll", "d3d12.dll", "dinput8.dll", "d3d11.dll", "version.dll", "winmm.dll"]
        .iter()
        .filter(|f| std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(f).exists())).unwrap_or(false))
        .map(|f| format!("{f} beside the game"))
        .collect::<Vec<_>>();
    found.extend(wrapper);
    log!("overlay: other overlays, hooks and the graphics driver in the game: {}", if found.is_empty() { "none seen".to_string() } else { found.join(", ") });
}
