//! Effects in the world, drawn by the overlay (overlay.rs) at their place in the 3D scene through Elden
//! Ring's own camera. The game thread publishes positions (havok coordinates, like the camera's) into FX;
//! the render thread projects them every frame. Nothing here touches the game.
//!
//! 0.11.0 rebuilt every effect on soft textures made at start-up (TEX: a round glow, a tapered streak,
//! a ground ring and smoke puffs) instead of flat lines and circles:
//! - Deku's Full Cowl: thin teal-green arcs that crackle close over his body, brighter in a move
//! - All Might: wisps of steam off his shoulders in a move
//! - (the other fighters have no particles: they were cut, at the player's request)
//! - dashes leave a ribbon of air behind the body (a tapered translucent band with bright edges, like
//!   JUMP FORCE's and Naraka's air dashes), thinner for Goku's and All Might's flight
//! - hits: a short soft flash, a thin ring and a few sparks, small
//! - Deku's Black Whip: twisting black tendrils with a teal glow, splayed where they grip
//! - Luffy's swing (0.14.0): his own arm, stretched from the shoulder to a fist gripping the ledge,
//!   wobbling like rubber when it catches and snapping back when he lets go
//! - boss tells (ui::Warnings): ground decals that fill toward the strike, and the attack's words in
//!   the game's own type

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use eldenring::cs::{CSCamExt, CSCamera};
use fromsoftware_shared::FromStatic;
use hudhook::imgui::{DrawListMut, FontId, ImColor32, TextureId, Ui};

use crate::ability::V3;

pub struct FighterFx {
    pub pos: V3,
    pub fwd: V3,
    pub style: &'static str,
    /// 0 calm .. 1 in the middle of a move
    pub power: f32,
    /// the body's scale (All Might is bigger)
    pub height: f32,
    /// flying (Goku's and All Might's air dash): a thinner trail
    pub flying: bool,
}

pub struct Whip {
    pub from: V3,
    pub to: V3,
    pub since: Instant,
}

/// 1.1.0 (user: runic magic circles in Elden Ring's palette): the boss tells' seals, each drawn offline
/// (work/sigils/sigils.py) and embedded (assets/sigils).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sigil {
    /// Radagon's holy shockwave and the cataclysm's holy-floor bursts: pale grace-gold
    Holy,
    /// the cataclysm's blast: Erdtree amber-gold
    Cataclysm,
    /// Placidusax's red lightning: crimson with ember edges
    Lightning,
}

#[derive(Clone, Copy)]
pub enum Shape {
    /// 1.1.0: a seal on the ground exactly `r` metres round (the spell's own reach); its runes light up
    /// around it over the marker's time and it is fully lit the moment it strikes
    Sigil { c: V3, r: f32, kind: Sigil, turn: f32 },
    /// a comet's lane on the ground from `from` to `to`, `w` wide (light runs along it over its time)
    Lane { from: V3, to: V3, w: f32 },
    /// where the boss gathers itself (a soft glow at its feet)
    Pillar { c: V3, h: f32 },
}

pub struct Marker {
    pub shape: Shape,
    pub color: [f32; 4],
    pub t0: Instant,
    pub dur: f32,
}

pub struct Impact {
    pub pos: V3,
    pub t0: Instant,
    pub color: [f32; 4],
    pub big: bool,
}

pub struct Banner {
    pub text: String,
    pub color: [f32; 4],
    pub t0: Instant,
    pub dur: f32,
}

#[derive(Default)]
pub struct State {
    pub fighter: Option<FighterFx>,
    pub whip: Option<Whip>,
    /// the whip found nothing to grip: it shoots out and snaps back (from, to, when)
    pub whip_miss: Option<(V3, V3, Instant)>,
    /// 0.13.0: where Deku's whip would catch if thrown now (a small marker)
    pub aim: Option<V3>,
    /// 0.15.0: the enemy the swing button would grab and pull in for an attack (a red glow on them)
    pub aim_enemy: Option<V3>,
    pub markers: Vec<Marker>,
    pub impacts: Vec<Impact>,
    /// 1.3.0: a new threat arriving (villains.rs entrances): where, its name, colour, since, for how long
    pub threats: Vec<(V3, String, [f32; 4], Instant, f32)>,
    /// the fighter's recent chest positions while moving fast (the dash ribbon)
    pub trail: Vec<(V3, Instant)>,
    pub banner: Option<Banner>,
    /// bosses open to a counter: (position, until)
    pub exposed: Vec<(V3, Instant)>,
    /// a seal that follows the player while a tell runs (the red lightning's mark): where, how wide
    pub live: Option<(V3, f32)>,
    /// 1.1.0: the comets' lanes while their tell runs: from, to, width; locked (they no longer follow)
    pub live_lanes: Vec<(V3, V3, f32)>,
    pub lanes_locked: bool,
    /// 0.14.0: the swing line is Luffy's stretched arm, not Deku's tendrils
    pub rubber: bool,
    /// 1.3.0: what else the swing line is: 0 Deku's tendrils, 1 Gon's fishing rod and line (the bobber on the grip),
    /// 2 Killua's yo-yo on its string
    pub cord: u8,
    /// 1.2.0: Naruto's shadow clones vanishing in a burst of smoke (where, when)
    pub poofs: Vec<(V3, Instant)>,
}

pub static FX: Mutex<State> = Mutex::new(State { fighter: None, whip: None, whip_miss: None, aim: None, aim_enemy: None, markers: Vec::new(), impacts: Vec::new(), threats: Vec::new(), trail: Vec::new(), banner: None, exposed: Vec::new(), live: None, live_lanes: Vec::new(), lanes_locked: false, rubber: false, cord: 0, poofs: Vec::new() });

pub fn marker(shape: Shape, color: [f32; 4], dur: f32) {
    FX.lock().unwrap().markers.push(Marker { shape, color, t0: Instant::now(), dur });
}

/// 1.3.0 (the user: a villain's arrival should be "a big indicator of where the villain spawned and that there is a new threat on
/// the battlefield"): a column of light over the spot reaching into the sky, the name over it, and an arrow at the screen's edge
/// while it is out of view. Shown whatever the boss-warning setting.
pub fn threat(pos: V3, name: impl Into<String>, color: [f32; 4], dur: f32) {
    let mut s = FX.lock().unwrap();
    s.threats.retain(|t| t.3.elapsed().as_secs_f32() < t.4);
    s.threats.push((pos, name.into(), color, Instant::now(), dur));
}

pub fn impact(pos: V3, color: [f32; 4], big: bool) {
    let mut s = FX.lock().unwrap();
    // one burst per enemy per moment: multi-hit moves don't stack a pile of them
    let now = Instant::now();
    if s.impacts.iter().any(|i| now.duration_since(i.t0).as_secs_f32() < 0.12 && dist(i.pos, pos) < 1.0) {
        return;
    }
    if s.impacts.len() < 16 {
        s.impacts.push(Impact { pos, t0: now, color, big });
    }
}

/// 1.2.0: a shadow clone's "poof": a burst of white smoke at `pos` (havok), as when Naruto's clones
/// vanish in the anime.
pub fn poof(pos: V3) {
    let mut s = FX.lock().unwrap();
    if s.poofs.len() < 8 {
        s.poofs.push((pos, Instant::now()));
    }
}

pub fn banner(text: impl Into<String>, color: [f32; 4], dur: f32) {
    FX.lock().unwrap().banner = Some(Banner { text: text.into(), color, t0: Instant::now(), dur });
}

static START_FX: OnceLock<Instant> = OnceLock::new();

pub fn whip_miss(from: V3, to: V3) {
    FX.lock().unwrap().whip_miss = Some((from, to, Instant::now()));
}

/// The fighter's own colour (impacts).
pub fn color_of(style: Option<&str>) -> [f32; 4] {
    match style {
        Some("teal_lightning") => [0.35, 1.0, 0.80, 1.0],
        _ => [1.0, 0.93, 0.78, 1.0],
    }
}

fn dist(a: V3, b: V3) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
}

/// Every frame from the game thread: the fighter, and their trail while they move fast.
pub fn publish_fighter(f: Option<FighterFx>, speed: f32) {
    let mut s = FX.lock().unwrap();
    let now = Instant::now();
    if let Some(ff) = &f {
        if speed > 12.0 {
            let chest = (ff.pos.0, ff.pos.1 + 1.1 * ff.height, ff.pos.2);
            // map blocks re-base havok coordinates: a jump in the trail starts a new one
            if s.trail.last().is_some_and(|(p, _)| dist(*p, chest) > 6.0) {
                s.trail.clear();
            }
            s.trail.push((chest, now));
        }
    }
    s.trail.retain(|(_, t)| now.duration_since(*t).as_secs_f32() < TRAIL_LIFE);
    if s.trail.len() > 40 {
        let n = s.trail.len() - 40;
        s.trail.drain(..n);
    }
    s.fighter = f;
    let keep = |t0: Instant, d: f32| now.duration_since(t0).as_secs_f32() < d;
    s.markers.retain(|m| keep(m.t0, m.dur + 0.35));
    s.impacts.retain(|i| keep(i.t0, 0.4));
    s.poofs.retain(|(_, t0)| keep(*t0, POOF_LIFE));
    s.exposed.retain(|(_, until)| now < *until);
    if s.whip_miss.as_ref().is_some_and(|w| !keep(w.2, 0.35)) {
        s.whip_miss = None;
    }
    if s.banner.as_ref().is_some_and(|b| !keep(b.t0, b.dur)) {
        s.banner = None;
    }
}

pub fn clear() {
    let mut s = FX.lock().unwrap();
    *s = State::default();
}

const TRAIL_LIFE: f32 = 0.28;
const POOF_LIFE: f32 = 0.85;

/// 0.14.2: Luffy's right shoulder joint (R_UpperArm) in his swing animation (sheets/anims.json
/// luffy_swing), every 0.1 s from its start, in the skeleton's metres (right, up, forward); after
/// 1.6 s it holds. Worked out with tools_scripts/anims/pose.py from the same keys the game plays.
const LUFFY_SHOULDER: [(f32, f32, f32); 17] = [
    (0.172, 1.333, 0.001), (0.160, 1.536, 0.092), (0.163, 1.538, 0.078), (0.168, 1.542, 0.047), (0.171, 1.543, 0.016),
    (0.172, 1.543, 0.001), (0.173, 1.542, -0.019), (0.174, 1.535, -0.066), (0.174, 1.521, -0.116), (0.172, 1.507, -0.149),
    (0.172, 1.505, -0.153), (0.172, 1.508, -0.147), (0.172, 1.513, -0.137), (0.172, 1.518, -0.125), (0.172, 1.523, -0.114),
    (0.172, 1.526, -0.105), (0.172, 1.527, -0.101),
];

/// Where Luffy's hidden right arm joins his body, `tm` seconds into his swing (havok coordinates).
fn luffy_shoulder(f: &FighterFx, tm: f32) -> V3 {
    let x = (tm.max(0.0) * 10.0).min(16.0);
    let i = (x.floor() as usize).min(15);
    let k = x - i as f32;
    let (a, b) = (LUFFY_SHOULDER[i], LUFFY_SHOULDER[(i + 1).min(16)]);
    let (r, u, fw) = (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k, a.2 + (b.2 - a.2) * k);
    // the character's right in havok space: (fwd.z, 0, -fwd.x) (0.14.2 had it mirrored: the arm came
    // off his left shoulder)
    let right = (f.fwd.2, 0.0, -f.fwd.0);
    let h = f.height;
    (f.pos.0 + (right.0 * r + f.fwd.0 * fw) * h, f.pos.1 + u * h, f.pos.2 + (right.2 * r + f.fwd.2 * fw) * h)
}

/// 1.3.0 (the user: "the line should be starting by the tip of the fishing rod right?"): the tip of Gon's real rod (his swing
/// body, recolor::swing_skin: 1.5 m long, held 12 cm up from its butt in his right hand) in his swing animation
/// (sheets/anims.json whip), every 0.1 s from its start, in the skeleton's metres (right, up, forward); after 2.2 s it holds.
/// Worked out with tools_scripts/anims/pose.py from the same keys the game plays and the rod's own vertices on R_Hand.
const GON_ROD_TIP: [(f32, f32, f32); 23] = [
    (-0.464, 1.796, -0.838), (-0.056, 1.977, 1.659), (-0.149, 1.989, 1.622), (-0.348, 2.018, 1.517), (-0.531, 2.050, 1.384),
    (-0.607, 2.065, 1.315), (-0.716, 2.088, 1.203), (-0.916, 2.140, 0.919), (-1.059, 2.190, 0.589), (-1.115, 2.218, 0.366),
    (-1.120, 2.221, 0.339), (-1.112, 2.215, 0.384), (-1.096, 2.206, 0.459), (-1.072, 2.194, 0.549), (-1.044, 2.182, 0.635),
    (-1.020, 2.173, 0.699), (-1.009, 2.169, 0.725), (-1.030, 2.157, 0.701), (-1.091, 2.123, 0.547), (-1.068, 2.025, 0.088),
    (-0.826, 1.893, -0.473), (-0.568, 1.822, -0.762), (-0.464, 1.796, -0.838),
];

/// Where the tip of Gon's rod is, `tm` seconds into his swing (havok coordinates).
fn gon_rod_tip(f: &FighterFx, tm: f32) -> V3 {
    let x = (tm.max(0.0) * 10.0).min(22.0);
    let i = (x.floor() as usize).min(21);
    let k = x - i as f32;
    let (a, b) = (GON_ROD_TIP[i], GON_ROD_TIP[i + 1]);
    let (r, u, fw) = (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k, a.2 + (b.2 - a.2) * k);
    let right = (f.fwd.2, 0.0, -f.fwd.0);
    let h = f.height;
    (f.pos.0 + (right.0 * r + f.fwd.0 * fw) * h, f.pos.1 + u * h, f.pos.2 + (right.2 * r + f.fwd.2 * fw) * h)
}

// ---------------------------------------------------------------- textures (render thread)

pub struct Tex {
    /// a round soft glow (white, alpha falls off like a gaussian)
    glow: TextureId,
    /// a streak: soft across (v), tapered at both ends (u)
    streak: TextureId,
    /// a ribbon: soft across with bright edges, full along (u)
    ribbon: TextureId,
    /// a ground ring: a crisp bright rim with a soft fill fading inward
    ring: TextureId,
    /// smoke puffs (soft noisy blobs)
    smoke: [TextureId; 3],
    /// 1.1.0 danger zones: a fill deepening toward the rim, the rim alone (a crisp line with a faint
    /// halo), and a ring of fine rune ticks; all with their rim at ZONE_RIM of the texture's half-size
    zone_fill: TextureId,
    zone_rim: TextureId,
    zone_runes: TextureId,
    /// 1.1.0 lanes: a soft fill with crisp edges at LANE_EDGE across
    lane: TextureId,
    /// 1.1.0 the seals (rim at SIGIL_RIM of the half size) and the comet lane's tile and head
    sigil_holy: TextureId,
    sigil_cataclysm: TextureId,
    sigil_lightning: TextureId,
    comet_tile: TextureId,
    comet_head: TextureId,
    /// 1.1.0 the counter's sundered seal: its cracked core, and one rune shard (at 12 o'clock) in white
    /// and in crimson
    counter_core: TextureId,
    counter_shard: TextureId,
    counter_shard_red: TextureId,
}

/// Where the seals' rim sits in their textures (the half size is the reach x SIGIL_RIM), and the comet
/// tile's rails (its half width is the lane's half width x LANE_TEX); a tile is LANE_TILE lane widths long.
const SIGIL_RIM: f32 = 1.06;
const LANE_TEX: f32 = 1.3;
const LANE_TILE: f32 = 544.0 / 320.0 * LANE_TEX;

static SIGIL_PNG: [&[u8]; 8] = [
    include_bytes!("../assets/sigils/holy.png"),
    include_bytes!("../assets/sigils/cataclysm.png"),
    include_bytes!("../assets/sigils/lightning.png"),
    include_bytes!("../assets/sigils/lane.png"),
    include_bytes!("../assets/sigils/head.png"),
    include_bytes!("../assets/sigils/counter_core.png"),
    include_bytes!("../assets/sigils/counter_shard.png"),
    include_bytes!("../assets/sigils/counter_shard_red.png"),
];

/// Where the zone textures' rim sits (fraction of the half-size): decals are drawn r / ZONE_RIM wide so
/// the rim lands exactly on the spell's reach.
const ZONE_RIM: f32 = 0.96;
const LANE_EDGE: f32 = 0.94;

static TEX: OnceLock<Tex> = OnceLock::new();

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn white_alpha(n: u32, f: impl Fn(f32, f32) -> f32) -> Vec<u8> {
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (u, v) = ((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32);
            let a = (f(u, v).clamp(0.0, 1.0) * 255.0).round() as u8;
            px.extend_from_slice(&[255, 255, 255, a]);
        }
    }
    px
}

/// Value noise (for the smoke), tileable enough for a puff.
fn noise(seed: u32, x: f32, y: f32) -> f32 {
    let h = |ix: i32, iy: i32| {
        let n = (ix as u32).wrapping_mul(374761393) ^ (iy as u32).wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
        let n = (n ^ (n >> 13)).wrapping_mul(1274126177);
        (n & 0xFFFF) as f32 / 65535.0
    };
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = h(ix, iy) + (h(ix + 1, iy) - h(ix, iy)) * sx;
    let b = h(ix, iy + 1) + (h(ix + 1, iy + 1) - h(ix, iy + 1)) * sx;
    a + (b - a) * sy
}

/// Make the textures (overlay initialize).
pub fn load_textures(rc: &mut dyn hudhook::RenderContext) {
    let up = |rc: &mut dyn hudhook::RenderContext, px: Vec<u8>, n: u32| rc.load_texture(&px, n, n).ok();
    let mut up = |px: Vec<u8>, n: u32| up(rc, px, n);
    let glow = up(
        white_alpha(128, |u, v| {
            let r2 = (u - 0.5).powi(2) + (v - 0.5).powi(2);
            (-r2 * 22.0).exp() * smooth(0.5, 0.42, r2.sqrt())
        }),
        128,
    );
    let streak = up(
        white_alpha(128, |u, v| {
            let across = (-((v - 0.5) * 2.0).powi(2) * 4.5).exp();
            let along = smooth(0.0, 0.25, u) * smooth(1.0, 0.7, u);
            across * along
        }),
        128,
    );
    let ribbon = up(
        white_alpha(128, |_, v| {
            let d = ((v - 0.5) * 2.0).abs();
            let fill = 0.35 * (1.0 - d * d);
            let edge = (-((d - 0.82) * 9.0).powi(2)).exp();
            (fill + edge * 0.9) * smooth(1.0, 0.94, d)
        }),
        128,
    );
    let ring = up(
        white_alpha(256, |u, v| {
            let r = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 2.0;
            let rim = (-((r - 0.955) * 55.0).powi(2)).exp();
            let fill = 0.32 * smooth(0.35, 0.95, r) * smooth(1.0, 0.95, r);
            rim + fill
        }),
        256,
    );
    let zone_fill = up(
        white_alpha(512, |u, v| {
            let r = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 2.0 / ZONE_RIM;
            if r > 1.0 {
                return 0.0;
            }
            // nearly clear in the middle, deepening toward the rim, a soft band just inside it
            0.10 + 0.30 * smooth(0.25, 1.0, r).powf(1.6) + 0.25 * (-((r - 0.97) * 30.0).powi(2)).exp()
        }),
        512,
    );
    let zone_rim = up(
        white_alpha(512, |u, v| {
            let r = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 2.0 / ZONE_RIM;
            let line = (-((r - 1.0) * 260.0).powi(2)).exp();
            let halo = (-((r - 1.0) * 40.0).powi(2)).exp() * 0.35;
            line + halo
        }),
        512,
    );
    let zone_runes = up(
        white_alpha(512, |u, v| {
            let (x, y) = (u - 0.5, v - 0.5);
            let r = (x * x + y * y).sqrt() * 2.0 / ZONE_RIM;
            let a = y.atan2(x) / std::f32::consts::TAU + 0.5;
            // 36 short ticks, every third a longer one, and a hairline circle inside them
            let k = a * 36.0;
            let i = k.floor() as i32;
            let f = k - k.floor();
            let tick = smooth(0.0, 0.06, f) * smooth(0.22, 0.16, f);
            let (r0, r1) = if i % 3 == 0 { (0.86, 0.935) } else { (0.895, 0.935) };
            let band = smooth(r0 - 0.005, r0 + 0.005, r) * smooth(r1 + 0.005, r1 - 0.005, r);
            let hair = (-((r - 0.85) * 400.0).powi(2)).exp() * 0.6;
            tick * band + hair
        }),
        512,
    );
    let lane = up(
        white_alpha(128, |_, v| {
            let d = ((v - 0.5) * 2.0).abs() / LANE_EDGE;
            if d > 1.08 {
                return 0.0;
            }
            let edge = (-((d - 1.0) * 30.0).powi(2)).exp();
            let fill = 0.22 * smooth(1.02, 0.98, d) * (0.5 + 0.5 * d * d);
            edge + fill
        }),
        128,
    );
    let mut smoke = [None; 3];
    for (k, slot) in smoke.iter_mut().enumerate() {
        *slot = up(
            white_alpha(96, |u, v| {
                let r = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 2.0;
                let mut n = 0.0;
                let mut amp = 0.55;
                let mut f = 3.0;
                for o in 0..4 {
                    n += noise(k as u32 * 17 + o, u * f, v * f) * amp;
                    amp *= 0.5;
                    f *= 2.0;
                }
                let body = smooth(1.0, 0.25, r);
                (body * (0.35 + n) - 0.25).max(0.0) * 1.3
            }),
            96,
        );
    }
    let mut sig = SIGIL_PNG.iter().map(|data| crate::portraits::decode(data).and_then(|(px, w, h)| rc.load_texture(&px, w, h).ok()));
    let sigils: [Option<TextureId>; 8] = std::array::from_fn(|_| sig.next().flatten());
    match (glow, streak, ribbon, ring, smoke, (zone_fill, zone_rim, zone_runes, lane), sigils) {
        (Some(glow), Some(streak), Some(ribbon), Some(ring), [Some(a), Some(b), Some(c)], (Some(zone_fill), Some(zone_rim), Some(zone_runes), Some(lane)), [Some(sh), Some(sc), Some(sl), Some(ct), Some(ch), Some(k0), Some(k1), Some(k2)]) => {
            let _ = TEX.set(Tex { glow, streak, ribbon, ring, smoke: [a, b, c], zone_fill, zone_rim, zone_runes, lane, sigil_holy: sh, sigil_cataclysm: sc, sigil_lightning: sl, comet_tile: ct, comet_head: ch, counter_core: k0, counter_shard: k1, counter_shard_red: k2 });
            crate::log!("world_fx: textures ready");
        }
        _ => crate::log!("world_fx: textures could not be made; world effects off"),
    }
}

// ---------------------------------------------------------------- render thread

struct Cam {
    pos: V3,
    right: V3,
    up: V3,
    fwd: V3,
    /// focal length in pixels
    f: f32,
    w: f32,
    h: f32,
}

impl Cam {
    fn read(w: f32, h: f32) -> Option<Cam> {
        let cam = unsafe { CSCamera::instance() }.ok()?;
        let c = &cam.pers_cam_1;
        let p = c.position();
        let (r, u, f) = (c.right(), c.up(), c.forward());
        let fov = c.fov;
        if !(0.2..3.0).contains(&fov) {
            return None;
        }
        let fy = (h * 0.5) / (fov * 0.5).tan();
        Some(Cam { pos: (p.0, p.1, p.2), right: (r.0, r.1, r.2), up: (u.0, u.1, u.2), fwd: (f.0, f.1, f.2), f: fy, w, h })
    }

    /// Screen point and depth, or None behind the camera.
    fn project(&self, p: V3) -> Option<([f32; 2], f32)> {
        let d = (p.0 - self.pos.0, p.1 - self.pos.1, p.2 - self.pos.2);
        let dot = |a: V3| d.0 * a.0 + d.1 * a.1 + d.2 * a.2;
        let z = dot(self.fwd);
        if z < 0.3 {
            return None;
        }
        let (x, y) = (dot(self.right), dot(self.up));
        Some(([self.w * 0.5 + x / z * self.f, self.h * 0.5 - y / z * self.f], z))
    }

    /// Pixels per metre at depth z.
    fn px(&self, z: f32) -> f32 {
        self.f / z.max(0.3)
    }
}

fn col(c: [f32; 4], a: f32) -> ImColor32 {
    ImColor32::from_rgba_f32s(c[0], c[1], c[2], (c[3] * a).clamp(0.0, 1.0))
}

fn hash(n: u32) -> f32 {
    let x = n.wrapping_mul(2654435761) ^ (n >> 15);
    let x = x.wrapping_mul(2246822519) ^ (x >> 13);
    (x % 10_000) as f32 / 10_000.0 - 0.5
}

/// A soft round glow.
fn glow(dl: &DrawListMut, t: &Tex, c: [f32; 2], r: f32, color: [f32; 4], a: f32) {
    if r < 0.5 || a <= 0.003 {
        return;
    }
    dl.add_image(t.glow, [c[0] - r, c[1] - r], [c[0] + r, c[1] + r]).col(col(color, a)).build();
}

/// A tapered soft stroke from a to b, `wd` pixels wide, with texture `tex` (streak or ribbon).
fn stroke(dl: &DrawListMut, tex: TextureId, a: [f32; 2], b: [f32; 2], wd: f32, color: [f32; 4], al: f32) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 || al <= 0.003 {
        return;
    }
    let (nx, ny) = (-dy / len * wd * 0.5, dx / len * wd * 0.5);
    dl.add_image_quad(tex, [a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny])
        .uv([0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0])
        .col(col(color, al))
        .build();
}

/// A strip along screen points with widths (a ribbon: `tex` u runs along it, v across).
fn strip(dl: &DrawListMut, tex: TextureId, pts: &[([f32; 2], f32, f32)], color: [f32; 4]) {
    // pts: (point, width px, alpha)
    for k in 1..pts.len() {
        let (a, wa, aa) = pts[k - 1];
        let (b, wb, ab) = pts[k];
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.3 {
            continue;
        }
        let (nx, ny) = (-dy / len, dx / len);
        let al = (aa + ab) * 0.5;
        if al <= 0.003 {
            continue;
        }
        let (ha, hb) = (wa * 0.5, wb * 0.5);
        dl.add_image_quad(tex, [a[0] + nx * ha, a[1] + ny * ha], [b[0] + nx * hb, b[1] + ny * hb], [b[0] - nx * hb, b[1] - ny * hb], [a[0] - nx * ha, a[1] - ny * ha])
            .uv([0.5, 0.0], [0.5, 0.0], [0.5, 1.0], [0.5, 1.0])
            .col(col(color, al))
            .build();
    }
}

/// A square texture laid flat on the ground at `c` (radius `r` metres), split into cells so the
/// perspective holds.
fn decal(dl: &DrawListMut, cam: &Cam, tex: TextureId, c: V3, r: f32, color: [f32; 4], a: f32) {
    decal_turned(dl, cam, tex, c, r, color, a, 0.0);
}

/// The same, turned `turn` radians about its centre; big decals get a finer grid (1.1.0: a 16 m zone
/// on a 6 x 6 grid bent visibly out of round up close).
fn decal_turned(dl: &DrawListMut, cam: &Cam, tex: TextureId, c: V3, r: f32, color: [f32; 4], a: f32, turn: f32) {
    if a <= 0.003 || r < 0.05 {
        return;
    }
    const N: usize = 14;
    let n = if r > 6.0 { N } else { 6 };
    let (sn, cs) = turn.sin_cos();
    let mut grid = [[None; N + 1]; N + 1];
    for i in 0..=n {
        for j in 0..=n {
            let (gx, gz) = ((i as f32 / n as f32 * 2.0 - 1.0) * r, (j as f32 / n as f32 * 2.0 - 1.0) * r);
            let (x, z) = (c.0 + gx * cs - gz * sn, c.2 + gx * sn + gz * cs);
            grid[i][j] = cam.project((x, c.1 + 0.06, z));
        }
    }
    for i in 0..n {
        for j in 0..n {
            if let (Some((p1, z1)), Some((p2, z2)), Some((p3, z3)), Some((p4, z4))) = (grid[i][j], grid[i + 1][j], grid[i + 1][j + 1], grid[i][j + 1]) {
                // 1.1.0: cells right at the camera fade out (they were cut off in hard-edged blocks when a
                // zone passed under it)
                let near = smooth(0.8, 3.5, z1.min(z2).min(z3).min(z4));
                if near <= 0.01 {
                    continue;
                }
                let colr = col(color, a * near);
                let (u0, u1, v0, v1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32, j as f32 / n as f32, (j + 1) as f32 / n as f32);
                dl.add_image_quad(tex, p1, p2, p3, p4).uv([u0, v0], [u1, v0], [u1, v1], [u0, v1]).col(colr).build();
            }
        }
    }
}

/// 1.1.0 (user: never over my character or the item boxes): where the boss tells may not draw. The
/// game's own HUD boxes (bars, buffs, the item and equipment boxes, the boss bar, the runes) and ours,
/// and the fighter's body on screen; tells fade out softly at their edges.
struct Mask {
    rects: Vec<[f32; 4]>,
    body: Option<[f32; 4]>,
}

fn rect_dist(r: &[f32; 4], p: [f32; 2]) -> f32 {
    let dx = (r[0] - p[0]).max(p[0] - r[2]).max(0.0);
    let dy = (r[1] - p[1]).max(p[1] - r[3]).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

impl Mask {
    fn new(cam: &Cam, w: f32, h: f32, fighter: Option<&FighterFx>) -> Mask {
        let k = h / 1080.0;
        let ox = (w - h * 16.0 / 9.0).max(0.0) / 2.0;
        let r = |x0: f32, y0: f32, x1: f32, y1: f32| [ox + x0 * k, y0 * k, ox + x1 * k, y1 * k];
        let rects = vec![
            r(40.0, 18.0, 545.0, 122.0),    // health, focus, stamina
            r(40.0, 150.0, 570.0, 365.0),   // Elden Force's fighter box and abilities
            r(40.0, 655.0, 440.0, 720.0),   // buffs
            r(40.0, 785.0, 375.0, 1068.0),  // the item and equipment boxes
            r(440.0, 828.0, 1480.0, 898.0), // the boss bar
            r(1645.0, 998.0, 1892.0, 1072.0), // runes
        ];
        let body = fighter.and_then(|f| {
            let (feet, z) = cam.project(f.pos)?;
            let (head, _) = cam.project((f.pos.0, f.pos.1 + 1.95 * f.height, f.pos.2))?;
            let hw = 0.42 * f.height * cam.px(z);
            Some([feet[0] - hw, head[1] - hw * 0.3, feet[0] + hw, feet[1] + hw * 0.3])
        });
        Mask { rects, body }
    }

    /// How much of a tell may show at this screen point (0 hidden .. 1).
    fn at(&self, p: [f32; 2]) -> f32 {
        let mut f = 1.0f32;
        for r in &self.rects {
            f = f.min(smooth(0.0, 14.0, rect_dist(r, p)));
        }
        if let Some(b) = &self.body {
            f = f.min(smooth(0.0, 26.0, rect_dist(b, p)));
        }
        f
    }
}

/// A seal texture laid flat at `c` (`half` metres from its centre to the texture's edge), turned by
/// `turn`, over the angles a0..a1 only (radians, the seal's own): a polar grid of cells, each faded by
/// the mask and near the camera.
fn decal_polar(dl: &DrawListMut, cam: &Cam, mask: &Mask, tex: TextureId, c: V3, half: f32, turn: f32, a0: f32, a1: f32, alpha: f32) {
    if alpha <= 0.003 || half < 0.05 || a1 <= a0 {
        return;
    }
    let na = (((a1 - a0) / std::f32::consts::TAU) * 72.0).ceil().max(1.0) as usize;
    const NR: usize = 10;
    let at = |a: f32, rr: f32| {
        let w = (c.0 + (a + turn).cos() * rr * half, c.1 + 0.06, c.2 + (a + turn).sin() * rr * half);
        (cam.project(w), [0.5 + 0.5 * rr * a.cos(), 0.5 - 0.5 * rr * a.sin()])
    };
    for i in 0..na {
        let (aa, ab) = (a0 + (a1 - a0) * i as f32 / na as f32, a0 + (a1 - a0) * (i + 1) as f32 / na as f32);
        for j in 0..NR {
            let (ra, rb) = (j as f32 / NR as f32, (j + 1) as f32 / NR as f32);
            let (p1, u1) = at(aa, ra);
            let (p2, u2) = at(aa, rb);
            let (p3, u3) = at(ab, rb);
            let (p4, u4) = at(ab, ra);
            let (Some((p1, z1)), Some((p2, z2)), Some((p3, z3)), Some((p4, z4))) = (p1, p2, p3, p4) else { continue };
            let near = smooth(0.8, 3.5, z1.min(z2).min(z3).min(z4));
            let centre = [(p1[0] + p2[0] + p3[0] + p4[0]) / 4.0, (p1[1] + p2[1] + p3[1] + p4[1]) / 4.0];
            let a = alpha * near * mask.at(centre);
            if a <= 0.003 {
                continue;
            }
            dl.add_image_quad(tex, p1, p2, p3, p4).uv(u1, u2, u3, u4).col(col([1.0; 4], a)).build();
        }
    }
}

/// A seal exactly `r` metres round: the whole seal faintly, and on top of it the part its light has
/// reached (`u` 0..1 of the wait, sweeping round; the whole seal bright the moment it strikes).
fn sigil(dl: &DrawListMut, cam: &Cam, mask: &Mask, t: &Tex, kind: Sigil, c: V3, r: f32, turn: f32, u: f32, alpha: f32) {
    let tex = match kind {
        Sigil::Holy => t.sigil_holy,
        Sigil::Cataclysm => t.sigil_cataclysm,
        Sigil::Lightning => t.sigil_lightning,
    };
    let half = r * SIGIL_RIM;
    let tau = std::f32::consts::TAU;
    decal_polar(dl, cam, mask, tex, c, half, turn, 0.0, tau, 0.42 * alpha);
    let lit = u.clamp(0.0, 1.0);
    if lit > 0.0 {
        // the last moment before it strikes, it flares
        let flare = 1.0 + 0.5 * smooth(0.85, 1.0, lit);
        decal_polar(dl, cam, mask, tex, c, half, turn, std::f32::consts::FRAC_PI_2 - tau * lit, std::f32::consts::FRAC_PI_2, (0.6 * alpha * flare).min(1.0));
    }
}

/// 1.1.0 (user: show the boss is open, in the seals' style; detached shards red so the time left reads
/// at a glance): the sundered seal at the boss's feet. Its six rune shards break away one by one,
/// clockwise from the top, as the opening runs out: each turns crimson the moment it goes, drifts out
/// and dims; the next to go flickers first. Above the boss, a small halo with the same six shards.
fn counter_seal(dl: &DrawListMut, cam: &Cam, mask: &Mask, t: &Tex, c: V3, left: f32, alpha: f32, time: f32) {
    let total = crate::bosses::EXPOSED_FOR;
    let p = (1.0 - left / total).clamp(0.0, 1.0);
    let r = 3.2;
    let half = r * SIGIL_RIM;
    let tau = std::f32::consts::TAU;
    let beat = 0.85 + 0.15 * (time * 7.0).sin();
    decal_polar(dl, cam, mask, t.counter_core, c, half, 0.0, 0.0, tau, alpha * beat);
    let gone = (p * 6.0).floor() as usize;
    let mut ring = [(false, 1.0f32); 6];
    for k in 0..6 {
        let turn = -(k as f32) * tau / 6.0;
        let dir = (turn + std::f32::consts::FRAC_PI_2).cos();
        let dirz = (turn + std::f32::consts::FRAC_PI_2).sin();
        if k < gone {
            // broken away: crimson, drifting out and dimming
            let since = (p * 6.0 - (k + 1) as f32).max(0.0) / 6.0 * total;
            let drift = 0.9 * smooth(0.0, 0.5, since);
            let fade = 1.0 - 0.55 * smooth(0.0, 0.6, since);
            let cc = (c.0 + dir * drift, c.1, c.2 + dirz * drift);
            decal_polar(dl, cam, mask, t.counter_shard_red, cc, half, turn, 0.0, tau, alpha * fade);
            ring[k] = (true, fade);
        } else {
            // still in place; the next to go flickers
            let flick = if k == gone && (p * 6.0).fract() > 0.6 { 0.55 + 0.45 * (time * 30.0).sin().abs() } else { 1.0 };
            decal_polar(dl, cam, mask, t.counter_shard, c, half, turn, 0.0, tau, alpha * flick);
            ring[k] = (false, flick);
        }
    }
    // the halo above the boss: its core and the six shards as arcs, facing the camera
    if let Some((sp, z)) = cam.project((c.0, c.1 + 4.4, c.2)) {
        let rp = (0.7 * cam.px(z)).clamp(10.0, 70.0);
        let m = mask.at(sp);
        if m > 0.01 {
            dl.add_image(t.counter_core, [sp[0] - rp * 1.06, sp[1] - rp * 1.06], [sp[0] + rp * 1.06, sp[1] + rp * 1.06]).col(col([1.0; 4], alpha * m * 0.9)).build();
            for (k, (broken, a)) in ring.iter().enumerate() {
                let a0 = -std::f32::consts::FRAC_PI_2 + k as f32 * tau / 6.0 - tau / 12.0 + 0.1;
                let a1 = a0 + tau / 6.0 - 0.2;
                let out = if *broken { 0.25 * rp } else { 0.0 };
                let mid = (a0 + a1) / 2.0;
                let pts: Vec<[f32; 2]> = (0..=12)
                    .map(|i| {
                        let a = a0 + (a1 - a0) * i as f32 / 12.0;
                        [sp[0] + mid.cos() * out + a.cos() * rp, sp[1] + mid.sin() * out + a.sin() * rp]
                    })
                    .collect();
                let color = if *broken { [0.86, 0.18, 0.13, 1.0] } else { [0.98, 0.95, 0.88, 1.0] };
                dl.add_polyline(pts.clone(), col([color[0], color[1], color[2], 1.0], 0.35 * alpha * a * m)).thickness((rp * 0.16).max(3.0)).build();
                dl.add_polyline(pts, col(color, alpha * a * m)).thickness((rp * 0.06).max(1.5)).build();
            }
        }
    }
}

/// A comet's lane: tiles of runic rails and glintstone seals from `from` to the chevrons at `to`, `w`
/// wide; the first `lit` of its length bright (the comet's light running along it), the rest at `dim`.
fn comet_lane(dl: &DrawListMut, cam: &Cam, mask: &Mask, t: &Tex, from: V3, to: V3, w: f32, lit: f32, dim: f32, alpha: f32) {
    let (dx, dz) = (to.0 - from.0, to.2 - from.2);
    let len = (dx * dx + dz * dz).sqrt();
    if len < 0.5 || alpha <= 0.003 {
        return;
    }
    let (fx, fz) = (dx / len, dz / len);
    let (sx, sz) = (fz, -fx);
    let hw = w / 2.0 * LANE_TEX;
    let tile = w * LANE_TILE;
    let head = (w * LANE_TEX).min(len * 0.4);
    let body = len - head;
    let ground = |d: f32, side: f32| {
        let k = d / len;
        (from.0 + fx * d + sx * side, from.1 + (to.1 - from.1) * k + 0.06, from.2 + fz * d + sz * side)
    };
    let quad = |tex: TextureId, d0: f32, d1: f32, v0: f32, v1: f32, a: f32| {
        // four rows per piece, so the perspective holds
        for q in 0..4 {
            let (e0, e1) = (d0 + (d1 - d0) * q as f32 / 4.0, d0 + (d1 - d0) * (q + 1) as f32 / 4.0);
            let (w0, w1) = (v0 + (v1 - v0) * q as f32 / 4.0, v0 + (v1 - v0) * (q + 1) as f32 / 4.0);
            let pts = [ground(e0, -hw), ground(e1, -hw), ground(e1, hw), ground(e0, hw)];
            let pr: Vec<_> = pts.iter().filter_map(|p| cam.project(*p)).collect();
            if pr.len() < 4 {
                continue;
            }
            let near = smooth(0.8, 3.5, pr.iter().map(|p| p.1).fold(f32::MAX, f32::min));
            let centre = [(pr[0].0[0] + pr[2].0[0]) / 2.0, (pr[0].0[1] + pr[2].0[1]) / 2.0];
            let aa = a * near * mask.at(centre);
            if aa > 0.003 {
                dl.add_image_quad(tex, pr[0].0, pr[1].0, pr[2].0, pr[3].0).uv([0.0, w0], [0.0, w1], [1.0, w1], [1.0, w0]).col(col([1.0; 4], aa)).build();
            }
        }
    };
    let mut d = 0.0;
    while d < body - 0.01 {
        let d1 = (d + tile).min(body);
        let bright = if (d + d1) / 2.0 / len < lit { 1.0 } else { dim };
        // the texture's top is the comet's way: v runs 1 (start) .. 0
        quad(t.comet_tile, d, d1, 1.0, 1.0 - (d1 - d) / tile, alpha * bright);
        d = d1;
    }
    quad(t.comet_head, body, len, 1.0, 0.0, alpha * if lit >= 0.99 { 1.0 } else { dim.max(0.6) });
}

/// A lightning arc between two screen points: a soft glow under a thin bright core.
fn arc(dl: &DrawListMut, t: &Tex, a: [f32; 2], b: [f32; 2], seed: u32, jag: f32, color: [f32; 4], al: f32, k: f32) -> Vec<[f32; 2]> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let (nx, ny) = (-dy / len, dx / len);
    let segs = 7;
    let mut pts = Vec::with_capacity(segs + 1);
    for i in 0..=segs {
        let u = i as f32 / segs as f32;
        let env = (u * std::f32::consts::PI).sin();
        let off = hash(seed.wrapping_add(i as u32 * 7919)) * jag * len * env;
        pts.push([a[0] + dx * u + nx * off, a[1] + dy * u + ny * off]);
    }
    for w in pts.windows(2) {
        stroke(dl, t.streak, w[0], w[1], 9.0 * k, color, 0.35 * al);
    }
    let core = [0.85 + 0.15 * color[0], 1.0, 0.95, 1.0];
    for w in pts.windows(2) {
        dl.add_line(w[0], w[1], col(core, 0.95 * al)).thickness((1.3 * k).max(1.0)).build();
    }
    pts
}

#[derive(Clone, Copy)]
enum Kind {
    /// an arc between two points on the body (a fixed seed: it flickers by its short life)
    Arc { to: V3, seed: u32 },
    /// a spark flying off
    Spark,
    /// a wisp of steam
    Steam { tex: u8, spin: f32 },
}

#[derive(Clone, Copy)]
struct Particle {
    p: V3,
    v: V3,
    age: f32,
    life: f32,
    size: f32,
    kind: Kind,
}

struct Sim {
    parts: Vec<Particle>,
    rng: u32,
    last: Option<Instant>,
    carry: f32,
}

static SIM: Mutex<Sim> = Mutex::new(Sim { parts: Vec::new(), rng: 0x9E37_79B9, last: None, carry: 0.0 });

impl Sim {
    fn rnd(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng % 10_000) as f32 / 10_000.0
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.rnd()
    }
    /// A point on the body's surface (a capsule hugging the fighter): arms and shoulders out wider.
    fn body_point(&mut self, f: &FighterFx) -> V3 {
        let y = self.range(0.15, 1.65);
        let r = if (1.0..1.5).contains(&y) { 0.3 } else { 0.2 } * self.range(0.8, 1.1);
        let a = self.range(0.0, std::f32::consts::TAU);
        (f.pos.0 + a.cos() * r * f.height, f.pos.1 + y * f.height, f.pos.2 + a.sin() * r * f.height)
    }
    fn spawn(&mut self, f: &FighterFx, dt: f32) {
        let rate = match f.style {
            // Full Cowl: always a few arcs, a storm of them in a move
            "teal_lightning" => 16.0 + 34.0 * f.power,
            // steam only while he strains (moves, Awakening)
            "steam" => 30.0 * ((f.power - 0.2) / 0.8).clamp(0.0, 1.0),
            _ => 0.0,
        };
        self.carry += rate * dt;
        let n = self.carry.floor() as usize;
        self.carry -= n as f32;
        for _ in 0..n.min(8) {
            if self.parts.len() > 160 {
                break;
            }
            match f.style {
                "teal_lightning" => {
                    let a = self.body_point(f);
                    // short arcs that run along the body, not out into the air
                    let (dx, dy, dz) = (self.range(-0.3, 0.3), self.range(-0.45, 0.45), self.range(-0.3, 0.3));
                    let to = (a.0 + dx * f.height, a.1 + dy * f.height, a.2 + dz * f.height);
                    let seed = self.rng;
                    let life = self.range(0.05, 0.11);
                    self.parts.push(Particle { p: a, v: (0.0, 0.0, 0.0), age: 0.0, life, size: 1.0, kind: Kind::Arc { to, seed } });
                    if f.power > 0.5 && self.rnd() < 0.35 {
                        let v = (self.range(-2.5, 2.5), self.range(0.5, 3.5), self.range(-2.5, 2.5));
                        let life = self.range(0.15, 0.3);
                        self.parts.push(Particle { p: to, v, age: 0.0, life, size: 1.0, kind: Kind::Spark });
                    }
                }
                "steam" => {
                    let side = (f.fwd.2, 0.0, -f.fwd.0);
                    let s = if self.rnd() < 0.5 { 1.0 } else { -1.0 };
                    let sh = self.range(0.12, 0.3) * s;
                    let p = (f.pos.0 + side.0 * sh * f.height, f.pos.1 + self.range(1.35, 1.55) * f.height, f.pos.2 + side.2 * sh * f.height);
                    let v = (side.0 * s * 0.3 + self.range(-0.15, 0.15), self.range(0.7, 1.3), side.2 * s * 0.3 + self.range(-0.15, 0.15));
                    let tex = (self.rnd() * 3.0) as u8 % 3;
                    let spin = self.range(-1.2, 1.2);
                    let (life, size) = (self.range(0.9, 1.5), self.range(0.2, 0.32) * f.height);
                    self.parts.push(Particle { p, v, age: 0.0, life, size, kind: Kind::Steam { tex, spin } });
                }
                _ => {}
            }
        }
    }
}

/// What the player chose for the boss tells (ui::Warnings).
#[derive(Clone, Copy)]
pub struct Show {
    pub text: bool,
    pub visual: bool,
    /// 1.1.0: the warnings' opacity from the settings (0.1..1)
    pub opacity: f32,
}

/// Draw everything for this frame (overlay render thread).
pub fn draw(ui: &Ui, w: f32, h: f32, font: Option<FontId>, show: Show) {
    let Some(t) = TEX.get() else { return };
    let Some(cam) = Cam::read(w, h) else { return };
    let dl = ui.get_background_draw_list();
    let now = Instant::now();
    let s = FX.lock().unwrap();
    let mut sim = SIM.lock().unwrap();
    let dt = sim.last.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(0.0).clamp(0.0, 0.05);
    sim.last = Some(now);
    let k = (h / 1080.0).max(0.5);

    // ---- boss tells (under everything else): seals on the ground, never over the fighter or the HUD
    if show.visual {
        let mask = Mask::new(&cam, w, h, s.fighter.as_ref());
        let op = show.opacity;
        for m in &s.markers {
            let tm = now.duration_since(m.t0).as_secs_f32();
            let u = (tm / m.dur.max(0.01)).clamp(0.0, 1.0);
            let fade = if tm > m.dur { 1.0 - (tm - m.dur) / 0.35 } else { (tm / 0.15).min(1.0) };
            let time = ui.time() as f32;
            match m.shape {
                Shape::Sigil { c, r, kind, turn } => sigil(&dl, &cam, &mask, t, kind, c, r, turn + time * 0.04, u, fade * op),
                Shape::Lane { from, to, w: lw } => comet_lane(&dl, &cam, &mask, t, from, to, lw, u, 0.35, fade * op),
                Shape::Pillar { c, h: _ } => {
                    decal(&dl, &cam, t.glow, c, 3.2, m.color, 0.4 * fade * (1.0 - u * 0.4));
                }
            }
        }
        if let Some((c, r)) = s.live {
            let pulse = 0.8 + 0.2 * ((ui.time() as f32) * 6.0).sin();
            sigil(&dl, &cam, &mask, t, Sigil::Lightning, c, r, ui.time() as f32 * 0.3, 0.0, 0.85 * pulse * op);
        }
        // the comets' lanes while they follow the player (faint), then locked (bright): they fly there
        for (from, to, lw) in &s.live_lanes {
            let a = if s.lanes_locked { 1.0 } else { 0.55 };
            comet_lane(&dl, &cam, &mask, t, *from, *to, *lw, if s.lanes_locked { 1.0 } else { 0.0 }, 0.5, a * op);
        }
        // bosses open to a counter: the sundered seal (1.1.0)
        for (p, until) in &s.exposed {
            let left = until.duration_since(now).as_secs_f32();
            counter_seal(&dl, &cam, &mask, t, *p, left, op * (left / 0.25).min(1.0), ui.time() as f32);
        }
    }
    if show.text {
        for (p, until) in &s.exposed {
            let left = until.duration_since(now).as_secs_f32();
            if let (Some((sp, _)), Some(fid)) = (cam.project((p.0, p.1 + 3.2, p.2)), font) {
                let _f = ui.push_font(fid);
                let txt = "COUNTER";
                let sz = ui.calc_text_size(txt);
                let a = (left / 0.3).min(1.0) * 0.9;
                dl.add_text([sp[0] - sz[0] / 2.0 + 1.5, sp[1] + 1.5], col([0.0, 0.0, 0.0, 1.0], 0.7 * a), txt);
                dl.add_text([sp[0] - sz[0] / 2.0, sp[1]], col([1.0, 0.86, 0.48, 1.0], a), txt);
            }
        }
    }

    // ---- 1.3.0: new threats arriving (always shown)
    for (p, name, c, t0, dur) in &s.threats {
        let tm = now.duration_since(*t0).as_secs_f32();
        if tm > *dur {
            continue;
        }
        let fade = (tm / 0.25).min(1.0) * (((dur - tm) / 0.5).min(1.0));
        let pulse = 0.75 + 0.25 * (ui.time() as f32 * 7.0).sin();
        let base = cam.project(*p);
        let top = cam.project((p.0, p.1 + 40.0, p.2));
        let on_screen = base.is_some_and(|(sp, _)| sp[0] > 0.0 && sp[0] < w && sp[1] > 0.0 && sp[1] < h);
        if let (Some((b, z)), Some((tp, _))) = (base, top) {
            // the column: a soft wide band and a bright core, rising out of the spot
            let wpx = (cam.px(z) * 1.6).clamp(10.0 * k, 90.0 * k);
            for (wd, a) in [(wpx, 0.18), (wpx * 0.45, 0.35), (wpx * 0.12, 0.9)] {
                dl.add_line(b, tp, col(*c, a * fade * pulse)).thickness(wd).build();
            }
            decal(&dl, &cam, t.glow, *p, 6.0, *c, 0.8 * fade);
        }
        let head = cam.project((p.0, p.1 + 4.0, p.2));
        if let (true, Some((hp, _)), Some(fid)) = (on_screen, head, font) {
            let _f = ui.push_font(fid);
            let sz = ui.calc_text_size(name);
            let at = [hp[0] - sz[0] / 2.0, hp[1] - sz[1] - 14.0 * k];
            dl.add_text([at[0] + 2.0, at[1] + 2.0], col([0.0, 0.0, 0.0, 1.0], 0.8 * fade), name);
            dl.add_text(at, col(*c, fade), name);
            let d = 9.0 * k;
            let (cx, cy) = (hp[0], hp[1] - 4.0 * k);
            dl.add_triangle([cx - d, cy - d], [cx + d, cy - d], [cx, cy + d * 0.6], col(*c, fade * pulse)).filled(true).build();
        } else if !on_screen {
            // out of view: an arrow at the edge of the screen pointing the way
            let d = (p.0 - cam.pos.0, p.1 - cam.pos.1, p.2 - cam.pos.2);
            let (x, y) = (d.0 * cam.right.0 + d.1 * cam.right.1 + d.2 * cam.right.2, d.0 * cam.up.0 + d.1 * cam.up.1 + d.2 * cam.up.2);
            let ang = (-y).atan2(x);
            let (dx, dy) = (ang.cos(), ang.sin());
            let m = 70.0 * k;
            let sx = ((w * 0.5 - m) / dx.abs().max(1e-3)).min((h * 0.5 - m) / dy.abs().max(1e-3));
            let tip = [w * 0.5 + dx * sx, h * 0.5 + dy * sx];
            let r = 26.0 * k;
            let (px, py) = (-dy, dx);
            let back = [tip[0] - dx * r * 1.6, tip[1] - dy * r * 1.6];
            dl.add_triangle(tip, [back[0] + px * r, back[1] + py * r], [back[0] - px * r, back[1] - py * r], col(*c, fade * pulse)).filled(true).build();
            if let Some(fid) = font {
                let _f = ui.push_font(fid);
                let sz = ui.calc_text_size(name);
                let at = [(back[0] - dx * 30.0 * k - sz[0] / 2.0).clamp(8.0, w - sz[0] - 8.0), (back[1] - dy * 30.0 * k - sz[1] / 2.0).clamp(8.0, h - sz[1] - 8.0)];
                dl.add_text([at[0] + 2.0, at[1] + 2.0], col([0.0, 0.0, 0.0, 1.0], 0.8 * fade), name);
                dl.add_text(at, col(*c, fade), name);
            }
        }
    }

    // ---- the dash ribbon: a band of air behind the body, tapering into the past
    if s.trail.len() >= 3 {
        let (hgt, flying) = s.fighter.as_ref().map(|f| (f.height, f.flying)).unwrap_or((1.0, false));
        let width_m = if flying { 0.32 } else { 0.55 } * hgt;
        let n = s.trail.len();
        let pts: Vec<([f32; 2], f32, f32)> = s
            .trail
            .iter()
            .enumerate()
            .filter_map(|(i, (p, t0))| {
                let age = now.duration_since(*t0).as_secs_f32() / TRAIL_LIFE;
                let along = i as f32 / (n - 1) as f32; // 0 tail .. 1 head
                let taper = along.powf(0.8) * (1.0 - age).max(0.0);
                cam.project(*p).map(|(sp, z)| (sp, width_m * cam.px(z) * taper, 0.55 * taper))
            })
            .collect();
        strip(&dl, t.ribbon, &pts, [0.92, 0.96, 1.0, 1.0]);
        // two thin streams peeling off the shoulders
        for side in [-1.0f32, 1.0] {
            let f = s.fighter.as_ref().map(|f| f.fwd).unwrap_or((0.0, 0.0, 1.0));
            let off = (f.2 * side * 0.32 * hgt, 0.18 * hgt, -f.0 * side * 0.32 * hgt);
            let pts: Vec<([f32; 2], f32, f32)> = s
                .trail
                .iter()
                .enumerate()
                .filter_map(|(i, (p, t0))| {
                    let age = now.duration_since(*t0).as_secs_f32() / TRAIL_LIFE;
                    let along = i as f32 / (n - 1) as f32;
                    let taper = along * (1.0 - age).max(0.0);
                    cam.project((p.0 + off.0, p.1 + off.1, p.2 + off.2)).map(|(sp, z)| (sp, (0.05 * hgt * cam.px(z)).max(1.5), 0.5 * taper))
                })
                .collect();
            strip(&dl, t.streak, &pts, [1.0, 1.0, 1.0, 1.0]);
        }
    }

    // ---- the fighter's particles
    if let Some(f) = &s.fighter {
        sim.spawn(f, dt);
    }
    for p in sim.parts.iter_mut() {
        p.age += dt;
        p.p = (p.p.0 + p.v.0 * dt, p.p.1 + p.v.1 * dt, p.p.2 + p.v.2 * dt);
        match p.kind {
            Kind::Spark => p.v.1 -= 6.0 * dt,
            Kind::Steam { .. } => {
                p.v.0 *= 1.0 - 0.8 * dt;
                p.v.2 *= 1.0 - 0.8 * dt;
            }
            _ => {}
        }
    }
    sim.parts.retain(|p| p.age < p.life);
    let teal = [0.20, 0.95, 0.72, 1.0];
    for p in sim.parts.iter() {
        let u = p.age / p.life;
        let Some((sp, z)) = cam.project(p.p) else { continue };
        let kz = (5.0 / z).clamp(0.35, 1.6) * k;
        match p.kind {
            Kind::Arc { to, seed } => {
                let c = teal;
                if let Some((tp, _)) = cam.project(to) {
                    let flick = if hash(seed.wrapping_add((p.age * 90.0) as u32)) > -0.3 { 1.0 } else { 0.35 };
                    let pts = arc(&dl, t, sp, tp, seed.wrapping_add((p.age * 45.0) as u32), 0.5, c, flick * (1.0 - u * 0.5), kz);
                    if let Some(mid) = pts.get(pts.len() / 2) {
                        glow(&dl, t, *mid, 14.0 * kz, c, 0.22 * flick);
                    }
                }
            }
            Kind::Spark => {
                let tail = [sp[0] - p.v.0 * 4.0 * kz, sp[1] + p.v.1 * 4.0 * kz];
                stroke(&dl, t.streak, tail, sp, 4.0 * kz, [0.6, 1.0, 0.88, 1.0], 1.0 - u);
            }
            Kind::Steam { tex, spin } => {
                let a = (u * 6.0).min(1.0) * (1.0 - u).powf(1.4) * 0.45;
                let r = p.size * (1.0 + u * 2.2) * cam.px(z);
                let ang = spin * p.age;
                let (sn, cs) = ang.sin_cos();
                let corner = |x: f32, y: f32| [sp[0] + (x * cs - y * sn) * r, sp[1] + (x * sn + y * cs) * r];
                dl.add_image_quad(t.smoke[tex as usize % 3], corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0))
                    .col(col([0.93, 0.93, 0.95, 1.0], a))
                    .build();
            }
        }
    }

    // ---- Deku's Black Whip: three black tendrils twisting round each other, a teal glow under them
    let tendrils = |from: V3, to: V3, reach: f32, tm: f32, alpha: f32, grip: bool| {
        let side = {
            let (dx, dz) = (to.0 - from.0, to.2 - from.2);
            let l = (dx * dx + dz * dz).sqrt().max(1e-3);
            (-dz / l, 0.0, dx / l)
        };
        let n = 22;
        for strand in 0..3 {
            let ph = strand as f32 * std::f32::consts::TAU / 3.0;
            let mut pts = Vec::with_capacity(n + 1);
            for i in 0..=n {
                let u = i as f32 / n as f32 * reach;
                let twist = u * 14.0 - tm * 9.0 + ph;
                let rad = 0.09 * (1.0 - u * 0.6);
                let sag = -(u * (1.0 - u)) * 0.5;
                let (ts, tc) = twist.sin_cos();
                let p = (
                    from.0 + (to.0 - from.0) * u + side.0 * tc * rad,
                    from.1 + (to.1 - from.1) * u + sag + ts * rad,
                    from.2 + (to.2 - from.2) * u + side.2 * tc * rad,
                );
                if let Some((sp, z)) = cam.project(p) {
                    let wpx = (0.075 * (1.0 - u * 0.7) * cam.px(z)).max(2.0);
                    pts.push((sp, wpx, alpha));
                }
            }
            let glow_pts: Vec<_> = pts.iter().map(|(p, wd, a)| (*p, wd * 4.0, a * 0.22)).collect();
            strip(&dl, t.streak, &glow_pts, teal);
            for wnd in pts.windows(2) {
                dl.add_line(wnd[0].0, wnd[1].0, col([0.02, 0.04, 0.04, 1.0], 0.95 * alpha)).thickness(wnd[0].1).build();
            }
            for wnd in pts.windows(2).step_by(3) {
                dl.add_line(wnd[0].0, wnd[1].0, col([0.35, 1.0, 0.82, 1.0], 0.5 * alpha)).thickness(1.0).build();
            }
        }
        if grip {
            // where it holds: tendrils splayed against the surface and a small flash when it catches
            if let Some((sp, z)) = cam.project(to) {
                let kz = cam.px(z);
                for i in 0..5u32 {
                    let a = i as f32 * 1.256 + hash(i * 31) * 0.5;
                    let l = (0.25 + 0.15 * hash(i * 7).abs()) * kz;
                    let e = [sp[0] + a.cos() * l, sp[1] + a.sin() * l];
                    dl.add_line(sp, e, col([0.02, 0.04, 0.04, 1.0], 0.9 * alpha)).thickness((0.03 * kz).max(1.0)).build();
                }
                let catch = (1.0 - tm / 0.25).max(0.0);
                glow(&dl, t, sp, 0.9 * kz * (1.0 + catch), teal, 0.25 + 0.5 * catch);
            }
        }
    };
    // ---- Luffy's arm, stretched like rubber from his shoulder to a fist on the grip. 0.14.1: his real
    // right arm is hidden by his swing animation, so this is his only right arm; the tones are his skin
    // as the game lights it (sampled from play: shadow, mid, light), a round tube shaded top to bottom,
    // no sleeve (his vest has none), a gentle wobble only as it catches
    let skin_mid = [0.62, 0.52, 0.43, 1.0];
    let skin_dark = [0.36, 0.28, 0.22, 1.0];
    let skin_light = [0.80, 0.70, 0.60, 1.0];
    let rubber_arm = |from: V3, to: V3, reach: f32, tm: f32, alpha: f32, grip: bool| {
        let len = dist(from, to).max(0.1);
        let side = {
            let (dx, dz) = (to.0 - from.0, to.2 - from.2);
            let l = (dx * dx + dz * dz).sqrt().max(1e-3);
            (-dz / l, 0.0, dx / l)
        };
        let wob = if grip { 0.10 * (-tm * 7.0).exp() * (tm * 30.0).sin() } else { 0.0 };
        let n = 28;
        let mut pts = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let u = i as f32 / n as f32 * reach;
            let bend = (u * std::f32::consts::PI).sin();
            let sag = -bend * (0.012 * len).min(0.25);
            let w = bend * wob;
            let p = (from.0 + (to.0 - from.0) * u + side.0 * w, from.1 + (to.1 - from.1) * u + sag, from.2 + (to.2 - from.2) * u + side.2 * w);
            if let Some((sp, z)) = cam.project(p) {
                // an upper arm's thickness at the shoulder, a forearm's along the stretch, the wrist
                let k = (u / reach.max(0.01)).min(1.0);
                let wm = if k < 0.08 { 0.115 - 0.2 * k } else if k > 0.93 { 0.07 } else { 0.098 - 0.02 * k };
                pts.push((sp, (wm * cam.px(z)).max(2.0)));
            }
        }
        if pts.len() < 2 {
            return;
        }
        // a round tube: dark rim, the mid tone, a lit band above the middle, a soft shade below it
        for (wd, c, off) in [(1.0f32, skin_dark, 0.0f32), (0.78, skin_mid, 0.0), (0.3, skin_light, -0.2), (0.22, skin_dark, 0.3)] {
            for wnd in pts.windows(2) {
                let (a, b) = (wnd[0].0, wnd[1].0);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let l = (dx * dx + dy * dy).sqrt().max(1e-3);
                // offset across the tube, toward its top on screen
                let (mut nx, mut ny) = (-dy / l, dx / l);
                if ny > 0.0 {
                    nx = -nx;
                    ny = -ny;
                }
                let o = -off * wnd[0].1;
                let al = if c == skin_dark && off > 0.0 { 0.35 } else { 1.0 };
                dl.add_line([a[0] - nx * o, a[1] - ny * o], [b[0] - nx * o, b[1] - ny * o], col(c, al * alpha)).thickness((wnd[0].1 * wd).max(1.0)).build();
            }
        }
        // the fist on the grip (only once it reaches)
        if let Some(&(sp, wd)) = pts.last().filter(|_| reach > 0.97) {
            let r = wd * 0.95;
            dl.add_circle(sp, r * 1.08, col(skin_dark, alpha)).filled(true).num_segments(20).build();
            dl.add_circle(sp, r * 0.9, col(skin_mid, alpha)).filled(true).num_segments(20).build();
            dl.add_circle([sp[0] - r * 0.25, sp[1] - r * 0.3], r * 0.38, col(skin_light, 0.8 * alpha)).filled(true).build();
            for kk in 0..3 {
                let x = sp[0] - r * 0.4 + kk as f32 * r * 0.4;
                dl.add_line([x, sp[1] - r * 0.05], [x, sp[1] + r * 0.55], col(skin_dark, 0.6 * alpha)).thickness((0.12 * r).max(1.0)).build();
            }
        }
    };
    // ---- 1.3.0: Gon's fishing rod (Hunterpedia: his rod from Whale Island, a long plain wooden rod he casts to hook and
    // pull things and swing from) and Killua's yo-yos (steel, 50 kg each, on thin wire): a thin line from his hand or the
    // rod's tip, sagging until it reaches, then taut; the bobber or the yo-yo where it holds
    let cord_line = |kind: u8, hand: V3, to: V3, reach: f32, tm: f32, alpha: f32, grip: bool| {
        let up = (0.0f32, 1.0f32, 0.0f32);
        let len = dist(hand, to).max(0.1);
        let dirv = ((to.0 - hand.0) / len, (to.1 - hand.1) / len, (to.2 - hand.2) / len);
        let from = if kind == 1 {
            // (the user: the drawn rod "looks odd being drawn over his hand": his real rod is in his hand now, his swing body,
            // look.rs SWING_PROP) the line leaves from its tip: `hand` is the tip already (gon_rod_tip)
            hand
        } else if kind == 4 {
            // Gon's rod tip with no swing animation to follow (a cast that finds nothing): about 1.4 m up his raised arm
            let tipd = (dirv.0 * 0.7 + up.0 * 0.3, dirv.1 * 0.7 + up.1 * 0.3, dirv.2 * 0.7 + up.2 * 0.3);
            let tl = (tipd.0 * tipd.0 + tipd.1 * tipd.1 + tipd.2 * tipd.2).sqrt().max(1e-3);
            (hand.0 + tipd.0 / tl * 1.4, hand.1 + tipd.1 / tl * 1.4, hand.2 + tipd.2 / tl * 1.4)
        } else if kind == 3 {
            // (0.14-era drawn rod, kept for a fighter without a real one)
            let d = (dirv.0 * 0.55 + up.0 * 0.45, dirv.1 * 0.55 + up.1 * 0.45, dirv.2 * 0.55 + up.2 * 0.45);
            let dl_ = (d.0 * d.0 + d.1 * d.1 + d.2 * d.2).sqrt().max(1e-3);
            let tip = (hand.0 + d.0 / dl_ * 1.5, hand.1 + d.1 / dl_ * 1.5, hand.2 + d.2 / dl_ * 1.5);
            let mut rod = Vec::new();
            for i in 0..=8 {
                let u = i as f32 / 8.0;
                let bow = (u * u) * if grip { 0.18 } else { 0.06 };
                let p = (hand.0 + (tip.0 - hand.0) * u + dirv.0 * bow, hand.1 + (tip.1 - hand.1) * u - bow * 0.6, hand.2 + (tip.2 - hand.2) * u + dirv.2 * bow);
                if let Some((sp, z)) = cam.project(p) {
                    rod.push((sp, ((0.035 - 0.022 * u) * cam.px(z)).max(1.2)));
                }
            }
            for wnd in rod.windows(2) {
                dl.add_line(wnd[0].0, wnd[1].0, col([0.20, 0.13, 0.07, 1.0], alpha)).thickness(wnd[0].1 + 1.0).build();
                dl.add_line(wnd[0].0, wnd[1].0, col([0.66, 0.50, 0.30, 1.0], alpha)).thickness(wnd[0].1 * 0.6).build();
            }
            let u = 1.0;
            let bow = if grip { 0.18 } else { 0.06 };
            (hand.0 + (tip.0 - hand.0) * u + dirv.0 * bow, hand.1 + (tip.1 - hand.1) * u - bow * 0.6, hand.2 + (tip.2 - hand.2) * u + dirv.2 * bow)
        } else {
            hand
        };
        let span = dist(from, to).max(0.1);
        let slack = if grip { (0.25 * (-tm * 6.0).exp()).max(0.0) } else { 0.04 * span };
        let n = 24;
        let mut pts = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let u = i as f32 / n as f32 * reach;
            let sag = -(u * (1.0 - u)) * 4.0 * slack;
            let p = (from.0 + (to.0 - from.0) * u, from.1 + (to.1 - from.1) * u + sag, from.2 + (to.2 - from.2) * u);
            if let Some((sp, _)) = cam.project(p) {
                pts.push(sp);
            }
        }
        // fishing line: pale and fine; the yo-yo's wire: steel, with Killua's electricity running down it now and then
        let (shade, core) = if kind == 1 || kind == 4 { ([0.15, 0.15, 0.15, 1.0], [0.92, 0.92, 0.88, 1.0]) } else { ([0.10, 0.12, 0.16, 1.0], [0.78, 0.84, 0.92, 1.0]) };
        for wnd in pts.windows(2) {
            dl.add_line(wnd[0], wnd[1], col(shade, 0.45 * alpha)).thickness(2.6).build();
            dl.add_line(wnd[0], wnd[1], col(core, 0.95 * alpha)).thickness(1.2).build();
        }
        if kind == 2 && pts.len() > 4 && (tm * 3.0).fract() < 0.35 {
            let blue = [0.55, 0.80, 1.0, 1.0];
            for (i, wnd) in pts.windows(2).enumerate() {
                let j = hash(i as u32 * 13 + (tm * 40.0) as u32) * 3.0;
                dl.add_line(wnd[0], [wnd[1][0] + j, wnd[1][1] - j], col(blue, 0.7 * alpha)).thickness(1.4).build();
            }
        }
        let Some(&end) = pts.last() else { return };
        let kz = cam.project(to).map(|(_, z)| cam.px(z)).unwrap_or(40.0);
        if kind == 1 || kind == 4 {
            // the red and white float, the hook under it
            let r = (0.07 * kz).max(3.0);
            dl.add_circle(end, r * 1.12, col([0.1, 0.05, 0.05, 1.0], alpha)).filled(true).num_segments(20).build();
            dl.add_circle(end, r, col([0.95, 0.95, 0.92, 1.0], alpha)).filled(true).num_segments(20).build();
            dl.add_triangle([end[0] - r, end[1]], [end[0] + r, end[1]], [end[0], end[1] - r], col([0.85, 0.12, 0.10, 1.0], alpha)).filled(true).build();
            dl.add_circle([end[0], end[1] - r * 0.5], r * 0.72, col([0.85, 0.12, 0.10, 1.0], alpha)).filled(true).num_segments(16).build();
            dl.add_line([end[0], end[1] + r], [end[0], end[1] + r * 1.9], col([0.6, 0.6, 0.62, 1.0], alpha)).thickness(1.2).build();
        } else {
            // the yo-yo: a steel disc, its rim lit, spinning
            let r = (0.08 * kz).max(3.5);
            dl.add_circle(end, r * 1.1, col([0.05, 0.06, 0.08, 1.0], alpha)).filled(true).num_segments(24).build();
            dl.add_circle(end, r, col([0.55, 0.60, 0.68, 1.0], alpha)).filled(true).num_segments(24).build();
            dl.add_circle(end, r * 0.55, col([0.30, 0.34, 0.40, 1.0], alpha)).filled(true).num_segments(20).build();
            let a = tm * 30.0;
            dl.add_line([end[0] - a.cos() * r * 0.9, end[1] - a.sin() * r * 0.9], [end[0] + a.cos() * r * 0.9, end[1] + a.sin() * r * 0.9], col([0.9, 0.94, 1.0, 1.0], 0.8 * alpha)).thickness((0.15 * r).max(1.0)).build();
            if grip {
                let wrap = (1.0 - tm / 0.3).max(0.0);
                dl.add_circle(end, r * (1.5 + wrap), col([0.78, 0.84, 0.92, 1.0], 0.6 * alpha)).num_segments(24).thickness(1.2).build();
            }
        }
    };
    if let (Some(wh), Some(f)) = (&s.whip, &s.fighter) {
        let tm = now.duration_since(wh.since).as_secs_f32();
        let side = (f.fwd.2, 0.0, -f.fwd.0);
        if s.rubber {
            // from his right shoulder joint, where his swing animation has it at this moment; it shoots
            // out over 0.12 s (Gum-Gum Rocket)
            let shoulder = luffy_shoulder(f, tm);
            let reach = (tm / 0.12).min(1.0);
            rubber_arm(shoulder, wh.to, reach, if reach >= 1.0 { tm - 0.12 } else { tm }, 1.0, reach >= 1.0);
        } else {
            // his raised right hand ((fwd.z, 0, -fwd.x) is his right; 0.14.2 and before used his left)
            let hand = (wh.from.0 + side.0 * 0.25, wh.from.1 + 0.25 * f.height, wh.from.2 + side.2 * 0.25);
            // it shoots out over 0.1 s, then holds taut
            let reach = (tm / 0.1).min(1.0);
            if s.cord == 1 {
                cord_line(1, gon_rod_tip(f, tm), wh.to, reach, tm, 1.0, reach >= 1.0);
            } else if s.cord > 0 {
                cord_line(s.cord, hand, wh.to, reach, tm, 1.0, reach >= 1.0);
            } else {
                tendrils(hand, wh.to, reach, tm, 1.0, reach >= 1.0);
            }
        }
    }
    if let Some((from, to, t0)) = s.whip_miss {
        // out to its full reach and back, finding nothing (Luffy's arm also snaps back this way when he
        // lets go: lib.rs starts it at the turn)
        let tm = now.duration_since(t0).as_secs_f32();
        let reach = if tm < 0.14 { tm / 0.14 } else { (1.0 - (tm - 0.14) / 0.2).max(0.0) };
        if s.rubber {
            // his own arm is back as soon as he lets go: the stretched one snaps in fast
            let reach = if tm < 0.14 { tm / 0.14 } else { (1.0 - (tm - 0.14) / 0.09).max(0.0) };
            let from = s.fighter.as_ref().map(|f| luffy_shoulder(f, 1.6)).unwrap_or(from);
            rubber_arm(from, to, reach, tm, 1.0, false);
        } else if s.cord > 0 {
            cord_line(if s.cord == 1 { 4 } else { s.cord }, from, to, reach, tm, 0.85, false);
        } else {
            tendrils(from, to, reach, tm, 0.85, false);
        }
    }

    // ---- 0.13.0: where the whip would catch: a small diamond, the same size on screen near or far
    // (hidden while he swings: the tendril is there). 0.15.0: dark and quiet (it was distracting): a
    // dark diamond with a thin dim edge and no glow. When the button would grab an enemy and pull in
    // for an attack instead, the marker sits on the enemy and glows red.
    let pulse = 0.5 + 0.5 * (now.duration_since(*START_FX.get_or_init(Instant::now)).as_secs_f32() * 4.0).sin();
    let diamond = |sp: [f32; 2], r: f32, fill: [f32; 4], fill_a: f32, edge: [f32; 4], edge_a: f32| {
        let pts = [[sp[0], sp[1] - r], [sp[0] + r, sp[1]], [sp[0], sp[1] + r], [sp[0] - r, sp[1]]];
        dl.add_triangle(pts[0], pts[1], pts[2], col(fill, fill_a)).filled(true).build();
        dl.add_triangle(pts[0], pts[2], pts[3], col(fill, fill_a)).filled(true).build();
        for i in 0..4 {
            let (a0, a1) = (pts[i], pts[(i + 1) % 4]);
            dl.add_line(a0, a1, col([0.0, 0.0, 0.0, 1.0], 0.45)).thickness(2.6 * k).build();
            dl.add_line(a0, a1, col(edge, edge_a)).thickness(1.2 * k).build();
        }
    };
    if let (Some(e), None) = (s.aim_enemy, &s.whip) {
        if let Some((sp, _)) = cam.project(e) {
            let red = [1.0, 0.16, 0.10, 1.0];
            let r = 9.0 * k;
            glow(&dl, t, sp, r * (4.5 + 1.0 * pulse), red, 0.45 + 0.2 * pulse);
            glow(&dl, t, sp, r * 2.0, [1.0, 0.45, 0.30, 1.0], 0.55);
            dl.add_circle(sp, r * (2.2 + 0.3 * pulse), col(red, 0.7)).num_segments(32).thickness(2.0 * k).build();
            diamond(sp, r, [0.45, 0.04, 0.02, 1.0], 0.9, red, 0.95);
            dl.add_circle(sp, 1.8 * k, col([1.0, 0.75, 0.65, 1.0], 0.95)).filled(true).build();
        }
    } else if let (Some(a), None) = (s.aim, &s.whip) {
        // Luffy's edge is a dull straw-hat gold, Deku's a deep teal
        let edge = if s.rubber { [0.62, 0.48, 0.16, 1.0] } else if s.cord == 1 { [0.85, 0.20, 0.15, 1.0] } else if s.cord == 2 { [0.55, 0.80, 1.0, 1.0] } else { [teal[0] * 0.45, teal[1] * 0.45, teal[2] * 0.45, 1.0] };
        if let Some((sp, _)) = cam.project(a) {
            diamond(sp, 5.5 * k, [0.03, 0.03, 0.04, 1.0], 0.7, edge, 0.55 + 0.15 * pulse);
        }
    }

    // ---- 1.2.0: shadow clone poofs: a quick bright core, then a ring of billowing white smoke puffs
    // rolling outward and upward, each turning slowly, thinning as they spread
    for (pos, t0) in &s.poofs {
        let tm = now.duration_since(*t0).as_secs_f32();
        if tm > POOF_LIFE {
            continue;
        }
        let u = tm / POOF_LIFE;
        let Some((sp, z)) = cam.project(*pos) else { continue };
        let m = cam.px(z);
        // the core: a soft white flash in the first moment
        let core = (1.0 - (tm / 0.12)).max(0.0);
        if core > 0.0 {
            glow(&dl, t, sp, 0.9 * m, [1.0, 1.0, 1.0, 1.0], 0.75 * core);
        }
        let seed = (pos.0 * 31.0 + pos.2 * 17.0) as i32 as u32;
        let ease = 1.0 - (1.0 - u).powi(3);
        for i in 0..14u32 {
            let h = hash(seed + i * 7);
            let ang = i as f32 / 14.0 * std::f32::consts::TAU + h * 0.6;
            // out across the camera's view and up, the far ones a little smaller
            let reach = (0.35 + 0.55 * hash(seed + i * 13)) * ease;
            let lift = (0.15 + 0.5 * hash(seed + i * 3)) * ease;
            let wp = (pos.0 + ang.cos() * reach, pos.1 + ang.sin() * reach * 0.8 + lift, pos.2 + ang.sin() * reach * 0.6);
            let Some((pp, pz)) = cam.project(wp) else { continue };
            let r = (0.32 + 0.38 * u + 0.12 * h) * cam.px(pz);
            let a = (u * 9.0).min(1.0) * (1.0 - u).powf(1.6) * (0.55 + 0.25 * h);
            let spin = (h - 0.5) * 2.4 * tm + h * 6.0;
            let (sn, cs) = spin.sin_cos();
            let corner = |x: f32, y: f32| [pp[0] + (x * cs - y * sn) * r, pp[1] + (x * sn + y * cs) * r];
            // a cool shadow under the white, so the puffs read as volume against bright skies too
            let grey = 0.80 + 0.17 * hash(seed + i * 29);
            dl.add_image_quad(t.smoke[(i % 3) as usize], corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0))
                .col(col([grey, grey, grey + 0.02, 1.0], a))
                .build();
        }
        let _ = m;
    }

    // ---- hits: a quick soft flash, a thin ring racing out, a few sparks
    for i in &s.impacts {
        let tm = now.duration_since(i.t0).as_secs_f32();
        let life = if i.big { 0.32 } else { 0.22 };
        if tm > life {
            continue;
        }
        let Some((sp, z)) = cam.project(i.pos) else { continue };
        let m = cam.px(z) * if i.big { 1.4 } else { 1.0 };
        let u = tm / life;
        let a = 1.0 - u;
        glow(&dl, t, sp, 0.55 * m * (0.6 + 0.6 * u), [1.0, 0.98, 0.92, 1.0], 0.7 * a * a);
        glow(&dl, t, sp, 0.9 * m, i.color, 0.25 * a);
        dl.add_circle(sp, (0.2 + 0.7 * u.sqrt()) * m, col(i.color, 0.55 * a)).num_segments(40).thickness((0.035 * m * a).max(1.0)).build();
        let seed = (i.pos.0 * 13.0 + i.pos.2 * 7.0) as i32 as u32;
        for r in 0..6u32 {
            let ang = (r as f32 + hash(seed + r) * 0.8) * 1.047;
            let (sn, cs) = ang.sin_cos();
            let r0 = (0.15 + 0.6 * u) * m;
            let r1 = r0 + 0.35 * m * (1.0 - u);
            stroke(&dl, t.streak, [sp[0] + cs * r0, sp[1] + sn * r0], [sp[0] + cs * r1, sp[1] + sn * r1], 0.06 * m, [1.0, 1.0, 0.95, 1.0], a);
        }
    }

    // ---- a boss tell's words, above the boss's health bar: the game's own type between gilded rules
    if show.text {
        if let (Some(b), Some(fid)) = (&s.banner, font) {
            let tm = now.duration_since(b.t0).as_secs_f32();
            let a = (tm / 0.18).min(1.0) * ((b.dur - tm) / 0.3).clamp(0.0, 1.0);
            let _f = ui.push_font(fid);
            let sz = ui.calc_text_size(&b.text);
            let (x, y) = (w * 0.5 - sz[0] / 2.0, h * 0.775);
            let half = sz[0] / 2.0 + 90.0 * k;
            let cx = w * 0.5;
            let tint = [b.color[0] * 0.5 + 0.5, b.color[1] * 0.5 + 0.5, b.color[2] * 0.5 + 0.5, 1.0];
            for yy in [y - 8.0 * k, y + sz[1] + 6.0 * k] {
                let (clear, c) = (col(tint, 0.0), col(tint, 0.6 * a));
                dl.add_rect_filled_multicolor([cx - half, yy], [cx, yy + 1.2], clear, c, c, clear);
                dl.add_rect_filled_multicolor([cx, yy], [cx + half, yy + 1.2], c, clear, clear, c);
            }
            dl.add_text([x + 1.5, y + 1.5], col([0.0, 0.0, 0.0, 1.0], 0.75 * a), &b.text);
            dl.add_text([x, y], col(tint, 0.95 * a), &b.text);
        }
    }
}
