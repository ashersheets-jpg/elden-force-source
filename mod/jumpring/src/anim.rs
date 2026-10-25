//! Body animations for moves the mod makes (double jump, air dash, slam, and every step of an ability:
//! lunges, leaps, dives, punches, kicks, casting poses). The DLL can't start an animation itself
//! (request_animation_id is ignored), so it tells the player's behaviour script (hks/elden_force.lua):
//! it writes the Lua for the animation to jr_cue.lua next to the script, then applies one of two short
//! marker SpEffects (alternating, so every cue is a fresh edge). The script sees the marker with
//! env(GetSpEffectID), runs the file, and so plays Elden Ring's own animation events.
//! The marker rows are "unused" rows of the game's own SpEffectParam, rewritten in memory from the
//! neutral talisman template: no stats, no icon, no visuals, gone after a few frames.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use eldenring::cs::{ChrInsExt, PlayerIns, SoloParamRepository, SpEffectParam};
use eldenring::param::SP_EFFECT_PARAM_ST;
use fromsoftware_shared::FromStatic;

use crate::log;

pub const A: i32 = 7280;
pub const B: i32 = 102006;
const TEMPLATE: u32 = 310000;
const SECONDS: f32 = 0.12;

/// What the body does. Each maps to Lua run by the behaviour script (JR_* helpers in elden_force.lua).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// air dash in the air (toward the stick), a quick dash along the ground
    Dash,
    /// a leap
    Jump,
    /// a plunging dive
    Dive,
    /// a quick strike with the right hand (punch, jab, slash): light attack 1
    Strike,
    /// the follow-up strike of a combo: light attack 2
    Strike2,
    /// a heavy finisher: strong attack
    Heavy,
    /// a kick (the game's kick)
    Kick,
    /// a hand thrust forward to release a blast or projectile
    Cast,
    /// both arms raised to the sky (charging a giant attack overhead)
    Raise,
    /// powering up: a fighter's battle stance roar
    PowerUp,
    /// a throw (grenade, disc)
    Throw,
    /// hands cupped at the hip, then both palms thrust forward (Kamehameha, Tailed Beast Bomb)
    TwoHand,
    /// hands together in front of the face, flung apart (Stun Grenade)
    Flash,
    /// 1.2.0: Naruto's Rasen Shuriken: held high one-handed, leaning back, then a pitcher's overhand throw
    Pitch,
    /// a wide sweep of the left arm (Dancing Flame)
    Sweep,
    /// Luffy's stretching punch
    Pistol,
    /// Luffy's two stretching palms (Grizzly Magnum)
    Grizzly,
    /// 0.10.0: Superman flight, fist first (Goku's and All Might's air dash)
    Fly,
    /// 0.10.0: an anime air dash, leaning into it
    AirDash,
    /// 0.10.0: Deku's Black Whip: the arm flung up to the tendril
    Whip,
    /// 0.10.0: Deku's Shoot Style flying side kick
    FlyingKick,
    /// 0.14.1: Luffy's swing (his real right arm hidden: the overlay draws the stretched one)
    LuffySwing,
    /// 0.16.0: Deku's and Luffy's skydive after a swing (falling over a second): own animation only
    Fall,
    /// 0.10.0: an Elden Ring Ash of War on the bare hands (SwordArtsParam row): weapon.rs gives the hands
    /// that Ash of War for a moment and the behaviour script plays it as the game does on L2
    Arts(i32),
}

impl Cue {
    fn lua(self) -> &'static str {
        match self {
            Cue::Dash => "JR_Dash()",
            Cue::Jump => "JR_Leap()",
            Cue::Dive => "JR_Dive()",
            Cue::Strike => "JR_Strike(1)",
            Cue::Strike2 => "JR_Strike(2)",
            Cue::Heavy => "JR_Strike(3)",
            Cue::Kick => "JR_Kick()",
            Cue::Cast => "JR_Gesture(JR_G_CAST)",
            Cue::Raise => "JR_Gesture(JR_G_RAISE)",
            Cue::PowerUp => "JR_Gesture(JR_G_POWER)",
            Cue::Throw => "JR_Gesture(JR_G_THROW)",
            Cue::TwoHand | Cue::Flash | Cue::Sweep | Cue::Grizzly => "JR_Gesture(JR_G_CAST)",
            Cue::Pitch => "JR_Gesture(JR_G_THROW)",
            Cue::Pistol => "JR_Strike(1)",
            Cue::Fly | Cue::AirDash => "JR_Dash()",
            Cue::Whip | Cue::LuffySwing => "JR_Leap()",
            Cue::Fall => "",
            Cue::FlyingKick => "JR_Kick()",
            Cue::Arts(_) => "JR_Arts()",
        }
    }

    /// Elden Force's own animation for this cue (sheet: anims), when the player's binders have them.
    /// Sword fighters keep Elden Ring's weapon swings for their slashes.
    fn own(self) -> Option<u32> {
        let sword = SWORD.load(Ordering::Relaxed);
        crate::sheets::ANIMS.iter().find(|a| a.cue == self.name() && !(sword && a.weapon_swing)).map(|a| a.id)
    }

    fn name(self) -> &'static str {
        match self {
            Cue::Dash => "Dash",
            Cue::Jump => "Jump",
            Cue::Dive => "Dive",
            Cue::Strike => "Strike",
            Cue::Strike2 => "Strike2",
            Cue::Heavy => "Heavy",
            Cue::Kick => "Kick",
            Cue::Cast => "Cast",
            Cue::Raise => "Raise",
            Cue::PowerUp => "PowerUp",
            Cue::Throw => "Throw",
            Cue::TwoHand => "TwoHand",
            Cue::Flash => "Flash",
            Cue::Pitch => "Pitch",
            Cue::Sweep => "Sweep",
            Cue::Pistol => "Pistol",
            Cue::Grizzly => "Grizzly",
            Cue::Fly => "Fly",
            Cue::AirDash => "AirDash",
            Cue::Whip => "Whip",
            Cue::FlyingKick => "FlyingKick",
            Cue::LuffySwing => "LuffySwing",
            Cue::Fall => "Fall",
            Cue::Arts(_) => "",
        }
    }
}

/// The current fighter swings a sword (Asta): Strike / Strike2 / Heavy stay Elden Ring's weapon attacks.
pub static SWORD: AtomicBool = AtomicBool::new(false);

static READY: AtomicBool = AtomicBool::new(false);
static FLIP: AtomicBool = AtomicBool::new(false);

fn cue_file() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| crate::recolor::mod_dir().map(|d| d.join("action").join("script").join("jr_cue.lua"))).as_ref()
}

pub(crate) fn prepare() -> bool {
    if READY.load(Ordering::Relaxed) {
        return true;
    }
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
    let Some(template) = repo.get::<SpEffectParam>(TEMPLATE).map(|r| r as *const SP_EFFECT_PARAM_ST) else { return false };
    for id in [A, B] {
        let Some(r) = repo.get_mut::<SpEffectParam>(id as u32) else {
            log!("anim: SpEffect {id} missing");
            return false;
        };
        unsafe { std::ptr::copy_nonoverlapping(template, r as *mut SP_EFFECT_PARAM_ST, 1) };
        r.set_max_hp_rate(1.0);
        r.set_effect_endurance(SECONDS);
        r.set_icon_id(-1);
        r.set_vfx_id(-1);
        r.set_state_info(0);
    }
    READY.store(true, Ordering::Relaxed);
    true
}

/// Ask the behaviour script for the animation of `cue`.
pub fn cue(player: &mut PlayerIns, cue: Cue) {
    if let Cue::Arts(row) = cue {
        crate::weapon::set_arts(row, 1.0);
    }
    if crate::anims::READY.load(Ordering::Relaxed) {
        if let Some(id) = cue.own() {
            clear(player);
            crate::anims::play(player, id);
            return;
        }
    }
    if cue != Cue::Fall {
        play(player, cue.lua());
    }
}

/// Ask the behaviour script to run `lua` (one line) now. Lab auditions use this directly.
pub fn play(player: &mut PlayerIns, lua: &str) {
    if !prepare() {
        return;
    }
    let Some(path) = cue_file() else { return };
    // 0.10.0: two cues in one frame: the script runs the file once, so the second joins the first
    // (and the marker stays as the first one left it: flipping twice would be no edge at all)
    static LAST: std::sync::Mutex<Option<(std::time::Instant, String)>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    let joined = last.as_ref().is_some_and(|(t, _)| t.elapsed() < std::time::Duration::from_millis(12));
    let text = match last.as_ref() {
        Some((_, prev)) if joined => format!("{prev}{lua}\n"),
        _ => format!("{lua}\n"),
    };
    *last = Some((std::time::Instant::now(), text.clone()));
    drop(last);
    if let Err(e) = std::fs::write(path, &text) {
        log!("anim: can't write {}: {e}", path.display());
        return;
    }
    #[cfg(feature = "lab")]
    log!("lab cue: {}{}", text.trim().replace('\n', " | "), if joined { " (joined)" } else { "" });
    if joined {
        return;
    }
    // a fresh edge for the script every time: the other marker than last time
    player.chr_ins.remove_speffect(A);
    player.chr_ins.remove_speffect(B);
    let flip = !FLIP.fetch_xor(true, Ordering::Relaxed);
    player.apply_speffect(if flip { A } else { B }, true);
}

/// A move ended: take the markers off now (the player's own weapon hits count again).
pub fn clear(player: &mut PlayerIns) {
    player.chr_ins.remove_speffect(A);
    player.chr_ins.remove_speffect(B);
}
