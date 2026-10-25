//! 1.2.0: combo trees (the user: "like A2's and 2B's combos in my friend's NieR mashup"). Every fighter
//! (not Your Tarnished, who keeps their own weapons) has a full NieR-style tree on the attack buttons:
//!
//! - RB chains a light string of five hits (LB stays the guard and the parry). RT anywhere in it branches off:
//!   after the 1st hit a LAUNCHER (the enemy goes up, the fighter follows: an air combo),
//!   after the 2nd a RUSH (a multi-hit flurry), after the 3rd and later the FINISHER.
//! - RT on its own starts a heavy string (two heavy hits, then the finisher).
//! - In the air, RB chains three air hits and RT is the AIR FINISHER (a spike back to the ground).
//! - RB or RT right out of a dodge or a sprint is a DASH attack.
//!
//! Damage (x the attack rating, BALANCING.md): light hits 1.0-1.3 (RB was 1.37), heavies 2.0 / 2.2 (RT
//! was 2.38), launcher 1.3, rush 2.2, air hits 0.9-1.1, dash 1.4 (lowered after the user's first test). The finishers are "just a little stronger
//! than one of our combo moves" (the user): a combo skill lists 4.8 x 1.15 = 5.52; a finisher deals 6.0.
//! They share one FINISHER_COOLDOWN_S (a combo skill's 6 s): until it is ready again the finisher's
//! spot plays a strong heavy instead (2.6), so a string can't end in a finisher every two seconds.
//!
//! The bodies are Elden Ring's own motion-captured attacks and Ashes of War (anim.rs, scripts.rs aow),
//! plus a few of Elden Force's own moves. 1.2.0 after the user's test ("very fluid hand to hand combat,
//! like Ninja Gaiden 2 Black or Naraka's fists"): the body starts the frame the button goes down (no
//! lunge first: only the dash attack closes distance), a press during a blow waits until it has played
//! out (its window), ordinary hits carry no effects, and a blow reaches only who stands right in front
//! (scripts::STRIKE_HIT, an invisible hitbox; the animation's own hitbox lands too). The punch ledger
//! (ledger.rs start_punch) makes every enemy a node touches take exactly its damage. Finishers get a
//! small flash and, for some fighters, one effect of their own.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::anim::Cue::*;
use crate::anim::Cue::Heavy as HeavyHit;
use crate::scripts::{self, act, aow, fire, go, At, Aim, Dest, Step};
use crate::scripts::{
    DRYLEAF_WHIRLWIND, GIANT_HUNT, GROUND_SLAM, KICK, LIFESTEAL_FIST, LIONS_CLAW, PALM_BLAST, RAGING_BEAST, REPEATING_THRUST,
    SAVAGE_LIONS_CLAW, SPINNING_SLASH, STORM_ASSAULT, STORM_KICK,
};

/// Finishers (ground and air) share this cooldown: a combo skill's.
pub const FINISHER_COOLDOWN_S: f32 = 6.0;
/// A finisher's damage (x attack rating): a combo skill lists 4.8 x 1.15 = 5.52.
pub const FINISHER_DMG: f32 = 6.0;
/// After a node has played out, the next press must come within this long or the string starts over.
pub const CHAIN_GRACE_S: f32 = 0.6;

// 1.2.0 round 4 (the user: "every animation finishes ... if you mistimed it, you get punished", "every
// attack must land as predicted every time"): commit, then connect.
//
// - A node plays out in full (`length`): the next one starts only when it is over, whatever was pressed.
// - The next press counts from the node's `window` on (its blow has landed, the follow-through plays).
//   A press before that is too early: after a light or an air hit it is ignored (press again in time),
//   after anything heavier it breaks the string (`strict`): the node still plays out, and the next press
//   starts over.
// - Until its blow lands (`hit_at`) a node can't be dodged out of (the behaviour script's JR_Commit).
// - Every node costs stamina (`cost`, Elden Ring-like: a five-hit light string is about half a starting
//   fighter's bar, a launcher, three air hits and the air finisher about three quarters); with none left a
// press does nothing.
// - The blow lands on everyone inside the node's `reach` (metres, a 60-degree wedge ahead) at `hit_at`,
//   every time, and on nobody outside it (combat.rs Step::Strike); a launched enemy is held right in
//   front of the fighter's chest (juggle.rs) so the air hits always reach it.
// - The enemy reacts with its own Elden Ring hit animations: `dmg_level` (AtkParam dmgLevel: a flinch,
//   knocked off its feet, blown upward, slammed down) and `poise` (big enemies stand through light hits).

/// AtkParam dmgLevel values the strike carries (Elden Ring's own hit reactions)
pub const DMG_SMALL: u8 = 1;
pub const DMG_MEDIUM: u8 = 2;
pub const DMG_LARGE: u8 = 3;
pub const DMG_BLOW: u8 = 4;
pub const DMG_SLAM: u8 = 6;
pub const DMG_UPWARD: u8 = 7;
pub const DMG_NONE: u8 = 0;
/// the launch's and air blows' poise damage: through any attack's armour (a soldier mid-swing played its
/// armoured flinch instead of the airborne reaction)
pub const LAUNCH_POISE: f32 = 999.0;
/// lab: every blow's dmgLevel (0: each node's own)
#[cfg(feature = "lab")]
pub static LAB_LEVEL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Light,
    Heavy,
    Launcher,
    Rush,
    Finisher,
    Air,
    AirFinisher,
    Dash,
}

pub struct Node {
    /// "goku_l1": the training panel and the logs
    pub id: &'static str,
    /// the anime name, shown on the controls page and the training panel
    pub name: &'static str,
    pub kind: Kind,
    /// damage x attack rating
    pub dmg: f32,
    /// seconds from the node's start until the next press chains (earlier presses wait for it)
    pub window: f32,
    pub steps: Vec<Step>,
}

pub struct Tree {
    pub light: Vec<Node>,
    pub heavy: Vec<Node>,
    pub launcher: Node,
    pub rush: Node,
    pub finisher: Node,
    /// the finisher's spot while finishers cool down
    pub strong: Node,
    pub air: Vec<Node>,
    pub air_finisher: Node,
    /// the air finisher's spot while finishers cool down
    pub air_spike: Node,
    pub dash: Node,
}

fn n(id: &'static str, name: &'static str, kind: Kind, dmg: f32, window: f32, steps: Vec<Step>) -> Node {
    Node { id, name, kind, dmg, window, steps }
}

impl Node {
    /// Seconds from the start until the node has played out in full (wind-up, blow, follow-through,
    /// recovery): the next node starts no earlier.
    pub fn length(&self) -> f32 {
        self.window
            + match self.kind {
                Kind::Light => 0.24,
                Kind::Heavy => 0.36,
                Kind::Launcher => 0.2,
                Kind::Rush => 0.3,
                Kind::Finisher => 0.5,
                Kind::Air => 0.14,
                Kind::AirFinisher => 0.35,
                Kind::Dash => 0.3,
            }
    }

    /// Seconds from the start until the (first) blow lands: no dodging out of it before then.
    pub fn hit_at(&self) -> f32 {
        let mut t = 0.0;
        for st in &self.steps {
            match *st {
                Step::Wait(w) => t += w,
                Step::Move { dur, .. } => t += dur,
                Step::Strike => return t,
                _ => {}
            }
        }
        t
    }

    /// Stamina the node costs.
    pub fn cost(&self) -> i32 {
        match self.kind {
            Kind::Light => 10,
            Kind::Heavy => 18,
            Kind::Launcher => 20,
            Kind::Rush => 22,
            Kind::Finisher => 24,
            Kind::Air => 8,
            Kind::AirFinisher => 16,
            Kind::Dash => 14,
        }
    }

    /// A press before this node's window breaks the string (heavier moves); otherwise it is ignored.
    pub fn strict(&self) -> bool {
        !matches!(self.kind, Kind::Light | Kind::Air)
    }

    /// How far ahead the blow reaches (metres, in a 60-degree wedge).
    pub fn reach(&self) -> f32 {
        match self.kind {
            Kind::Light | Kind::Air => 2.5,
            Kind::Heavy | Kind::Launcher | Kind::Dash => 2.6,
            Kind::Rush | Kind::Finisher | Kind::AirFinisher => 2.9,
        }
    }

    /// The enemy's own hit reaction (AtkParam dmgLevel) and the blow's poise damage.
    pub fn reaction(&self) -> (u8, f32) {
        #[cfg(feature = "lab")]
        {
            let o = LAB_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
            if o == 99 {
                // no hit reaction at all
                return (0, 0.0);
            }
            if o > 0 {
                return (o as u8, if o == 7 { LAUNCH_POISE } else { 60.0 });
            }
        }
        match self.kind {
            Kind::Light => (DMG_SMALL, 14.0),
            // 1.2.0 round 5: the launch is a "launched upward" hit no armour or shield stands through: the
            // soldiers' reaction to it is Elden Force's airborne pose (make_enemy_pose.py). The air blows
            // cause none of their own (in the air theirs is "blown away"): juggle.rs replays the pose instead,
            // the limbs whipping and settling on every hit
            Kind::Air => (DMG_NONE, 0.0),
            Kind::Heavy => (DMG_MEDIUM, 34.0),
            Kind::Dash => (DMG_MEDIUM, 26.0),
            Kind::Rush => (DMG_MEDIUM, 18.0),
            Kind::Launcher => (DMG_UPWARD, if crate::combat::lab_poise_on() { LAUNCH_POISE } else { 40.0 }),
            Kind::Finisher => (DMG_BLOW, 60.0),
            Kind::AirFinisher => (DMG_SLAM, 50.0),
        }
    }
}

// small helpers for the trees (1.2.0 after the user's test: hand-to-hand, no lunges, no effects on
// ordinary hits, nothing that reaches past who stands in front)
/// the body plays at once, facing who is in front; `at`: seconds into it when the blow lands
fn blow(body: Step, at: f32) -> Vec<Step> {
    vec![Step::FaceTarget, body, Step::Wait(at), strike()]
}
/// more blows of one body (a flurry): a strike lands at each of `at` (seconds from the start)
fn flurry(body: Step, at: &[f32]) -> Vec<Step> {
    let mut v = vec![Step::FaceTarget, body];
    let mut t = 0.0;
    for &a in at {
        v.push(Step::Wait(a - t));
        // each blow of a flurry tracks the enemy (the body may have carried the fighter round them)
        v.push(Step::FaceTarget);
        v.push(strike());
        t = a;
    }
    v
}
/// the blow lands (combat.rs Step::Strike: on everyone inside the node's reach, scripts::STRIKE_HIT)
fn strike() -> Step {
    Step::Strike
}
fn with(mut v: Vec<Step>, more: Vec<Step>) -> Vec<Step> {
    v.extend(more);
    v
}
fn stop(s: f32) -> Step {
    Step::HitStop(s)
}
/// 1.2.0 round 5 (user: "it is giant hunt. that is the one I want for every character"): every launcher
/// is Giant Hunt's scooping uppercut, played in full: about a second crouched low, the fist connects at
/// about 1.1 s (lab footage; our blow lands at GH_HIT, just ahead of Giant Hunt's own, so the reaction is ours), the
/// arm finishes thrown straight up; only then does the fighter leap after them.
const GH_HIT: f32 = 1.18;
/// the target can't be hurt until then (s): Giant Hunt's own blow (about 1.1 s) passes through it
pub const GH_SAFE_UNTIL: f32 = 1.14;
const GH_WINDOW: f32 = 1.6;

/// the launcher's climb after the enemy (skipped when nobody went up)
fn follow() -> Vec<Step> {
    // 1.2.0 round 5 (the user: "I don't want our player to just teleport up into the air with the enemy"):
    // the uppercut lands with a jolt, its arm-up finish plays, then the fighter leaps after them, meeting
    // them at the top
    vec![Step::Launch(3.8), stop(0.08), Step::Wait(0.34), go(Dest::Up { height: 2.3, toward: 0.0 }, 0.42), Step::Hover(1.2)]
}
/// an air hit: closer to the juggled enemy only when it is within reach, then hang there
fn air(body: Step, at: f32) -> Vec<Step> {
    // (a moment's hit-stop on every blow: the user wanted each to read as a huge punch)
    vec![go(Dest::Near { gap: 1.1, max: 3.5 }, 0.08), body, Step::Wait(at), strike(), Step::Juggle(0.5), stop(0.09), Step::Hover(1.1)]
}
/// the air finisher's fall: the held enemies smashed down, the fighter straight down to the ground
fn spike_down() -> Vec<Step> {
    vec![Step::Spike, go(Dest::Drop, 0.16), strike(), stop(0.1)]
}
/// the dash attack: the one move that closes distance (up to 9 m, as far as it looks for someone)
fn dash(body: Step, at: f32) -> Vec<Step> {
    // (twice: a dodge's own motion can still be carrying the fighter back during the first)
    vec![go(Dest::Near { gap: 1.1, max: 9.0 }, 0.16), go(Dest::Near { gap: 1.1, max: 9.0 }, 0.08), body, Step::Wait(at), strike()]
}

/// A fighter's tree (None: Your Tarnished, who fights with their own weapons).
/// Lab (script "notrees"): no fighter has a combo tree (Elden Ring's own attacks play, e.g. another mod's)
pub static LAB_NO_TREES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 1.2.0 (the user, after four rounds on these trees: "scrap the combo system ... and use Carian Combo
/// Warriors"): the fighters' basic attacks are Carian Combo Warriors' movesets now (hks/jr_dispatch.lua),
/// so no fighter has a tree; the trees stay here unused until they are taken out for good.
pub const TREES_RETIRED: bool = true;

pub fn tree(fighter: &str) -> Option<&'static Tree> {
    if TREES_RETIRED || LAB_NO_TREES.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    static TREES: OnceLock<HashMap<&'static str, Tree>> = OnceLock::new();
    TREES.get_or_init(build).get(fighter)
}

/// Every bullet any tree fires (scripts.rs all_bullets: the mod's own bullets in the shared pool).
pub fn all_bullets() -> Vec<i32> {
    let mut out = vec![scripts::STRIKE_HIT];
    for f in crate::sheets::FIGHTERS.iter() {
        let Some(t) = tree(f.id) else { continue };
        for node in t.nodes() {
            for s in &node.steps {
                let b = match *s {
                    Step::Fire { b, .. } | Step::Ring { b, .. } => b,
                    _ => continue,
                };
                if !out.contains(&b) {
                    out.push(b);
                }
            }
        }
    }
    out
}

impl Tree {
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.light.iter().chain(self.heavy.iter()).chain(self.air.iter()).chain([&self.launcher, &self.rush, &self.finisher, &self.strong, &self.air_finisher, &self.air_spike, &self.dash])
    }
}

fn build() -> HashMap<&'static str, Tree> {
    use Kind::*;
    use Step::{Flash, Voice};
    let mut m = HashMap::new();
    // damage (x attack rating): light 1.0-1.3, heavy 2.0 / 2.2, launcher 1.3, rush 2.2, air 0.9-1.1, dash 1.4,
    // strong 2.2, air spike 1.5, finishers FINISHER_DMG. A node's window: its blow has played out.

    // ---- Goku: Turtle School martial arts
    m.insert("goku", Tree {
        light: vec![
            n("goku_l1", "Jab", Light, 1.0, 0.36, blow(act(Strike), 0.16)),
            n("goku_l2", "Cross", Light, 1.0, 0.38, blow(act(Strike2), 0.17)),
            n("goku_l3", "Rising Knee", Light, 1.1, 0.48, blow(aow(KICK), 0.28)),
            n("goku_l4", "Spinning Back Kick", Light, 1.2, 0.55, blow(aow(STORM_KICK), 0.32)),
            n("goku_l5", "Palm Strike", Light, 1.3, 0.62, blow(aow(LIFESTEAL_FIST), 0.36)),
        ],
        heavy: vec![
            n("goku_h1", "Hammer Fist", Heavy, 2.0, 0.72, with(blow(act(HeavyHit), 0.36), vec![stop(0.05)])),
            n("goku_h2", "Spinning Backfist", Heavy, 2.2, 0.75, with(blow(aow(SPINNING_SLASH), 0.34), vec![stop(0.05)])),
        ],
        launcher: n("goku_launch", "Rising Dragon", Launcher, 1.3, GH_WINDOW, with(blow(aow(GIANT_HUNT), GH_HIT), follow())),
        rush: n("goku_rush", "Kaio-ken Rush", Rush, 2.2, 1.0, flurry(aow(RAGING_BEAST), &[0.2, 0.42, 0.64, 0.86])),
        finisher: n("goku_fin", "Dragon Fist", Finisher, FINISHER_DMG, 1.0, with(blow(aow(PALM_BLAST), 0.36), vec![Voice, Flash, stop(0.14)])),
        strong: n("goku_strong", "Ki Straight", Heavy, 2.2, 0.72, with(blow(aow(LIFESTEAL_FIST), 0.36), vec![stop(0.05)])),
        air: vec![
            n("goku_a1", "Air Jab", Air, 0.9, 0.34, air(act(Strike), 0.14)),
            n("goku_a2", "Air Roundhouse", Air, 1.0, 0.36, air(act(FlyingKick), 0.12)),
            n("goku_a3", "Air Hook", Air, 1.1, 0.38, air(act(Strike2), 0.15)),
        ],
        air_finisher: n("goku_afin", "Meteor Smash", AirFinisher, FINISHER_DMG, 0.9, with(vec![act(HeavyHit), Step::Wait(0.12), Flash], spike_down())),
        air_spike: n("goku_aspike", "Axe Kick", Air, 1.5, 0.6, with(vec![act(FlyingKick), Step::Wait(0.1)], spike_down())),
        dash: n("goku_dash", "Flash Step Strike", Dash, 1.4, 0.5, dash(act(Strike2), 0.15)),
    });

    // ---- Naruto: scrappy brawling, flips and kicks
    m.insert("naruto", Tree {
        light: vec![
            n("naruto_l1", "Leaf Jab", Light, 1.0, 0.34, blow(act(Strike), 0.16)),
            n("naruto_l2", "Straight", Light, 1.0, 0.38, blow(act(Strike2), 0.17)),
            n("naruto_l3", "Leaf Whirlwind", Light, 1.1, 0.52, blow(aow(STORM_KICK), 0.3)),
            n("naruto_l4", "Front Kick", Light, 1.2, 0.5, blow(aow(KICK), 0.28)),
            n("naruto_l5", "Uzumaki Spin", Light, 1.3, 0.7, flurry(aow(DRYLEAF_WHIRLWIND), &[0.3, 0.55])),
        ],
        heavy: vec![
            n("naruto_h1", "Heel Drop", Heavy, 2.0, 0.72, with(blow(act(HeavyHit), 0.36), vec![stop(0.05)])),
            n("naruto_h2", "Spinning Kick", Heavy, 2.2, 0.75, with(blow(aow(SPINNING_SLASH), 0.34), vec![stop(0.05)])),
        ],
        launcher: n("naruto_launch", "U-Zu-Ma-Ki!", Launcher, 1.3, GH_WINDOW, with(blow(aow(GIANT_HUNT), GH_HIT), follow())),
        rush: n("naruto_rush", "Barrage", Rush, 2.2, 1.0, flurry(aow(RAGING_BEAST), &[0.2, 0.42, 0.64, 0.86])),
        finisher: n("naruto_fin", "Rasengan", Finisher, FINISHER_DMG, 1.0, with(blow(aow(PALM_BLAST), 0.36), vec![Voice, fire(scripts::OBUBBLE, At::Ahead(0.9), Aim::Forward), Flash, stop(0.14)])),
        strong: n("naruto_strong", "Palm Thrust", Heavy, 2.2, 0.72, with(blow(aow(PALM_BLAST), 0.36), vec![stop(0.05)])),
        air: vec![
            n("naruto_a1", "Air Kick", Air, 0.9, 0.34, air(act(FlyingKick), 0.12)),
            n("naruto_a2", "Air Punch", Air, 1.0, 0.36, air(act(Strike2), 0.15)),
            n("naruto_a3", "Air Jab", Air, 1.1, 0.36, air(act(Strike), 0.14)),
        ],
        air_finisher: n("naruto_afin", "Barrage Heel Drop", AirFinisher, FINISHER_DMG, 0.9, with(vec![act(FlyingKick), Step::Wait(0.12), Flash], spike_down())),
        air_spike: n("naruto_aspike", "Heel Drop", Air, 1.5, 0.6, with(vec![act(HeavyHit), Step::Wait(0.12)], spike_down())),
        dash: n("naruto_dash", "Sliding Kick", Dash, 1.4, 0.5, dash(aow(KICK), 0.24)),
    });

    // ---- Luffy: rubber blows
    m.insert("luffy", Tree {
        light: vec![
            n("luffy_l1", "Gum-Gum Pistol", Light, 1.0, 0.38, blow(act(Pistol), 0.15)),
            n("luffy_l2", "Left Straight", Light, 1.0, 0.38, blow(act(Strike2), 0.17)),
            n("luffy_l3", "Gum-Gum Whip", Light, 1.1, 0.52, blow(aow(STORM_KICK), 0.3)),
            n("luffy_l4", "Gum-Gum Pistol", Light, 1.2, 0.4, blow(act(Pistol), 0.15)),
            n("luffy_l5", "Gum-Gum Stamp", Light, 1.3, 0.55, blow(aow(KICK), 0.28)),
        ],
        heavy: vec![
            n("luffy_h1", "Gum-Gum Hammer", Heavy, 2.0, 0.72, with(blow(act(HeavyHit), 0.36), vec![stop(0.05)])),
            n("luffy_h2", "Gum-Gum Bazooka", Heavy, 2.2, 0.66, with(blow(act(Grizzly), 0.22), vec![stop(0.06)])),
        ],
        launcher: n("luffy_launch", "Gum-Gum Uppercut", Launcher, 1.3, GH_WINDOW, with(blow(aow(GIANT_HUNT), GH_HIT), follow())),
        rush: n("luffy_rush", "Gum-Gum Gatling", Rush, 2.2, 1.0, flurry(aow(REPEATING_THRUST), &[0.15, 0.3, 0.45, 0.6, 0.75])),
        finisher: n("luffy_fin", "Gum-Gum Bazooka", Finisher, FINISHER_DMG, 0.9, with(blow(act(Grizzly), 0.22), vec![Voice, Flash, stop(0.14)])),
        strong: n("luffy_strong", "Gum-Gum Rifle", Heavy, 2.2, 0.6, with(blow(act(Pistol), 0.15), vec![stop(0.05)])),
        air: vec![
            n("luffy_a1", "Air Pistol", Air, 0.9, 0.36, air(act(Pistol), 0.14)),
            n("luffy_a2", "Gum-Gum Whip", Air, 1.0, 0.36, air(act(FlyingKick), 0.12)),
            n("luffy_a3", "Air Hook", Air, 1.1, 0.38, air(act(Strike2), 0.15)),
        ],
        air_finisher: n("luffy_afin", "Gum-Gum Battle Axe", AirFinisher, FINISHER_DMG, 0.9, with(vec![act(HeavyHit), Step::Wait(0.14), Flash], spike_down())),
        air_spike: n("luffy_aspike", "Gum-Gum Axe", Air, 1.5, 0.6, with(vec![act(FlyingKick), Step::Wait(0.1)], spike_down())),
        dash: n("luffy_dash", "Gum-Gum Rocket", Dash, 1.4, 0.5, dash(act(Pistol), 0.14)),
    });

    // ---- Deku: Shoot Style, mostly kicks
    m.insert("deku", Tree {
        light: vec![
            n("deku_l1", "Full Cowl Jab", Light, 1.0, 0.34, blow(act(Strike), 0.16)),
            n("deku_l2", "Shoot Style Kick", Light, 1.0, 0.48, blow(aow(KICK), 0.28)),
            n("deku_l3", "St. Louis Smash", Light, 1.1, 0.52, blow(aow(STORM_KICK), 0.3)),
            n("deku_l4", "Hook", Light, 1.2, 0.38, blow(act(Strike2), 0.17)),
            n("deku_l5", "Whirlwind Kicks", Light, 1.3, 0.7, flurry(aow(DRYLEAF_WHIRLWIND), &[0.3, 0.55])),
        ],
        heavy: vec![
            n("deku_h1", "Shoot Style Axe", Heavy, 2.0, 0.8, with(blow(aow(STORM_ASSAULT), 0.45), vec![stop(0.05)])),
            n("deku_h2", "5% Detroit Smash", Heavy, 2.2, 0.72, with(blow(act(HeavyHit), 0.36), vec![stop(0.05)])),
        ],
        launcher: n("deku_launch", "Rising Kick", Launcher, 1.3, GH_WINDOW, with(blow(aow(GIANT_HUNT), GH_HIT), follow())),
        rush: n("deku_rush", "Full Cowl Barrage", Rush, 2.2, 1.0, flurry(aow(RAGING_BEAST), &[0.2, 0.42, 0.64, 0.86])),
        finisher: n("deku_fin", "Manchester Smash", Finisher, FINISHER_DMG, 1.2, with(blow(aow(SAVAGE_LIONS_CLAW), 0.6), vec![Voice, Flash, stop(0.14)])),
        strong: n("deku_strong", "Detroit Smash", Heavy, 2.2, 0.72, with(blow(act(HeavyHit), 0.36), vec![stop(0.05)])),
        air: vec![
            n("deku_a1", "Air Kick", Air, 0.9, 0.34, air(act(FlyingKick), 0.12)),
            n("deku_a2", "Air Smash", Air, 1.0, 0.36, air(act(Strike2), 0.15)),
            n("deku_a3", "Air Side Kick", Air, 1.1, 0.36, air(act(FlyingKick), 0.12)),
        ],
        air_finisher: n("deku_afin", "Smash Dive", AirFinisher, FINISHER_DMG, 0.9, with(vec![act(HeavyHit), Step::Wait(0.14), Flash], spike_down())),
        air_spike: n("deku_aspike", "Shoot Style Drop", Air, 1.5, 0.6, with(vec![act(FlyingKick), Step::Wait(0.1)], spike_down())),
        dash: n("deku_dash", "Full Cowl Dash", Dash, 1.4, 0.5, dash(act(FlyingKick), 0.12)),
    });

    // ---- All Might: heavy, deliberate punches
    m.insert("all_might", Tree {
        light: vec![
            n("allmight_l1", "Texas Jab", Light, 1.0, 0.38, blow(act(Strike), 0.17)),
            n("allmight_l2", "Carolina Cross", Light, 1.0, 0.4, blow(act(Strike2), 0.18)),
            n("allmight_l3", "Missouri Hook", Light, 1.1, 0.6, blow(aow(LIFESTEAL_FIST), 0.36)),
            n("allmight_l4", "Body Blow", Light, 1.2, 0.4, blow(act(Strike), 0.17)),
            n("allmight_l5", "Detroit Hammer", Light, 1.3, 0.75, with(blow(act(HeavyHit), 0.38), vec![stop(0.04)])),
        ],
        heavy: vec![
            n("allmight_h1", "Detroit Smash", Heavy, 2.0, 0.75, with(blow(act(HeavyHit), 0.38), vec![stop(0.05)])),
            n("allmight_h2", "New Hampshire Smash", Heavy, 2.2, 0.85, with(blow(aow(GROUND_SLAM), 0.5), vec![stop(0.06)])),
        ],
        launcher: n("allmight_launch", "Oklahoma Uppercut", Launcher, 1.3, GH_WINDOW, with(blow(aow(GIANT_HUNT), GH_HIT), follow())),
        rush: n("allmight_rush", "Plus Ultra Rush", Rush, 2.2, 1.0, flurry(aow(REPEATING_THRUST), &[0.15, 0.3, 0.45, 0.6, 0.75])),
        finisher: n("allmight_fin", "United States of Smash", Finisher, FINISHER_DMG, 1.2, with(blow(aow(LIONS_CLAW), 0.6), vec![Voice, Flash, fire(scripts::STORM_STOMP, At::Feet, Aim::Forward), stop(0.16)])),
        strong: n("allmight_strong", "Smash", Heavy, 2.2, 0.75, with(blow(act(HeavyHit), 0.38), vec![stop(0.05)])),
        air: vec![
            n("allmight_a1", "Air Jab", Air, 0.9, 0.36, air(act(Strike), 0.15)),
            n("allmight_a2", "Air Hook", Air, 1.0, 0.38, air(act(Strike2), 0.16)),
            n("allmight_a3", "Air Hammer", Air, 1.1, 0.4, air(act(HeavyHit), 0.2)),
        ],
        air_finisher: n("allmight_afin", "Nevada Smash", AirFinisher, FINISHER_DMG, 0.9, with(vec![act(HeavyHit), Step::Wait(0.14), Flash], spike_down())),
        air_spike: n("allmight_aspike", "Hammer Drop", Air, 1.5, 0.6, with(vec![act(HeavyHit), Step::Wait(0.12)], spike_down())),
        dash: n("allmight_dash", "Carolina Smash", Dash, 1.4, 0.5, dash(act(Strike2), 0.16)),
    });

    // ---- Bakugo: aggressive brawling; explosive only at the very end
    m.insert("bakugo", Tree {
        light: vec![
            n("bakugo_l1", "Jab", Light, 1.0, 0.34, blow(act(Strike), 0.16)),
            n("bakugo_l2", "Right Hook", Light, 1.0, 0.38, blow(act(Strike2), 0.17)),
            n("bakugo_l3", "Kick", Light, 1.1, 0.48, blow(aow(KICK), 0.28)),
            n("bakugo_l4", "Spinning Backhand", Light, 1.2, 0.55, blow(aow(SPINNING_SLASH), 0.3)),
            n("bakugo_l5", "Overhand", Light, 1.3, 0.7, with(blow(act(HeavyHit), 0.36), vec![stop(0.04)])),
        ],
        heavy: vec![
            n("bakugo_h1", "Palm Strike", Heavy, 2.0, 0.7, with(blow(aow(PALM_BLAST), 0.36), vec![stop(0.05)])),
            n("bakugo_h2", "Hammer", Heavy, 2.2, 0.72, with(blow(act(HeavyHit), 0.36), vec![stop(0.05)])),
        ],
        launcher: n("bakugo_launch", "Uppercut", Launcher, 1.3, GH_WINDOW, with(blow(aow(GIANT_HUNT), GH_HIT), follow())),
        rush: n("bakugo_rush", "Barrage", Rush, 2.2, 1.0, flurry(aow(RAGING_BEAST), &[0.2, 0.42, 0.64, 0.86])),
        finisher: n("bakugo_fin", "Point-Blank Explosion", Finisher, FINISHER_DMG, 1.0, with(blow(aow(PALM_BLAST), 0.36), vec![Voice, fire(scripts::EXPLOSIVE_CLUMP_HIT, At::Ahead(1.1), Aim::Forward), stop(0.14)])),
        strong: n("bakugo_strong", "Straight", Heavy, 2.2, 0.72, with(blow(act(HeavyHit), 0.36), vec![stop(0.05)])),
        air: vec![
            n("bakugo_a1", "Air Jab", Air, 0.9, 0.34, air(act(Strike), 0.14)),
            n("bakugo_a2", "Air Spin Kick", Air, 1.0, 0.36, air(act(FlyingKick), 0.12)),
            n("bakugo_a3", "Air Hook", Air, 1.1, 0.38, air(act(Strike2), 0.15)),
        ],
        air_finisher: n("bakugo_afin", "Howitzer Drop", AirFinisher, FINISHER_DMG, 0.9, with(vec![act(HeavyHit), Step::Wait(0.14), fire(scripts::EXPLOSIVE_CLUMP_HIT, At::Ahead(0.8), Aim::Forward)], spike_down())),
        air_spike: n("bakugo_aspike", "Hammer Drop", Air, 1.5, 0.6, with(vec![act(HeavyHit), Step::Wait(0.12)], spike_down())),
        dash: n("bakugo_dash", "Blast Dash", Dash, 1.4, 0.5, dash(act(Strike2), 0.15)),
    });

    // ---- Todoroki: precise strikes; one burst of his quirk at the end
    m.insert("todoroki", Tree {
        light: vec![
            n("todoroki_l1", "Jab", Light, 1.0, 0.34, blow(act(Strike), 0.16)),
            n("todoroki_l2", "Cross", Light, 1.0, 0.38, blow(act(Strike2), 0.17)),
            n("todoroki_l3", "Low Kick", Light, 1.1, 0.48, blow(aow(KICK), 0.28)),
            n("todoroki_l4", "Spinning Kick", Light, 1.2, 0.52, blow(aow(STORM_KICK), 0.3)),
            n("todoroki_l5", "Elbow", Light, 1.3, 0.7, with(blow(act(HeavyHit), 0.36), vec![stop(0.04)])),
        ],
        heavy: vec![
            n("todoroki_h1", "Palm Strike", Heavy, 2.0, 0.7, with(blow(aow(PALM_BLAST), 0.36), vec![stop(0.05)])),
            n("todoroki_h2", "Hammer", Heavy, 2.2, 0.72, with(blow(act(HeavyHit), 0.36), vec![stop(0.05)])),
        ],
        launcher: n("todoroki_launch", "Rising Strike", Launcher, 1.3, GH_WINDOW, with(blow(aow(GIANT_HUNT), GH_HIT), follow())),
        rush: n("todoroki_rush", "Flurry", Rush, 2.2, 1.0, flurry(aow(RAGING_BEAST), &[0.2, 0.42, 0.64, 0.86])),
        finisher: n("todoroki_fin", "Ice Spike", Finisher, FINISHER_DMG, 1.0, with(blow(aow(scripts::HOARFROST_STOMP), 0.4), vec![Voice, Flash, stop(0.14)])),
        strong: n("todoroki_strong", "Straight", Heavy, 2.2, 0.72, with(blow(act(HeavyHit), 0.36), vec![stop(0.05)])),
        air: vec![
            n("todoroki_a1", "Air Jab", Air, 0.9, 0.34, air(act(Strike), 0.14)),
            n("todoroki_a2", "Air Kick", Air, 1.0, 0.36, air(act(FlyingKick), 0.12)),
            n("todoroki_a3", "Air Hook", Air, 1.1, 0.38, air(act(Strike2), 0.15)),
        ],
        air_finisher: n("todoroki_afin", "Glacial Drop", AirFinisher, FINISHER_DMG, 0.9, with(vec![act(HeavyHit), Step::Wait(0.14), Flash], spike_down())),
        air_spike: n("todoroki_aspike", "Hammer Drop", Air, 1.5, 0.6, with(vec![act(HeavyHit), Step::Wait(0.12)], spike_down())),
        dash: n("todoroki_dash", "Sliding Strike", Dash, 1.4, 0.5, dash(act(Strike2), 0.15)),
    });

    // ---- Asta: his sword (Elden Ring's straight sword swings)
    m.insert("asta", Tree {
        light: vec![
            n("asta_l1", "Slash", Light, 1.0, 0.42, blow(act(Strike), 0.2)),
            n("asta_l2", "Return Slash", Light, 1.0, 0.44, blow(act(Strike2), 0.2)),
            n("asta_l3", "Slash", Light, 1.1, 0.42, blow(act(Strike), 0.2)),
            n("asta_l4", "Return Slash", Light, 1.2, 0.44, blow(act(Strike2), 0.2)),
            n("asta_l5", "Overhead Cleave", Light, 1.3, 0.8, with(blow(act(HeavyHit), 0.4), vec![stop(0.04)])),
        ],
        heavy: vec![
            n("asta_h1", "Demon Slash", Heavy, 2.0, 0.8, with(blow(act(HeavyHit), 0.4), vec![stop(0.05)])),
            n("asta_h2", "Second Cleave", Heavy, 2.2, 0.8, with(blow(act(HeavyHit), 0.4), vec![stop(0.05)])),
        ],
        launcher: n("asta_launch", "Rising Slash", Launcher, 1.3, GH_WINDOW, with(blow(aow(GIANT_HUNT), GH_HIT), follow())),
        rush: n("asta_rush", "Flurry of Slashes", Rush, 2.2, 1.0, with(blow(act(Strike), 0.2), with(vec![Step::Wait(0.22)], with(blow(act(Strike2), 0.2), with(vec![Step::Wait(0.2)], blow(act(Strike), 0.2)))))),
        finisher: n("asta_fin", "Black Slash", Finisher, FINISHER_DMG, 1.0, with(blow(act(HeavyHit), 0.4), vec![Voice, fire(scripts::DESTINED_DEATH, At::Ahead(1.2), Aim::Forward), stop(0.16)])),
        strong: n("asta_strong", "Heavy Cleave", Heavy, 2.2, 0.8, with(blow(act(HeavyHit), 0.4), vec![stop(0.05)])),
        air: vec![
            n("asta_a1", "Air Slash", Air, 0.9, 0.38, air(act(Strike), 0.16)),
            n("asta_a2", "Air Return Slash", Air, 1.0, 0.4, air(act(Strike2), 0.16)),
            n("asta_a3", "Air Slash", Air, 1.1, 0.38, air(act(Strike), 0.16)),
        ],
        air_finisher: n("asta_afin", "Black Meteor Drop", AirFinisher, FINISHER_DMG, 0.9, with(vec![act(HeavyHit), Step::Wait(0.16), Flash], spike_down())),
        air_spike: n("asta_aspike", "Plunging Cut", Air, 1.5, 0.6, with(vec![act(HeavyHit), Step::Wait(0.14)], spike_down())),
        dash: n("asta_dash", "Bull Thrust", Dash, 1.4, 0.5, dash(act(Strike), 0.18)),
    });

    m
}

/// 1.2.0: a button in the Move List (menu_pages.rs): the game's own attack, jump and dodge buttons,
/// the left stick click, the swing button (ours), or a lock-on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Light,
    Heavy,
    Jump,
    Dodge,
    Stick,
    Swing,
    /// 1.2.0: LB (the left hand's attack)
    Left,
    /// 1.2.0: two-handing (Y + RB): Carian Combo Warriors' kick stance
    TwoHand,
    /// 1.2.x: the weapon skill (Ash of War, LT), Your Tarnished's weapons in the Move List
    Skill,
    Hold(&'static Btn),
}

/// One line of the Move List: the buttons (in order), what it is, and a note.
pub struct Line {
    pub buttons: Vec<Btn>,
    pub name: String,
    pub note: &'static str,
}

fn line(buttons: &[Btn], name: impl Into<String>, note: &'static str) -> Line {
    Line { buttons: buttons.to_vec(), name: name.into(), note }
}

/// The Move List's combo lines for a fighter's tree.
pub fn combo_lines(t: &Tree) -> Vec<Line> {
    use Btn::*;
    let names = |v: &[Node]| v.iter().map(|n| n.name).collect::<Vec<_>>().join("  >  ");
    vec![
        line(&[Light, Light, Light, Light, Light], names(&t.light), "light string: press as each blow lands; every hit costs stamina"),
        line(&[Light, Heavy], format!("{}  >  air combo", t.launcher.name), "LAUNCHER: they go up, you follow (press too early and the string breaks)"),
        line(&[Light, Light, Heavy], t.rush.name, "rush"),
        line(&[Light, Light, Light, Heavy], t.finisher.name, "FINISHER (or after more hits)"),
        line(&[Heavy, Heavy, Heavy], format!("{}  >  {}  >  {}", t.heavy[0].name, t.heavy[1].name, t.finisher.name), "heavy string"),
        line(&[Light, Light, Light], names(&t.air), "in the air: three hits a jump"),
        line(&[Heavy], t.air_finisher.name, "AIR FINISHER, in the air"),
        line(&[Light], t.dash.name, "out of a dodge or a sprint"),
    ]
}

/// 1.3.0: a fighter's unique ways of moving (the HUD's movement list): the Move List's movement lines without
/// the three every fighter has, and without the fighter's extra double jumps and air dashes (the user: only the
/// abilities unique to each character).
pub fn own_movement_lines(fighter: &str) -> Vec<Line> {
    const JUMPS_AND_DASHES: [&str; 4] = ["Full Cowl", "Flame Dash", "Double Jump", "Speed of Lightning"];
    let mut all = movement_lines(fighter);
    let n = all.len().saturating_sub(3);
    all.truncate(n);
    all.retain(|l| !JUMPS_AND_DASHES.contains(&l.name.as_str()));
    all
}

/// The Move List's movement lines for a fighter (their own first, then what every fighter has).
pub fn movement_lines(fighter: &str) -> Vec<Line> {
    use Btn::*;
    static HOLD_JUMP: Btn = Btn::Jump;
    static SPRINT: Btn = Btn::Dodge;
    let mut v = match fighter {
        "goku" => vec![
            line(&[Swing], "Instant Transmission", "locked on: behind them in a blink (4 s)"),
            line(&[Hold(&SPRINT), Jump], "Smash Jump", "from a sprint: across the land, a knockback wave on landing (4 s)"),
            line(&[Dodge], "Flight", "in the air: a Superman dash, three a jump"),
        ],
        "naruto" => vec![
            line(&[Jump], "Shadow Clone Throw", "in the air: a clone hurls you on, three times a jump"),
            line(&[Hold(&SPRINT), Jump], "Leap", "from a sprint or a crouch: a long, low leap"),
        ],
        "luffy" => vec![line(&[Swing], "Gum-Gum Swing", "hold: swing from what your arm catches")],
        "deku" => vec![
            line(&[Swing], "Black Whip Swing", "hold: swing; at an enemy, pull in for a kick"),
            line(&[Jump], "Full Cowl", "two double jumps and two air dashes"),
        ],
        "all_might" => vec![
            line(&[Hold(&SPRINT), Jump], "Smash Jump", "from a sprint: across the land, a knockback wave on landing (4 s)"),
            line(&[Dodge], "Flight", "in the air: a Superman dash, three a jump"),
        ],
        "bakugo" => vec![line(&[Jump, Hold(&HOLD_JUMP)], "Explosion Propulsion", "after the double jump, hold: blasts from your palms carry you where you aim (12 a jump, 6 against a boss)")],
        "todoroki" => vec![
            line(&[Hold(&SPRINT), Dodge], "Ice Slide", "dodge from a sprint: surf your ice (2.5 s)"),
            line(&[Jump], "Ice Ramp", "on the ice: launch off it"),
            line(&[Dodge], "Flame Dash", "in the air: two air dashes trailing fire"),
        ],
        "asta" => vec![
            line(&[Hold(&SPRINT), Jump], "Anti-Magic Leap", "from a sprint or a crouch: a long, low leap"),
            line(&[Jump], "Double Jump", "twice a jump"),
            line(&[Swing], "Blade Glider", "in the air: ride your Demon-Slayer Sword down like a surfboard (again or landing: away)"),
        ],
        // 1.3.0: the new fighters
        "ichigo" => vec![line(&[Swing], "Shunpo (Flash Step)", "a blink the way the stick points, on the ground or twice in the air")],
        "sasuke" => vec![line(&[Swing], "Amenotejikara", "locked on: behind them in a blink; otherwise a step 10 m ahead")],
        "zoro" => vec![line(&[Hold(&SPRINT), Dodge], "Lion's Song", "dodge from a sprint: a long drawing-cut dash (2.5 s)")],
        "yami" => vec![line(&[Hold(&SPRINT), Jump], "Dark Cloaked Leap", "from a sprint or a crouch: a long, low leap")],
        "gojo" => vec![
            line(&[Swing], "Blue Step", "a blink 12 m the way the stick points"),
        ],
        "yuji" => vec![line(&[Hold(&SPRINT), Jump], "Superhuman Leap", "from a sprint: across the land, a shockwave on landing (4 s)")],
        "gon" => vec![line(&[Swing], "Fishing Rod Swing", "hold: swing from what the line catches")],
        "killua" => vec![
            line(&[Swing], "Yo-Yo Swing", "hold: swing on his yo-yo from what it catches; nothing in reach, a Godspeed blink"),
            line(&[Dodge], "Speed of Lightning", "in the air: three air dashes"),
        ],
        "jotaro" => vec![line(&[Hold(&SPRINT), Jump], "Star Platinum Leap", "from a sprint: Star Platinum throws him across the land (4 s)")],
        "dio" => vec![line(&[Swing], "The World: Step", "a blink 10 m the way the stick points")],
        _ => vec![],
    };
    v.extend([
        line(&[Jump], "Double Jump", "in the air"),
        line(&[Dodge], "Air Dash", match fighter {
            "goku" | "all_might" => "in the air, where the stick points (the flight above)",
            "naruto" => "in the air, where the stick points: three times a jump",
            "luffy" => "in the air, where the stick points: once a jump",
            _ => "in the air, where the stick points: twice a jump",
        }),
        line(&[Stick], "Slam", "in the air: straight down, a shockwave"),
    ]);
    v
}
