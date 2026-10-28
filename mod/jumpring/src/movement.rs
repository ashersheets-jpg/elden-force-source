//! Fighter movement (hooks.movement): faster and more fluid than the Tarnished's, in the spirit of an
//! anime brawler. Measured in the lab: the physics motion multiplier scales distance without touching
//! the animation (run 3.9 -> 5.9 m/s at 1.5), the behaviour animation speed shortens a dodge (0.75 s ->
//! 0.52 s at 1.5).
//! - running, walking and jumps cover 1.5x the ground
//! - dodges (rolls and backsteps) play 1.75x as fast and go further (0.10.0: faster)
//! - double jump: jump again in the air
//! - air dash: dodge in the air for a burst the way the stick points (side-dashes), once per jump;
//!   0.10.0: faster, and the stick keeps steering it while it lasts. Goku and All Might fly instead
//!   (fighters.air_dash "fly"): a faster, longer Superman flight, fist first
//! - full air control (0.10.0): after a double jump or an air dash the fighter goes where the stick
//!   points, and the stick also steers a normal jump
//! - slam: crouch in the air to dive straight down and shake the ground where you land
//! - slide: crouch while running to slide forward into the crouch
//! - Deku's Black Whip (0.10.0): a tendril to a wall, tree or ledge ahead and above, and he swings
//!   from it like Spider-Man, letting go with his momentum (combat.rs pulls him to an enemy instead).
//!   0.11.0: it grips something real near him (the top edge of a wall or roof, high on a trunk or a
//!   pillar), preferring high and ahead, and swings as a true pendulum: the rope adds no speed, the
//!   swing and the let-go are capped, and it can't be thrown again for a moment. With nothing in
//!   reach it shoots out and snaps back.
//! Each move also asks the behaviour script for a body animation (anim.rs).
//! Animation ids (from the lab trace): 20xxx/22xxx locomotion, 27xxx/28xxx dodges, 2020xx jumps,
//! 39xxxx crouch.

use std::time::Instant;

use eldenring::cs::{CSHavokMan, PlayerIns};
use eldenring::position::{HavokPosition, PositionDelta};
use fromsoftware_shared::FromStatic;

use crate::ability::{self, V3};
use crate::anim::{self, Cue};
use crate::input;
use crate::log;
use crate::motion::{MAP_FILTER, Motion};

pub const RUN_MULT: f32 = 1.5;
pub const DODGE_SPEED: f32 = 1.75;
const DODGE_DIST: f32 = 1.45;

/// Our flight (m/s, m/s^2).
const JUMP_UP: f32 = 7.5;
const GRAVITY_UP: f32 = 13.0;
const GRAVITY_DOWN: f32 = 9.5;
const MAX_FALL: f32 = 16.0;
const DASH_SPEED: f32 = 31.0;
const DASH_TIME: f32 = 0.2;
const DASH_CARRY: f32 = 11.0;
/// Goku and All Might: Superman flight
const FLY_SPEED: f32 = 42.0;
const FLY_TIME: f32 = 0.4;
const FLY_CARRY: f32 = 16.0;
const FLY_LIFT: f32 = 1.5;
/// air control: the speed the stick can reach, how fast the body answers it, how fast it turns a dash
const AIR_MAX: f32 = 10.0;
const AIR_RESPONSE: f32 = 5.0;
const DASH_TURN: f32 = 9.0;
const AIR_DRAG: f32 = 0.35;
/// the stick's pull on the game's own jump (on top of the jump's own motion)
const JUMP_STEER: f32 = 4.5;
const SLAM_SPEED: f32 = 32.0;
/// Black Whip: how far it reaches, the swing's gravity, its longest hold, top speed, and the time
/// before it can be thrown again (0.11.0: it could be spammed and the speed compounded)
const WHIP_RANGE: f32 = 34.0;
const WHIP_GRAVITY: f32 = 20.0;
/// 1.2.0: Asta's glider: how fast he sinks and cruises (m/s), how quickly the stick turns it, when the
/// ride pose is held still (s: upright, surfing), and the board ride's moveset (the Dismounter, a curved
/// greatsword). His body shows his blade under his feet meanwhile (look.rs BOARD).
const GLIDE_SINK: f32 = 2.4;
const GLIDE_SPEED: f32 = 9.0;
const GLIDE_TURN: f32 = 2.5;
const GLIDE_POSE_AT: f32 = 2.4;
/// the hop onto the blade plays this many times faster, so he's up and surfing within a second
const GLIDE_RAMP: f32 = 2.5;
/// lab: another moment to hold the ride pose at (ms; 0: GLIDE_POSE_AT), script "glidepose"
pub static LAB_GLIDE_POSE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// lab: another weapon row for the ride (script "gliderow")
pub static LAB_GLIDE_ROW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// lab: which board body (script "boardbody": 1 bound to his pelvis, 2 to his right foot)
pub static LAB_BOARD_BODY: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

fn glide_pose_at() -> f32 {
    match LAB_GLIDE_POSE.load(std::sync::atomic::Ordering::Relaxed) {
        0 => GLIDE_POSE_AT,
        ms => ms as f32 / 1000.0,
    }
}
const GLIDE_BOARD: u32 = 8_020_000;
const WHIP_MAX_S: f32 = 3.5;
const WHIP_MAX_SPEED: f32 = 26.0;
const WHIP_COOLDOWN: f32 = 0.45;
/// a swing from (nearly) standing starts with this much speed along the swing
const WHIP_KICK: f32 = 9.0;
/// 0.13.0 (Spider-Man swinging): above this speed (m/s) the swing follows the way he travels, and a
/// grip to the side moves the swing's pivot only this much of the way toward it
const WHIP_MOVING: f32 = 4.0;
const WHIP_SIDE_KEEP: f32 = 0.22;
/// the longest rope a swing uses (m), and how fast the slack is taken up (m/s)
const WHIP_MAX_ROPE: f32 = 16.0;
const WHIP_REEL: f32 = 12.0;
/// the pivot is at most this high over him (m)
const WHIP_MAX_RISE: f32 = 20.0;
/// the swing drives itself along up to a cruising speed (m/s, m/s^2); the stick turns it (rad/s)
const WHIP_CRUISE: f32 = 17.0;
const WHIP_DRIVE: f32 = 7.0;
const WHIP_STEER: f32 = 0.9;
/// let go near the top of the far side: thrown up this fast (about 3.5 m with GRAVITY_UP)
const WHIP_PEAK_LIFT: f32 = 9.5;
/// top speed along the ground after letting go, and the next throw while the whip is held
const WHIP_RELEASE_MAX: f32 = 24.0;
const WHIP_CHAIN_AFTER: f32 = 0.3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Locomotion,
    Dodge,
    Jump,
    Crouch,
    Other,
}

fn kind(anim: i32) -> Kind {
    match anim {
        20000..=22999 => Kind::Locomotion,
        27000..=28999 => Kind::Dodge,
        202000..=202999 => Kind::Jump,
        390000..=399999 => Kind::Crouch,
        _ => Kind::Other,
    }
}

/// The mod's own air poses (fly, air dash, Black Whip, flying kick, Luffy's swing and, 0.16.0, the
/// swing fall dive): never on the ground.
pub fn own_air(anim: i32) -> bool {
    matches!(anim, 60220 | 60241 | 60350 | 60360 | 67030 | DIVE_ANIM)
}

/// 0.16.0: the skydive pose (anims.json "dive").
const DIVE_ANIM: i32 = 65030;
/// Falling this long (or this fast) in a flight after a swing: the dive pose.
const DIVE_AFTER_S: f32 = 0.2;
const DIVE_SPEED: f32 = 14.0;
/// No dive with the ground this close below.
const DIVE_CLEAR: f32 = 4.0;
/// Falling this long in a flight of ours ends in the heavy (superhero) landing.
const HARD_LANDING_S: f32 = 1.2;
/// 0.16.0: a flyer's flight under this horizontal speed (after its burst) has lost its momentum.
const FLY_STALL: f32 = 3.0;

// 1.2.0: each fighter's own way of moving (the user: "movement that plays into their abilities in the
// anime, but balanced for our game"). Luffy and Deku swing (above); the rest:
/// over the ground faster than this (m/s) counts as sprinting
const SPRINT: f32 = 6.5;
/// the dash attack still follows a dodge this long after it ends
const DASH_AFTER_S: f32 = 0.4;
/// Naruto: shadow clones throw him (in place of his double jump), twice per jump; sprinting into a wall
/// and jumping, he runs up it (tree-climbing practice) and flips off the top
const CLONE_THROWS: u8 = 3;
const CLONE_FORWARD: f32 = 13.0;
const CLONE_UP: f32 = 6.5;
const WALL_RUN_UP: f32 = 7.0;
const WALL_RUN_S: f32 = 0.75;
/// Bakugo: holding jump in the air after his double jump, explosions from his palms push him where the
/// stick points (up without it), a blast every BLAST_EVERY_S, each costing stamina
const BLAST_EVERY_S: f32 = 0.22;
const BLAST_PUSH: f32 = 11.5;
const BLAST_LIFT: f32 = 5.0;
const BLAST_STAMINA: i32 = 14;
/// blasts per jump (refilled on landing): about three seconds of flight, never a hover
const BLAST_MAX: u8 = 12;
/// 1.3.0 (the user found his movement "potentially overpowered", then on review: "Stays the same outside of boss
/// combat, but when fighting a boss it drains stamina and goes down to 6"): 6 a jump while a boss bar is up
const BLAST_MAX_BOSS: u8 = 6;
/// Todoroki: dodging out of a sprint, he surfs a ramp of ice; jump on it (or its end) launches him up.
/// His air dash leaves fire behind.
const ICE_SLIDE_SPEED: f32 = 18.0;
const ICE_SLIDE_S: f32 = 1.4;
const ICE_SLIDE_COOLDOWN_S: f32 = 2.5;
const ICE_LAUNCH_UP: f32 = 10.0;
/// Asta: jumping from a sprint or a crouch, an anti-magic leap; in the air next to a wall, jump drives his
/// sword into it and kicks off
const ASTA_LEAP_FORWARD: f32 = 22.0;
const ASTA_LEAP_UP: f32 = 11.5;
/// Goku: Instant Transmission: dodge twice quickly while locked on, and he is behind the enemy
const TELEPORT_BEHIND: f32 = 2.4;
const TELEPORT_COOLDOWN_S: f32 = 4.0;
/// All Might: jumping from a sprint, a Smash jump across the land; he lands with a shockwave that knocks
/// enemies back (no damage)
const SMASH_JUMP_FORWARD: f32 = 21.0;
const SMASH_JUMP_UP: f32 = 15.0;
const SMASH_JUMP_COOLDOWN_S: f32 = 4.0;

pub fn current_anim(player: &PlayerIns) -> i32 {
    let ta = &player.chr_ins.modules.time_act;
    ta.anim_queue[(ta.read_idx as usize) % ta.anim_queue.len()].anim_id
}

/// The animation without its weapon section: with a weapon in hand the game plays its moveset's copy
/// (a002_202000 is the jump with a straight sword: id 2202000). 0.13.0: Asta's held sword made every
/// jump and roll unknown here, so he had no double jump, air dash or faster dodge.
pub fn base_anim(anim: i32) -> i32 {
    if anim >= 1_000_000 { anim % 1_000_000 } else { anim }
}

/// Our own flight after a double jump, an air dash or a slam: velocity integrated every frame until
/// the feet touch the ground.
struct Flight {
    vel: V3,
    /// a dash burst still running (seconds left) and its direction
    dash_left: f32,
    dash_dir: V3,
    slam: bool,
    /// Superman flight (Goku, All Might)
    fly: bool,
    /// 0.16.0: how long it has been falling, and when the dive pose was last asked for
    fall_time: f32,
    dived: Option<Instant>,
    /// 1.3.0: when the flight pose was last asked for (it is held while the flight lasts)
    posed: Option<Instant>,
    last: Instant,
    /// top horizontal speed air control may reach (the speed we came in with, at least AIR_MAX)
    cap: f32,
    /// 1.2.0: Asta rides his blade down (the glider): since when
    glide: Option<Instant>,
}

/// Deku swinging from his Black Whip.
struct Swing {
    /// the point the swing turns about (above his path), and where the tendril holds
    anchor: V3,
    grip: V3,
    len: f32,
    /// the rope's length once the slack is taken up
    max_len: f32,
    /// the way the swing carries him (horizontal unit vector)
    dir: V3,
    vel: V3,
    since: Instant,
    last: Instant,
    /// where the body was put last frame
    put: V3,
    /// 1.2.0 round 4: the last spot the body was known to be in the open (the user's video: a swing
    /// along a cliff face carried Deku into the rock, twice): every move is checked from here
    safe: V3,
    /// how long the swing has been pressed against a wall
    wall_time: f32,
    /// where the body really was last frame
    prev: Option<V3>,
    /// havok minus block coordinates when the body was put (a re-base changes it; a real jump doesn't)
    offset: V3,
    /// how long the swing has had almost no speed (hanging against a wall under the grip)
    stall: f32,
}

/// How a swing ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Release {
    /// the whip let go (or the swing ran out: landed, too long)
    Button,
    /// jump pressed while swinging
    Jump,
    /// the far side of the arc reached with the whip still held
    ArcEnd,
}

/// Something the rest of the mod reacts to.
pub enum Event {
    /// a slam reached the ground here (havok coordinates)
    SlamLanded(V3),
}

#[derive(Default)]
pub struct Movement {
    /// what we last wrote (so the game's own values are left alone otherwise)
    wrote_motion: Option<f32>,
    wrote_speed: Option<f32>,
    last_kind: Option<Kind>,
    /// double jumps and air dashes used since leaving the ground
    air_jumps: u8,
    air_dashes: u8,
    /// how many of each the fighter gets (2 with Deku's Full Cowl passive, and for the flyers)
    pub air_moves: u8,
    /// the air dash is a Superman flight (fighters.air_dash "fly")
    pub flies: bool,
    /// 0.14.1: the swing is Luffy's stretched arm (his own swing animation)
    pub rubber: bool,
    /// 0.16.0: the fighter swings (Deku, Luffy): long falls play the dive pose
    pub swinger: bool,
    /// 1.2.0 (user): the skydive pose on long falls for every fighter now that they all move
    pub dives: bool,
    /// 1.2.0: air dashes per jump (lib.rs: 3 for Goku and All Might, 2 for every other fighter without
    /// a swing and for Deku's Full Cowl, 1 for Luffy)
    pub air_dash_max: u8,
    slam_used: bool,
    airborne_since: Option<Instant>,
    motion: Option<Motion>,
    flight: Option<Flight>,
    swing: Option<Swing>,
    gravity_off: bool,
    /// the behaviour script was told we fly the body (and when, to renew it)
    fly_marker: bool,
    fly_sent: Option<Instant>,
    /// on the ground with a jump/fall animation since, and when a landing was last asked for
    air_anim_grounded: Option<Instant>,
    land_sent: Option<Instant>,
    /// 1.3.1 round 2: the animation a landing was asked from, and the game's landings seen to follow one (they are numbered
    /// among the jumps: the guard below took them for a stuck fall and landed again every second, the user's "stuck in a
    /// landing animation" after Deku's Shoot Style, Jotaro's loops; 202140 sixteen times in a row in the playtest log)
    land_from: i32,
    landings: std::collections::HashSet<i32>,
    /// last position and time, for the momentum we carry into a double jump
    last_pos: Option<(V3, Instant)>,
    velocity: V3,
    last_steer: Option<Instant>,
    /// the whip can be thrown again from then
    whip_ready: Option<Instant>,
    /// with the whip held, the next throw after a swing's arc ends (Spider-Man's chained swings)
    chain_at: Option<Instant>,
    /// where the whip would catch now, and when that was worked out (the marker)
    aim: Option<V3>,
    aim_at: Option<Instant>,
    /// a fall over ground is being kept harmless: HP before it, and when the fighter landed
    fall_guard: Option<(i32, Option<Instant>)>,
    /// 1.2.0: the active fighter (lib.rs), for their own way of moving
    pub fighter: &'static str,
    /// Asta's glider: the board ride is still to be started (once the blade is in his hands), and the
    /// blade is out (put away when the flight ends)
    glide_cue: bool,
    /// 1.2.0: when the swing button (L3 + RB / Q) was last held: its L3 is no slam
    chord_at: Option<Instant>,
    glide_out: bool,
    /// in a sprint animation last frame (the dash attack)
    sprint_anim: bool,
    /// when the last dodge (roll, backstep, air dash) began: a dash attack follows it (combos.rs)
    last_dodge: Option<Instant>,
    /// Naruto's clone throws this jump; Asta's wall kick used this jump
    clone_throws: u8,
    wall_kicked: bool,
    /// Naruto's wall run: until, the way off the wall
    wall_run: Option<(Instant, V3)>,
    /// Bakugo's last blast, and blasts this jump
    blast_at: Option<Instant>,
    blasts: u8,
    /// Todoroki's ice slide: until, its way; ready again at
    ice_slide: Option<(Instant, V3, Option<Instant>)>,
    ice_ready: Option<Instant>,
    /// Goku: when Instant Transmission is ready again
    teleport_ready: Option<Instant>,
    /// 1.2.0: the last air dash: one press reads as a press for two frames, and spent two dashes
    last_air_dash: Option<Instant>,
    /// All Might: the Smash jump is in the air (its landing shockwave), and when it is ready again
    smash_jump: bool,
    smash_ready: Option<Instant>,
    /// 1.3.0: the blink fighters (Ichigo's Shunpo, Killua's Godspeed, Gojo's Blue, Sasuke's and DIO's steps): when the
    /// next is ready, and how many since leaving the ground
    blink_ready: Option<Instant>,
    blinks: u8,
    /// Zoro's Lion's Song dash: ready again at
    lion_ready: Option<Instant>,
    /// Gojo's Infinity: floating since
    float_since: Option<Instant>,
}

impl Movement {
    /// Our own flight, slide or swing is moving the player.
    pub fn flying(&self) -> bool {
        self.flight.is_some() || self.motion.is_some() || self.swing.is_some()
    }

    /// 1.2.0: off the ground (a jump, a fall, our flight or swing): combos play their air hits.
    pub fn airborne(&self) -> bool {
        self.airborne_since.is_some() || self.flight.is_some() || self.swing.is_some()
    }

    /// 1.2.0: just out of a dodge, or sprinting: the next attack is a dash attack.
    pub fn dash_context(&self) -> bool {
        self.last_dodge.is_some_and(|t| t.elapsed().as_secs_f32() < DASH_AFTER_S) || (self.airborne_since.is_none() && (self.speed() > SPRINT || self.sprint_anim))
    }

    /// Horizontal speed of the body (m/s), for the speed lines.
    pub fn speed(&self) -> f32 {
        let v = self.flight.as_ref().map(|f| f.vel).or(self.swing.as_ref().map(|s| s.vel)).unwrap_or(self.velocity);
        (v.0 * v.0 + v.2 * v.2).sqrt()
    }

    /// Black Whip: where the tendril caught, while Deku swings (for the overlay).
    pub fn whip_anchor(&self) -> Option<V3> {
        self.swing.as_ref().map(|s| s.grip)
    }

    pub fn tick(&mut self, player: &mut PlayerIns, ability_moving: bool, whip_held: bool) -> Option<Event> {
        // (1.2.0 stress test) dead: a flight, swing or board ride ends with the fighter, not after
        if player.chr_ins.modules.data.hp <= 0 {
            if self.flight.is_some() || self.swing.is_some() || self.glide_out {
                log!("move: stopped, the fighter is dead");
                self.reset();
            }
            // 2.0.6 (lab limbo runs: killed mid-jump, mid-flight or mid-Awakening, the fighter stood on at 0 HP, unkillable,
            // until the next load): the fall guard's "can't die" switch was only ever let go by fall_guard below, which
            // a dead fighter never reaches. A death in the air is a death: the switch goes off with it.
            if self.fall_guard.take().is_some() {
                set_no_dead(NO_DEAD_FALL, false);
                log!("move: the fall guard let go: the fighter died in the air");
            }
            return None;
        }
        let anim = base_anim(current_anim(player));
        let k = kind(anim);
        self.sprint_anim = matches!(anim, 20200..=20299);
        let pos = ability::position(player);
        self.track_velocity(pos);
        if ability_moving {
            // an ability took over: stop our flight where it is
            self.flight = None;
            self.swing = None;
        }

        // the glider is Asta's: a switch to another fighter mid-air puts it away
        if self.fighter != "asta" {
            if let Some(f) = self.flight.as_mut() {
                f.glide = None;
            }
        }
        let gliding_for = self.flight.as_ref().and_then(|f| f.glide).map(|t| t.elapsed().as_secs_f32());
        match gliding_for {
            Some(t) => {
                self.glide_out = true;
                if self.glide_cue && (crate::weapon::move_applied(GLIDE_BOARD) || t > 0.2) {
                    // the curved greatsword's dash attack: he steps onto the blade and rides it
                    self.glide_cue = false;
                    anim::play(player, "ExecEventAllBody(\"W_AttackBothDash\")");
                    log!("move: glider ride ({:.2}s)", t);
                }
            }
            None if self.glide_out => {
                // landed (or something else took over): the blade goes
                self.glide_out = false;
                self.glide_cue = false;
                crate::look::BOARD.store(0, std::sync::atomic::Ordering::Relaxed);
                crate::weapon::MOVE_TRACELESS.store(false, std::sync::atomic::Ordering::Relaxed);
                crate::weapon::set_move(0, 0.0);
                // the board ride carries itself on (gravity off) unless something follows it: a landing on
                // the ground, the fall in the air
                if player.chr_ins.modules.physics.is_touching_ground {
                    anim::play(player, "JR_Land()");
                    self.land_sent = Some(Instant::now());
                    self.land_from = base_anim(current_anim(player));
                } else if self.flight.is_none() {
                    anim::play(player, "ExecEventAllBody(\"W_FallLoop\")");
                }
                log!("move: glider put away");
            }
            None => {}
        }
        let (motion_mult, anim_speed) = if self.flight.is_some() || self.swing.is_some() {
            // the jump / roll / flight animation plays for the look only; we move the body (the glider's
            // ride pose is held still once the blade is under his feet)
            (0.0, match gliding_for {
                Some(t) if t * GLIDE_RAMP > glide_pose_at() => 0.0,
                Some(_) => GLIDE_RAMP,
                None => 1.0,
            })
        } else {
            match k {
                Kind::Locomotion | Kind::Jump => (RUN_MULT, 1.0),
                Kind::Dodge => (DODGE_DIST, DODGE_SPEED),
                _ => (1.0, 1.0),
            }
        };
        // Written every frame while ours differs from normal (the game may reset them), and once back to
        // normal afterwards; never touched otherwise.
        if motion_mult != 1.0 || self.wrote_motion.is_some_and(|w| w != 1.0) {
            player.chr_ins.modules.physics.motion_multiplier = motion_mult;
            self.wrote_motion = Some(motion_mult);
        }
        if anim_speed != 1.0 || self.wrote_speed.is_some_and(|w| w != 1.0) {
            player.chr_ins.modules.behavior.animation_speed = anim_speed;
            self.wrote_speed = Some(anim_speed);
        }

        // 0.14.1: inside rock (a swing or flight pushed into a cliff the rays didn't stop): out on top
        if self.flying() {
            if let Some(top) = inside_solid(player) {
                let p = player.chr_ins.modules.physics.position;
                log!("move: inside solid ground at y {:.1}: out on top at {:.1}", p.1, top);
                let phys = &mut player.chr_ins.modules.physics;
                phys.position = HavokPosition(p.0, top + 0.1, p.2, p.3);
                phys.chr_proxy_pos_update_requested = true;
                self.swing = None;
                self.flight = None;
                self.motion = None;
            }
        }
        // In the air: a jump before its landing (2021xx), the fall animation (4050), falling off
        // something (the physics flags alone stay "touching ground" for most of a jump), or our flight.
        let ph = &player.chr_ins.modules.physics;
        let airborne = self.flight.is_some() || self.swing.is_some() || matches!(anim, 202000..=202099 | 4050..=4059) || (ph.is_falling && !ph.is_touching_ground);
        // 0.12.0: a fall onto ground never kills. Elden Ring judges the fall by the height it started
        // at, whatever the fall damage rate, so in the air above anything solid (within 150 m) the game's
        // own "player can't die" switch is on (it keeps HP at 1), and for a moment after landing; the HP
        // the landing took is then given back. A fall with nothing below (the void) is left to the game.
        self.fall_guard(player);
        #[cfg(feature = "lab")]
        {
            let ph = &player.chr_ins.modules.physics;
            if ph.is_falling || !ph.is_touching_ground || matches!(anim, 4050..=4059) {
                log!("lab fall: anim {anim} falling {} ground {} timer {:.2} y {:.1} hp {}", ph.is_falling, ph.is_touching_ground, player.chr_ins.modules.fall.fall_timer, ph.position.1, player.chr_ins.modules.data.hp);
            }
        }
        let ph = &player.chr_ins.modules.physics;
        if !airborne {
            self.air_jumps = 0;
            self.air_dashes = 0;
            self.slam_used = false;
            self.airborne_since = None;
            self.clone_throws = 0;
            self.wall_kicked = false;
            self.blasts = 0;
            self.blinks = 0;
            self.float_since = None;
        } else {
            self.airborne_since.get_or_insert_with(Instant::now);
        }
        let presses = player.chr_ins.modules.action_request.new_action_presses;
        #[cfg(feature = "lab")]
        let lab_dash = crate::lab::FORCE_AIRDASH.swap(false, std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(feature = "lab"))]
        let lab_dash = false;
        let air_time = self.airborne_since.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0);
        let air_moves = if self.flies { self.air_moves.max(2) } else { self.air_moves.max(1) };

        if self.swing.is_some() && (!whip_held || presses.jump()) {
            self.let_go(player, if presses.jump() { Release::Jump } else { Release::Button });
        }
        // the whip still held after a swing's arc: the next one is thrown on the way down
        if let Some(at) = self.chain_at {
            let falling = self.flight.as_ref().is_some_and(|f| f.vel.1 < 2.0);
            if !whip_held || !airborne || self.swing.is_some() || ability_moving || at.elapsed().as_secs_f32() > 1.5 {
                self.chain_at = None;
            } else if Instant::now() >= at && falling {
                self.chain_at = None;
                self.whip_ready = None;
                self.whip(player);
            }
        }

        // 1.2.0: the dash attack follows a dodge any time during it and up to DASH_AFTER_S after it ends
        if k == Kind::Dodge || self.flight.as_ref().is_some_and(|f| f.dash_left > 0.0) {
            self.last_dodge = Some(Instant::now());
        }
        // 1.2.0: the fighter's own way of moving first; it takes the press when it acts
        let took = !ability_moving && self.swing.is_none() && self.fighter_move(player, airborne, air_time, k, presses);

        // 1.2.0 (user: a double jump into a charged attack was cut off by the skydive): in the mod's own air
        // poses (a shadow clone's throw, the air dash's lean, the skydive) the behaviour script takes no
        // attack, so the jump attack starts from here (the dive then waits for it to end)
        if airborne && !ability_moving && self.swing.is_none() && (presses.r1() || presses.r2()) && matches!(anim, 60241 | DIVE_ANIM)
            && !self.flight.as_ref().is_some_and(|f| f.fly || f.slam)
        {
            let heavy = presses.r2();
            anim::play(player, if heavy { "JR_AirAttack(true)" } else { "JR_AirAttack(false)" });
            if let Some(f) = &mut self.flight {
                f.fall_time = 0.0;
            }
            log!("move: {} jump attack out of an air pose ({anim})", if heavy { "heavy" } else { "light" });
        }

        // 1.2.0 (user: Asta's glider sometimes turned into the ground slam): the game sees the L3 of the swing
        // button (L3 + RB) as an L3 press of its own, a moment after the chord fired; that L3, held or within
        // 0.4 s of it, and any L3 on the glider, is no slam
        if whip_held {
            self.chord_at = Some(Instant::now());
        }
        let chord_l3 = self.chord_at.is_some_and(|t| t.elapsed().as_secs_f32() < 0.4)
            || self.flight.as_ref().is_some_and(|f| f.glide.is_some());

        if !took && self.motion.is_none() && self.swing.is_none() && !ability_moving {
            let fwd = ability::forward(player);
            if airborne && air_time > 0.15 && presses.l3() && !self.slam_used && !chord_l3 {
                // slam: straight down, hard
                self.slam_used = true;
                let carry = self.horizontal(0.15);
                self.start_flight((carry.0, -SLAM_SPEED, carry.2));
                if let Some(f) = &mut self.flight {
                    f.slam = true;
                }
                anim::cue(player, Cue::Dive);
                log!("move: slam");
            } else if airborne && air_time > 0.12 && presses.jump() && self.air_jumps < air_moves && !self.slam_used {
                // double jump: up, keeping the way we were already going, turned toward the stick
                self.air_jumps += 1;
                let mut h = self.horizontal(1.0);
                if let Some(s) = input::stick_world() {
                    let sp = (h.0 * h.0 + h.2 * h.2).sqrt().max(5.0);
                    h = (s.0 * sp, 0.0, s.2 * sp);
                }
                self.start_flight((h.0, JUMP_UP, h.2));
                // (out of one of the mod's own poses the game refuses its jump: our lean instead)
                anim::cue(player, if own_air(anim) { Cue::AirDash } else { Cue::Jump });
                log!("move: double jump ({:.1} m/s carried)", (h.0 * h.0 + h.2 * h.2).sqrt());
            } else if airborne && air_time > 0.08 && (presses.sp_move() || presses.rolling() || presses.backstep() || lab_dash) && self.air_dashes < self.air_dash_max.max(1) && !self.slam_used && self.last_air_dash.is_none_or(|t| t.elapsed().as_secs_f32() > 0.2) {
                self.last_air_dash = Some(Instant::now());
                // air dash: where the stick points (side-dashes), else where the fighter faces
                self.air_dashes += 1;
                let stick = input::stick_world();
                let dir = stick.unwrap_or(fwd);
                let (speed, time, carry) = if self.flies { (FLY_SPEED, FLY_TIME, FLY_CARRY) } else { (DASH_SPEED, DASH_TIME, DASH_CARRY) };
                self.start_flight((dir.0 * speed, 0.0, dir.2 * speed));
                if let Some(f) = &mut self.flight {
                    f.dash_left = time;
                    f.dash_dir = dir;
                    f.cap = carry;
                    f.fly = self.flies;
                }
                let locked = player.chr_ins.is_locked_on;
                if self.flies {
                    ability::face(player, dir);
                    anim::cue(player, Cue::Fly);
                } else if locked {
                    // locked on: the side / back rolls keep facing the enemy
                    anim::cue(player, Cue::Dash);
                } else {
                    ability::face(player, dir);
                    anim::cue(player, Cue::AirDash);
                }
                log!("move: air dash{} stick {:?} dir ({:.2}, {:.2}) facing ({:.2}, {:.2})", if self.flies { " (flight)" } else { "" }, input::stick(), dir.0, dir.2, fwd.0, fwd.2);
            } else if k == Kind::Crouch && self.last_kind == Some(Kind::Locomotion) && !airborne {
                // crouching out of a run: slide into it, the way we were running
                let dir = input::stick_world().unwrap_or(fwd);
                self.motion = Some(Motion::dash((dir.0 * 6.0, 0.0, dir.2 * 6.0), 0.45));
                log!("move: slide");
            } else if airborne && k == Kind::Jump && self.flight.is_none() {
                // the stick steers the game's own jump too
                self.steer_jump(player);
            }
        }
        if !(airborne && k == Kind::Jump && self.flight.is_none()) {
            self.last_steer = None;
        }
        self.last_kind = Some(k);

        let mut event = None;
        if let Some(m) = &mut self.motion {
            if m.step(player) {
                self.motion = None;
            }
        } else if self.swing.is_some() {
            self.swing_step(player);
        } else if self.flight.is_some() {
            event = self.fly(player);
        }
        let want_off = self.flight.is_some() || self.swing.is_some() || self.motion.as_ref().is_some_and(|m| m.airborne());
        if want_off != self.gravity_off {
            player.chr_ins.modules.physics.gravity_disabled = want_off;
            self.gravity_off = want_off;
        }
        BODY_FLOWN.store(self.flying(), std::sync::atomic::Ordering::Relaxed);
        // 0.14.3: THE vanishing. After our flight or swing put the body down, the behaviour script could
        // stay in its jump or fall state with the fighter standing (no landing ever came). Elden Ring then
        // judges a fighter stuck in a fall ("hamari" fall death): the body dissolves, the camera is taken,
        // and the death waits (the fall guard keeps them alive), so they walked about unseen until a move
        // broke it. On the ground with an air animation for 1.5 s (a normal jump lands well before): land it the game's way.
        if !self.flying() && !ability_moving {
            // (1.3.1 round 2) a jump-numbered animation that replaced the pose a landing was asked from, within a second of it, is
            // the game's landing: never landed again
            if self.land_sent.is_some_and(|t| t.elapsed().as_secs_f32() < 1.0) && anim != self.land_from && matches!(anim, 202000..=202199)
                && self.landings.insert(anim)
            {
                log!("move: {anim} is a landing");
            }
            let air_anim = matches!(anim, 202000..=202199 | 4050..=4059) && !self.landings.contains(&anim);
            // 0.15.0: and the mod's own flight and swing poses (fly, air dash, Black Whip, Luffy's swing,
            // flying kick). The player's 0.15.0 test: after a swing and a double jump Deku stood 30 s in
            // the whip pose, his body drawn off where the rope had held it ("Deku disappeared"), until a
            // move replaced it. These never belong on the ground: landed after 0.3 s.
            let own_air = own_air(anim);
            let p = player.chr_ins.modules.physics.position;
            // (the game's own word counts too: in the stuck cases it said "on the ground" while the floor
            // was one our rays don't see)
            let ph = &player.chr_ins.modules.physics;
            let standing = (ph.is_touching_ground && !ph.is_falling) || crate::motion::ground_below(player, p.0, p.1 + 0.3, p.2, 0.35).is_some();
            if (air_anim || own_air) && standing {
                let since = *self.air_anim_grounded.get_or_insert_with(Instant::now);
                let wait = if own_air { 0.3 } else { 1.5 };
                // (a landing asked from this same animation a moment ago left it playing: asking again only restarts it)
                let again = self.land_from == anim && self.land_sent.is_some_and(|t| t.elapsed().as_secs_f32() < 2.5);
                if again && !own_air && self.landings.insert(anim) {
                    log!("move: {anim} is a landing (it stayed after one)");
                } else if since.elapsed().as_secs_f32() > wait && self.land_sent.is_none_or(|t| t.elapsed().as_secs_f32() > 1.0) {
                    log!("move: on the ground in an air animation ({anim}); landing");
                    anim::play(player, "JR_Land()");
                    self.land_sent = Some(Instant::now());
                    self.land_from = anim;
                }
            } else {
                self.air_anim_grounded = None;
            }
        }
        #[cfg(feature = "lab")]
        if self.flying() {
            let c = &player.chr_ins;
            log!("lab alpha: anim {anim} opacity {:.2}/{:.2} tint {:.2} ({:.2}) camo {:.2} base {:.2} ({:.2}) fade2 {:.1}/{:.1}", c.opacity_keyframes_multiplier, c.opacity_keyframes_multiplier_previous, c.tint_alpha_multiplier, c.tint_alpha_multiplier_modifier, c.camouflage_transparency, c.base_transparency, c.base_transparency_modifier, c.squared_fade_out_start_distance, c.squared_fade_out_start_distance_override);
        }
        let marker = self.swing.is_some() || self.flight.as_ref().is_some_and(|f| !f.slam);
        // 0.14.3: renewed every 2 s while it lasts. Sent once per flight, it ran out after 240 frames
        // (4 s) in long swing chains and high falls: the game's own fall then started under our flight
        let renew = marker && self.fly_sent.is_none_or(|t| t.elapsed().as_secs_f32() > 2.0);
        if marker != self.fly_marker || renew {
            // the behaviour script holds off its fall while we fly the body (hks: JR_FlyFor)
            anim::play(player, if marker { "JR_FlyFor(240)" } else { "JR_FlyFor(0)" });
            self.fly_marker = marker;
            self.fly_sent = marker.then(Instant::now);
        }
        event
    }

    fn fall_guard(&mut self, player: &mut PlayerIns) {
        // (2.0.6) a death under way: no guard
        if dying() {
            if self.fall_guard.take().is_some() {
                set_no_dead(NO_DEAD_FALL, false);
            }
            return;
        }
        let ph = &player.chr_ins.modules.physics;
        let in_air = (ph.is_falling && !ph.is_touching_ground) || self.flight.is_some() || self.swing.is_some();
        let hp = player.chr_ins.modules.data.hp;
        let p = ph.position;
        // a fall only hurts on landing: in the air, a blow the switch held at 1 HP was lethal (a death, not a fighter
        // left standing on 1 HP)
        if in_air && hp == 1 && self.fall_guard.is_some_and(|(before, _)| before > 1) {
            log!("move: a lethal blow in the air: the fall guard lets go");
            self.fall_guard = None;
            set_no_dead(NO_DEAD_FALL, false);
            crate::bosses::kill_player(&mut player.chr_ins);
            return;
        }
        if in_air && hp > 0 {
            let over_ground = crate::motion::ground_below(player, p.0, p.1 + 0.3, p.2, 150.0).is_some();
            match (&mut self.fall_guard, over_ground) {
                (None, true) => {
                    self.fall_guard = Some((hp, None));
                    set_no_dead(NO_DEAD_FALL, true);
                }
                // 2.0.6: the health just before landing, not the most it had in the air: a landing gives back only
                // what the landing took (blows taken in the air, a boss's lethal one held at 1 HP among them, were all
                // healed on landing: a player's log, Gojo from 1 HP back to 601)
                (Some((before, landed)), true) => {
                    *before = hp;
                    *landed = None;
                }
                // 0.15.0: over the void only while a fall there isn't the player's doing (just loaded):
                // their own fall over an edge kills again (rescue.rs)
                (Some((before, landed)), false) if VOID_SAFE.load(std::sync::atomic::Ordering::Relaxed) => {
                    *before = hp;
                    *landed = None;
                }
                (None, false) if VOID_SAFE.load(std::sync::atomic::Ordering::Relaxed) => {
                    self.fall_guard = Some((hp, None));
                    set_no_dead(NO_DEAD_FALL, true);
                }
                (slot, false) => {
                    if slot.take().is_some() {
                        set_no_dead(NO_DEAD_FALL, false);
                    }
                }
            }
            return;
        }
        let Some((before, landed)) = &mut self.fall_guard else { return };
        let at = *landed.get_or_insert_with(Instant::now);
        // 2.0.6 (lab killtest: a boss's lethal blow just after a landing was given back as if the landing took it, the
        // fighter back to full health): the switch holds a lethal blow at 1 HP, so 1 HP from more is a death, not a fall
        // (not the landing itself: a fall's own damage lands with it, in its first frames, and falls never kill)
        if hp == 1 && *before > 1 && at.elapsed().as_secs_f32() > 0.15 {
            log!("move: a lethal blow as they landed: the fall guard lets go");
            self.fall_guard = None;
            set_no_dead(NO_DEAD_FALL, false);
            crate::bosses::kill_player(&mut player.chr_ins);
            return;
        }
        if hp < *before && hp > 0 {
            // the landing's damage, given back
            player.chr_ins.modules.data.hp = *before;
        }
        if at.elapsed().as_secs_f32() > 1.0 {
            self.fall_guard = None;
            set_no_dead(NO_DEAD_FALL, false);
        }
    }

    fn track_velocity(&mut self, pos: V3) {
        let now = Instant::now();
        if let Some((p, t)) = self.last_pos {
            let dt = now.duration_since(t).as_secs_f32();
            if dt > 0.001 {
                let v = ((pos.0 - p.0) / dt, (pos.1 - p.1) / dt, (pos.2 - p.2) / dt);
                // map blocks re-base havok coordinates: ignore those jumps
                if (v.0 * v.0 + v.2 * v.2).sqrt() < 60.0 {
                    // smoothed over a few frames
                    self.velocity = (self.velocity.0 * 0.5 + v.0 * 0.5, v.1, self.velocity.2 * 0.5 + v.2 * 0.5);
                }
            }
        }
        self.last_pos = Some((pos, now));
    }

    /// Horizontal momentum right now (our flight's, else what the body has been doing), times `keep`.
    fn horizontal(&self, keep: f32) -> V3 {
        let v = self.flight.as_ref().map(|f| f.vel).unwrap_or(self.velocity);
        (v.0 * keep, 0.0, v.2 * keep)
    }

    fn start_flight(&mut self, vel: V3) {
        let cap = (vel.0 * vel.0 + vel.2 * vel.2).sqrt().max(AIR_MAX);
        self.flight = Some(Flight { vel, dash_left: 0.0, dash_dir: (0.0, 0.0, 0.0), slam: false, fly: false, fall_time: 0.0, dived: None, posed: None, last: Instant::now(), cap, glide: None });
    }

    /// The stick nudges the body during the game's own jump (no flight of ours).
    fn steer_jump(&mut self, player: &mut PlayerIns) {
        let now = Instant::now();
        let dt = self.last_steer.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(0.0).clamp(0.0, 0.05);
        self.last_steer = Some(now);
        let Some(s) = input::stick_world() else { return };
        let (dx, dz) = (s.0 * JUMP_STEER * dt, s.2 * JUMP_STEER * dt);
        let pos = player.chr_ins.modules.physics.position;
        if dt <= 0.0 || crate::motion::wall_ahead(player, &pos, dx, 0.0, dz) {
            return;
        }
        let phys = &mut player.chr_ins.modules.physics;
        phys.position = HavokPosition(pos.0 + dx, pos.1, pos.2 + dz, pos.3);
        phys.chr_proxy_pos_update_requested = true;
    }

    /// One frame of our flight. Ends on the ground.
    fn fly(&mut self, player: &mut PlayerIns) -> Option<Event> {
        let swinger = self.swinger;
        let mut stalled = false;
        let f = self.flight.as_mut()?;
        let now = Instant::now();
        let dt = now.duration_since(f.last).as_secs_f32().clamp(0.0, 0.05);
        f.last = now;
        let stick = input::stick_world();

        if f.dash_left > 0.0 {
            f.dash_left -= dt;
            // the stick turns the dash while it lasts
            if let Some(s) = stick {
                let cur = f.dash_dir.2.atan2(f.dash_dir.0);
                let want = s.2.atan2(s.0);
                let mut d = want - cur;
                while d > std::f32::consts::PI {
                    d -= std::f32::consts::TAU;
                }
                while d < -std::f32::consts::PI {
                    d += std::f32::consts::TAU;
                }
                let a = cur + d.clamp(-DASH_TURN * dt, DASH_TURN * dt);
                f.dash_dir = (a.cos(), 0.0, a.sin());
            }
            let speed = if f.fly { FLY_SPEED } else { DASH_SPEED };
            f.vel = (f.dash_dir.0 * speed, if f.fly { FLY_LIFT } else { 0.0 }, f.dash_dir.2 * speed);
            if f.dash_left <= 0.0 {
                // out of the burst: keep going that way and float
                f.vel = (f.dash_dir.0 * f.cap, if f.fly { 0.5 } else { 0.8 }, f.dash_dir.2 * f.cap);
            }
        } else if f.slam {
            f.vel.1 = -SLAM_SPEED;
        } else if f.glide.is_some() {
            // the glider: a slow, steady sink and a steady cruise, the stick turning it (straight on without)
            f.vel.1 += (-GLIDE_SINK - f.vel.1) * (4.0 * dt).min(1.0);
            let sp = (f.vel.0 * f.vel.0 + f.vel.2 * f.vel.2).sqrt().max(0.1);
            let (hx, hz) = match stick {
                Some(s) => (s.0, s.2),
                None => (f.vel.0 / sp, f.vel.2 / sp),
            };
            let k = (GLIDE_TURN * dt).min(1.0);
            f.vel.0 += (hx * GLIDE_SPEED - f.vel.0) * k;
            f.vel.2 += (hz * GLIDE_SPEED - f.vel.2) * k;
            f.fall_time = 0.0;
        } else {
            let mut g = if f.vel.1 > 0.0 { GRAVITY_UP } else { GRAVITY_DOWN };
            if f.fly && f.vel.1 <= 0.0 {
                // flyers glide down
                g *= 0.45;
            }
            f.vel.1 = (f.vel.1 - g * dt).max(-MAX_FALL);
            // full air control: the body goes where the stick points
            let top = f.cap.max(AIR_MAX);
            if let Some(s) = stick {
                let k = (AIR_RESPONSE * dt).min(1.0);
                f.vel.0 += (s.0 * top - f.vel.0) * k;
                f.vel.2 += (s.2 * top - f.vel.2) * k;
            } else {
                let drag = 1.0 - AIR_DRAG * dt;
                f.vel.0 *= drag;
                f.vel.2 *= drag;
            }
            let sp = (f.vel.0 * f.vel.0 + f.vel.2 * f.vel.2).sqrt();
            if sp > top {
                f.vel.0 *= top / sp;
                f.vel.2 *= top / sp;
            }
            // the cap eases down to normal air speed after a fast dash
            f.cap = (f.cap - 6.0 * dt).max(AIR_MAX);
            // 0.16.0: a Superman flight that lost its momentum (a hill or wall in the way) is a flight no
            // more: normal gravity, and the flight pose goes (below)
            if f.fly && sp < FLY_STALL {
                f.fly = false;
                stalled = true;
            }
            f.fall_time = if f.vel.1 < -1.0 { f.fall_time + dt } else { 0.0 };
        }
        let face = if f.dash_left > 0.0 || f.fly || f.glide.is_some() || stick.is_some() { Some((f.vel.0, 0.0, f.vel.2)) } else { None };

        let (mut dx, dy, mut dz) = (f.vel.0 * dt, f.vel.1 * dt, f.vel.2 * dt);
        let slam = f.slam;
        let pos = player.chr_ins.modules.physics.position;
        let havok = unsafe { CSHavokMan::instance() }.ok();
        let mut landed = false;
        let mut ny = pos.1 + dy;
        if let Some(h) = &havok {
            // walls: stop sideways, keep falling (and end an air dash: no pushing on into the wall)
            if crate::motion::wall_ahead(player, &pos, dx, dy, dz) {
                dx = 0.0;
                dz = 0.0;
                f.vel.0 = 0.0;
                f.vel.2 = 0.0;
                f.dash_left = 0.0;
                f.dash_dir = (0.0, 0.0, 0.0);
                if f.fly {
                    f.fly = false;
                    f.vel.1 = f.vel.1.min(0.0);
                    stalled = true;
                    log!("move: flight met a wall ahead at ({:.1}, {:.1}, {:.1}) going ({:.2}, {:.2})", pos.0, pos.1, pos.2, dx, dz);
                }
            }
            // ceiling
            if dy > 0.0 && crate::motion::ray(player, &HavokPosition(pos.0, pos.1 + 1.6, pos.2, 0.0), PositionDelta(0.0, dy + 0.4, 0.0)).is_some() {
                ny = pos.1;
                f.vel.1 = 0.0;
            }
            // ground: anything between here and where this frame ends
            if f.vel.1 <= 0.0 {
                let (nx, nz) = (pos.0 + dx, pos.2 + dz);
                let depth = 1.0 + (-dy).max(0.0) + 0.08;
                if let Some(g) = crate::motion::ray(player, &HavokPosition(nx, pos.1 + 1.0, nz, 0.0), PositionDelta(0.0, -depth, 0.0)) {
                    if ny <= g.1 + 0.05 {
                        ny = g.1;
                        landed = true;
                    }
                }
            }
            // 0.14.3: nothing solid along the whole step, at the feet, the middle and the head (the swing
            // has had this since 0.13.0; without it a flight off a swing came down through slanted rock
            // and landed on a face inside the cliff, out of sight, for 20 s)
            if !landed {
                let sweep = |dx: f32, dy: f32, dz: f32| {
                    let len = (dx * dx + dy * dy + dz * dz).sqrt();
                    if len < 1e-4 {
                        return false;
                    }
                    let k = (len + 0.35) / len;
                    [0.3f32, 1.0, 1.7].iter().any(|hh| crate::motion::ray(player, &HavokPosition(pos.0, pos.1 + hh, pos.2, 0.0), PositionDelta(dx * k, dy * k, dz * k)).is_some())
                };
                let dyy = ny - pos.1;
                if sweep(dx, dyy, dz) {
                    if !sweep(dx, 0.0, dz) {
                        // only the rise or the fall runs into it: no higher, or it holds him
                        if dyy < 0.0 {
                            landed = true;
                        }
                        ny = pos.1;
                        f.vel.1 = f.vel.1.min(0.0);
                    } else {
                        dx = 0.0;
                        dz = 0.0;
                        f.vel.0 = 0.0;
                        f.vel.2 = 0.0;
                        f.dash_left = 0.0;
                        if sweep(0.0, dyy, 0.0) {
                            if dyy < 0.0 {
                                landed = true;
                            }
                            ny = pos.1;
                            f.vel.1 = f.vel.1.min(0.0);
                        }
                    }
                }
            }
        }
        if let Some(d) = face {
            if (d.0 * d.0 + d.2 * d.2).sqrt() > 2.0 && !(player.chr_ins.is_locked_on && !self.flies) {
                ability::face(player, d);
            }
        }
        let phys = &mut player.chr_ins.modules.physics;
        phys.position = HavokPosition(pos.0 + dx, ny, pos.2 + dz, phys.position.3);
        phys.chr_proxy_pos_update_requested = true;
        // 0.10.0: our flight is no fall either: the game would cut the flight's animation for its own
        // falling one (4050) within a few frames
        if !slam {
            phys.is_falling = false;
            phys.is_touching_ground = true;
        }
        // our flight is no fall: no fall damage from it
        player.chr_ins.modules.fall.fall_timer = 0.0;

        let anim = base_anim(current_anim(player));
        let hard = !slam && (f.dived.is_some() || f.fall_time > HARD_LANDING_S);
        // 0.16.0: Deku and Luffy dive after a second of falling (or at full fall speed) with the ground
        // still well below: renewed before the pose runs out, never close to the ground
        let mut dive = false;
        // 1.2.0 (user: a double jump into a charged attack was cut off by the dive): never over an attack
        let attacking = (30_000..40_000).contains(&anim);
        if attacking {
            f.fall_time = 0.0;
        }
        if !landed && !attacking && (swinger || self.dives) && !slam && !f.fly && f.dash_left <= 0.0 && (f.fall_time > DIVE_AFTER_S || f.vel.1 <= -DIVE_SPEED) {
            let p = pos;
            let clear = crate::motion::ground_below(player, p.0, p.1 + 0.3, p.2, DIVE_CLEAR).is_none();
            let due = match f.dived {
                None => true,
                Some(t) => t.elapsed().as_secs_f32() > 7.5 || (anim != DIVE_ANIM && t.elapsed().as_secs_f32() > 0.6),
            };
            if clear && due {
                f.dived = Some(Instant::now());
                dive = true;
            }
        }
        // 1.3.0 (playtest: "Goku loses his flight animation if you fly 3x fast"): the game cuts the flight pose for its own
        // fall (4050) or jump (2021xx) within 0.1 - 0.3 s, worst on quick repeated dashes (lab lbtest): asked for again
        // while the flight lasts
        if f.fly && !landed && !attacking && anim != 60220 && f.posed.is_none_or(|t| t.elapsed().as_secs_f32() > 0.12) {
            f.posed = Some(Instant::now());
            anim::cue(player, Cue::Fly);
        }
        if dive {
            anim::cue(player, Cue::Fall);
            log!("move: dive ({:.1} m/s down)", -self.flight.as_ref().map(|f| f.vel.1).unwrap_or(0.0));
        }
        if stalled && !landed && anim == 60220 {
            // out of the flight pose now: down on the ground if it is right there, else a short lean
            let p = player.chr_ins.modules.physics.position;
            if let Some(g) = crate::motion::ground_below(player, p.0, p.1 + 0.3, p.2, 1.6) {
                let phys = &mut player.chr_ins.modules.physics;
                phys.position = HavokPosition(p.0, g, p.2, p.3);
                phys.chr_proxy_pos_update_requested = true;
                landed = true;
            } else {
                anim::cue(player, Cue::AirDash);
            }
            log!("move: flight lost its momentum");
        }
        if landed {
            self.flight = None;
            if std::mem::take(&mut self.smash_jump) {
                // All Might's Smash jump lands: a shockwave that only knocks enemies back
                let at = (pos.0 + dx, ny + 0.3, pos.2 + dz);
                puff(player, STORM_STOMP_FX, at, (0.0, 0.0, 1.0));
                puff(player, ROCK_BLASTER_FX, at, (0.0, 0.0, 1.0));
                anim::play(player, "JR_LandHard()");
                self.land_sent = Some(Instant::now());
                self.land_from = base_anim(current_anim(player));
                log!("move: smash jump landed");
                return None;
            }
            if slam {
                log!("move: slam landed");
                return Some(Event::SlamLanded((pos.0 + dx, ny, pos.2 + dz)));
            }
            // 0.16.0: down from a flight in one of our air poses (the dive, the flight): land it now
            // instead of a moment standing in the pose
            // after a skydive or a long fall: the superhero landing (Elden Ring's heavy landing)
            if own_air(anim) || hard {
                anim::play(player, if hard { "JR_LandHard()" } else { "JR_Land()" });
                self.land_sent = Some(Instant::now());
                self.land_from = anim;
            }
        }
        None
    }

    // ------------------------------------------------------------ 1.2.0: each fighter's own movement

    /// Naruto, Bakugo, Todoroki, Asta, Goku and All Might. True when it acted on this frame's press (the
    /// generic double jump / air dash then leave it alone).
    fn fighter_move(&mut self, player: &mut PlayerIns, airborne: bool, air_time: f32, k: Kind, presses: eldenring::cs::ChrActions) -> bool {
        let now = Instant::now();
        let fwd = ability::forward(player);
        let way = input::stick_world().unwrap_or(fwd);
        let speed = self.speed();
        let dodge = presses.sp_move() || presses.rolling() || presses.backstep();
        // the ground moves start from the ground: the game's falling flag flickers on slopes and bumps, so
        // anything within half a metre below counts
        let p0 = player.chr_ins.modules.physics.position;
        let grounded = !airborne || crate::motion::ground_below(player, p0.0, p0.1 + 0.3, p0.2, 0.8).is_some();
        let sprinting = grounded && (speed > SPRINT || matches!(base_anim(current_anim(player)), 20200..=20299));
        #[cfg(feature = "lab")]
        if presses.jump() || dodge {
            log!("lab move: {} jump {} dodge {dodge} airborne {airborne} grounded {grounded} speed {speed:.1} anim {} air_jumps {} timer {:.2}", self.fighter, presses.jump(), base_anim(current_anim(player)), self.air_jumps, player.chr_ins.modules.action_request.action_timers.jump);
        }
        match self.fighter {
            "naruto" => {
                // 1.2.0 (user): Asta's leap from a sprint or a crouch
                if grounded && presses.jump() && (sprinting || k == Kind::Crouch) {
                    let p = ability::position(player);
                    crate::world_fx::poof((p.0, p.1 + 0.2, p.2));
                    self.start_flight((way.0 * ASTA_LEAP_FORWARD, ASTA_LEAP_UP, way.2 * ASTA_LEAP_FORWARD));
                    if let Some(f) = &mut self.flight {
                        f.cap = ASTA_LEAP_FORWARD;
                    }
                    self.air_jumps = 0;
                    ability::face(player, way);
                    anim::cue(player, Cue::Jump);
                    log!("move: Naruto's leap");
                    return true;
                }
                // shadow clones throw him: his double jump
                if airborne && air_time > 0.12 && presses.jump() && self.clone_throws < CLONE_THROWS && !self.slam_used {
                    self.clone_throws += 1;
                    self.air_jumps = u8::MAX / 2;
                    let p = ability::position(player);
                    // the clone that threw him vanishes in smoke behind and below him (world_fx)
                    crate::world_fx::poof((p.0 - way.0 * 0.9, p.1 + 0.5, p.2 - way.2 * 0.9));
                    self.start_flight((way.0 * CLONE_FORWARD, CLONE_UP, way.2 * CLONE_FORWARD));
                    if let Some(f) = &mut self.flight {
                        f.cap = CLONE_FORWARD;
                    }
                    ability::face(player, way);
                    anim::cue(player, Cue::AirDash);
                    log!("move: a shadow clone throws Naruto ({} of {CLONE_THROWS})", self.clone_throws);
                    return true;
                }
            }
            "bakugo" => {
                // explosion propulsion: jump held in the air once the double jump is used
                let held = player.chr_ins.modules.action_request.action_timers.jump > 0.0;
                if airborne && held && self.air_jumps >= 1 && air_time > 0.3 && self.blast_at.is_none_or(|t| t.elapsed().as_secs_f32() >= BLAST_EVERY_S) {
                    let st = player.chr_ins.modules.data.stamina;
                    let max = if crate::ledger::bosses().is_empty() { BLAST_MAX } else { BLAST_MAX_BOSS };
                    if st >= BLAST_STAMINA && self.blasts < max {
                        self.blasts += 1;
                        player.chr_ins.modules.data.stamina = st - BLAST_STAMINA;
                        if self.blasts == 1 {
                            log!("move: Bakugo's explosions carry him");
                        }
                        self.blast_at = Some(now);
                        let stick = input::stick_world();
                        let mut v = self.flight.as_ref().map(|f| f.vel).unwrap_or(self.velocity);
                        let push = stick.unwrap_or((0.0, 0.0, 0.0));
                        v = (v.0 * 0.6 + push.0 * BLAST_PUSH, v.1.max(0.0) * 0.4 + BLAST_LIFT, v.2 * 0.6 + push.2 * BLAST_PUSH);
                        let p = ability::position(player);
                        // the blasts go off at his palms, alternating hands, just behind him (the push's way)
                        let side = (-fwd.2, 0.0, fwd.0);
                        let s = if self.blasts % 2 == 0 { 0.38 } else { -0.38 };
                        let back = if stick.is_some() { 0.35 } else { 0.0 };
                        puff(player, BLAST_FX, (p.0 + side.0 * s - push.0 * back, p.1 + 0.95, p.2 + side.2 * s - push.2 * back), (-push.0, -1.0, -push.2));
                        self.start_flight(v);
                        if let Some(f) = &mut self.flight {
                            f.cap = AIR_MAX.max(BLAST_PUSH * 1.3);
                        }
                        if let Some(s) = stick {
                            ability::face(player, s);
                        }
                        anim::cue(player, Cue::AirDash);
                        return false;
                    }
                }
            }
            "todoroki" => {
                if let Some((until, dir, launch)) = self.ice_slide {
                    let p = ability::position(player);
                    // the ice trail behind him
                    if launch.is_none_or(|t| t.elapsed().as_secs_f32() > 0.1) {
                        puff(player, ICE_FX, (p.0, p.1 + 0.1, p.2), dir);
                        self.ice_slide = Some((until, dir, Some(now)));
                    }
                    if presses.jump() || now >= until || self.motion.is_none() {
                        self.ice_slide = None;
                        self.motion = None;
                        let up = if presses.jump() || now >= until { ICE_LAUNCH_UP } else { 0.0 };
                        if up > 0.0 {
                            self.start_flight((dir.0 * ICE_SLIDE_SPEED, up, dir.2 * ICE_SLIDE_SPEED));
                            if let Some(f) = &mut self.flight {
                                f.cap = ICE_SLIDE_SPEED;
                            }
                            anim::cue(player, Cue::Jump);
                            log!("move: Todoroki launches off his ice");
                        }
                        return presses.jump();
                    }
                    return false;
                }
                if sprinting && dodge && self.ice_ready.is_none_or(|t| now >= t) {
                    self.ice_ready = Some(now + std::time::Duration::from_secs_f32(ICE_SLIDE_COOLDOWN_S));
                    self.motion = Some(Motion::dash((way.0 * ICE_SLIDE_SPEED * ICE_SLIDE_S, 0.0, way.2 * ICE_SLIDE_SPEED * ICE_SLIDE_S), ICE_SLIDE_S));
                    self.ice_slide = Some((now + std::time::Duration::from_secs_f32(ICE_SLIDE_S), way, None));
                    ability::face(player, way);
                    anim::cue(player, Cue::AirDash);
                    log!("move: Todoroki surfs his ice");
                    return true;
                }
                // his air dash trails fire
                if airborne && air_time > 0.08 && dodge && self.air_dashes < 1 {
                    let p = ability::position(player);
                    puff(player, FLAME_FX, (p.0, p.1 + 0.8, p.2), (-way.0, 0.0, -way.2));
                }
            }
            "asta" => {
                // an anti-magic leap from a sprint or a crouch
                if grounded && presses.jump() && (sprinting || k == Kind::Crouch) {
                    let p = ability::position(player);
                    puff(player, BLACK_FX, (p.0, p.1 + 0.1, p.2), way);
                    self.start_flight((way.0 * ASTA_LEAP_FORWARD, ASTA_LEAP_UP, way.2 * ASTA_LEAP_FORWARD));
                    if let Some(f) = &mut self.flight {
                        f.cap = ASTA_LEAP_FORWARD;
                    }
                    self.air_jumps = 0;
                    ability::face(player, way);
                    anim::cue(player, Cue::Jump);
                    log!("move: Asta's anti-magic leap");
                    return true;
                }
            }
            // 1.3.0: Zoro's Lion's Song: a dodge from a sprint is a long iai dash, the blade drawn on the way
            "zoro" => {
                if sprinting && dodge && self.lion_ready.is_none_or(|t| now >= t) {
                    self.lion_ready = Some(now + std::time::Duration::from_secs_f32(LION_COOLDOWN_S));
                    self.motion = Some(Motion::dash((way.0 * LION_DIST, 0.0, way.2 * LION_DIST), LION_S));
                    ability::face(player, way);
                    let p = ability::position(player);
                    puff(player, SLASH_FX, (p.0, p.1 + 1.0, p.2), way);
                    anim::cue(player, Cue::AirDash);
                    log!("move: Zoro's Lion's Song");
                    return true;
                }
            }
            // 1.3.0: Yami's dark-cloaked leap from a sprint or a crouch (Asta's, in darkness)
            "yami" => {
                if grounded && presses.jump() && (sprinting || k == Kind::Crouch) {
                    let p = ability::position(player);
                    puff(player, BLACK_FX, (p.0, p.1 + 0.1, p.2), way);
                    self.start_flight((way.0 * ASTA_LEAP_FORWARD, ASTA_LEAP_UP, way.2 * ASTA_LEAP_FORWARD));
                    if let Some(f) = &mut self.flight {
                        f.cap = ASTA_LEAP_FORWARD;
                    }
                    self.air_jumps = 0;
                    ability::face(player, way);
                    anim::cue(player, Cue::Jump);
                    log!("move: Yami's dark leap");
                    return true;
                }
            }
            // 1.3.0: Gojo's Infinity: jump held in the air once the double jump is used, he floats (nothing pulls
            // him down) for up to FLOAT_S (1.3.1, the user: removed)
            "gojo" if false => {
                let held = player.chr_ins.modules.action_request.action_timers.jump > 0.0;
                if airborne && held && self.air_jumps >= 1 && air_time > 0.3 {
                    let since = *self.float_since.get_or_insert(now);
                    if since.elapsed().as_secs_f32() < FLOAT_S {
                        let stick = input::stick_world().unwrap_or((0.0, 0.0, 0.0));
                        let v = (stick.0 * FLOAT_DRIFT, 0.0, stick.2 * FLOAT_DRIFT);
                        self.start_flight(v);
                        if let Some(f) = &mut self.flight {
                            f.cap = FLOAT_DRIFT;
                        }
                        return false;
                    }
                }
            }
            // 1.2.0 (user): Goku has All Might's Smash jump too; 1.3.0: Yuji's and Jotaro's superhuman leaps
            "all_might" | "goku" | "yuji" | "jotaro" => {
                if sprinting && presses.jump() && self.smash_ready.is_none_or(|t| now >= t) {
                    self.smash_ready = Some(now + std::time::Duration::from_secs_f32(SMASH_JUMP_COOLDOWN_S));
                    let p = ability::position(player);
                    puff(player, STORM_STOMP_FX, (p.0, p.1 + 0.2, p.2), way);
                    self.start_flight((way.0 * SMASH_JUMP_FORWARD, SMASH_JUMP_UP, way.2 * SMASH_JUMP_FORWARD));
                    if let Some(f) = &mut self.flight {
                        f.cap = SMASH_JUMP_FORWARD;
                    }
                    self.smash_jump = true;
                    ability::face(player, way);
                    anim::cue(player, Cue::Jump);
                    // 1.3.0: Jotaro's: Star Platinum appears where he stood and heaves him away (stands.rs)
                    if self.fighter == "jotaro" {
                        *crate::stands::REQUEST.lock().unwrap() = Some((0.8, crate::stands::LAUNCH));
                    }
                    log!("move: All Might's Smash jump");
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    /// 1.2.0 (the user: "deploy his blade as an ability that slowly glides him back down like a Fortnite
    /// glider"): Asta's glider on the swing button (L3 + RB / Q), in the air: he rides his blade down, the
    /// curved greatsword's board ride from Carian Combo Warriors, the Greatsword's black slab under his
    /// feet. Again (or the ground) puts it away.
    pub fn glide(&mut self, player: &mut PlayerIns) {
        if let Some(f) = self.flight.as_mut().filter(|f| f.glide.is_some()) {
            f.glide = None;
            crate::look::BOARD.store(0, std::sync::atomic::Ordering::Relaxed);
                crate::weapon::MOVE_TRACELESS.store(false, std::sync::atomic::Ordering::Relaxed);
            crate::weapon::set_move(0, 0.0);
            log!("move: glider away");
            return;
        }
        let ph = &player.chr_ins.modules.physics;
        let airborne = self.flight.is_some() || (ph.is_falling && !ph.is_touching_ground) || self.airborne_since.is_some();
        if !airborne || self.swing.is_some() {
            return;
        }
        if self.flight.is_none() {
            let v = self.velocity;
            self.start_flight(v);
        }
        let way = crate::ability::forward(player);
        if let Some(f) = &mut self.flight {
            f.glide = Some(Instant::now());
            f.slam = false;
            f.dash_left = 0.0;
            let sp = (f.vel.0 * f.vel.0 + f.vel.2 * f.vel.2).sqrt();
            if sp < GLIDE_SPEED * 0.5 {
                f.vel.0 = way.0 * GLIDE_SPEED * 0.6;
                f.vel.2 = way.2 * GLIDE_SPEED * 0.6;
            }
        }
        // the curved greatsword's moveset plays the ride; his body shows the blade under his feet
        let row = match LAB_GLIDE_ROW.load(std::sync::atomic::Ordering::Relaxed) {
            0 => GLIDE_BOARD,
            r => r,
        };
        crate::weapon::MOVE_TRACELESS.store(true, std::sync::atomic::Ordering::Relaxed);
        crate::weapon::set_move(row, 60.0);
        crate::look::BOARD.store(if LAB_BOARD_BODY.load(std::sync::atomic::Ordering::Relaxed) == 2 { crate::look::BOARD_FOOT } else { crate::look::BOARD_PELVIS }, std::sync::atomic::Ordering::Relaxed);
        self.glide_cue = true;
        log!("move: glider out");
    }

    /// 1.2.0: Goku's Instant Transmission on the swing button (lib.rs; L3 + RB / Q): behind the lock-on
    /// target (or the enemy in front), facing it, every TELEPORT_COOLDOWN_S. False when it can't now.
    pub fn teleport(&mut self, player: &mut PlayerIns) -> bool {
        let now = Instant::now();
        if self.teleport_ready.is_some_and(|t| now < t) {
            return false;
        }
        if self.instant_transmission(player) {
            self.teleport_ready = Some(now + std::time::Duration::from_secs_f32(TELEPORT_COOLDOWN_S));
            return true;
        }
        false
    }

    /// 1.3.0: the blink fighters' step (L3 + RB / Q): `dist` metres the way the stick points (else ahead), on the
    /// ground or in the air (BLINK_AIR a jump), every BLINK_COOLDOWN_S. Short of a wall it stops at the wall.
    pub fn blink(&mut self, player: &mut PlayerIns, dist: f32, fx: i32) -> bool {
        let now = Instant::now();
        if self.blink_ready.is_some_and(|t| now < t) {
            return false;
        }
        let ph = &player.chr_ins.modules.physics;
        let airborne = self.flight.is_some() || (ph.is_falling && !ph.is_touching_ground);
        if airborne && self.blinks >= BLINK_AIR {
            return false;
        }
        let p = ph.position;
        let way = input::stick_world().unwrap_or_else(|| ability::forward(player));
        // stop short of whatever is in the way (at the chest's height)
        let reach = crate::motion::ray(player, &HavokPosition(p.0, p.1 + 1.0, p.2, 0.0), PositionDelta(way.0 * dist, 0.0, way.2 * dist))
            .map(|h| (((h.0 - p.0).powi(2) + (h.2 - p.2).powi(2)).sqrt() - 0.8).max(0.0))
            .unwrap_or(dist);
        if reach < 1.5 {
            return false;
        }
        let to = (p.0 + way.0 * reach, p.2 + way.2 * reach);
        let y = if airborne { p.1 } else { crate::motion::ground_below(player, to.0, p.1 + 2.0, to.1, 6.0).unwrap_or(p.1) };
        puff(player, fx, (p.0, p.1 + 1.0, p.2), way);
        self.motion = None;
        if airborne {
            self.blinks += 1;
            let v = (way.0 * 3.0, 1.0, way.2 * 3.0);
            self.start_flight(v);
        } else {
            self.flight = None;
        }
        let phys = &mut player.chr_ins.modules.physics;
        phys.position = HavokPosition(to.0, y, to.1, p.3);
        phys.chr_proxy_pos_update_requested = true;
        ability::face(player, way);
        puff(player, fx, (to.0, y + 1.0, to.1), way);
        anim::cue(player, Cue::Dash);
        self.blink_ready = Some(now + std::time::Duration::from_secs_f32(BLINK_COOLDOWN_S));
        log!("move: {} blinks {reach:.1} m", self.fighter);
        true
    }

    /// Goku's Instant Transmission: behind the lock-on target, facing it. False if there is no floor
    /// there (he stays where he is).
    fn instant_transmission(&mut self, player: &mut PlayerIns) -> bool {
        if !player.chr_ins.is_locked_on {
            return false;
        }
        let t = player.chr_ins.lock_on_target_position;
        let p = player.chr_ins.modules.physics.position;
        let (dx, dz) = (t.0 - p.0, t.2 - p.2);
        let l = (dx * dx + dz * dz).sqrt();
        if !(0.5..=30.0).contains(&l) {
            return false;
        }
        let d = (dx / l, 0.0, dz / l);
        let to = (t.0 + d.0 * TELEPORT_BEHIND, t.2 + d.2 * TELEPORT_BEHIND);
        let Some(g) = crate::motion::ground_below(player, to.0, t.1 + 2.0, to.1, 6.0) else { return false };
        // nothing solid between the target and the spot (no teleporting through a wall); 1.3.1 (the user: fast moves must never
        // leave anyone in a wall): and room for his body past the spot (a wall just behind it took half of him), at the feet too
        let clear = TELEPORT_BEHIND + 0.6;
        if crate::motion::ray(player, &HavokPosition(t.0, t.1 + 1.0, t.2, 0.0), PositionDelta(d.0 * clear, 0.0, d.2 * clear)).is_some()
            || crate::motion::ray(player, &HavokPosition(t.0, t.1 + 0.4, t.2, 0.0), PositionDelta(d.0 * clear, 0.0, d.2 * clear)).is_some_and(|h| h.1 > g + 0.5)
        {
            return false;
        }
        puff(player, KI_FX, (p.0, p.1 + 1.0, p.2), d);
        self.flight = None;
        self.motion = None;
        let phys = &mut player.chr_ins.modules.physics;
        phys.position = HavokPosition(to.0, g, to.1, p.3);
        phys.chr_proxy_pos_update_requested = true;
        ability::face(player, (-d.0, 0.0, -d.2));
        puff(player, KI_FX, (to.0, g + 1.0, to.1), (-d.0, 0.0, -d.2));
        anim::cue(player, Cue::Dash);
        log!("move: Instant Transmission ({l:.1} m to the target)");
        true
    }

    // ------------------------------------------------------------ Black Whip

    /// The way Deku is travelling (horizontal unit vector) and how fast, when he is really moving.
    fn travel(&self) -> Option<(V3, f32)> {
        let v = self.flight.as_ref().map(|f| f.vel).or(self.swing.as_ref().map(|s| s.vel)).unwrap_or(self.velocity);
        let hs = (v.0 * v.0 + v.2 * v.2).sqrt();
        (hs > WHIP_MOVING).then(|| ((v.0 / hs, 0.0, v.2 / hs), hs))
    }

    /// Where the whip would catch right now: the top edge of a wall or roof, high on a trunk or a pillar,
    /// near and above, in frame. Standing, it looks where the camera looks (turned a little toward the
    /// stick); on the move (0.13.0) it looks along the way he travels, as Spider-Man's webs do, and a
    /// grip off to the side scores low (one behind him is never taken).
    fn pick(&self, player: &PlayerIns) -> Option<V3> {
        let h = unsafe { CSHavokMan::instance() }.ok()?;
        let pos = ability::position(player);
        let chest = (pos.0, pos.1 + 1.4, pos.2);
        let look = input::camera_axes().map(|(f, _)| f).unwrap_or_else(|| ability::forward(player));
        let look = match input::stick_world() {
            Some(s) => ((look.0 * 2.0 + s.0) / 3.0, 0.0, (look.2 * 2.0 + s.2) / 3.0),
            None => look,
        };
        let travel = self.travel();
        let base = match travel {
            Some((d, _)) => (d.0 * 0.65 + look.0 * 0.35, 0.0, d.2 * 0.65 + look.2 * 0.35),
            None => look,
        };
        let l = (base.0 * base.0 + base.2 * base.2).sqrt().max(1e-3);
        let base = (base.0 / l, 0.0, base.2 / l);
        let ray = |from: V3, d: V3| crate::motion::ray(player, &HavokPosition(from.0, from.1, from.2, 0.0), PositionDelta(d.0, d.1, d.2)).map(|p| (p.0, p.1, p.2));
        // a fan of rays over what the camera sees, up and ahead. Every hit is a candidate, lifted to the
        // top edge of what it hit when that edge is in reach (a roof line, a wall top, a ledge): where a
        // tendril would grab. Height counts, distance costs, off screen or past WHIP_RANGE never.
        let mut best: Option<(V3, f32)> = None;
        for pitch in [76f32, 66.0, 56.0, 46.0, 36.0, 26.0, 16.0] {
            let (ps, pc) = pitch.to_radians().sin_cos();
            for yaw in [0f32, -0.17, 0.17, -0.34, 0.34, -0.52, 0.52, -0.7, 0.7] {
                let (ys, yc) = yaw.sin_cos();
                let d = (base.0 * yc - base.2 * ys, 0.0, base.0 * ys + base.2 * yc);
                let Some(hit) = ray(chest, (d.0 * pc * WHIP_RANGE, ps * WHIP_RANGE, d.2 * pc * WHIP_RANGE)) else { continue };
                // the top: from above the hit and a little past the face, straight down
                let mut grip = hit;
                for up in [5.0f32, 2.5, 1.2] {
                    if ray((hit.0 - d.0 * 0.2, hit.1, hit.2 - d.2 * 0.2), (0.0, up, 0.0)).is_some() {
                        continue; // an overhang above the hit: no clear top this high
                    }
                    let top_from = (hit.0 + d.0 * 0.4, hit.1 + up, hit.2 + d.2 * 0.4);
                    if let Some(top) = ray(top_from, (0.0, -up - 0.2, 0.0)) {
                        if top.1 > hit.1 + 0.2 && top.1 < hit.1 + up - 0.1 {
                            grip = (top.0 - d.0 * 0.3, top.1, top.2 - d.2 * 0.3);
                            break;
                        }
                    }
                }
                let dist = ((grip.0 - chest.0).powi(2) + (grip.1 - chest.1).powi(2) + (grip.2 - chest.2).powi(2)).sqrt();
                let rise = grip.1 - pos.1;
                let flat = ((grip.0 - pos.0).powi(2) + (grip.2 - pos.2).powi(2)).sqrt();
                if dist < 4.5 || dist > WHIP_RANGE || rise < 3.0 || flat < 1.5 {
                    continue;
                }
                // in frame across the screen (above its top is fine: ledges overhead, as in Spider-Man)
                let framed = input::on_screen(grip).map(|(x, y, _)| x.abs() < 1.0 && y > -0.9).unwrap_or(true);
                if !framed {
                    continue;
                }
                // nothing solid between Deku and it short of the grip itself
                let to = (grip.0 - chest.0, grip.1 - chest.1, grip.2 - chest.2);
                if let Some(block) = ray(chest, (to.0 * 0.9, to.1 * 0.9, to.2 * 0.9)) {
                    let gap = ((block.0 - grip.0).powi(2) + (block.1 - grip.1).powi(2) + (block.2 - grip.2).powi(2)).sqrt();
                    if gap > 3.0 {
                        continue;
                    }
                }
                let ahead_k = ((grip.0 - pos.0) * base.0 + (grip.2 - pos.2) * base.2) / flat;
                let score = match travel {
                    // on the move: ahead along the swing first
                    Some((dir, _)) => {
                        let along = ((grip.0 - pos.0) * dir.0 + (grip.2 - pos.2) * dir.2) / flat;
                        if along < 0.1 {
                            continue;
                        }
                        rise.min(18.0) - dist * 0.35 + along * 7.0 + ahead_k * 2.0
                    }
                    None => rise.min(18.0) - dist * 0.45 + ahead_k * 3.0,
                };
                if best.is_none_or(|b| score > b.1) {
                    best = Some((grip, score));
                }
            }
        }
        best.map(|b| b.0)
    }

    /// Deku's whip marker (0.13.0): where the whip would catch if thrown now, refreshed ten times a
    /// second while he isn't swinging (lib.rs passes `on` for Deku outside menus).
    pub fn aim_tick(&mut self, player: &PlayerIns, on: bool) -> Option<V3> {
        if !on || self.swing.is_some() {
            self.aim = None;
            return None;
        }
        let now = Instant::now();
        if self.aim_at.is_none_or(|t| now.duration_since(t).as_secs_f32() > 0.1) {
            self.aim_at = Some(now);
            self.aim = self.pick(player);
        }
        self.aim
    }

    /// 1.3.0: whether the swing would catch something now (Killua steps instead when it wouldn't)
    pub fn grip_in_reach(&self, player: &PlayerIns) -> bool {
        self.swing.is_none() && self.pick(player).is_some()
    }

    /// Throw the whip at something to swing from. True when it caught something.
    pub fn whip(&mut self, player: &mut PlayerIns) -> bool {
        let now = Instant::now();
        if self.whip_ready.is_some_and(|t| now < t) || self.swing.is_some() {
            return false;
        }
        self.whip_ready = Some(now + std::time::Duration::from_secs_f32(WHIP_COOLDOWN));
        self.chain_at = None;
        let Some(grip) = self.pick(player) else {
            // nothing to hold in reach: the tendril shoots out and snaps back
            let pos = ability::position(player);
            let chest = (pos.0, pos.1 + 1.4, pos.2);
            let f = input::camera_axes().map(|(f, _)| f).unwrap_or_else(|| ability::forward(player));
            crate::world_fx::whip_miss(chest, (chest.0 + f.0 * 9.0, chest.1 + 6.0, chest.2 + f.2 * 9.0));
            self.whip_ready = Some(now + std::time::Duration::from_secs_f32(0.4));
            log!("move: whip found nothing to hold");
            return false;
        };
        self.attach(player, grip);
        true
    }

    /// Swing from `grip`. 0.13.0, after Spider-Man (PS5): the tendril holds the grip, but the swing turns
    /// about a point above his path. On the move, a grip off to the side only bends his line a little
    /// (WHIP_SIDE_KEEP of the way); his momentum is kept as it is, never redirected or added to.
    fn attach(&mut self, player: &mut PlayerIns, grip: V3) {
        let now = Instant::now();
        let pos = ability::position(player);
        let mut vel = self.flight.as_ref().map(|f| f.vel).unwrap_or(self.velocity);
        let rel = (grip.0 - pos.0, grip.1 - pos.1, grip.2 - pos.2);
        let flat = (rel.0 * rel.0 + rel.2 * rel.2).sqrt().max(1e-3);
        let (dir, moving) = match self.travel() {
            Some((d, _)) => (d, true),
            None => ((rel.0 / flat, 0.0, rel.2 / flat), false),
        };
        let along = rel.0 * dir.0 + rel.2 * dir.2;
        let side = (rel.0 - dir.0 * along, rel.2 - dir.2 * along);
        // the swing's pivot: as high as the grip (never more than WHIP_MAX_RISE over him), ahead along
        // his path about as far as it is high (a far grip doesn't drag him across to it), and on the
        // move only a little toward a grip off to the side
        let rise = (grip.1 - pos.1).min(WHIP_MAX_RISE);
        let reach = (rise * 0.8).max(3.0);
        let (along, side_k) = if moving { (along.clamp(3.0, reach), WHIP_SIDE_KEEP) } else { (along.min(reach), 0.0) };
        let pivot = (pos.0 + dir.0 * along + side.0 * side_k, pos.1 + rise, pos.2 + dir.2 * along + side.1 * side_k);
        let len = ((pivot.0 - pos.0).powi(2) + (pivot.1 - pos.1).powi(2) + (pivot.2 - pos.2).powi(2)).sqrt();
        // the rope is taken up so the bottom of the arc clears the ground under him and under the pivot
        // (measured from his height: the grip's own roof doesn't count)
        let here = crate::motion::ground_below(player, pos.0, pos.1 + 1.0, pos.2, 60.0).unwrap_or(pos.1 - 30.0);
        let there = crate::motion::ground_below(player, pivot.0, pos.1 + 1.5, pivot.2, 60.0).unwrap_or(here);
        let floor = here.max(there);
        let max_len = len.min(pivot.1 - floor - 1.3).min(WHIP_MAX_ROPE).max(3.5);
        // from (nearly) standing: a kick along the swing to get going; never past the cap
        let hs = (vel.0 * vel.0 + vel.2 * vel.2).sqrt();
        if hs < WHIP_KICK {
            let need = WHIP_KICK - hs;
            vel = (vel.0 + dir.0 * need, vel.1.max(5.0), vel.2 + dir.2 * need);
        }
        let vel = cap(vel, WHIP_MAX_SPEED);
        self.flight = None;
        self.swing = Some(Swing { anchor: pivot, grip, len, max_len, dir, vel, since: now, last: now, put: pos, safe: pos, wall_time: 0.0, prev: None, offset: havok_offset(player), stall: 0.0 });
        ability::face(player, dir);
        anim::cue(player, if self.rubber { Cue::LuffySwing } else { Cue::Whip });
        log!("move: whip holds {:.1} m away, {:.1} m up (swing {len:.1} -> {max_len:.1} m, {:.1} m/s{})", flat, grip.1 - pos.1, (vel.0 * vel.0 + vel.2 * vel.2).sqrt(), if moving { ", on the move" } else { "" });
    }

    fn let_go(&mut self, player: &mut PlayerIns, how: Release) {
        let Some(s) = self.swing.take() else { return };
        let pos = ability::position(player);
        let mut v = s.vel;
        // letting go on the way up the far side, at or near the top: the swing throws him 3-4 m up
        let ahead = (pos.0 - s.anchor.0) * s.dir.0 + (pos.2 - s.anchor.2) * s.dir.2;
        let peak = how == Release::ArcEnd || (ahead > s.len * 0.1 && v.1 > -4.0 && s.since.elapsed().as_secs_f32() > 0.3);
        if peak {
            v.1 = WHIP_PEAK_LIFT;
        } else if how == Release::Jump {
            v.1 = v.1.max(0.0) + 5.0;
        } else {
            v.1 = v.1.max(-2.0) + 1.0;
        }
        let h = (v.0 * v.0 + v.2 * v.2).sqrt();
        if h > WHIP_RELEASE_MAX {
            v.0 *= WHIP_RELEASE_MAX / h;
            v.2 *= WHIP_RELEASE_MAX / h;
        }
        self.start_flight(v);
        if let Some(f) = &mut self.flight {
            f.cap = (v.0 * v.0 + v.2 * v.2).sqrt().max(AIR_MAX);
        }
        // letting go is like a jump: the double jump is there again (the air dash is not refilled)
        self.air_jumps = 0;
        // 0.15.0: always one of the mod's own air poses. The jump the game was asked for here is refused in
        // mid-air, so the swing pose (drawn hanging from the rope, off the body's real place) stayed on
        // through the fall: the player saw Luffy vanish in the air until he landed.
        anim::cue(player, Cue::AirDash);
        log!("move: whip let go ({how:?}{}) at {:.1} m/s, {:.1} up", if peak { ", peak boost" } else { "" }, (v.0 * v.0 + v.2 * v.2).sqrt(), v.1);
    }

    /// One frame of the swing: a pendulum on the tendril.
    fn swing_step(&mut self, player: &mut PlayerIns) {
        let Some(s) = self.swing.as_mut() else { return };
        let now = Instant::now();
        let dt = now.duration_since(s.last).as_secs_f32().clamp(0.0, 0.05);
        s.last = now;
        let pos = ability::position(player);
        // the body isn't where we put it: a map block re-based the coordinates (everything moved by
        // the same offset), so the swing's points move with it. Nothing ever jumps the body itself.
        let moved = (pos.0 - s.put.0, pos.1 - s.put.1, pos.2 - s.put.2);
        let offset = havok_offset(player);
        if (moved.0 * moved.0 + moved.1 * moved.1 + moved.2 * moved.2).sqrt() > 4.0 {
            // 0.14.0: a real re-base moves the havok <-> block offset by the same amount. When it didn't,
            // the body itself was thrown (a position we wrote landed after the game re-based, putting him
            // that far off: inside the ground, where nobody could see him): put him back on the arc.
            let shift = (offset.0 - s.offset.0, offset.1 - s.offset.1, offset.2 - s.offset.2);
            // (1.2.0: and only by whole map cells: the block position can flicker by odd amounts)
            let rebase = (shift.0 - moved.0).abs() + (shift.1 - moved.1).abs() + (shift.2 - moved.2).abs() < 1.5 && crate::rebase::on_grid(shift);
            if rebase {
                log!("move: whip follows a coordinate shift of ({:.1}, {:.1}, {:.1})", moved.0, moved.1, moved.2);
                s.anchor = (s.anchor.0 + moved.0, s.anchor.1 + moved.1, s.anchor.2 + moved.2);
                s.grip = (s.grip.0 + moved.0, s.grip.1 + moved.1, s.grip.2 + moved.2);
                s.safe = (s.safe.0 + moved.0, s.safe.1 + moved.1, s.safe.2 + moved.2);
                s.put = pos;
            } else {
                // 0.14.1: something else moved the body (the void guard, the game): the swing ends there.
                // (0.14.0 put him back on the arc, and fought the guard every frame inside the cliff)
                log!("move: whip swing ends: the body was moved ({:.1}, {:.1}, {:.1}) by something else", moved.0, moved.1, moved.2);
                self.swing = None;
                return;
            }
            s.prev = None;
        }
        s.offset = offset;
        let t = s.since.elapsed().as_secs_f32();
        // the game's own collision held the body back from where we put it last frame (a fence, a
        // character): the swing loses that speed instead of piling it up, and counts as against a wall
        let mut stuck = false;
        let off = ((pos.0 - s.put.0).powi(2) + (pos.1 - s.put.1).powi(2) + (pos.2 - s.put.2).powi(2)).sqrt();
        if off > 0.12 && off <= 4.0 && t > 0.05 {
            stuck = true;
            s.vel = (s.vel.0 * 0.6, s.vel.1.min(2.0), s.vel.2 * 0.6);
        }
        // and however it happens, the swing's speed is never more than the body really shows for long:
        // what he actually covered since last frame against what the swing says
        if let Some(prev) = s.prev {
            if dt > 0.004 && t > 0.2 {
                let real = ((pos.0 - prev.0).powi(2) + (pos.1 - prev.1).powi(2) + (pos.2 - prev.2).powi(2)).sqrt() / dt;
                let said = (s.vel.0 * s.vel.0 + s.vel.1 * s.vel.1 + s.vel.2 * s.vel.2).sqrt();
                if said > 3.0 && real < said * 0.35 && real < 20.0 {
                    stuck = true;
                    let k = ((real + 1.0) / said).min(1.0);
                    s.vel = (s.vel.0 * k, s.vel.1 * k, s.vel.2 * k);
                }
            }
        }
        s.prev = Some(pos);
        s.vel.1 -= WHIP_GRAVITY * dt;
        // the rope reels in smoothly to its swing length
        if s.len > s.max_len {
            s.len = (s.len - WHIP_REEL * dt).max(s.max_len);
        }
        let r = (pos.0 - s.anchor.0, pos.1 - s.anchor.1, pos.2 - s.anchor.2);
        let rl = (r.0 * r.0 + r.1 * r.1 + r.2 * r.2).sqrt().max(1e-3);
        let n = (r.0 / rl, r.1 / rl, r.2 / rl);
        // a little drive along the swing below the pivot, up to a cruising speed (the swing never stalls,
        // and never speeds up past it on its own)
        let sp = (s.vel.0 * s.vel.0 + s.vel.1 * s.vel.1 + s.vel.2 * s.vel.2).sqrt();
        if sp < WHIP_CRUISE && n.1 < -0.3 {
            let fd = s.dir.0 * n.0 + s.dir.2 * n.2;
            let tg = (s.dir.0 - n.0 * fd, -n.1 * fd, s.dir.2 - n.2 * fd);
            let tl = (tg.0 * tg.0 + tg.1 * tg.1 + tg.2 * tg.2).sqrt();
            if tl > 0.2 {
                let k = WHIP_DRIVE * dt / tl;
                s.vel = (s.vel.0 + tg.0 * k, s.vel.1 + tg.1 * k, s.vel.2 + tg.2 * k);
            }
        }
        // the stick steers gently: the swing's heading and the body's motion turn toward it
        if let Some(st) = input::stick_world() {
            let cur = s.dir.2.atan2(s.dir.0);
            let mut d = st.2.atan2(st.0) - cur;
            while d > std::f32::consts::PI {
                d -= std::f32::consts::TAU;
            }
            while d < -std::f32::consts::PI {
                d += std::f32::consts::TAU;
            }
            if d.abs() < 2.4 {
                let turn = d.clamp(-WHIP_STEER * dt, WHIP_STEER * dt);
                let (ts, tc) = turn.sin_cos();
                // rotate (x, z) about the vertical: atan2(z, x) grows by `turn`
                s.dir = (s.dir.0 * tc - s.dir.2 * ts, 0.0, s.dir.0 * ts + s.dir.2 * tc);
                s.vel = (s.vel.0 * tc - s.vel.2 * ts, s.vel.1, s.vel.0 * ts + s.vel.2 * tc);
            }
        }
        // rising ground (a slope, a rock) under the arc ahead: the tendril shortens to carry him over
        // it instead of dragging him along the ground into it
        let look = (pos.0 + s.vel.0 * 0.25, pos.2 + s.vel.2 * 0.25);
        if let Some(g) = crate::motion::ground_below(player, look.0, pos.1 + 2.5, look.1, 4.0) {
            if pos.1 < g + 1.2 && t > 0.15 {
                s.len = (s.len - WHIP_REEL * dt).max(3.5);
                s.max_len = s.max_len.min(s.len);
                s.vel.1 = s.vel.1.max(0.0);
            }
        }
        let mut np = (pos.0 + s.vel.0 * dt, pos.1 + s.vel.1 * dt, pos.2 + s.vel.2 * dt);
        let d = (np.0 - s.anchor.0, np.1 - s.anchor.1, np.2 - s.anchor.2);
        let dl = (d.0 * d.0 + d.1 * d.1 + d.2 * d.2).sqrt().max(1e-3);
        if dl > s.len {
            // on the rope: back onto the sphere, the outward speed turned along the arc (Spider-Man
            // keeps his speed through a swing; a little is lost, nothing is gained)
            let n = (d.0 / dl, d.1 / dl, d.2 / dl);
            np = (s.anchor.0 + n.0 * s.len, s.anchor.1 + n.1 * s.len, s.anchor.2 + n.2 * s.len);
            let before = (s.vel.0 * s.vel.0 + s.vel.1 * s.vel.1 + s.vel.2 * s.vel.2).sqrt();
            let out = s.vel.0 * n.0 + s.vel.1 * n.1 + s.vel.2 * n.2;
            if out > 0.0 {
                s.vel = (s.vel.0 - n.0 * out, s.vel.1 - n.1 * out, s.vel.2 - n.2 * out);
                let after = (s.vel.0 * s.vel.0 + s.vel.1 * s.vel.1 + s.vel.2 * s.vel.2).sqrt();
                if after > 1.0 {
                    let k = ((before * 0.97) / after).clamp(1.0, 1.3);
                    s.vel = (s.vel.0 * k, s.vel.1 * k, s.vel.2 * k);
                }
            }
        }
        s.vel = cap(s.vel, WHIP_MAX_SPEED);
        #[cfg(feature = "lab")]
        log!("lab swing: t {t:.2} pos ({:.1}, {:.1}, {:.1}) pivot ({:.1}, {:.1}, {:.1}) len {:.1} vel ({:.1}, {:.1}, {:.1}) speed {:.1}", np.0, np.1, np.2, s.anchor.0, s.anchor.1, s.anchor.2, s.len, s.vel.0, s.vel.1, s.vel.2, (s.vel.0 * s.vel.0 + s.vel.1 * s.vel.1 + s.vel.2 * s.vel.2).sqrt());
        let (mut dx, dy, mut dz) = (np.0 - pos.0, np.1 - pos.1, np.2 - pos.2);
        let p = player.chr_ins.modules.physics.position;
        // a wall or a steep slope in the way: no sideways push into it, the swing goes on up it
        if crate::motion::wall_ahead(player, &p, dx, 0.0, dz) {
            dx = 0.0;
            dz = 0.0;
            s.vel.0 *= 0.5;
            s.vel.2 *= 0.5;
            stuck = true;
        }
        // 0.13.0: nothing solid between the body now and where this frame puts it, at the feet, the
        // middle and the head, rising or falling (the sideways wall check alone let a rising swing
        // carry him up through a roof edge into the church)
        let mut dy = dy;
        if let Ok(h) = unsafe { CSHavokMan::instance() } {
            let len = (dx * dx + dy * dy + dz * dz).sqrt();
            if len > 1e-4 {
                let k = (len + 0.35) / len;
                let blocked = [0.5f32, 1.0, 1.7].iter().any(|hh| {
                    crate::motion::ray(player, &HavokPosition(p.0, p.1 + hh, p.2, 0.0), PositionDelta(dx * k, dy * k, dz * k)).is_some()
                });
                if blocked {
                    // try the move without its rising part, then not at all
                    let flat_ok = ![0.5f32, 1.0, 1.7].iter().any(|hh| {
                        crate::motion::ray(player, &HavokPosition(p.0, p.1 + hh, p.2, 0.0), PositionDelta(dx * k, 0.0, dz * k)).is_some()
                    });
                    if flat_ok {
                        // rising into an overhang: no higher; falling onto something: it holds him
                        if dy > 0.0 {
                            s.vel.1 = s.vel.1.min(0.0);
                        }
                        dy = 0.0;
                    } else {
                        dx = 0.0;
                        dz = 0.0;
                        s.vel.0 *= 0.5;
                        s.vel.2 *= 0.5;
                        stuck = true;
                        if dy > 0.0 && crate::motion::ray(player, &HavokPosition(p.0, p.1 + 1.7, p.2, 0.0), PositionDelta(0.0, dy + 0.35, 0.0)).is_some() {
                            dy = 0.0;
                            s.vel.1 = s.vel.1.min(0.0);
                        }
                    }
                }
            }
        }
        // swung into a wall and held there: he springs up it and flies on (as Spider-Man runs up one)
        s.wall_time = if stuck { s.wall_time + dt } else { 0.0 };
        // 0.14.0: a swing that ran into a wall and stalled (hardly moving, the rope pulling him up the
        // face) hung there for seconds under the grip with the camera trapped behind the stonework: the
        // body "went invisible". Stalled for a moment, he springs up and over the top instead.
        let hsp = (s.vel.0 * s.vel.0 + s.vel.2 * s.vel.2).sqrt();
        s.stall = if t > 0.35 && hsp < 4.5 && s.vel.1.abs() < 4.0 { s.stall + dt } else { 0.0 };
        let wall_pop = s.wall_time > 0.2 || s.stall > 0.25;
        let np = (p.0 + dx, p.1 + dy, p.2 + dz);
        let ground = crate::motion::ground_below(player, p.0 + dx, np.1 + 0.5, p.2 + dz, 0.6);
        let landed = dy < 0.0 && t > 0.7 && ground.is_some_and(|g| np.1 <= g + 0.05);
        // the far side of the arc: up near the pivot's height ahead of it, or the swing turning back
        let ahead = (np.0 - s.anchor.0) * s.dir.0 + (np.2 - s.anchor.2) * s.dir.2;
        let arc_end = t > 0.35 && ahead > 0.0 && (np.1 > s.anchor.1 - s.len * 0.3 || (ahead > s.len * 0.25 && s.vel.1 < 1.0));
        let ny = match ground {
            Some(g) if np.1 < g => g,
            _ => np.1,
        };
        let vh = (s.vel.0, 0.0, s.vel.2);
        // 1.2.0 round 4: never into rock. The checks above cast from the body, and a ray that starts
        // inside the rock sees nothing; this one comes from the last spot known to be in the open, so a
        // cliff face between there and here is always seen. Crossing one, he is put back there and the
        // swing ends (he drops away from the face).
        let to = (p.0 + dx, ny, p.2 + dz);
        let from = s.safe;
        let way = (to.0 - from.0, to.1 - from.1, to.2 - from.2);
        let far = (way.0 * way.0 + way.1 * way.1 + way.2 * way.2).sqrt();
        if far > 0.05 {
            let crosses = [0.9f32, 1.5].iter().any(|hh| crate::motion::ray(player, &HavokPosition(from.0, from.1 + hh, from.2, 0.0), PositionDelta(way.0, way.1, way.2)).is_some());
            if crosses {
                log!("move: whip swing would cross into rock ({far:.1} m from the open): back out, the swing ends");
                {
                    let phys = &mut player.chr_ins.modules.physics;
                    phys.position = HavokPosition(from.0, from.1, from.2, p.3);
                    phys.chr_proxy_pos_update_requested = true;
                }
                s.vel = (0.0, 0.0, 0.0);
                self.let_go(player, Release::Button);
                return;
            }
            s.safe = to;
        }
        s.put = (p.0 + dx, ny, p.2 + dz);
        {
            let phys = &mut player.chr_ins.modules.physics;
            phys.position = HavokPosition(p.0 + dx, ny, p.2 + dz, p.3);
            phys.chr_proxy_pos_update_requested = true;
        }
        if (vh.0 * vh.0 + vh.2 * vh.2).sqrt() > 2.0 {
            ability::face(player, vh);
        }
        player.chr_ins.modules.fall.fall_timer = 0.0;
        player.chr_ins.modules.physics.is_falling = false;
        player.chr_ins.modules.physics.is_touching_ground = true;
        if wall_pop {
            // up past the grip (the top of what he hit) and on over it
            let clear = (s.grip.1 + 1.6 - p.1).max(0.0);
            let lift = (2.0 * GRAVITY_UP * clear).sqrt().clamp(WHIP_PEAK_LIFT * 1.15, 17.0);
            log!("move: whip swing met a wall ({t:.1}s, stalled {:.2}s): up it, {lift:.1} m/s to clear {clear:.1} m", s.stall);
            let on = (s.dir.0 * 4.0, s.dir.2 * 4.0);
            s.vel = (on.0, 0.0, on.1);
            self.let_go(player, Release::ArcEnd);
            if let Some(f) = &mut self.flight {
                f.vel.1 = lift;
            }
            self.chain_at = Some(now + std::time::Duration::from_secs_f32(WHIP_CHAIN_AFTER));
        } else if arc_end {
            // the swing is spent: he flies on, thrown up a little; holding the whip throws the next
            log!("move: whip arc ends ({t:.1}s)");
            self.let_go(player, Release::ArcEnd);
            self.chain_at = Some(now + std::time::Duration::from_secs_f32(WHIP_CHAIN_AFTER));
        } else if landed || t > WHIP_MAX_S || dl < 2.5 {
            log!("move: whip ends (landed {landed}, {t:.1}s, rope {dl:.1} m)");
            self.let_go(player, Release::Button);
        }
    }

    /// Leaving the world (load, death): forget everything we wrote.
    /// (the fall guard's switch goes off with it)
    pub fn reset(&mut self) {
        if self.fall_guard.is_some() {
            set_no_dead(NO_DEAD_FALL, false);
        }
        BODY_FLOWN.store(false, std::sync::atomic::Ordering::Relaxed);
        // Asta's glider: the board goes with it (or his body would keep showing it on the ground)
        if self.glide_out || crate::look::BOARD.load(std::sync::atomic::Ordering::Relaxed) > 0 {
            crate::look::BOARD.store(0, std::sync::atomic::Ordering::Relaxed);
            crate::weapon::MOVE_TRACELESS.store(false, std::sync::atomic::Ordering::Relaxed);
            crate::weapon::set_move(0, 0.0);
        }
        *self = Self { air_moves: self.air_moves, flies: self.flies, rubber: self.rubber, fighter: self.fighter, air_dash_max: self.air_dash_max, dives: self.dives, ..Self::default() };
    }
}

/// 0.14.1: the body is inside solid ground: nothing under its middle (a ray from inside a mesh sees
/// no face), but straight down from high above there is ground over it. The top of it, then.
/// (Under a roof or a bridge the ray down from the body does hit the floor, so that never counts.)
pub fn inside_solid(player: &PlayerIns) -> Option<f32> {
    let p = player.chr_ins.modules.physics.position;
    let mid = p.1 + 0.9;
    if crate::motion::ground_below(player, p.0, mid, p.2, 300.0).is_some() {
        return None;
    }
    let h = unsafe { CSHavokMan::instance() }.ok()?;
    let hit = crate::motion::ray(player, &HavokPosition(p.0, mid + 60.0, p.2, 0.0), PositionDelta(0.0, -60.0, 0.0))?;
    if hit.1 <= mid {
        return None;
    }
    // 0.14.2: and nothing at all is seen from there, up or sideways (inside a mesh no face is; in a room
    // with a floor on a layer we don't know, the walls or the ceiling are, and that is no reason to
    // move anyone)
    let o = HavokPosition(p.0, mid, p.2, 0.0);
    let seen = [(0.0, 40.0, 0.0), (40.0, 0.0, 0.0), (-40.0, 0.0, 0.0), (0.0, 0.0, 40.0), (0.0, 0.0, -40.0)]
        .iter()
        .any(|d| crate::motion::ray(player, &o, PositionDelta(d.0, d.1, d.2)).is_some());
    (!seen).then_some(hit.1)
}

/// 0.14.3: standing inside rock (on a face inside a cliff, where a move left him): a surface over his
/// head seen from high above, nothing at all seen from his head upward or sideways (from inside a mesh
/// no face is), so he is not in a room or a cave. The top of it, then.
pub fn inside_rock_standing(player: &PlayerIns) -> Option<f32> {
    let p = player.chr_ins.modules.physics.position;
    let head = p.1 + 1.7;
    let top = crate::motion::ray(player, &HavokPosition(p.0, head + 80.0, p.2, 0.0), PositionDelta(0.0, -80.0, 0.0))?;
    if top.1 < head + 0.5 {
        return None;
    }
    let o = HavokPosition(p.0, head, p.2, 0.0);
    let seen = [(0.0, 80.0, 0.0), (30.0, 0.0, 0.0), (-30.0, 0.0, 0.0), (0.0, 0.0, 30.0), (0.0, 0.0, -30.0)]
        .iter()
        .any(|d| crate::motion::ray(player, &o, PositionDelta(d.0, d.1, d.2)).is_some());
    (!seen).then_some(top.1)
}

/// Havok minus block coordinates of the player (changes when the map re-bases havok space).
fn havok_offset(player: &PlayerIns) -> V3 {
    let (h, b) = (player.chr_ins.modules.physics.position, &player.block_position);
    (h.0 - b.x, h.1 - b.y, h.2 - b.z)
}

/// `v` with its length capped at `max`.
fn cap(v: V3, max: f32) -> V3 {
    let l = (v.0 * v.0 + v.1 * v.1 + v.2 * v.2).sqrt();
    if l > max { (v.0 * max / l, v.1 * max / l, v.2 * max / l) } else { v }
}

/// 0.14.0: our flight, slide or swing moves the body (it then says "touching ground" to the game, so
/// look.rs can't take that for standing).
pub static BODY_FLOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 0.15.0: a fall into the void now wouldn't be the player's doing (set by rescue.rs every frame).
pub static VOID_SAFE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Who holds the game's own "player can't die" switch (it holds HP at 1).
pub const NO_DEAD_FALL: u8 = 1;
pub const NO_DEAD_ART: u8 = 2;
pub const NO_DEAD_LOOK: u8 = 4;
pub const NO_DEAD_RESCUE: u8 = 8;
/// 1.3.0: a villain's special, held to its share of the bar (villains.rs special_guard)
pub const NO_DEAD_SPECIAL: u8 = 16;
static NO_DEAD: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// 1.1.0: the switch is on while any of its holders wants it. Before, each one (the fall guard, the boss
/// art guard, a fighter change, the rescue) turned it off for everyone when its own need ended: the
/// user's lethal hit from Rennala's cataclysm landed while it was half on, and they got up again at
/// 0 HP after their death animation.
pub fn set_no_dead(who: u8, on: bool) {
    let mask = if on { NO_DEAD.fetch_or(who, std::sync::atomic::Ordering::Relaxed) | who } else { NO_DEAD.fetch_and(!who, std::sync::atomic::Ordering::Relaxed) & !who };
    if let Ok(f) = unsafe { eldenring::cs::WorldChrManDbgFlags::instance_mut() } {
        f.player_no_dead = mask != 0 && !dying();
    }
}

/// 2.0.6: when the player's death began (ms since the epoch; 0: none). Lab limbo runs: a lethal blow in the air was given
/// its death (bosses::kill_player), and on the next frame the fall guard, the player still in the air, held them up again:
/// they stood at 1 HP for 40 s. From a death's start until the next load (or DEATH_HOLD), no holder can stop it.
static DYING_SINCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const DEATH_HOLD_MS: u64 = 12_000;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// A death under way (begin_death within DEATH_HOLD_MS, no load since).
pub fn dying() -> bool {
    let t = DYING_SINCE.load(std::sync::atomic::Ordering::Relaxed);
    t != 0 && now_ms().saturating_sub(t) < DEATH_HOLD_MS
}

/// The player's death starts now: every "can't die" holder lets go and none can hold them up until it's over.
pub fn begin_death() {
    DYING_SINCE.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
    if let Ok(f) = unsafe { eldenring::cs::WorldChrManDbgFlags::instance_mut() } {
        f.player_no_dead = false;
    }
}

/// A load: any death is over.
pub fn end_death() {
    DYING_SINCE.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Who holds the switch now (bits NO_DEAD_*).
pub fn no_dead_mask() -> u8 {
    // (2.0.6: a death under way is held by nothing)
    if dying() {
        return 0;
    }
    NO_DEAD.load(std::sync::atomic::Ordering::Relaxed)
}

/// Every holder lets go: the player is meant to die now (a fall to their death, the last resort).
pub fn allow_death() {
    NO_DEAD.store(0, std::sync::atomic::Ordering::Relaxed);
    if let Ok(f) = unsafe { eldenring::cs::WorldChrManDbgFlags::instance_mut() } {
        f.player_no_dead = false;
    }
}

// 1.2.0: the movement effects: Elden Ring's own bullets, muted to 1 damage (they are the look only)
const BLAST_FX: i32 = crate::scripts::BLAST;
/// 1.3.0: the new fighters' movement
const SLASH_FX: i32 = crate::scripts::VACUUM_SLICE;
pub const LIGHTNING_FX: i32 = crate::scripts::LIGHTNING_SLASH;
pub const SHUNPO_FX: i32 = crate::scripts::CRYSTAL_BURST;
pub const DARK_FX: i32 = crate::scripts::BLACK_FLAME;
/// Zoro's Lion's Song: this far, this long, ready again after
const LION_DIST: f32 = 14.0;
const LION_S: f32 = 0.35;
const LION_COOLDOWN_S: f32 = 2.5;
/// Gojo's Infinity float: how long, how fast the stick drifts him
const FLOAT_S: f32 = 2.0;
const FLOAT_DRIFT: f32 = 4.0;
/// the blink fighters: a blink every this long, this many in the air a jump
const BLINK_COOLDOWN_S: f32 = 0.8;
const BLINK_AIR: u8 = 2;
const ICE_FX: i32 = crate::scripts::HOARFROST;
const FLAME_FX: i32 = crate::scripts::FLAME_SLING;
const BLACK_FX: i32 = crate::scripts::BLACK_FLAME;
const KI_FX: i32 = crate::scripts::CRYSTAL_BURST;
const STORM_STOMP_FX: i32 = crate::scripts::STORM_STOMP;
const ROCK_BLASTER_FX: i32 = crate::scripts::ROCK_BLASTER;

/// A movement effect at `at` (havok), muted first so it never hurts anything.
fn puff(player: &PlayerIns, b: i32, at: V3, dir: V3) {
    crate::damage::mute(&[b], "physical");
    let l = (dir.0 * dir.0 + dir.1 * dir.1 + dir.2 * dir.2).sqrt().max(1e-3);
    let _ = ability::fire_at(player, b, at, (dir.0 / l, dir.1 / l, dir.2 / l));
}
