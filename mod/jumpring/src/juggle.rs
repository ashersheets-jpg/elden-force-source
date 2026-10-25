//! 1.2.0: juggles for the combo trees (combos.rs). A launcher knocks the enemy in front of the fighter up
//! into the air and holds it there while the fighter follows it up for an air combo; every air hit keeps
//! it up a little longer, an air finisher smashes it down, and otherwise it drops by itself.
//!
//! Only ordinary enemies about a person's size are lifted (never a boss with a health bar, nothing
//! taller than LIFT_MAX_HEIGHT or heavier than LIFT_MAX_WEIGHT): a troll or a dragon thrown about by a
//! jab would look silly. The enemy's own collision is left alone; the mod only moves its body and turns
//! its gravity off while it is held, and gives the gravity back the moment it lets go (also when the
//! enemy dies, the player leaves the world, or the enemy goes out of the list of characters around).

use std::time::{Duration, Instant};

use eldenring::cs::{ChrIns, PlayerIns, WorldChrMan};
use eldenring::position::HavokPosition;
use fromsoftware_shared::FromStatic;

use crate::ability::V3;
use crate::log;

/// Tallest and heaviest enemy a launcher lifts.
const LIFT_MAX_HEIGHT: f32 = 2.6;
const LIFT_MAX_WEIGHT: f32 = 200.0;
/// How long a launched enemy stays up without more hits, and the longest any juggle lasts.
const HOLD_S: f32 = 1.8;
const HOLD_MAX_S: f32 = 4.0;
/// How fast it rises to the hold height and falls in a spike (m/s).
const RISE_SPEED: f32 = 14.0;
const SPIKE_SPEED: f32 = 30.0;
/// how fast a held enemy is drawn in front of the fighter for an air hit (m/s)
const BRING_SPEED: f32 = 12.0;
/// 1.2.0 round 4 (user: the air hits still missed, "especially for Deku"): while the fighter is up with
/// it, the held enemy rides right in front of their chest: this far ahead, this fast (m/s), from this
/// long after the launch (it rises first)
const ATTACH_AHEAD: f32 = 1.05;
const ATTACH_SPEED: f32 = 22.0;
const ATTACH_AFTER_S: f32 = 0.18;
/// its own animations while held: a little slowed, so its hit reactions read as hang time (it no longer
/// freezes: the user, "being punched 10 feet in the air ... deserves some sort of animation")
const HELD_ANIM_SPEED: f32 = 0.8;

/// 1.2.0 round 5: how a launched enemy moves in the air. 4 "impact" (the user's pick: every motion comes
/// from a blow, the airborne pose of make_enemy_pose.py) everywhere; the lab can still switch to the
/// earlier previews (0 none, 1 sky hang, 2 tumble, 3 tumble then hang) for comparison.
#[cfg(feature = "lab")]
pub static POSE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(4);

/// prototype 4's enemy animation (lab): 1 its hit reactions play out at their own pace, 2 the launch's
/// knock-down reaction is held (frozen) through the air string, each hit replaying it a moment
#[cfg(feature = "lab")]
pub static IMPACT_STYLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn impact_style() -> u8 {
    #[cfg(feature = "lab")]
    {
        IMPACT_STYLE.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(feature = "lab"))]
    {
        0
    }
}

fn pose_mode() -> u8 {
    #[cfg(feature = "lab")]
    {
        POSE.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(feature = "lab"))]
    {
        4
    }
}

/// prototype 4 ("impact"): every motion of a launched enemy comes from a hit. The uppercut throws it up
/// (m/s) and a little away, snapping its body back (deg/s); it rises under GRAVITY to the top, then
/// hangs: falls only HANG_FALL m/s at most. Each air hit stops the world a moment (the node's hit-stop),
/// pops it up and away from the fighter, and snaps its body back again; a spring (stiffness, damping)
/// settles the body to REST deg back. It never moves itself to the fighter: the fighter goes to it.
const IMPACT_LAUNCH_UP: f32 = 10.5;
/// the enemies' thrown-upward reaction, rebuilt with Elden Force's airborne pose (src/anims.rs)
const AIRBORNE_REACTION: i32 = 8700;
/// how long our air blow has to land on the held enemy (ms)
const OPEN_MS: u64 = 300;
/// 1.2.0 round 5, third pass (the user: "he still just looks frozen in the air ... like a floating
/// mannequin"): the clip holds several reactions in a row (tools_scripts/anims/make_enemy_pose.py, its
/// SEG_* must match): the launch, four different hits, and a looping limp hang. The play time is jumped
/// between them: every air blow plays the next hit, and the hang loops in between.
const SEG_LAUNCH_END: f32 = 0.8;
const SEG_HITS: [f32; 4] = [0.8, 1.5, 2.2, 2.9];
const SEG_HIT_LEN: f32 = 0.7;
const SEG_LOOP: f32 = 3.6;
const LOOP_LEN: f32 = 1.8;
/// each hit's whole-body knock on top (deg/s): [pitch, roll] - a gut shot folds it, a blow from either
/// side rolls it, one to the head whips it back
const HIT_SPIN: [(f32, f32); 4] = [(240.0, 0.0), (60.0, 390.0), (-290.0, 0.0), (60.0, -390.0)];
/// the roll's spring (springier than the pitch: it wobbles out)
const ROLL_SPRING: f32 = 55.0;
const ROLL_DAMP: f32 = 6.0;

fn qmul(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    (
        a.3 * b.0 + a.0 * b.3 + a.1 * b.2 - a.2 * b.1,
        a.3 * b.1 - a.0 * b.2 + a.1 * b.3 + a.2 * b.0,
        a.3 * b.2 + a.0 * b.1 - a.1 * b.0 + a.2 * b.3,
        a.3 * b.3 - a.0 * b.0 - a.1 * b.1 - a.2 * b.2,
    )
}

fn qrot(q: (f32, f32, f32, f32), v: V3) -> V3 {
    let r = qmul(qmul(q, (v.0, v.1, v.2, 0.0)), (-q.0, -q.1, -q.2, q.3));
    (r.0, r.1, r.2)
}

fn axis_angle(a: V3, deg: f32) -> (f32, f32, f32, f32) {
    let (s, c) = (deg.to_radians() / 2.0).sin_cos();
    (a.0 * s, a.1 * s, a.2 * s, c)
}
const IMPACT_LAUNCH_AWAY: f32 = 1.2;
/// (positive: forward, face down over the fist - the user's Goku reference: flat, shot up from beneath)
const IMPACT_LAUNCH_SNAP: f32 = 420.0;
const IMPACT_GRAVITY: f32 = 15.0;
const IMPACT_HANG_GRAVITY: f32 = 3.0;
const IMPACT_HANG_FALL: f32 = 1.4;
const IMPACT_HIT_UP: f32 = 3.6;
const IMPACT_HIT_AWAY: f32 = 2.4;
/// an air blow's nod (its limbs' reaction is the replayed pose: src/combos.rs, make_enemy_pose.py)
const IMPACT_HIT_SNAP: f32 = 110.0;
const IMPACT_SPRING: f32 = 70.0;
const IMPACT_DAMP: f32 = 7.0;
/// the body's rest pitch: flat (make_enemy_pose.py PITCH)
const IMPACT_REST: f32 = 62.0;

/// prototype: the launch's rise (s) and height (m), the enemy flung up and slowing to the top
const PROTO_RISE_S: f32 = 0.5;
const PROTO_RISE_H: f32 = 3.8;
/// prototype: the fighter reaches it after this long (their leap), and only then does it ride in front
const PROTO_MEET_S: f32 = 0.72;

struct Held {
    /// prototype pose: the body's turn about its own right axis (deg), how fast it turns (deg/s), a hit's
    /// jolt (until when)
    turn: f32,
    turn_rate: f32,
    /// what the turn moved the body by last frame (taken off again before anything else reads it)
    comp: V3,
    /// 1.2.0 round 5: its AI is held still while it is up (a soldier raised its shield and swung in the
    /// air); its own movement / action switches from before, given back when it is let go
    ai_before: (bool, bool),
    /// prototype 4 ("impact": every motion comes from a hit): its velocity (block space, m/s), how far it
    /// is knocked back (deg) and how fast that changes, and the way the last hit pushed it (world, flat)
    vel: V3,
    tilt: f32,
    tilt_vel: f32,
    /// the roll about its long axis (deg) and its speed
    roll: f32,
    roll_vel: f32,
    /// where the clip's current reaction ends (play time), and how many blows it has taken
    seg_end: f32,
    blows: u32,
    push: (f32, f32),
    kick_until: Option<Instant>,
    chr: usize,
    /// height above the ground it was launched from (block coordinates)
    ground_y: f32,
    height: f32,
    until: Instant,
    began: Instant,
    spike: bool,
    /// our blow may hit it until then (held, it can't be hit by anything else), and its health when the
    /// opening began (the opening closes once our blow has landed)
    open: Option<(Instant, i32)>,
    /// block <-> havok offset when it was taken (its position is kept in block coordinates)
    pos_block: V3,
    /// 1.2.0: drawn to this spot (block coordinates) for an air hit: in front of the fighter, at their height
    bring: Option<V3>,
    /// its animation speed before it was held (stunned while up)
    anim_speed: f32,
}

#[derive(Default)]
pub struct Juggle {
    held: Vec<Held>,
}

fn block_offset(player: &PlayerIns) -> V3 {
    let (h, b) = (player.chr_ins.modules.physics.position, &player.block_position);
    (h.0 - b.x, h.1 - b.y, h.2 - b.z)
}

/// The characters around, by address: the mod only ever touches one it can still find here.
fn find(chr: usize) -> Option<&'static mut ChrIns> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    for e in crate::chars::near(wcm, 96).iter() {
        if e.chr_ins.as_ptr() as usize == chr {
            return Some(unsafe { &mut *e.chr_ins.as_ptr() });
        }
    }
    None
}

/// Its AI back as it was (the juggle let go of it).
fn wake(c: &mut ChrIns, before: (bool, bool)) {
    c.debug_flags.set_disabled_movement(before.0);
    c.debug_flags.set_disabled_secondary_actions(before.1);
    // its own poise again (held, it can't be staggered out of the reaction)
    c.modules.super_armor.sa_durability = c.modules.super_armor.sa_durability_max;
    c.chr_flags1c5.set_is_invincible(false);
    // and the airborne reaction ends with the juggle (the user: "the animation continued after the enemy
    // landed"): its clip run out, the enemy's own knock-down / getting up takes over
    for a in c.modules.time_act.anim_queue.iter_mut() {
        if a.anim_id > 0 && a.anim_id % 1_000_000 == AIRBORNE_REACTION {
            a.play_time = (a.anim_length - 0.02).max(a.play_time);
        }
    }
}

fn liftable(c: &ChrIns) -> bool {
    if c.modules.data.hp <= 0 || c.modules.data.max_hp <= 0 {
        return false;
    }
    // 1.3.0 (the user: villains "impossible to knock down unless you get a parry on them")
    if crate::villains::is_villain_npc(c.npc_param_id) && !crate::parry::WORLD_SLOWED.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    if crate::ledger::bosses().contains(&c.field_ins_handle) {
        return false;
    }
    let ph = &c.modules.physics;
    let tall = ph.chr_hit_height.max(ph.hit_height);
    tall <= LIFT_MAX_HEIGHT && ph.weight <= LIFT_MAX_WEIGHT
}

impl Juggle {
    /// Knock the hostile nearest to `at` (havok, within `radius`) up `height` metres. True if one went up.
    pub fn launch(&mut self, player: &PlayerIns, at: V3, radius: f32, height: f32) -> bool {
        let Ok(wcm) = (unsafe { WorldChrMan::instance() }) else { return false };
        let me = &player.chr_ins as *const _ as usize;
        let mut best: Option<(f32, usize)> = None;
        for e in crate::chars::near(wcm, 64).iter() {
            let addr = e.chr_ins.as_ptr() as usize;
            if addr == me {
                continue;
            }
            let c = unsafe { e.chr_ins.as_ref() };
            if !crate::combat::hostile(c.team_type) || !liftable(c) {
                continue;
            }
            let p = c.modules.physics.position;
            let d = ((p.0 - at.0).powi(2) + (p.2 - at.2).powi(2)).sqrt();
            if d <= radius && (p.1 - at.1).abs() < 3.0 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, addr));
            }
        }
        let Some((_, chr)) = best else { return false };
        let off = block_offset(player);
        let Some(c) = find(chr) else { return false };
        let p = c.modules.physics.position;
        let now = Instant::now();
        let pos_block = (p.0 - off.0, p.1 - off.1, p.2 - off.2);
        if let Some(h) = self.held.iter_mut().find(|h| h.chr == chr) {
            // launched again while still up: higher, longer
            h.height = (h.height + height * 0.5).min(height * 1.5);
            h.until = (now + Duration::from_secs_f32(HOLD_S)).min(h.began + Duration::from_secs_f32(HOLD_MAX_S));
            h.spike = false;
            return true;
        }
        c.modules.physics.gravity_disabled = true;
        let ai_before = (c.debug_flags.disabled_movement(), c.debug_flags.disabled_secondary_actions());
        c.debug_flags.set_disabled_movement(true);
        c.debug_flags.set_disabled_secondary_actions(true);
        let anim_speed = c.modules.behavior.animation_speed;
        let fp = player.chr_ins.modules.physics.position;
        let (dx, dz) = (p.0 - fp.0, p.2 - fp.2);
        let dl = (dx * dx + dz * dz).sqrt().max(0.01);
        let push = (dx / dl, dz / dl);
        // prototype 4: the uppercut throws it up and a little away, snapping its body back
        self.held.push(Held { turn: 0.0, turn_rate: -620.0, comp: (0.0, 0.0, 0.0), ai_before, vel: (push.0 * IMPACT_LAUNCH_AWAY, IMPACT_LAUNCH_UP, push.1 * IMPACT_LAUNCH_AWAY), tilt: 0.0, tilt_vel: IMPACT_LAUNCH_SNAP, roll: 0.0, roll_vel: 0.0, seg_end: SEG_LAUNCH_END, blows: 0, push, kick_until: Some(now + Duration::from_millis(if impact_style() == 2 { 300 } else { 380 })), chr, ground_y: pos_block.1, height, until: now + Duration::from_secs_f32(HOLD_S), began: now, spike: false, open: None, pos_block, bring: None, anim_speed });
        log!("juggle: launched an enemy ({:.1} m up)", height);
        true
    }

    /// The enemies in the air stay up `secs` longer (an air hit), never past HOLD_MAX_S in all.
    pub fn extend(&mut self, secs: f32) {
        let now = Instant::now();
        for h in &mut self.held {
            h.until = (h.until.max(now) + Duration::from_secs_f32(secs)).min(h.began + Duration::from_secs_f32(HOLD_MAX_S));
            // prototype: every air hit jolts it
            h.kick_until = Some(now + Duration::from_millis(180));
            h.turn_rate = h.turn_rate.min(-300.0);
        }
    }

    /// prototype 4: an air blow lands on the held enemies from `from` (havok, the fighter)
    pub fn hit(&mut self, from: V3) {
        let now = Instant::now();
        for h in &mut self.held {
            let Some(c) = find(h.chr) else { continue };
            let p = c.modules.physics.position;
            let (dx, dz) = (p.0 - h.comp.0 - from.0, p.2 - h.comp.2 - from.2);
            let dl = (dx * dx + dz * dz).sqrt().max(0.01);
            h.push = (dx / dl, dz / dl);
            h.vel = (h.vel.0 + h.push.0 * IMPACT_HIT_AWAY, h.vel.1.max(0.0) + IMPACT_HIT_UP, h.vel.2 + h.push.1 * IMPACT_HIT_AWAY);
            // the next of the clip's four hit reactions (make_enemy_pose.py), and its whole-body knock
            let k = (h.blows % 4) as usize;
            h.blows += 1;
            h.tilt_vel += IMPACT_HIT_SNAP * 0.3 + HIT_SPIN[k].0;
            h.roll_vel += HIT_SPIN[k].1;
            let mut replayed = false;
            for a in c.modules.time_act.anim_queue.iter_mut() {
                if a.anim_id > 0 && a.anim_id % 1_000_000 == AIRBORNE_REACTION {
                    a.play_time = SEG_HITS[k];
                    replayed = true;
                }
            }
            h.seg_end = SEG_HITS[k] + SEG_HIT_LEN;
            #[cfg(feature = "lab")]
            log!("lab juggle: air blow, reaction {k} played {replayed}");
            let _ = replayed;
            h.kick_until = Some(now + Duration::from_millis(if impact_style() == 2 { 130 } else { 320 }));
        }
    }

    pub fn impact_mode() -> bool {
        pose_mode() == 4
    }

    /// 1.2.0: every held enemy is drawn to `at` (havok: in front of the fighter, at their feet' height).
    pub fn bring(&mut self, player: &PlayerIns, at: V3) {
        let off = block_offset(player);
        for h in &mut self.held {
            h.bring = Some((at.0 - off.0, at.1 - off.1, at.2 - off.2));
        }
    }

    /// Our blow is on its way: the held enemies can be hit for a moment (until it lands).
    pub fn open(&mut self) {
        let now = Instant::now();
        for h in &mut self.held {
            let Some(c) = find(h.chr) else { continue };
            h.open = Some((now + Duration::from_millis(OPEN_MS), c.modules.data.hp));
            c.chr_flags1c5.set_is_invincible(false);
        }
    }

    /// An air finisher: everything held is driven straight down.
    pub fn spike(&mut self) {
        let now = Instant::now();
        for h in &mut self.held {
            h.spike = true;
            h.until = now + Duration::from_millis(600);
        }
    }

    pub fn holding(&self) -> bool {
        !self.held.is_empty()
    }

    /// 1.2.0 round 4: where every held enemy really is now (havok), for the air blows.
    pub fn positions(&self) -> Vec<V3> {
        self.held
            .iter()
            .filter(|h| !h.spike)
            .filter_map(|h| find(h.chr).map(|c| (c.modules.physics.position, h.comp)))
            .map(|(p, k)| (p.0 - k.0, p.1 - k.1, p.2 - k.2))
            .collect()
    }

    /// The highest held enemy's position (havok), for the air combo to follow.
    pub fn target(&self, player: &PlayerIns) -> Option<V3> {
        let off = block_offset(player);
        let impact = pose_mode() == 4;
        self.held.first().map(|h| (h.pos_block.0 + off.0, if impact { h.pos_block.1 } else { h.ground_y + h.height } + off.1, h.pos_block.2 + off.2))
    }

    /// Every frame in the world. `with_fighter`: the fighter is up in the air with it (an air combo).
    pub fn tick(&mut self, player: &PlayerIns, dt: f32, with_fighter: bool, slow: f32) {
        if self.held.is_empty() {
            return;
        }
        if pose_mode() == 4 {
            self.impact_tick(player, dt * slow);
            return;
        }
        let off = block_offset(player);
        // right in front of the fighter's chest, at their height (block coordinates)
        let fp = player.chr_ins.modules.physics.position;
        let fw = crate::ability::forward(player);
        let front = (fp.0 + fw.0 * ATTACH_AHEAD - off.0, fp.1 + 0.05 - off.1, fp.2 + fw.2 * ATTACH_AHEAD - off.2);
        let now = Instant::now();
        self.held.retain_mut(|h| {
            let Some(c) = find(h.chr) else { return false };
            let done = c.modules.data.hp <= 0 || now >= h.until;
            if done {
                wake(c, h.ai_before);
                c.chr_ctrl.additional_orientation_quat = fromsoftware_shared::F32Vector4(0.0, 0.0, 0.0, 1.0);
                c.modules.physics.gravity_disabled = false;
                c.modules.behavior.animation_speed = h.anim_speed.max(0.5);
                return false;
            }
            // its own hit reactions play while it is up (a little slowed: hang time)
            c.modules.behavior.animation_speed = if h.spike { h.anim_speed.max(0.5) } else { HELD_ANIM_SPEED };
            let p = c.modules.physics.position;
            // where it is now (it may have been pushed by the hits), height on top of the launch spot
            let cur = (p.0 - off.0 - h.comp.0, p.1 - off.1 - h.comp.1, p.2 - off.2 - h.comp.2);
            let meet = if pose_mode() != 0 { PROTO_MEET_S } else { ATTACH_AFTER_S };
            let attached = with_fighter && !h.spike && now.duration_since(h.began).as_secs_f32() >= meet;
            if attached {
                // riding in front of the fighter: every air blow reaches it
                let (dx, dy, dz) = (front.0 - cur.0, front.1 - cur.1, front.2 - cur.2);
                let d = (dx * dx + dy * dy + dz * dz).sqrt();
                let step = (ATTACH_SPEED * dt).min(d);
                h.pos_block = if d > 1e-3 { (cur.0 + dx / d * step, cur.1 + dy / d * step, cur.2 + dz / d * step) } else { front };
                // and it stays up at least as long as the fighter does
                h.until = h.until.max(now + Duration::from_millis(250)).min(h.began + Duration::from_secs_f32(HOLD_MAX_S));
            } else {
                let want = if h.spike { h.ground_y } else { h.bring.map(|b| b.1).unwrap_or(h.ground_y + h.height) };
                let speed = if h.spike { SPIKE_SPEED } else { RISE_SPEED };
                let dy = (want - cur.1).clamp(-speed * dt, speed * dt);
                // drawn across to the fighter's front (quickly, never a jump)
                let (mut nx, mut nz) = (cur.0, cur.2);
                if let (Some(bp), false) = (h.bring, h.spike) {
                    let (dx, dz) = (bp.0 - cur.0, bp.2 - cur.2);
                    let d = (dx * dx + dz * dz).sqrt();
                    let step = (BRING_SPEED * dt).min(d);
                    if d > 1e-3 {
                        nx += dx / d * step;
                        nz += dz / d * step;
                    }
                }
                h.pos_block = (nx, cur.1 + dy, nz);
            }
            if h.spike && h.pos_block.1 <= h.ground_y + 0.05 {
                wake(c, h.ai_before);
                c.chr_ctrl.additional_orientation_quat = fromsoftware_shared::F32Vector4(0.0, 0.0, 0.0, 1.0);
                c.modules.behavior.animation_speed = h.anim_speed.max(0.5);
                c.modules.physics.gravity_disabled = false;
                return false;
            }
            // prototype: the rise (flung up, slowing to the top) and the body's pose, turned about its middle
            let mode = pose_mode();
            let mut comp = (0.0f32, 0.0f32, 0.0f32);
            if mode != 0 {
                let t = now.duration_since(h.began).as_secs_f32();
                if !h.spike && !(attached && t >= PROTO_MEET_S) {
                    let u = (t / PROTO_RISE_S).min(1.0);
                    h.pos_block.1 = h.ground_y + PROTO_RISE_H * (1.0 - (1.0 - u) * (1.0 - u));
                }
                let kicked = h.kick_until.is_some_and(|k| now < k);
                let settle = |turn: f32, want: f32, dt: f32| -> f32 {
                    // to the nearest turn equal to `want` (whole flips apart), smoothly
                    let k = ((turn - want) / 360.0).round();
                    let goal = want + k * 360.0;
                    turn + (goal - turn) * (1.0 - (-9.0 * dt).exp())
                };
                let speed;
                if h.spike {
                    h.turn += -720.0 * dt;
                    speed = 1.0;
                } else {
                    match mode {
                        1 => {
                            // A: the hit reaction plays a moment, then holds; tipped back toward the sky
                            let want = -100.0 + if kicked { 28.0 } else { 0.0 };
                            h.turn = settle(h.turn, want, dt * 0.6);
                            speed = if t < 0.22 || kicked { 1.0 } else { 0.02 };
                        }
                        2 => {
                            // B: flails while it somersaults, slowing at the top, every hit spins it on
                            let base = -560.0 + 340.0 * (t / 0.6).min(1.0);
                            h.turn_rate = if kicked { h.turn_rate.min(-480.0) } else { h.turn_rate + (base - h.turn_rate) * (1.0 - (-4.0 * dt).exp()) };
                            h.turn += h.turn_rate * dt;
                            speed = 0.85;
                        }
                        _ => {
                            // B then A: flipped up spinning, slowing into the sky hang at the top
                            if t < 0.5 && !kicked {
                                h.turn += -620.0 * (1.0 - t / 0.5) * dt;
                            } else if kicked {
                                h.turn += -520.0 * dt;
                            } else {
                                h.turn = settle(h.turn, -100.0, dt);
                            }
                            speed = if t < 0.3 || kicked { 1.0 } else { 0.02 };
                        }
                    }
                }
                c.modules.behavior.animation_speed = speed;
                // the model turns (ChrCtrl's additional orientation, display only) about its right axis; the
                // body is moved by what that turn does to its middle (1 m up), so it spins about its middle
                let th = h.turn.to_radians();
                let (s2, c2) = (th / 2.0).sin_cos();
                c.chr_ctrl.additional_orientation_quat = fromsoftware_shared::F32Vector4(s2, 0.0, 0.0, c2);
                let local = (0.0f32, 1.0 - th.cos(), -th.sin());
                let q = c.modules.physics.orientation;
                let qv = (q.0, q.1, q.2);
                let tt = (2.0 * (qv.1 * local.2 - qv.2 * local.1), 2.0 * (qv.2 * local.0 - qv.0 * local.2), 2.0 * (qv.0 * local.1 - qv.1 * local.0));
                comp = (local.0 + q.3 * tt.0 + (qv.1 * tt.2 - qv.2 * tt.1), local.1 + q.3 * tt.1 + (qv.2 * tt.0 - qv.0 * tt.2), local.2 + q.3 * tt.2 + (qv.0 * tt.1 - qv.1 * tt.0));
            }
            #[cfg(feature = "lab")]
            {
                static LASTLOG: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
                let mut l = LASTLOG.lock().unwrap();
                if l.is_none_or(|t| t.elapsed().as_secs_f32() > 0.1) {
                    *l = Some(now);
                    log!("lab juggle: t {:.2} foe ({:.1}, {:.1}, {:.1}) was ({:.1}, {:.1}, {:.1}) me ({:.1}, {:.1}, {:.1}) turn {:.0} attached {attached} spike {} comp ({:.1}, {:.1}, {:.1}) anim {}", now.duration_since(h.began).as_secs_f32(), h.pos_block.0 + off.0, h.pos_block.1 + off.1, h.pos_block.2 + off.2, p.0, p.1, p.2, fp.0, fp.1, fp.2, h.turn, h.spike, comp.0, comp.1, comp.2, { let ta = &c.modules.time_act; ta.anim_queue[(ta.read_idx as usize) % 10].anim_id });
                }
            }
            h.comp = comp;
            let ph = &mut c.modules.physics;
            ph.position = HavokPosition(h.pos_block.0 + off.0 + comp.0, h.pos_block.1 + off.1 + comp.1, h.pos_block.2 + off.2 + comp.2, p.3);
            ph.chr_proxy_pos_update_requested = true;
            ph.gravity_disabled = true;
            c.modules.fall.fall_timer = 0.0;
            true
        });
    }

    /// prototype 4: the hit-driven juggle (see IMPACT_*), one frame of `dt` game seconds.
    fn impact_tick(&mut self, player: &PlayerIns, dt: f32) {
        let off = block_offset(player);
        let now = Instant::now();
        self.held.retain_mut(|h| {
            let Some(c) = find(h.chr) else { return false };
            let reset = |c: &mut ChrIns, speed: f32| {
                c.chr_ctrl.additional_orientation_quat = fromsoftware_shared::F32Vector4(0.0, 0.0, 0.0, 1.0);
                c.modules.physics.gravity_disabled = false;
                c.modules.behavior.animation_speed = speed;
            };
            if c.modules.data.hp <= 0 || now >= h.until {
                wake(c, h.ai_before);
                reset(c, h.anim_speed.max(0.5));
                return false;
            }
            let p = c.modules.physics.position;
            // where the mod has it (its own hit reactions' root motion would carry it off: the mod alone
            // moves it while it is up)
            let cur = h.pos_block;
            // motion: thrown, slowing to the top, then hanging (or smashed down)
            if h.spike {
                // slammed down in front of the fighter (the blows' knock-away had carried it past the finisher)
                let fp = player.chr_ins.modules.physics.position;
                let fw = crate::ability::forward(player);
                let front = (fp.0 + fw.0 * 1.0 - off.0, fp.2 + fw.2 * 1.0 - off.2);
                h.vel = ((front.0 - cur.0) * 8.0, -SPIKE_SPEED, (front.1 - cur.2) * 8.0);
            } else {
                let g = if h.vel.1 > 0.0 { IMPACT_GRAVITY } else { IMPACT_HANG_GRAVITY };
                h.vel.1 = (h.vel.1 - g * dt).max(-IMPACT_HANG_FALL);
                let k = (-4.0 * dt).exp();
                h.vel.0 *= k;
                h.vel.2 *= k;
            }
            let mut np = (cur.0 + h.vel.0 * dt, cur.1 + h.vel.1 * dt, cur.2 + h.vel.2 * dt);
            if np.1 <= h.ground_y {
                np.1 = h.ground_y;
                h.pos_block = np;
                let ph = &mut c.modules.physics;
                ph.position = HavokPosition(np.0 + off.0, np.1 + off.1, np.2 + off.2, p.3);
                ph.chr_proxy_pos_update_requested = true;
                h.comp = (0.0, 0.0, 0.0);
                wake(c, h.ai_before);
                reset(c, h.anim_speed.max(0.5));
                return false;
            }
            h.pos_block = np;
            // held, nothing staggers it out of the airborne reaction: an air kick's own hitbox (its
            // animation's, besides our blow) hit it with a stagger of its own, and the reaction was gone
            // (poise doesn't stop that one). Only our blow can hit it, in a short opening that closes once
            // the blow has landed. The finisher's slam can hit it freely.
            if let Some((until, hp)) = h.open {
                if now >= until || c.modules.data.hp < hp {
                    h.open = None;
                }
            }
            c.chr_flags1c5.set_is_invincible(!h.spike && h.open.is_none());
            if !h.spike {
                c.modules.super_armor.sa_durability = 1.0e6;
            }
            // the clip: a reaction that has played out goes on into the limp hang, which loops
            if !h.spike {
                for a in c.modules.time_act.anim_queue.iter_mut() {
                    if a.anim_id > 0 && a.anim_id % 1_000_000 == AIRBORNE_REACTION && a.play_time >= h.seg_end {
                        if h.seg_end >= SEG_LOOP + LOOP_LEN - 1e-3 {
                            a.play_time = SEG_LOOP + (a.play_time - h.seg_end).min(LOOP_LEN);
                        } else {
                            a.play_time = SEG_LOOP;
                            h.seg_end = SEG_LOOP + LOOP_LEN;
                        }
                    }
                }
            }
            // the roll wobbles out
            let racc = -ROLL_SPRING * h.roll - ROLL_DAMP * h.roll_vel;
            h.roll_vel += racc * dt;
            h.roll += h.roll_vel * dt;
            // the body: snapped back by each hit, a spring settles it a little laid back
            let rest = if h.spike { 85.0 } else { IMPACT_REST };
            let acc = -IMPACT_SPRING * (h.tilt - rest) - IMPACT_DAMP * h.tilt_vel;
            h.tilt_vel += acc * dt;
            h.tilt += h.tilt_vel * dt;
            // the airborne pose plays at its own pace (its reactions and the limp hang are all in the clip;
            // an earlier preview's slowing left the hang all but stopped)
            let kicked = h.kick_until.is_some_and(|k| now < k);
            c.modules.behavior.animation_speed = match impact_style() {
                2 => {
                    if h.spike || kicked {
                        1.0
                    } else {
                        0.02
                    }
                }
                _ => 1.0,
            };
            // turn about its own right axis (positive: forward, face down - whichever way it faces: it is
            // turned away for the frames a blow lands), about its middle
            let q0 = c.modules.physics.orientation;
            let yaw = 2.0 * q0.1.atan2(q0.3);
            let fwd = (-yaw.sin(), -yaw.cos());
            let a = (fwd.1, 0.0f32, -fwd.0);
            // pitched about its right axis, then rolled about its (now level) long axis, in the world
            let qw = qmul(axis_angle((fwd.0, 0.0, fwd.1), h.roll), axis_angle(a, h.tilt));
            let v = (0.0f32, 1.0f32, 0.0f32);
            let rv = qrot(qw, v);
            let comp = (v.0 - rv.0, v.1 - rv.1, v.2 - rv.2);
            // the model's turn is in its own frame: the world axis turned back by its yaw
            // the model's turn is in its own frame: yaw^-1 * world turn * yaw
            let q = c.modules.physics.orientation;
            let yaw = (q.0, q.1, q.2, q.3);
            let local = qmul(qmul((-yaw.0, -yaw.1, -yaw.2, yaw.3), qw), yaw);
            c.chr_ctrl.additional_orientation_quat = fromsoftware_shared::F32Vector4(local.0, local.1, local.2, local.3);
            h.comp = comp;
            let ph = &mut c.modules.physics;
            ph.position = HavokPosition(np.0 + off.0 + comp.0, np.1 + off.1 + comp.1, np.2 + off.2 + comp.2, p.3);
            ph.chr_proxy_pos_update_requested = true;
            ph.gravity_disabled = true;
            c.modules.fall.fall_timer = 0.0;
            #[cfg(feature = "lab")]
            {
                static LASTLOG: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
                let mut l = LASTLOG.lock().unwrap();
                if l.is_none_or(|t| t.elapsed().as_secs_f32() > 0.1) {
                    *l = Some(now);
                    let fp = player.chr_ins.modules.physics.position;
                    log!("lab impact: t {:.2} foe ({:.1}, {:.1}, {:.1}) vel ({:.1}, {:.1}, {:.1}) me ({:.1}, {:.1}, {:.1}) tilt {:.0}", now.duration_since(h.began).as_secs_f32(), np.0 + off.0, np.1 + off.1, np.2 + off.2, h.vel.0, h.vel.1, h.vel.2, fp.0, fp.1, fp.2, h.tilt);
                }
            }
            true
        });
    }

    /// Let go of everything (the player leaves the world, or the fighter changes).
    pub fn release(&mut self) {
        for h in self.held.drain(..) {
            if let Some(c) = find(h.chr) {
                wake(c, h.ai_before);
                c.chr_ctrl.additional_orientation_quat = fromsoftware_shared::F32Vector4(0.0, 0.0, 0.0, 1.0);
                c.modules.physics.gravity_disabled = false;
                c.modules.behavior.animation_speed = h.anim_speed.max(0.5);
            }
        }
    }

    /// Leaving the world: the characters are gone, forget them without touching anything.
    pub fn forget(&mut self) {
        self.held.clear();
    }
}
