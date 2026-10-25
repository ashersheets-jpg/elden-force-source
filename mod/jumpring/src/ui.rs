//! State shared between the game's frame task (owns gameplay) and the overlay (draws it), plus the
//! saved profile (last fighter and Your Tarnished loadout) at %LOCALAPPDATA%/JumpRing/profile.txt.

use std::sync::Mutex;
use std::time::Instant;

use crate::sheets::{self, Slot};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    /// Fighter list; `custom` row leads to the loadout editor
    Roster,
    /// Your Tarnished: four slots, left/right changes the ability
    Loadout,
    /// Rebind the in-world buttons (rows: the rebindable inputs, then "reset to defaults")
    Controls,
    /// 0.15.0: spend free attribute points on the sheet in use (builds.rs)
    Build,
    /// 1.1.0: Elden Force settings: volumes, difficulty, boss warnings, Training, the Controls page
    Settings,
    /// 1.2.0: the Move List: a fighter's combo tree and movement (cursor: index into combo_fighters)
    Moves,
    /// 1.3.0: the people whose work Elden Force uses (credits.rs; cursor: the first line shown)
    Credits,
}

/// 1.1.0: the rows of the Settings page, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingRow {
    Music,
    Voices,
    Difficulty,
    Warnings,
    /// how see-through the boss tells' seals are (only while they show)
    Opacity,
    Training,
    /// 1.2.0: opens the Move List
    Moves,
    Controls,
    /// 1.3.0: opens the Credits page
    Credits,
}

/// The Settings page's rows now (the warnings' opacity only while the warnings draw on the ground).
pub fn setting_rows(s: &Shared) -> Vec<SettingRow> {
    let mut v = vec![SettingRow::Music, SettingRow::Voices, SettingRow::Difficulty, SettingRow::Warnings];
    if s.warnings.visual() {
        v.push(SettingRow::Opacity);
    }
    v.extend([SettingRow::Training, SettingRow::Moves, SettingRow::Controls, SettingRow::Credits]);
    v
}

/// 0.15.0: the build sheet in use, for the Build page (builds.rs fills it every frame).
#[derive(Clone, Copy, Default)]
pub struct BuildView {
    pub tarnished: bool,
    pub attrs: [u32; 8],
    pub free: u32,
    pub level: u32,
}

/// 1.2.x: the Move List's view (user: tabs for Carian Combo Warriors' inputs and our movement, every weapon's
/// moveset, sorting by fighter or weapon).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MovesView {
    /// 0 the moveset (Carian Combo Warriors), 1 movement (Elden Force)
    pub tab: u8,
    /// the moveset pages go by weapon instead of by fighter
    pub by_weapon: bool,
    /// index into movelists::WEAPONS
    pub weapon: usize,
    /// how far the page is scrolled (1080p pixels)
    pub scroll: f32,
    /// 1.3.0: opened from Elden Ring's pause menu (back closes it, instead of going to the fighter select)
    pub from_pause: bool,
    /// 1.3.0 (the user on review: "Instead of a button, make it a drop down to change between a weapon or
    /// character"): the "sort by" dropdown is open, and the option highlighted in it (0 fighter, 1 weapon)
    pub sort_open: bool,
    pub sort_pick: u8,
}

/// 1.3.0 (user: "a lot of people did not see the move set screen ... make that a clickable option on our pause
/// screen"): the pause menu's Move List button was clicked (overlay.rs); the frame task opens it.
pub static OPEN_MOVES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 1.3.0: a mouse click on the Move List's "sort by" dropdown: 1 the box (open / close), 2 fighter, 3 weapon
pub static SORT_CLICK: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub struct Menu {
    pub page: Page,
    /// row in FIGHTERS (Roster) or slot 0..4 (Loadout)
    pub cursor: usize,
    /// opened automatically on load-in (vs. from a grace)
    pub at_start: bool,
}

#[derive(Clone, Default)]
pub struct HudAbility {
    pub name: &'static str,
    pub cost: i32,
    pub cooldown: f32,
    pub ultimate: bool,
    pub combo: bool,
}

#[derive(Default)]
pub struct Hud {
    pub in_world: bool,
    pub at_grace: bool,
    /// 1.1.0: one of Elden Ring's own menus covers the game (the pause menu with equipment, inventory,
    /// status and system; the map; popups; cutscenes): our HUD steps aside
    pub game_menu: bool,
    pub fighter: &'static str,
    /// sheet id of the active fighter (portrait)
    pub fighter_id: &'static str,
    pub series: &'static str,
    pub fp: i32,
    /// the player's level (ability damage shown in the fighter select)
    pub level: u32,
    pub awakening: f32,
    pub abilities: [HudAbility; 4],
}

/// How hard Elden Ring's bosses hit (bosses.rs): chosen in the fighter select, saved in the profile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    /// 0.16.0: bosses' health, x Elden Ring's (their damage is Elden Ring's own at every difficulty).
    /// 0.16.1 (user, after the Godskin Apostle on Hard x2.7: "a bit too easy"): the user's numbers.
    pub fn boss_hp(self) -> f32 {
        match self {
            Difficulty::Easy => 1.5,
            // 1.3.0 (user: bosses need "higher health"): Normal 3.5 (was 3), Hard 6 (was 5)
            Difficulty::Normal => 3.5,
            Difficulty::Hard => 6.0,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Hard",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Difficulty::Easy => Difficulty::Normal,
            Difficulty::Normal => Difficulty::Hard,
            Difficulty::Hard => Difficulty::Easy,
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "easy" => Some(Difficulty::Easy),
            "normal" => Some(Difficulty::Normal),
            "hard" => Some(Difficulty::Hard),
            _ => None,
        }
    }
}

/// 0.11.0: which boss-tell warnings show (bosses.rs, world_fx.rs): chosen in the fighter select.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Warnings {
    Off,
    Text,
    Visual,
    All,
}

impl Warnings {
    pub const ALL: [Warnings; 4] = [Warnings::Off, Warnings::Text, Warnings::Visual, Warnings::All];
    pub fn label(self) -> &'static str {
        match self {
            Warnings::Off => "Off",
            Warnings::Text => "Text",
            Warnings::Visual => "Visual",
            Warnings::All => "All",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Warnings::Off => Warnings::Text,
            Warnings::Text => Warnings::Visual,
            Warnings::Visual => Warnings::All,
            Warnings::All => Warnings::Off,
        }
    }
    pub fn text(self) -> bool {
        matches!(self, Warnings::Text | Warnings::All)
    }
    pub fn visual(self) -> bool {
        matches!(self, Warnings::Visual | Warnings::All)
    }
    fn parse(s: &str) -> Option<Self> {
        Warnings::ALL.into_iter().find(|w| w.label().eq_ignore_ascii_case(s))
    }
}

pub struct Shared {
    pub hud: Hud,
    pub difficulty: Difficulty,
    pub warnings: Warnings,
    /// 1.1.0: volumes 0..=10 (audio.rs): our music, the fighters' voice quips
    pub music_vol: u8,
    pub voice_vol: u8,
    /// 1.1.0: the boss tells' opacity, 1..=10 (tenths)
    pub warn_opacity: u8,
    /// 0.16.0: Training: the enemy locked onto is a punching bag (training.rs); not saved
    pub training: bool,
    pub menu: Option<Menu>,
    pub fighter: &'static str,
    pub loadout: [&'static str; 4],
    pub notice: Option<(String, Instant)>,
    pub fx: Fx,
    pub build: BuildView,
    /// a point to spend on this attribute (the frame task takes it)
    pub build_spend: Option<usize>,
    pub moves: MovesView,
}

/// Screen effects drawn over the game (overlay.rs draw_fx).
pub struct Fx {
    /// a parry just happened: white flash and a ring of light
    pub parry_at: Option<Instant>,
    /// slow motion after a parry: darkened blue edges until then
    pub slow_until: Option<Instant>,
    /// 1.3.0: The World's stopped time: since when, for how long, the Stand's colour
    pub time_stop: Option<(Instant, f32, [f32; 4])>,
    /// 0..1: faint wind streaks at the screen's edges (sprinting, quick attacks, combos)
    pub wind: f32,
    /// 1.2.0: the combo counter: hits in the string, the last move's name, a finisher, when
    pub combo: Option<(u32, &'static str, bool, Instant)>,
    /// 1.2.0: seconds until the finishers are ready (the HUD's finisher line); None for Your Tarnished
    pub finisher_cd: Option<f32>,
}

pub static UI: Mutex<Shared> = Mutex::new(Shared {
    difficulty: Difficulty::Normal,
    warnings: Warnings::All,
    music_vol: 10,
    voice_vol: 10,
    warn_opacity: 8,
    training: false,
    hud: Hud {
        in_world: false,
        at_grace: false,
        game_menu: false,
        fighter: "",
        fighter_id: "",
        series: "",
        fp: 0,
        level: 1,
        awakening: 0.0,
        abilities: [const { HudAbility { name: "", cost: 0, cooldown: 0.0, ultimate: false, combo: false } }; 4],
    },
    menu: None,
    fighter: "goku",
    loadout: ["goku_kamehameha", "naruto_rasengan", "goku_meteor_combination", "goku_super_spirit_bomb"],
    notice: None,
    fx: Fx { parry_at: None, slow_until: None, time_stop: None, wind: 0.0, combo: None, finisher_cd: None },
    build: BuildView { tarnished: false, attrs: [0; 8], free: 0, level: 1 },
    build_spend: None,
    moves: MovesView { tab: 0, by_weapon: false, weapon: 0, scroll: 0.0, from_pause: false, sort_open: false, sort_pick: 0 },
});

/// The parry's flash and the slow-motion tint for `slow_for`.
pub fn parry_flash(slow_for: std::time::Duration) {
    let mut s = UI.lock().unwrap();
    s.fx.parry_at = Some(Instant::now());
    s.fx.slow_until = Some(Instant::now() + slow_for);
}

/// 1.3.0: time stops for `secs` (The World): the stopped world's tint, in the Stand's colour at its edges.
pub fn time_stop(secs: f32, color: [f32; 4]) {
    UI.lock().unwrap().fx.time_stop = Some((Instant::now(), secs, color));
}

/// Wind streaks at this strength (eased by the overlay).
pub fn set_wind(v: f32) {
    UI.lock().unwrap().fx.wind = v.clamp(0.0, 1.0);
}

/// 1.2.0: a combo node landed: the counter shows `hits` and the move's name.
pub fn set_combo(hits: u32, name: &'static str, finisher: bool) {
    UI.lock().unwrap().fx.combo = Some((hits, name, finisher, Instant::now()));
}

pub fn notice(text: impl Into<String>) {
    UI.lock().unwrap().notice = Some((text.into(), Instant::now()));
}

/// Abilities allowed in Your Tarnished slot `slot` (0..2 skills, 3 ultimates).
pub fn pool(slot: usize) -> Vec<&'static sheets::Ability> {
    let want = if slot == 3 { Slot::Ultimate } else { Slot::Skill };
    // 0.16.0: combos are always on Ability 3 (L3 + X), as for every fighter; listed first there
    let mut v: Vec<_> = sheets::ABILITIES.iter().filter(|a| a.creator_pool && a.slot == want && (slot >= 2 || !a.combo)).collect();
    if slot == 2 {
        v.sort_by_key(|a| !a.combo);
    }
    v
}

/// 1.2.0: the fighters with a combo tree, in the fighter select's order (the Move List pages).
pub fn combo_fighters() -> Vec<&'static str> {
    // (1.2.0: every fighter: their basic attacks are Carian Combo Warriors')
    crate::select_screen::ORDER.iter().copied().filter(|id| *id != "custom").collect()
}

/// Rows of the Controls page: the rebindable inputs (indexes into sheets::INPUTS).
pub fn control_rows() -> Vec<usize> {
    (0..sheets::INPUTS.len()).filter(|i| crate::binds::rebindable(*i)).collect()
}

pub fn control_label(id: &str) -> &'static str {
    match id {
        "ability_1" => "Ability 1",
        "ability_2" => "Ability 2",
        "ability_3" => "Ability 3",
        "ability_4" => "Ultimate (Awakening)",
        "special" => "Movement special (swing, blink or glide: the Move List says whose)",
        "menu_open" => "Elden Force menu (at a grace)",
        _ => "Action",
    }
}

fn profile_path() -> std::path::PathBuf {
    crate::log::dir().join("profile.txt")
}

pub fn load_profile() {
    let Ok(text) = std::fs::read_to_string(profile_path()) else { return };
    let mut ui = UI.lock().unwrap();
    for line in text.lines() {
        if let Some(f) = line.strip_prefix("fighter=") {
            if let Some(row) = sheets::fighter(f.trim()) {
                ui.fighter = row.id;
            }
        } else if let Some(d) = line.strip_prefix("difficulty=").and_then(|d| Difficulty::parse(d.trim())) {
            ui.difficulty = d;
        } else if let Some(wn) = line.strip_prefix("warnings=").and_then(|d| Warnings::parse(d.trim())) {
            ui.warnings = wn;
        } else if let Some(v) = line.strip_prefix("music_volume=").and_then(|v| v.trim().parse::<u8>().ok()) {
            ui.music_vol = v.min(10);
        } else if let Some(v) = line.strip_prefix("voice_volume=").and_then(|v| v.trim().parse::<u8>().ok()) {
            ui.voice_vol = v.min(10);
        } else if let Some(v) = line.strip_prefix("warning_opacity=").and_then(|v| v.trim().parse::<u8>().ok()) {
            ui.warn_opacity = v.clamp(1, 10);
        } else if let Some(l) = line.strip_prefix("loadout=") {
            for (i, id) in l.split(',').take(4).enumerate() {
                if let Some(a) = sheets::ability(id.trim()) {
                    if pool(i).iter().any(|p| p.id == a.id) {
                        ui.loadout[i] = a.id;
                    }
                }
            }
        }
    }
    crate::audio::set_volumes(ui.music_vol, ui.voice_vol);
}

pub fn save_profile(s: &Shared) {
    let d = s.difficulty.label().to_lowercase();
    let wn = s.warnings.label().to_lowercase();
    crate::audio::set_volumes(s.music_vol, s.voice_vol);
    let _ = std::fs::write(
        profile_path(),
        format!("fighter={}\nloadout={}\ndifficulty={d}\nwarnings={wn}\nmusic_volume={}\nvoice_volume={}\nwarning_opacity={}\n", s.fighter, s.loadout.join(","), s.music_vol, s.voice_vol, s.warn_opacity),
    );
}
