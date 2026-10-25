//! 1.3.0: Stands (the user, 2026-10-18: "figure out how to use Stands for our characters", then "make the stands appear
//! like they do in the show, poses and attacks should be as identical as you can make them in this game engine").
//! Jotaro's Star Platinum and DIO's The World, generated models (work/rigport/concepts_jojo.py) worn by one of the game's
//! spirit summons, the way Naruto's shadow clone is made (clones.rs): called through the summon manager once the fighter is
//! in the world, then kept out of sight (not drawn, not hit, not thinking) at his back.
//!
//! Researched (JoJo wiki, the Stardust Crusaders anime): a Stand is a spirit that rises out of its user when it fights and
//! goes back into him after; it can't stray far (Star Platinum about 2 m) and is otherwise silent. Star Platinum is
//! violet-skinned and throws the ORA barrage (an extremely fast run of precise punches ending in one big blow) while Jotaro
//! stands back and points at his enemy; Star Finger shoots two fingers out to pierce. The World is gold, throws the MUDA
//! barrage (it ends them with a kick as often as a punch) and stops time, looming over DIO. What the engine allows:
//! - It comes out of its user: it starts inside his body and slides out in front of him in a sixth of a second while it
//!   fades in, its outline flaring; at the end it slides back into him and fades. It hovers a hand's width off the ground.
//! - Its look: solid (as the anime draws it) while it fights. It materialises out of a glowing outline of itself and goes
//!   back the same way: the spirits' own shader (PhantomParam, the light at a summon's edges) on rows of its own
//!   (strip_fx.json row_new 9990201/2) in the Stand's colour (violet, gold) for those moments, and always on its afterimages.
//! - Its moves are Elden Ring's own Ashes of War on its bare hands (motion capture), played by its own behaviour script the
//!   way anim.rs has the player's play them (a spirit summon runs the player's script; the DLL can't start an attack on it
//!   directly): Repeating Thrust's jabs for the barrage, the bare fists' heavy straight punch for its last blow and their
//!   jab for Star Finger, Storm Kick for The World's finishing kick, War Cry's roar for the time stop, the heavy straight
//!   again for a single heavy blow.
//! - The barrage's many fists (the anime draws a dozen arms): two more of the same spirit (called with it as a group of
//!   three: group_rows) appear just beside and behind it, fainter, throwing the same jabs a few frames apart, so the fists blur.
//! - The user: Jotaro (and DIO) stand back and point at the enemy (the game's Point Forwards gesture) while it fights.
//! As in the anime and the JoJo games, the Stand throws the blows: their damage stays the move's own.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use eldenring::cs::{ChrIns, ChrInsExt, FieldInsHandle, PlayerIns, SoloParamRepository, WorldChrMan};
use fromsoftware_shared::FromStatic;

use crate::ability::{self, V3};
use crate::log;

/// a move asks for the Stand: how long it stays out, and what it does (ORA .. ROAR, or a raw animation id)
pub static REQUEST: Mutex<Option<(f32, i32)>> = Mutex::new(None);
/// the Stand is formed and kept at hand: a move's Step::Stand brings it out (otherwise the fighter throws the blow himself)
pub static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// (lab: spirit effects the Stand keeps, to see each one's look)
pub static KEEP: Mutex<Vec<i32>> = Mutex::new(Vec::new());
/// (lab: the afterimages' layout: side, back (m), lag (s) for each, their opacity, and whether they glow)
pub static AFTER: Mutex<Option<([(f32, f32, f32); 2], f32, bool)>> = Mutex::new(None);
/// (lab: 1 = its movement left on while it is out)
pub static MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

// what a move asks of it (scripts Step::Stand)
/// the ORA barrage: jabs with its afterimages, then one straight punch
pub const ORA: i32 = 1;
/// the MUDA barrage: jabs with its afterimages, then a kick
pub const MUDA: i32 = 2;
/// Star Finger (a straight thrust) or any single straight punch
pub const THRUST: i32 = 3;
/// a single heavy blow out of stopped time
pub const HEAVY: i32 = 4;
/// the time stop: it rises over its user's shoulder and roars
pub const ROAR: i32 = 5;
/// Jotaro's leap (movement.rs): it appears where he stood and heaves him away, staying there as he flies
pub const LAUNCH: i32 = 6;

// its body's moves: Elden Ring's Ashes of War on its bare hands (SwordArtsParam rows, as the fighters' aow() steps), played
// by its own behaviour script (cue())
use crate::scripts::{PALM_BLAST, REPEATING_THRUST, STORM_KICK, WAR_CRY};
// (Palm Blast doesn't start on a summon's bare hands (lab): the heavy blow is the fists' own straight)

/// What one of its bodies plays: an Ash of War on its bare hands, or one of the bare fists' own attacks (a line of Lua for
/// its behaviour script: hks/elden_force.lua JR_Strike)
#[derive(Clone, Copy)]
enum Body {
    Arts(i32),
    Lua(&'static str),
}
/// a quick straight jab (Star Finger's thrust)
const JAB: Body = Body::Lua("JR_Strike(1)");
/// a driving straight punch (the barrage's last blow: the anime's single big ORA!)
const STRAIGHT: Body = Body::Lua("JR_Strike(3)");
/// an Ash of War's own animation id: asked of a summon directly it does nothing but break off what it is doing
const BREAK_OFF: i32 = 609040000;
/// the hands it fights with (the player's bare hands' row: no damage of their own on a summon)
const HANDS: i32 = crate::weapon::UNARMED as i32;
/// the user's pose while it fights: the game's Point Forwards gesture (1.3.1 round 2, the user: "I liked the old animation":
/// back, played once as the Stand comes out and ended when it goes back)
const A_POINT: i32 = 80200;
/// when the barrage's last blow comes, before the end of its time out
const FINISH_BEFORE: f32 = 0.55;

/// the armour rows the Stands' bodies are worn through (DLC NPCs' helms used as bodies, lent like the villains': villains.rs)
pub const STAND_ROWS: [i32; 2] = [5360000, 5370000];
/// the spirit it is made from (the same as the shadow clone's: clones.rs; called in a group of three: GROUP_ROWS)
const NPC_ROW: i32 = 100000020;
const INIT_ROW: u32 = 23342;
const SPIRIT_FX: [i32; 8] = [295200, 295201, 296000, 296010, 297000, 20297000, 20297820, 290050];
const GAME_DELAY: f32 = 10.0;
/// its own look (strip_fx.json row_new): SpEffect, its visual and its shader, per Stand
const LOOK: [i32; 2] = [9990201, 9990202];
/// the Stands' rim colours: Star Platinum's violet, The World's gold
const RIM: [[u8; 3]; 2] = [[170, 120, 255], [255, 205, 80]];
/// where it stands when it fights: this far in front of him, a little to his right, a little off the ground
const OUT_AHEAD: f32 = 1.05;
const OUT_SIDE: f32 = 0.55;
const HOVER: f32 = 0.12;
/// where it looms for the time stop: behind his right shoulder, risen over him
const LOOM_BACK: f32 = -0.55;
const LOOM_SIDE: f32 = 0.45;
const LOOM_UP: f32 = 0.45;
/// how see-through it is drawn while out (its afterimages much more)
const OPACITY: f32 = 0.9;
const AFTER_OPACITY: f32 = 0.5;
/// its afterimages: beside and behind it, and how many seconds behind its jabs each one's start
const AFTER_AT: [(f32, f32); 2] = [(0.2, 0.0), (-0.2, 0.0)];
const AFTER_LAG: [f32; 2] = [0.05, 0.1];
const AFTER_GLOW: bool = false;
/// coming out of him and going back in (seconds)
const EMERGE: f32 = 0.16;
const RETREAT: f32 = 0.22;
/// the fighter has been in the world this long before his Stand is called (the world settled)
const CALL_AFTER: f32 = 6.0;

struct Out {
    what: i32,
    /// where it comes from: out of him, or from where it was (one move straight after another)
    from: V3,
    /// a spot of its own in the world, and the way it faces there (LAUNCH: it stays where he left from)
    anchor: Option<(V3, V3)>,
    since: Instant,
    until: Instant,
    /// the barrage's last blow asked for
    finished: bool,
    /// the afterimages' jabs asked for
    after_started: [bool; 2],
}

struct Stand {
    idx: usize,
    asked: Instant,
    handle: Option<FieldInsHandle>,
    after: Vec<FieldInsHandle>,
    found: Option<Instant>,
    cleaned: Instant,
    out: Option<Out>,
    /// going back into him: since when, from where
    back: Option<(Instant, V3)>,
    /// per body (it, then its afterimages): the marker its behaviour script saw last, and until when it may act
    flip: [bool; 3],
    free_until: [Option<Instant>; 3],
    /// a move asked for while it was still in another (War Cry's roar can't be cut by a new attack): that one is broken
    /// off first and this one starts on the next frame
    pending: Option<(Body, Instant)>,
    last_at: V3,
    seen_anim: i32,
}

#[derive(Default)]
pub struct Stands {
    s: Option<Stand>,
    restore_delay: bool,
    in_world_since: Option<Instant>,
    /// a call that never came: not asked again for this fighter until the next load
    gave_up: bool,
    /// its look's rows made (once per run)
    look_ready: bool,
    /// (1.3.1 round 3) a pointing move's call, held until his hand thrusts: (secs, what, when)
    held: Option<(f32, i32, Instant)>,
}

/// 1.3.1 round 3 (the user: "the stands should attack as he thrusts his hand forwards"; lab, point_time.txt: the Point Forwards
/// gesture's hand is out 0.75 s in, the Stand's first blow came 0.35 s in): he points first, the Stand comes out this much later.
/// The scripts' blows wait the same.
pub const POINT_LEAD: f32 = 0.4;

fn find(h: FieldInsHandle) -> Option<&'static mut ChrIns> {
    let wcm = unsafe { WorldChrMan::instance_mut() }.ok()?;
    wcm.chr_ins_by_handle_mut(&h).map(|c| unsafe { &mut *(c as *mut ChrIns) })
}

/// drawn (out) or not; never hit, never moving on its own; acting only while `free` (a move of ours starting in it)
fn present(c: &mut ChrIns, out: bool, free: bool) {
    c.chr_flags1c5.set_enable_render(out);
    c.chr_ctrl.flags.set_disable_hit(true);
    c.chr_ctrl.flags.set_disable_character_capsule_collision(true);
    let mode = MODE.load(std::sync::atomic::Ordering::Relaxed);
    c.debug_flags.set_disabled_movement(mode != 1);
    c.debug_flags.set_disabled_secondary_actions(!(free || mode == 1));
    c.debug_flags.set_disabled_hit(true);
    // it can't be hurt or die, and never runs out of the focus its Ashes of War cost
    let d = &mut c.modules.data;
    d.hp = d.max_hp;
    d.fp = d.max_fp;
}

fn place(c: &mut ChrIns, at: V3, dir: V3) {
    let ph = &mut c.modules.physics;
    ph.position = eldenring::position::HavokPosition(at.0, at.1, at.2, ph.position.3);
    ph.chr_proxy_pos_update_requested = true;
    ph.gravity_disabled = true;
    let l = (dir.0 * dir.0 + dir.2 * dir.2).sqrt();
    if l > 1e-3 {
        let th = (-dir.0 / l).atan2(-dir.2 / l);
        ph.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
        ph.interpolated_orientation = ph.orientation;
    }
}

/// The arts for a move of a Stand's (with the base game's in place of Shadow of the Erdtree's without it).
fn arts(row: i32) -> i32 {
    match row {
        STORM_KICK if !crate::weapon::dlc() => crate::scripts::KICK,
        PALM_BLAST if !crate::weapon::dlc() => crate::scripts::GIANT_HUNT,
        r => r,
    }
}

fn npc_cue_file() -> Option<std::path::PathBuf> {
    crate::recolor::mod_dir().map(|d| d.join("action").join("script").join("jr_npc_cue.lua"))
}

/// Any human-bodied NPC (a Stand, a villain) runs the player's behaviour script: `lua` (one line, hks/elden_force.lua's
/// JR_ helpers) is run by it on its next update: the anim.rs markers on it, the line in jr_npc_cue.lua (shared by every
/// NPC: the last writer in a frame wins). `flip` is that character's own marker state.
pub(crate) fn npc_cue(c: &mut ChrIns, lua: &str, flip: &mut bool) {
    if !crate::anim::prepare() {
        return;
    }
    if let Some(p) = npc_cue_file() {
        let _ = std::fs::write(p, format!("{lua}\n"));
    }
    c.remove_speffect(crate::anim::A);
    c.remove_speffect(crate::anim::B);
    *flip = !*flip;
    c.apply_speffect(if *flip { crate::anim::A } else { crate::anim::B }, true);
}

/// One of its bodies plays Ash of War `row` on its bare hands: the hands given that Ash of War, its behaviour script told
/// (hks/elden_force.lua JR_Cues: the anim.rs markers on it, jr_npc_cue.lua for spirits), as anim.rs does for the player.
fn cue(c: &mut ChrIns, body: Body, flip: &mut bool, free: &mut Option<Instant>) {
    if !crate::anim::prepare() {
        return;
    }
    let lua = match body {
        Body::Arts(row) => {
            crate::weapon::set_arts(arts(row), 1.2);
            "JR_Arts()"
        }
        Body::Lua(l) => l,
    };
    npc_cue(c, lua, flip);
    *free = Some(Instant::now() + Duration::from_millis(300));
}

/// its glowing look on or off (the main body glows only while it comes out of him and goes back in: solid in between)
fn look_on(c: &mut ChrIns, id: i32, on: bool) {
    let has = c.special_effect.entries().any(|e| e.param_id == id);
    if on && !has {
        c.apply_speffect(id, true);
    } else if !on && has {
        c.remove_speffect(id);
    }
}

fn lerp(a: V3, b: V3, t: f32) -> V3 {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
}

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// its own look: the spirit shader's rows (copies made by param_closure.py) given its colour, its visual pointed at its
/// shader, its effect at its visual. `flare` 0..1 brightens the rim (it flares as it comes out)
fn set_look(idx: usize, flare: f32) -> bool {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
    let id = LOOK[idx];
    let Some(ph) = repo.get_mut::<eldenring::cs::PhantomParam>(id as u32) else { return false };
    let [r, g, b] = RIM[idx];
    ph.set_edge_color_r(r);
    ph.set_edge_color_g(g);
    ph.set_edge_color_b(b);
    ph.set_edge_color_a(0.4 + 0.6 * flare);
    ph.set_edge_power(2.4 - 1.4 * flare);
    ph.set_front_color_r(r);
    ph.set_front_color_g(g);
    ph.set_front_color_b(b);
    ph.set_front_color_a(0.06 + 0.3 * flare);
    ph.set_diff_mul_color_r(255);
    ph.set_diff_mul_color_g(255);
    ph.set_diff_mul_color_b(255);
    ph.set_diff_mul_color_a(1.0);
    ph.set_spec_mul_color_a(1.0);
    ph.set_light_color_r(r);
    ph.set_light_color_g(g);
    ph.set_light_color_b(b);
    ph.set_light_color_a(0.2 * flare);
    ph.set_alpha(1.0);
    ph.set_glow_scale(0.15 + 0.85 * flare);
    let Some(v) = repo.get_mut::<eldenring::cs::SpEffectVfxParam>(id as u32) else { return false };
    v.set_phantom_param_overwrite_type(2);
    v.set_phantom_param_overwrite_id(id);
    let Some(sp) = repo.get_mut::<eldenring::cs::SpEffectParam>(id as u32) else { return false };
    sp.set_vfx_id(id);
    true
}

/// The afterimages: the game's own summon lookup is built when it loads (new BuddyParam rows never join it), so the Stand
/// is called through a group the game has of three spirits (BuddyParam 23200000..2, called by 232000), their rows made
/// copies of the human spirit's while it calls, then put back as they were.
const GROUP_TRIGGER: i32 = 232000;
const GROUP_ROWS: [u32; 3] = [23200000, 23200001, 23200002];
const SPIRIT_ROW: u32 = 25900000;
static SAVED: Mutex<Option<Vec<eldenring::param::BUDDY_PARAM_ST>>> = Mutex::new(None);

fn group_rows(on: bool) {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    let mut saved = SAVED.lock().unwrap();
    if on {
        let Some(src) = repo.get::<eldenring::cs::BuddyParam>(SPIRIT_ROW).cloned() else { return };
        if saved.is_none() {
            let orig: Vec<_> = GROUP_ROWS.iter().filter_map(|r| repo.get::<eldenring::cs::BuddyParam>(*r).cloned()).collect();
            if orig.len() != GROUP_ROWS.len() {
                return;
            }
            *saved = Some(orig);
        }
        // lab (2.0.2 bisect): noafter.on beside the log folder: the Stand alone, no afterimages
        #[cfg(feature = "lab")]
        let alone = crate::log::dir().join("noafter.on").exists();
        #[cfg(not(feature = "lab"))]
        let alone = false;
        for (k, row) in GROUP_ROWS.into_iter().enumerate() {
            if let Some(b) = repo.get_mut::<eldenring::cs::BuddyParam>(row) {
                *b = src.clone();
                b.set_trigger_sp_effect_id(if alone && k > 0 { -1 } else { GROUP_TRIGGER });
                b.set_x_offset([0.0, 0.8, -0.8][k]);
                b.set_z_offset(-1.0);
            }
        }
    } else if let Some(orig) = saved.take() {
        for (row, o) in GROUP_ROWS.into_iter().zip(orig) {
            if let Some(b) = repo.get_mut::<eldenring::cs::BuddyParam>(row) {
                *b = o;
            }
        }
    }
}

impl Stands {
    fn summon(&mut self, player: &mut PlayerIns, idx: usize) {
        let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
        let e = crate::villains::SHARED_EMPTY;
        // (2.0.2, the long-session exit: villains.rs BARE) made bare, dressed in its look once it is here (still out of sight)
        let _ = e;
        let bare = crate::villains::BARE;
        if let Some(c) = repo.get_mut::<eldenring::cs::CharaInitParam>(INIT_ROW) {
            c.set_equip_helm(bare[0]);
            c.set_equip_armer(bare[1]);
            c.set_equip_gaunt(bare[2]);
            c.set_equip_leg(bare[3]);
            c.set_equip_wep_right(HANDS);
            c.set_equip_wep_left(HANDS);
        }
        if !self.look_ready {
            self.look_ready = set_look(0, 0.0) && set_look(1, 0.0);
        }
        group_rows(true);
        let Ok(wcm) = (unsafe { WorldChrMan::instance_mut() }) else { return };
        let m = &mut wcm.summon_buddy_manager;
        // lab (2.0.2, the long-session exit: the Stand's calls): what the game's summon manager is holding
        #[cfg(feature = "lab")]
        log!("stands: summon manager: {} group owners, {} groups, {} eliminate entries, alive {}, active effect {}", m.groups.len(),
            m.groups.iter().map(|p| p.second.len()).sum::<usize>(), m.eliminate_target_entries.len(), m.player_has_alive_summon, m.active_summon_speffect_id);
        #[cfg(feature = "lab")]
        log!("stands: summon manager: stone {} active stone {} goods {} / {} warn {} range {}", m.buddy_stone_entity_id, m.active_summmon_buddy_stone_entity_id,
            m.requested_summon_goods_id, m.active_summon_goods_id, m.is_within_warn_range, m.is_within_activation_range);
        m.is_within_activation_range = true;
        m.spawn_origin = player.chr_ins.modules.physics.position;
        m.request_summon_speffect_id = GROUP_TRIGGER;
        m.requested_summon_goods_id = GROUP_TRIGGER;
        m.buddy_disappear_delay_sec = 1.0e7;
        self.restore_delay = true;
        let now = Instant::now();
        self.s = Some(Stand { idx, asked: now, handle: None, after: Vec::new(), found: None, cleaned: now, out: None, back: None, flip: [false; 3], free_until: [None; 3], pending: None, last_at: (0.0, 0.0, 0.0), seen_anim: 0 });
        log!("stands: {} called", crate::recolor::STAND_SKINS[idx].1);
    }

    fn dismiss(&mut self) {
        if self.s.take().is_some() {
            group_rows(false);
            if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
                wcm.summon_buddy_manager.disappear_requested = true;
                wcm.summon_buddy_manager.buddy_disappear_delay_sec = GAME_DELAY;
            }
            log!("stands: the Stand goes");
        }
    }

    /// leaving the world: forgotten (never touch the summon manager during a load: clones.rs)
    pub fn reset(&mut self) {
        READY.store(false, std::sync::atomic::Ordering::Relaxed);
        if self.s.take().is_some() {
            group_rows(false);
        }
        self.in_world_since = None;
        self.gave_up = false;
    }

    pub fn tick(&mut self, player: &mut PlayerIns, fighter: &str, other_summon: bool) {
        if self.s.as_ref().is_none_or(|s| s.handle.is_none()) {
            READY.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        let now = Instant::now();
        let since = *self.in_world_since.get_or_insert(now);
        let idx = crate::recolor::STAND_SKINS.iter().position(|(f, s)| *f == fighter && crate::recolor::skin_exists(s));
        let mut want = REQUEST.lock().unwrap().take();
        // (round 3) a pointing move: the point now, the Stand's part when his hand thrusts
        if let Some((secs, what)) = want.filter(|(_, w)| matches!(*w, ORA | MUDA | THRUST)) {
            crate::anims::play(player, A_POINT as u32);
            self.held = Some((secs, what, now + Duration::from_secs_f32(POINT_LEAD)));
            want = None;
        }
        if self.held.is_some_and(|(_, _, t)| now >= t) {
            want = self.held.take().map(|(secs, what, _)| (secs, what));
        }
        let Some(idx) = idx else {
            self.dismiss();
            if self.restore_delay && self.s.is_none() {
                self.restore_delay = false;
                if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
                    wcm.summon_buddy_manager.buddy_disappear_delay_sec = GAME_DELAY;
                }
            }
            return;
        };
        if self.s.as_ref().is_some_and(|s| s.idx != idx) {
            self.dismiss();
        }
        // lab (2.0.2 bisect): "standoff on": the Stand sent away (the game's own way) and not called again until "standoff off"
        #[cfg(feature = "lab")]
        if crate::lab::STAND_OFF.load(std::sync::atomic::Ordering::Relaxed) {
            self.dismiss();
            return;
        }
        // lab (2.0.2 bisect): nostand.on beside the log folder: the Stand is never called
        #[cfg(feature = "lab")]
        {
            static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            if *OFF.get_or_init(|| crate::log::dir().join("nostand.on").exists()) {
                return;
            }
        }
        if self.s.is_none() {
            if !self.gave_up && !other_summon && since.elapsed().as_secs_f32() > CALL_AFTER && player.chr_ins.modules.data.hp > 0 {
                self.summon(player, idx);
            }
            return;
        }
        let look_ready = self.look_ready;
        let s = self.s.as_mut().unwrap();
        if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
            wcm.summon_buddy_manager.buddy_disappear_delay_sec = 1.0e7;
            wcm.summon_buddy_manager.is_within_activation_range = true;
        }
        if s.handle.is_none() {
            if s.asked.elapsed() < Duration::from_millis(1200) || s.cleaned.elapsed() < Duration::from_millis(400) {
                return;
            }
            s.cleaned = now;
            let found: Vec<FieldInsHandle> = unsafe { WorldChrMan::instance() }
                .ok()
                .map(|wcm| wcm.summon_buddy_chr_set.characters().filter(|ch| ch.npc_param_id == NPC_ROW && ch.modules.data.hp > 0).map(|ch| ch.field_ins_handle).collect())
                .unwrap_or_default();
            if let Some((&h, rest)) = found.split_first() {
                s.handle = Some(h);
                s.after = rest.iter().copied().take(2).collect();
                // its look (and its afterimages'): made bare, dressed now while it is kept out of sight
                let e = crate::villains::SHARED_EMPTY;
                for h in std::iter::once(h).chain(s.after.iter().copied()) {
                    if let Some(c) = find(h) {
                        crate::villains::dress(c, [e[0], STAND_ROWS[s.idx], e[1], e[2]]);
                    }
                }
                s.found = Some(now);
                group_rows(false);
                log!("stands: the Stand is here (kept out of sight), {} afterimages", s.after.len());
            } else if s.asked.elapsed() > Duration::from_secs(8) {
                log!("stands: no Stand came");
                group_rows(false);
                self.s = None;
                self.gave_up = true;
            }
            return;
        }
        let Some(ch) = s.handle.and_then(find) else {
            self.s = None;
            return;
        };
        let formed = s.found.is_some_and(|t| t.elapsed() > Duration::from_millis(1200));
        READY.store(formed, std::sync::atomic::Ordering::Relaxed);
        let me = ability::position(player);
        let f = ability::forward(player);
        let right = (f.2, 0.0, -f.0);
        let front = (me.0 + f.0 * OUT_AHEAD + right.0 * OUT_SIDE, me.1 + HOVER, me.2 + f.2 * OUT_AHEAD + right.2 * OUT_SIDE);
        let loom = (me.0 + f.0 * LOOM_BACK + right.0 * LOOM_SIDE, me.1 + LOOM_UP, me.2 + f.2 * LOOM_BACK + right.2 * LOOM_SIDE);
        let inside = (me.0 - f.0 * 0.15, me.1, me.2 - f.2 * 0.15);
        if let Some((secs, what)) = want {
            if formed {
                let body = match what {
                    ORA | MUDA => Some(Body::Arts(REPEATING_THRUST)),
                    THRUST => Some(JAB),
                    HEAVY => Some(STRAIGHT),
                    ROAR => Some(Body::Arts(WAR_CRY)),
                    LAUNCH => Some(STRAIGHT),
                    // (lab: a raw animation id: a gesture or an event animation)
                    a => {
                        ch.modules.event.request_animation_id = a;
                        None
                    }
                };
                if let Some(b) = body {
                    if s.out.is_some() || s.back.is_some() {
                        // (an attack's own id asked of a summon breaks off what it is doing: villains.rs)
                        ch.modules.event.request_animation_id = BREAK_OFF;
                        s.pending = Some((b, now));
                    } else {
                        cue(ch, b, &mut s.flip[0], &mut s.free_until[0]);
                    }
                }
                // the user stands back and points at his enemy while it fights (1.3.1 round 2: back; the loop the user saw was the
                // landing after the dash in, played again every second: movement.rs)
                // (played when the move was asked for, POINT_LEAD before this)
                let from = if s.out.is_some() || s.back.is_some() { s.last_at } else { inside };
                let anchor = (what == LAUNCH).then(|| ((me.0 - f.0 * 0.7, me.1 + HOVER, me.2 - f.2 * 0.7), f));
                s.out = Some(Out { what, from, anchor, since: now, until: now + Duration::from_secs_f32(secs), finished: false, after_started: [false; 2] });
                s.back = None;
                log!("stands: out for {secs:.1} s ({what})");
            }
        }
        if let Some((b, at)) = s.pending {
            if at.elapsed() > Duration::from_millis(30) {
                s.pending = None;
                cue(ch, b, &mut s.flip[0], &mut s.free_until[0]);
            }
        }
        // its time out over: back into him
        if s.out.as_ref().is_some_and(|o| now >= o.until) {
            s.out = None;
            s.back = Some((now, s.last_at));
            // (and his pointing ends with it, never held on)
            if crate::movement::base_anim(crate::movement::current_anim(player)) == A_POINT {
                crate::anim::play(player, "ExecEventAllBody(\"W_Idle\")");
            }
        }
        let mut flare = 0.0;
        let mut shown = false;
        let mut opacity = OPACITY;
        if let Some(o) = s.out.as_mut() {
            let t = o.since.elapsed().as_secs_f32();
            let k = ease_out(t / EMERGE);
            let (to, face) = match o.anchor {
                Some((a, d)) => (a, d),
                None if o.what == ROAR => (loom, f),
                None => (front, f),
            };
            let at = lerp(o.from, to, k);
            place(ch, at, face);
            s.last_at = at;
            opacity = OPACITY * k;
            flare = 1.0 - k * 0.8;
            shown = true;
            // the barrage's last blow
            let left = o.until.saturating_duration_since(now).as_secs_f32();
            if matches!(o.what, ORA | MUDA) && !o.finished && left < FINISH_BEFORE {
                o.finished = true;
                cue(ch, if o.what == MUDA { Body::Arts(STORM_KICK) } else { STRAIGHT }, &mut s.flip[0], &mut s.free_until[0]);
                flare = 0.8;
            }
        } else if let Some((t0, from)) = s.back {
            let t = t0.elapsed().as_secs_f32() / RETREAT;
            if t >= 1.0 {
                s.back = None;
            } else {
                place(ch, lerp(from, inside, ease_out(t)), f);
                opacity = OPACITY * (1.0 - t);
                flare = 0.3 * (1.0 - t);
                shown = true;
            }
        }
        let free = s.free_until[0].is_some_and(|t| now < t);
        present(ch, shown && formed, free);
        // it materialises out of a glowing outline of itself and goes back the same way; solid while it fights
        let glowing = shown && (s.back.is_some() || s.out.as_ref().is_some_and(|o| o.since.elapsed().as_secs_f32() < EMERGE + 0.08));
        if look_ready {
            look_on(ch, LOOK[s.idx], glowing);
        }
        if shown {
            ch.opacity_keyframes_multiplier = opacity.max(0.02);
            set_look(s.idx, flare);
        } else {
            // at his back, out of sight, so it is already there when a move calls it
            place(ch, (me.0 - f.0 * 0.6, me.1, me.2 - f.2 * 0.6), f);
        }
        #[cfg(feature = "lab")]
        if shown {
            let ta = &ch.modules.time_act;
            let a = ta.anim_queue[(ta.read_idx as usize) % 10].anim_id;
            if a != s.seen_anim {
                s.seen_anim = a;
                log!("stands: playing {a}");
            }
        }
        // its afterimages: only in the barrage's jabs, beside and behind it, a few frames behind it
        let (after_at, after_opacity, after_glow) = AFTER.lock().unwrap().unwrap_or(([(AFTER_AT[0].0, AFTER_AT[0].1, AFTER_LAG[0]), (AFTER_AT[1].0, AFTER_AT[1].1, AFTER_LAG[1])], AFTER_OPACITY, AFTER_GLOW));
        let rush_t = s.out.as_ref().filter(|o| matches!(o.what, ORA | MUDA) && !o.finished).map(|o| o.since.elapsed().as_secs_f32());
        let mut start_after = [false; 2];
        for (k, h) in s.after.clone().into_iter().enumerate() {
            let Some(c) = find(h) else { continue };
            match rush_t {
                Some(t) if formed => {
                    let (side, back, lag) = after_at[k];
                    let k_in = ease_out((t - lag) / EMERGE);
                    let base = lerp(inside, front, k_in);
                    place(c, (base.0 + right.0 * side + f.0 * back, base.1, base.2 + right.2 * side + f.2 * back), f);
                    let on = t >= lag;
                    present(c, on, s.free_until[k + 1].is_some_and(|t| now < t));
                    c.opacity_keyframes_multiplier = after_opacity * k_in.max(0.02);
                    if look_ready {
                        look_on(c, LOOK[s.idx], after_glow);
                    }
                    start_after[k] = on;
                }
                _ => {
                    present(c, false, false);
                    place(c, (me.0 - f.0 * 0.6, me.1, me.2 - f.2 * 0.6), f);
                }
            }
        }
        if let Some(o) = s.out.as_mut() {
            for k in 0..2 {
                if start_after[k] && !o.after_started[k] {
                    o.after_started[k] = true;
                    if let Some(c) = s.after.get(k).copied().and_then(find) {
                        cue(c, Body::Arts(REPEATING_THRUST), &mut s.flip[k + 1], &mut s.free_until[k + 1]);
                        #[cfg(feature = "lab")]
                        log!("stands: afterimage {k} jabs");
                    }
                }
            }
        }
        // the spirit look off, its own look on, once it has formed (and on its afterimages)
        if formed && s.cleaned.elapsed().as_secs_f32() > 0.3 {
            s.cleaned = now;
            let keep = KEEP.lock().unwrap().clone();
            let look = LOOK[s.idx];
            for (n, h) in std::iter::once(s.handle.unwrap()).chain(s.after.iter().copied()).enumerate() {
                let Some(c) = find(h) else { continue };
                // its bare hands in the hand slots it really uses (the game leaves the main right slot empty: villains.rs)
                {
                    let p = unsafe { &mut *(c as *mut ChrIns as *mut PlayerIns) };
                    let pgd = unsafe { p.player_game_data.as_mut() };
                    for ids in [&mut p.chr_asm.equipment_param_ids, &mut pgd.equipment.chr_asm.equipment_param_ids] {
                        for k in 0..4 {
                            ids[k] = HANDS;
                        }
                    }
                }
                let ids: Vec<i32> = c.special_effect.entries().map(|e| e.param_id).filter(|id| SPIRIT_FX.contains(id) && !keep.contains(id)).collect();
                for id in ids {
                    c.remove_speffect(id);
                }
                let _ = (n, look);
                for id in keep.iter().copied() {
                    if !c.special_effect.entries().any(|e| e.param_id == id) {
                        c.apply_speffect(id, true);
                    }
                }
            }
        }
    }
}
