//! How every JUMP FORCE ability plays (sheet: abilities): a short anime-style sequence of dashes,
//! leaps, barrages, finishers and impact freezes. The visuals are Elden Ring's own bullets, borrowed
//! from the player's game in memory; only bullets that end by themselves when the mod fires them are
//! used. Bullets whose effects wait for a caster animation (Comet Azur's beam, Dragonfire, Meteorite
//! of Astel's emitter, Black Flame Tornado, the breaths...) stayed alive forever and piled up into the
//! 0.2.0 lag: the lab's audition (lab/judge2.txt) lists which ones end and which freeze.

/// Where a motion goes.
#[derive(Clone, Copy, Debug)]
pub enum Dest {
    /// to the lock-on target (or `range` ahead), stopping `gap` metres short
    Target { gap: f32 },
    /// through the target to `past` metres beyond it, `side` metres off the line (zig-zags)
    Through { past: f32, side: f32 },
    /// straight ahead / back along the facing
    Forward(f32),
    Back(f32),
    /// up into the air, drifting `toward` of the way to the target
    Up { height: f32, toward: f32 },
    /// from the air down onto the target (or the ground ahead)
    Down,
    /// 1.2.0 (combos): to the target only when it is within `max` metres (height included in the air),
    /// stopping `gap` short; farther away, no move at all (no pulling across the field)
    Near { gap: f32, max: f32 },
    /// 1.2.0: straight down to the ground below (an air finisher; never toward a far target)
    Drop,
}

/// Where a bullet appears.
#[derive(Clone, Copy, Debug)]
pub enum At {
    /// in front of the chest
    Hand,
    Feet,
    /// at the lock-on target (or `range` ahead)
    Target,
    /// `h` metres above the target
    AboveTarget(f32),
    /// `h` metres above the player
    Above(f32),
    /// `d` metres ahead at chest height
    Ahead(f32),
    /// shot k lands start + k * step metres ahead, on the ground (waves)
    Line(f32, f32),
    /// 1.3.0: at the target moved `back` metres toward the fighter, 1 m up (a big model over them whose bullet must not
    /// start inside them: it would end on the spot)
    NearTarget(f32),
    /// 1.3.0: `up` metres above the target and `back` metres toward the fighter (a steep drop onto them)
    OverTarget(f32, f32),
    /// 1.3.0: at a shoulder, `side` metres to the right (negative: the left), alternating sides shot by shot when
    /// `side` is 0 (Luffy's Gatling: his arms shoot out from there)
    Shoulder(f32),
}

/// Which way a bullet flies.
#[derive(Clone, Copy, Debug)]
pub enum Aim {
    Forward,
    Down,
    AtTarget,
    /// away from the ring's centre (rings only)
    Out,
    Back,
}

#[derive(Clone, Copy, Debug)]
pub enum Step {
    Move { to: Dest, dur: f32, arc: f32 },
    /// stay in the air
    Hover(f32),
    /// `n` shots, one every `every` seconds; `spread` radians between fan shots, `side` metres
    /// between parallel streams
    Fire { b: i32, at: At, aim: Aim, n: u32, every: f32, spread: f32, side: f32 },
    /// `n` bullets on a circle of radius `r` around `at`, all at once
    Ring { b: i32, at: At, n: u32, r: f32, aim: Aim },
    Wait(f32),
    /// 1.3.0 (Jotaro's and DIO's The World): every enemy within TIMESTOP_RANGE frozen this long (bosses for half of it)
    TimeStop(f32),
    Aura(bool),
    Voice,
    /// slow the whole game for `secs` (an impact freeze)
    HitStop(f32),
    /// fighter size multiplier (1.0 = back to normal)
    Grow(f32),
    /// the body's animation for what comes next
    Act(Cue),
    /// 1.2.0 (combos): knock the enemy in front up into the air, `height` metres, and hold it there
    /// for an air combo (juggle.rs; never bosses or big enemies)
    Launch(f32),
    /// 1.2.0: the enemies held in the air stay up this much longer (each air hit)
    Juggle(f32),
    /// 1.2.0: the held enemies are smashed down to the ground (an air finisher)
    Spike,
    /// 1.2.0: turn to the target at once when it is close (a combo hit faces who it hits)
    FaceTarget,
    /// 1.2.0: a small flash at the point of contact (the mod's own, drawn by world_fx), for finishers
    Flash,
    /// 1.3.0: one of Luffy's stretching fists from a shoulder (`side` metres to the right, negative the left) at the
    /// target's chest moved `off` (right, up) metres: the one of set `set` (FIST_SETS) whose reach ends there
    RubberFist(u8, f32, (f32, f32)),
    /// 1.3.0: the fighter stays on this spot for `secs` while the steps after go on (the animation moves only his body)
    Plant(f32),
    /// 1.3.0: bullet `b` from `at` straight at the target's chest, living just long enough to get there (its HitBulletID
    /// then bursts on them: Super Spirit Bomb)
    Lob { b: i32, at: At },
    /// 1.3.0: Luffy's Gear Fourth body on / off (look.rs GEAR4); always off when the move ends
    Gear4(bool),
    /// 1.3.0: a shadow clone vanishing in a puff of smoke there (world_fx)
    Poof(At),
    /// 1.3.0: every enemy within STUN_RANGE stands blinded (frozen) this long, bosses half as long (Stun Grenade)
    Stun(f32),
    /// 1.3.0: Naruto's Shadow Clone Jutsu: a clone of him is called (clones.rs)
    Clone,
    /// 1.3.0: the fighter's Stand comes out in front of him for this long, playing this body animation (stands.rs)
    Stand(f32, i32),
    /// 1.3.0: Gojo's Infinity: nothing reaches the fighter for `secs` (invincible, his aura on), the move and its
    /// cooldown going on without him
    Guard(f32),
    /// 1.2.0 round 4: a combo blow lands: STRIKE_HIT on every enemy inside the node's reach (a wedge
    /// ahead) and on the enemy held in the air, every time; nothing past it
    Strike,
    /// 1.2.0: one of a weapon's attacks on the bare hands (Carian Combo Warriors' movesets): the hands are
    /// dressed as weapon `row` for `secs`, then the behaviour script runs `lua`, which plays the attack
    /// (e.g. ExecEventAllBody("W_AttackRightLightDash"): the twinblade's sprint attack, its fire dash)
    Weapon { row: u32, lua: &'static str, secs: f32 },
    /// 1.2.x: the blue ball and spinning air held in the right palm while a move charges (aura.rs PALM)
    PalmFx(bool),
    /// 1.2.x Goku's beam (the next step fires bullet `b`): skipped when the target is under `min` metres (user: "disabled
    /// the beam for attacks under 2 meters"); otherwise `b` lives just long enough to reach the target, so it ends where
    /// the orb bursts instead of flying on past and homing back
    Beam { b: i32, min: f32 },
    /// as Weapon, showing weapon `model`'s model in the hand
    WeaponShown { row: u32, model: u32, lua: &'static str, secs: f32 },
    /// 1.3.1: the held weapon coated with SpEffect `row`'s visual (aura::CHIDORI_BLADE...) on / off; always off when the move ends
    Blade(i32, bool),
    /// 1.3.1: lightning crackling over his head (aura::HEAD_CRACKLE: Killua's Godspeed hair) on / off; always off when the move ends
    HeadFx(bool),
    /// 1.3.1: the form body (Gear4) and size this move put on stay for `secs` after it ends (Gon's adult form)
    KeepForm(f32),
}

use Step::*;

use crate::anim::Cue;

pub(crate) fn fire(b: i32, at: At, aim: Aim) -> Step {
    Fire { b, at, aim, n: 1, every: 0.0, spread: 0.0, side: 0.0 }
}
pub(crate) fn burst(b: i32, at: At, aim: Aim, n: u32, every: f32) -> Step {
    Fire { b, at, aim, n, every, spread: 0.0, side: 0.0 }
}
pub(crate) fn streams(b: i32, at: At, aim: Aim, n: u32, every: f32, side: f32) -> Step {
    Fire { b, at, aim, n, every, spread: 0.0, side }
}
pub(crate) fn fan(b: i32, at: At, aim: Aim, n: u32, every: f32, spread: f32) -> Step {
    Fire { b, at, aim, n, every, spread, side: 0.0 }
}
pub(crate) fn ring(b: i32, at: At, n: u32, r: f32) -> Step {
    Ring { b, at, n, r, aim: Aim::Out }
}
pub(crate) fn go(to: Dest, dur: f32) -> Step {
    Move { to, dur, arc: 0.0 }
}
pub(crate) fn leap(to: Dest, dur: f32, arc: f32) -> Step {
    Move { to, dur, arc }
}

// Elden Ring bullets that end by themselves (lab/judge2.txt).
pub const COMET: i32 = 10402100;
pub const SWIFT_SHARD: i32 = 10401000;
pub const CRYSTAL_BURST: i32 = 10409000;
pub const SHATTER_EARTH: i32 = 10410000;
pub const ROCK_BLASTER: i32 = 10411000;
pub const GAVEL: i32 = 10412000;
pub const FOUNDING_RAIN: i32 = 10421000;
pub const STARS_OF_RUIN: i32 = 10422000;
pub const FULL_MOON: i32 = 10436000;
pub const DARK_MOON: i32 = 10436100;
pub const CANNON_CHARGED: i32 = 10408050;
pub const GLINT_ARC: i32 = 10407000;
pub const ICECRAG: i32 = 10440000;
pub const ZAMOR: i32 = 10441000;
pub const SPIRIT_BOMB: i32 = 10470105; // Meteorite of Astel's projectile alone (its emitter freezes)
pub const ROCK_SLING: i32 = 10471000;
pub const GRAVITY_WELL: i32 = 10472000;
pub const COLLAPSING_STARS: i32 = 10472100;
pub const GHOSTFLAME: i32 = 10501000;
/// 1.2.x: Ghostflame with its pale fog at 70% opacity (user), Asta's Black Meteorite (strip_fx.json bullet_new)
pub const GHOSTFLAME_SOFT: i32 = 10501095;
pub const OBUBBLE: i32 = 10511000;
pub const FLAME_SLING: i32 = 10601000;
pub const GIANTSFLAME: i32 = 10610000;
pub const FELL_GOD: i32 = 10611000;
pub const BLACK_FLAME: i32 = 10621000;
pub const BLACK_FLAME_RITUAL: i32 = 10625000;
pub const TALONS: i32 = 10630000;
pub const REJECTION: i32 = 10640000;
pub const WRATH_OF_GOLD: i32 = 10641000;
pub const TRIPLE_RINGS: i32 = 10670100;
pub const LIGHTNING_STRIKE: i32 = 10692000;
pub const ADL_STRIKE: i32 = 10691000;
pub const FROZEN_SPEAR: i32 = 10692100;
pub const BLACK_BLADE: i32 = 10753000;
pub const FORTISSAX: i32 = 10694100;
pub const FIRES_SIN: i32 = 10790000;
pub const EXPLOSIVE_STONE: i32 = 10183000;
pub const EXPLOSIVE_CLUMP: i32 = 10183100;
pub const STORM_BLADE: i32 = 2060;
pub const VACUUM_SLICE: i32 = 2062;
pub const THUNDERBOLT: i32 = 2080;
pub const LIGHTNING_SLASH: i32 = 2095;
pub const GRAVITAS: i32 = 2190;
pub const HOARFROST: i32 = 2260;
pub const STORM_STOMP: i32 = 2280;
pub const DESTINED_DEATH: i32 = 2320;
pub const REDMANES: i32 = 2570;
pub const STORMCALLER: i32 = 2640;
pub const WAVES_OF_DARKNESS: i32 = 2810;
pub const ONYX_REPULSION: i32 = 3000;
pub const WAVE_OF_DESTRUCTION: i32 = 3006;
pub const ZAMOR_STORM: i32 = 3055;
/// 1.2.0: the combo trees' strike: an invisible hitbox half a metre round that moves 0.2 m and is gone
/// in 0.2 s (the last link of Detroit Smash's chain, no effect of its own): a punch or kick reaches only
/// who stands right in front
pub const STRIKE_HIT: i32 = 2098;
/// 1.3.0: Super Spirit Bomb (strip_fx.json 9990640..42): the blue-white sphere held above him, thrown, and its burst (its
/// child, one hit 5 m round)
pub const SPIRIT_BOMB_GATHER: i32 = 9990640;
/// 1.3.0: United States of Smash's tornado; Howitzer Impact's fire tornado and blast (strip_fx.json 9990650..52)
pub const SMASH_TORNADO: i32 = 9990650;
pub const HOWITZER_TORNADO: i32 = 9990651;
pub const HOWITZER_BLAST: i32 = 9990652;
pub const HOWITZER_BIG: i32 = 9990653;
/// 1.3.0: a ground shatter in rock colours (Shatter Earth without its Carian glow, strip_fx.json 9990654)
pub const GROUND_SHATTER: i32 = 9990654;
/// 1.3.0: DIO's road roller: falling, and where it lands (strip_fx.json 9990632, 9990631)
pub const ROAD_ROLLER_FALL: i32 = 9990632;
pub const ROAD_ROLLER: i32 = 9990631;
/// 1.3.0: the time stop's sphere (strip_fx.json 9990740/1: Rejection's ripple, Wrath of Gold's gold; no damage of their own)
pub const TIME_DOME: i32 = 9990740;
pub const TIME_DOME_GOLD: i32 = 9990741;
pub const SPIRIT_BOMB_THROWN: i32 = 9990641;
/// 1.3.0: Killua's electricity on a hit: the Lightning Pot's burst in his blue-white (strip_fx.json 9990630); no damage of its own
pub const KILLUA_ZAP: i32 = 9990630;
/// 1.3.0: the researched new heroes' effects (strip_fx.json 9990701..18; fx_copies.json)
pub const GETSUGA: i32 = 9990701;
pub const GETSUGA_BANKAI: i32 = 9990702;
pub const MUGETSU: i32 = 9990703;
pub const GREAT_FIREBALL: i32 = 9990704;
pub const INDRAS_ARROW: i32 = 9990706;
/// 1.3.1: Sasuke's Kirin: a huge blue-white bolt from above (strip_fx.json 9990734, fx_copies.json 900700061/2)
pub const KIRIN: i32 = 9990734;
/// 1.3.1: Yami's Black Moon's pull: Gravity Well with its swirl in black (strip_fx.json 9990735; the user: black, not purple)
pub const BLACK_PULL: i32 = 9990735;
/// 1.3.1: Gojo's Blue fired like Red: the blue sphere and its burst (strip_fx.json 9990736/7)
pub const GOJO_BLUE_SHOT: i32 = 9990736;
/// 1.3.1: Yuji's Piercing Blood: a thin blood-red beam (strip_fx.json 9990738)
pub const PIERCING_BLOOD: i32 = 9990738;
/// 1.3.1 round 2: DIO's Vaporization Freezing: the Freezing Pot's burst, 2 m of frost with real frost build-up (round 3, the user:
/// its sound "plays on repeat long after the attack ends": our copy, its frost effect without the sound: strip_fx.json)
pub const FREEZING_TOUCH: i32 = 9990747;
/// 1.3.1: Gon's Scissors' cut: a yellow-orange crescent, look only (strip_fx.json 9990745)
pub const GON_CUT: i32 = 9990745;
pub const BLACK_MOON: i32 = 9990708;
pub const GOJO_BLUE: i32 = 9990709;
pub const GOJO_RED: i32 = 9990710;
pub const GOJO_PURPLE: i32 = 9990712;
pub const BLACK_FLASH: i32 = 9990714;
pub const GON_BURST: i32 = 9990715;
pub const GON_PAPER: i32 = 9990716;
pub const GON_SCISSORS: i32 = 9990717;
pub const KILLUA_BOLT: i32 = 9990718;
/// 1.3.0 round 5 (strip_fx.json 9990720..24, fx_copies.json 900700045..49): Deku's green lightning on a hit and its big
/// impact, Stun Grenade's white flash, Jet Kindling's white-hot burst, Black Divider's giant black blade; no damage of their own
pub const DEKU_ZAP: i32 = 9990720;
pub const DEKU_SMASH: i32 = 9990721;
pub const STUN_FLASH: i32 = 9990722;
/// 1.3.1 round 2: Stun Grenade's blast, Howitzer Impact's look 3 m round (strip_fx.json)
pub const STUN_BLAST: i32 = 9990746;
pub const WHITE_HOT: i32 = 9990723;
pub const BLACK_DIVIDER: i32 = 9990724;
/// TBB Rasen Shuriken's dark purple burst and its dark core flying in the rings (9990725 / 26)
pub const TBB_BURST: i32 = 9990725;
pub const TBB_CORE: i32 = 9990726;
/// (the user, Oct 17) the Triple Rings of Light in Rasengan blue (9990727..30), and the burst: a 4x ball of spinning air and its
/// whirling wind (9990731 / 32)
pub const TBB_RINGS: i32 = 9990727;
pub const AIR_BALL: i32 = 9990731;
pub const AIR_WIND: i32 = 9990732;
pub const ZORO_TORNADO: i32 = 9990719;
/// 1.3.0: Luffy's stretching fist (strip_fx.json 9990611..18, tools_scripts/fist/make_fist.sh): his fist and forearm, a lit
/// model that shoots out from where it is fired along its aim and snaps back in 0.16 s, one per reach; it hurts no one (a
/// STRIKE_HIT lands the blow). (metres, bullet)
pub const RUBBER_FISTS: [(f32, i32); 8] =
    [(1.0, 9990611), (1.5, 9990612), (2.0, 9990613), (2.5, 9990614), (3.0, 9990615), (3.5, 9990616), (4.0, 9990617), (5.0, 9990618)];
/// 1.3.0: Grizzly Magnum's fists (strip_fx.json 9990621..28): giant Armament Haki fists on Gear 3 arms, held out 0.4 s
pub const HAKI_FISTS: [(f32, i32); 8] =
    [(1.5, 9990621), (2.0, 9990622), (2.5, 9990623), (3.0, 9990624), (3.5, 9990625), (4.0, 9990626), (5.0, 9990627), (6.0, 9990628)];
/// 1.3.0: King Kong Gun's fist (strip_fx.json 9990661..66): a 2.2 m haki fist on an inflated arm, held out 0.5 s
pub const KING_KONG_FISTS: [(f32, i32); 8] =
    [(2.0, 9990661), (3.0, 9990662), (4.0, 9990663), (5.0, 9990664), (6.0, 9990665), (7.0, 9990666), (7.0, 9990666), (7.0, 9990666)];
/// Step::RubberFist's sets: 0 his own (Gatling), 1 giant haki (Grizzly Magnum), 2 King Kong Gun
pub const FIST_SETS: [&[(f32, i32); 8]; 3] = [&RUBBER_FISTS, &HAKI_FISTS, &KING_KONG_FISTS];
/// 1.2.0: Explosive Stone's burst itself (its hit bullet: 1.6 m, gone in 0.07 s): an explosion right
/// where it is spawned (Bakugo's palms), not where a thrown stone lands
pub const BLAST: i32 = 10183001;
/// the clump's burst, the same at once
pub const EXPLOSIVE_CLUMP_HIT: i32 = 10183101;

// 0.10.0: Elden Ring Ashes of War the bare hands play for a move's body (SwordArtsParam rows; anim.rs
// Cue::Arts). Chosen in the lab (lab/anims/arts2-crop*.png): they look right on empty hands.
pub(crate) const LIONS_CLAW: i32 = 100; // a forward flip into a slam (Manchester Smash)
pub(crate) const GIANT_HUNT: i32 = 116; // a scooping uppercut that throws them up
pub(crate) const REPEATING_THRUST: i32 = 109; // a rapid barrage of jabs
pub(crate) const KICK: i32 = 503;
pub(crate) const LIGHTNING_RAM: i32 = 504; // a lightning shoulder charge
pub(crate) const HOARFROST_STOMP: i32 = 501; // an icy stomp
pub(crate) const WAR_CRY: i32 = 651; // a roaring power-up
pub(crate) const BLOODHOUND_STEP: i32 = 801; // a vanishing step
pub(crate) const SPINNING_SLASH: i32 = 103;
pub(crate) const WOLFS_ASSAULT: i32 = 1025; // a spinning leap down onto them
// from Shadow of the Erdtree (the base-game move stands in without the DLC)
pub(crate) const STORM_KICK: i32 = 1190;
pub(crate) const DRYLEAF_WHIRLWIND: i32 = 2000; // flying spinning kicks
pub(crate) const PALM_BLAST: i32 = 4010;
pub(crate) const RAGING_BEAST: i32 = 4060; // a flipping flurry
pub(crate) const BLINKBOLT: i32 = 4130; // a lightning-fast dash
// 1.2.0 (combo trees), from the 0.10.0 audition (lab/scripts/arts2.txt)
pub(crate) const LIFESTEAL_FIST: i32 = 205; // a driving straight punch
pub(crate) const GROUND_SLAM: i32 = 506; // both fists brought down on the ground
pub(crate) const RAPTOR: i32 = 802; // a spring straight up
pub(crate) const STORM_ASSAULT: i32 = 122; // a leaping strike down
pub(crate) const SAVAGE_LIONS_CLAW: i32 = 4150; // two flips into slams (DLC)
pub(crate) const BLIND_SPOT: i32 = 4090; // a sidestep behind them (DLC)
// 1.3.1 round 2 (the user: Kirin used the arm-pumping emote, "I need you to remove that"): the blade raised to the sky (Ash of War:
// Thunderbolt, EquipParamGem 138 -> SwordArtsParam 216), a real attack
pub(crate) const THUNDERBOLT_RAISE: i32 = 216;


pub(crate) fn aow(row: i32) -> Step {
    let dlc = crate::weapon::dlc();
    let row = match row {
        STORM_KICK if !dlc => KICK,
        DRYLEAF_WHIRLWIND if !dlc => SPINNING_SLASH,
        PALM_BLAST if !dlc => GIANT_HUNT,
        RAGING_BEAST if !dlc => REPEATING_THRUST,
        BLINKBOLT if !dlc => LIGHTNING_RAM,
        SAVAGE_LIONS_CLAW if !dlc => LIONS_CLAW,
        BLIND_SPOT if !dlc => BLOODHOUND_STEP,
        r => r,
    };
    Act(Cue::Arts(row))
}

pub struct Script {
    pub steps: Vec<Step>,
}

/// The body's motion for the next part of a move (anim.rs).
/// 1.3.1: how long a katana cut takes to land after the swing starts (Carian Combo Warriors' katana light and heavy attacks,
/// lab sasuke1): the hit and its effect wait for the blade
const CUT_LIGHT: f32 = 0.3;
const CUT_HEAVY: f32 = 0.55;
/// 1.3.1: how long Gon stays in his adult form after it comes out
const ADULT_SECS: f32 = 15.0;
/// Lion's Claw's slam lands this long after it starts (lab zoro1)
const TORA_GARI_LAND: f32 = 0.85;

pub(crate) fn act(c: Cue) -> Step {
    Act(c)
}

/// The sequence for ability `id`. Ultimates are long, cinematic and the player can't be hurt during them.
/// Every step that strikes, casts or powers up first plays a matching Elden Ring animation (`act`);
/// moves through the air play the game's roll, jump and plunge on their own (combat.rs).
/// 1.2.0 lab (script "proto on"): the abilities' Carian Combo Warriors versions waiting for the user's
/// approval (proto()).
pub static LAB_PROTO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Weapons whose Carian Combo Warriors moves the abilities borrow (EquipParamWeapon rows): the twinblade
/// (its fire set), the Longsword (Asta's own straight sword), the Dismounter (curved greatsword:
/// lightning and red orbs), the empty hand (the fists).
const TWINBLADE: u32 = 10_000_000;
const LONGSWORD: u32 = 2_000_000;
const DISMOUNTER: u32 = 8_020_000;
const FISTS: u32 = crate::weapon::UNARMED;
/// Their heavy variants: Special 1 / 2, type 0 / 1 (AttackRightHeavySpecialType), charged (SubStart).
const HEAVY_SPECIAL1: &str = "SetVariable(\"AttackRightHeavySpecialType\", 0) ExecEventAllBody(\"W_AttackRightHeavySpecial1Start\")";
const HEAVY_SPECIAL1_CHARGED: &str = "SetVariable(\"AttackRightHeavySpecialType\", 0) ExecEventAllBody(\"W_AttackRightHeavySpecial1SubStart\")";
const HEAVY_SPECIAL2_B: &str = "SetVariable(\"AttackRightHeavySpecialType\", 1) ExecEventAllBody(\"W_AttackRightHeavySpecial2Start\")";
/// 1.3.0 (the second rework round): the rest of the specials the abilities borrow, by their lab-reel names
/// (rsp / bsp: right / both hands; 1 / 2: Special 1 / 2; t0 / t1: the type; c: charged)
const HEAVY_SPECIAL1_B: &str = "SetVariable(\"AttackRightHeavySpecialType\", 1) ExecEventAllBody(\"W_AttackRightHeavySpecial1Start\")"; // rsp1t1
const HEAVY_SPECIAL1_CHARGED_B: &str = "SetVariable(\"AttackRightHeavySpecialType\", 1) ExecEventAllBody(\"W_AttackRightHeavySpecial1SubStart\")"; // rsp1t1c
const BOTH_SPECIAL1_CHARGED_B: &str = "SetVariable(\"AttackRightHeavySpecialType\", 1) ExecEventAllBody(\"W_AttackBothHeavySpecial1SubStart\")"; // bsp1t1c
/// 1.3.0: the fists' second heavy, type 1: a deep lunge with both arms driven forward (Grizzly Magnum's body)
const RIGHT_HEAVY2_B: &str = "SetVariable(\"AttackRightHeavySpecialType\", 1) ExecEventAllBody(\"W_AttackRightHeavy2Start\")";
/// 1.3.0: the paired fists' second light attack: a rapid flurry whose fists blur into spiralling wind (Gum-Gum Gatling's body)
const DUAL_LIGHT2: &str = "ExecEventAllBody(\"W_AttackDualLight2\")";
// 1.3.1 round 2 (the user: Ichigo's sword gets the katana moveset, "do not change his attacks at all"): his abilities keep the
// Bastard Sword's greatsword swings they were made with (JR_Strike's three, on that row for the move)
const ZANGETSU_GREATSWORD: u32 = 3_000_000;
const R_LIGHT1: &str = "ExecEventAllBody(\"W_AttackRightLight1\")";
const R_LIGHT2: &str = "ExecEventAllBody(\"W_AttackRightLight2\")";
const R_HEAVY1: &str = "ExecEventAllBody(\"W_AttackRightHeavy1Start\")";
/// 1.3.1: the straight sword's backstep attack (Carian Combo Warriors' Deep Stinger: a long lunging thrust)
const BACKSTEP_THRUST: &str = "ExecEventAllBody(\"W_AttackRightBackstep\")";
const BOTH_SPECIAL2_B: &str = "SetVariable(\"AttackRightHeavySpecialType\", 1) ExecEventAllBody(\"W_AttackBothHeavySpecial2Start\")"; // bsp2t1
/// the two-handed heavy special, War Cry's kind (type 1; a024_032620; Carian Combo Warriors' twinblade):
/// a flaming spin that bursts into a great explosion of fire, a fireball hanging after it (Todoroki's
/// Dancing Flame; the user's pick). Type 0 (032600/032605) is a leaping slash with a red streak, not it.
/// No R2 is held, so its charge window is held open (JR_HoldSpecial) or it would cut to its quick end
const BOTH_HEAVY_FIRE: &str = "JR_HoldSpecial(24) SetVariable(\"AttackRightHeavySpecialType\", 1) ExecEventAllBody(\"W_AttackBothHeavySpecial1Start\")";
/// a falling heavy attack (their jump attack, the way their script starts one)
const FALLING_HEAVY: &str = "SetVariable(\"JumpAttackForm\", 2) SetVariable(\"JumpAttackFormRequest\", 1) SetVariable(\"JumpAttack_HandCondition\", 0) SetVariable(\"JumpAttack_Land\", 0) ExecEventAllBody(\"W_JumpAttack_Start_Falling\")";

/// 1.2.0: abilities built on Carian Combo Warriors' moves, for the user to try before they replace the old ones.
/// Lab ("provar <n>"): which prototype variant proto() plays where an ability has several.
pub static LAB_PROTO_VARIANT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
/// Carian Combo Warriors bullets nothing in the shipped movesets fires (their Ash of War rework's): candidates
/// for abilities (FINDINGS-1.2.0 section 11)
/// Goku's beam: Air Comets without their pillar hit, with the orb's speed and homing so they trail its path, and the orb
/// slowed so the beam lasts its flight
const CCW_AIR_COMET: i32 = 2100061;
/// 1.3.0: Carian Combo Warriors' Bomb Punch (the fists' two-handed counter): one punch and a storm of purple-white
/// lightning and rock 20 m across (its bullets 2100025 / 2100026)
const BOMB_PUNCH: &str = "ExecEventAllBody(\"W_AttackBothHeavyCounter\")";
const CCW_FULL_MOON_SLOW: i32 = 1009002670;
pub(crate) const CCW_FULL_MOON: i32 = 1009002680; // CCW's Rennala's Full Moon orb at 20 m/s, its impact burst torso-sized (from 1009002660) (the user: "It's okay if it has the impact burst")

fn proto(id: &str) -> Option<Vec<Step>> {
    use Cue::{Cast, FlyingKick, Heavy, Kick, Pitch, Strike, Strike2};
    let variant = LAB_PROTO_VARIANT.load(std::sync::atomic::Ordering::Relaxed);
    Some(match id {
        // Detroit Smash: their heavy special punch, the shockwave out of it
        "allmight_detroit_smash" => vec![
            go(Dest::Target { gap: 1.5 }, 0.12),
            Weapon { row: FISTS, lua: HEAVY_SPECIAL1, secs: 2.6 },
            Wait(0.55), fire(WAVE_OF_DESTRUCTION, At::Hand, Aim::Forward), fire(ONYX_REPULSION, At::Hand, Aim::Forward),
            fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.1), Wait(0.5),
        ],
        // Raging Strike: Instant Transmission behind them, then their charged punch that bursts in sparks
        "goku_raging_strike" => vec![
            Aura(true), aow(BLOODHOUND_STEP), Wait(0.12), go(Dest::Through { past: 1.6, side: 0.0 }, 0.08),
            Weapon { row: FISTS, lua: HEAVY_SPECIAL1_CHARGED, secs: 2.4 },
            Wait(0.45), fire(ROCK_BLASTER, At::Ahead(1.2), Aim::Back), fire(STRIKE_HIT, At::Target, Aim::Forward),
            HitStop(0.12), Wait(0.5), Aura(false),
        ],
        // ---- 1.3.0 rework, second round (the user: the first options "look super rigid", "rework it with CCW moves
        // that fit them like we did to Todoroki's dancing flame", and no Awakening that flings him straight up to
        // slam down). Each is one whole Carian Combo Warriors move, its own effects; our unseen strikes land with
        // its blast (timed from the lab reels, lab/scripts/ccwsp_*.txt) so the ledger makes it the listed damage
        // ---- 1.3.0 round 5: the original fighters' moves, researched (the user, 2026-10-17 night: "make everything true to the shows
        _ => return None,
    })
}

pub fn script(id: &str) -> Option<Script> {
    use Cue::*;
    let proto_steps = if LAB_PROTO.load(std::sync::atomic::Ordering::Relaxed) { proto(id) } else { None };
    let steps = if let Some(p) = proto_steps { p } else { match id {
        // ---- Goku: blue ki and impossible speed
        "goku_kamehameha" => vec![
            // 1.2.x (user's picks): the palm thrust with the blue ball charging in his right palm, then at the
            // shove's full extension (0.85 s) the Full Moon orb, slowed, with a beam of Air Comets behind it
            // for its whole flight, until it explodes on them
            aow(PALM_BLAST), PalmFx(true), Wait(0.85), PalmFx(false),
            fire(CCW_FULL_MOON_SLOW, At::Hand, Aim::AtTarget), Beam { b: CCW_AIR_COMET, min: 2.0 }, streams(CCW_AIR_COMET, At::Hand, Aim::AtTarget, 8, 0.07, 0.0), HitStop(0.08),
        ],
        // (do your research) and keep the combat fluid"; no straight-up lifts and slams)
        "goku_meteor_combination" => vec![
            // (approved 2026-10-17) (Dragon Ball's Meteor Combination: a rush of blows that knocks the foe skyward and ends in a Kamehameha; his Instant
            // Transmission Kamehameha against Cell): a flipping flurry, a kick, an uppercut that knocks them up, then he vanishes,
            // reappears beside them and fires the Kamehameha into them
            Aura(true), go(Dest::Target { gap: 1.3 }, 0.14),
            aow(RAGING_BEAST), Step::Flash, burst(STRIKE_HIT, At::Target, Aim::Forward, 3, 0.12), Wait(0.24), Step::Flash,
            act(Kick), Wait(0.2), fire(STRIKE_HIT, At::Target, Aim::Forward), Step::Flash,
            aow(GIANT_HUNT), Wait(0.3), fire(STRIKE_HIT, At::Target, Aim::Forward), Step::Flash, Launch(2.5), Juggle(2.0),
            aow(BLOODHOUND_STEP), Wait(0.1), go(Dest::Through { past: 2.5, side: 2.2 }, 0.08), FaceTarget,
            aow(PALM_BLAST), PalmFx(true), Wait(0.85), PalmFx(false),
            fire(CCW_FULL_MOON, At::Hand, Aim::AtTarget), HitStop(0.1), Wait(0.3), Aura(false),
        ],
        "goku_raging_strike" => vec![
            // Instant Transmission behind the target, then a point-blank strike
            Aura(true), aow(BLOODHOUND_STEP), Wait(0.12), go(Dest::Through { past: 1.6, side: 0.0 }, 0.08), aow(PALM_BLAST),
            // (1.3.0: Rock Blaster's glintstone blue gone, the user's no-Carian-blue rule: an impact flash and the gold sparks)
            Step::Flash, fire(STRIKE_HIT, At::Target, Aim::Forward), fire(WRATH_OF_GOLD, At::Ahead(1.0), Aim::Back),
            HitStop(0.12), Aura(false),
        ],
        "goku_super_spirit_bomb" => vec![
            // (approved 2026-10-17) 1.3.0 (researched, Dragon Ball wiki: the energy of everyone gathers above him into a giant
            // sphere and he hurls it; the user: no flinging him into the air, no emote, then "the same throw animation as Naruto's
            // rasen shuriken"): the blue-white sphere (Rennala's Full Moon, recoloured, 4x) gathers over him while he draws back
            // for Elden Ring's spear throw and hurls it overhand; it leaves at the release (1.08 s), drifts down onto them and
            // bursts in blue-white light 9 m across
            Aura(true), Voice, FaceTarget, fire(SPIRIT_BOMB_GATHER, At::Above(5.0), Aim::Forward), act(Pitch), Wait(1.08),
            // (1.3.1, the user: it once did 0 damage on Margit: the burst is aimed at the chest of a big boss that moves; the blow
            // lands on them as it bursts, so the listed damage always comes)
            Lob { b: SPIRIT_BOMB_THROWN, at: At::Above(5.0) }, Wait(1.25), fire(STRIKE_HIT, At::Target, Aim::Forward), ring(STRIKE_HIT, At::Target, 6, 2.5),
            Wait(0.55), Aura(false),
        ],
        // ---- Naruto: rasengan spirals, wind and shadow clones
        "naruto_rasengan" => vec![
            // 1.2.x (user's picks): a palm thrust with a blue ball of spinning air charging in his right palm, and
            // at the shove's full extension (0.85 s) Carian Combo Warriors' Full Moon orb bursting from it
            aow(PALM_BLAST), PalmFx(true), Wait(0.85), PalmFx(false), fire(CCW_FULL_MOON, At::Hand, Aim::AtTarget), HitStop(0.08),
        ],
        "naruto_tbb_rasen_shuriken" => vec![
            // (approved 2026-10-17) (Naruto: the Rasenshuriken: spinning rings of wind round a core, bursting into a huge sphere of spinning wind blades)
            // (the user, Oct 17: "use the other triple rings of light attack that he used to have rather than a rennala moon ... color
            // the rings of light the same rasengan blue ... instead of a purple explosion ... a big spinning ball of air"): the spear
            // throw; the Triple Rings of Light in Rasengan blue, and where they reach a huge ball of spinning air with wind whirling
            // round it and a ring of wind blades
            Aura(true), act(Pitch), Wait(1.08), Aura(false),
            fire(TBB_RINGS, At::Hand, Aim::AtTarget), Wait(0.6),
            fire(STRIKE_HIT, At::Target, Aim::Forward), ring(STRIKE_HIT, At::Target, 6, 1.5),
            fire(AIR_BALL, At::Target, Aim::Forward), fire(AIR_WIND, At::Target, Aim::Forward), ring(VACUUM_SLICE, At::Target, 8, 2.5),
            HitStop(0.2), Wait(1.0),
        ],
        "naruto_uzumaki_barrage" => vec![
            // Naruto Uzumaki Barrage: his shadow clones kick from every side (U-ZU-MA-KI), kick the enemy
            // up, and Naruto drops on them from above with a heel kick
            // (1.3.1, the user: back to this, the old version, from before the 2026-10-17 rework)
            go(Dest::Target { gap: 1.2 }, 0.14), aow(STORM_KICK), fire(STORM_BLADE, At::Ahead(1.0), Aim::Forward),
            go(Dest::Through { past: 1.4, side: 1.6 }, 0.1), act(FlyingKick), fire(VACUUM_SLICE, At::Target, Aim::AtTarget),
            go(Dest::Through { past: 1.4, side: -1.6 }, 0.1), aow(DRYLEAF_WHIRLWIND), fire(VACUUM_SLICE, At::Target, Aim::AtTarget),
            go(Dest::Target { gap: 1.0 }, 0.08), aow(GIANT_HUNT), fire(STORM_STOMP, At::Target, Aim::Forward),
            go(Dest::Up { height: 3.6, toward: 0.9 }, 0.2), Hover(0.12),
            leap(Dest::Down, 0.16, 0.0), fire(GAVEL, At::Ahead(0.8), Aim::Down), fire(SHATTER_EARTH, At::Feet, Aim::Forward),
            HitStop(0.1),
        ],
        "naruto_shadow_clone" => vec![
            // (the user, 2026-10-17: the Tailed Beast Bomb becomes a shadow clone that uses his three skills at half damage, with
            // half his health; clones.rs) Shadow Clone Jutsu: his chakra flares, and a clone of him appears beside him in a
            // puff of smoke and fights at his side
            Aura(true), Voice, FaceTarget, act(Cast), Wait(0.3), Step::Clone, Poof(At::Ahead(1.5)), Wait(0.6), Aura(false),
        ],
        "naruto_tailed_beast_bomb" => vec![
            // 1.3.0 (the user's pick, rework round 2): the curved greatsword's two-handed second special: a red sphere gathered in front of him bursts
            // outward, and he springs back out of it
            Aura(true), Voice, FaceTarget,
            Weapon { row: DISMOUNTER, lua: BOTH_SPECIAL2_B, secs: 3.0 },
            Wait(1.2), ring(STRIKE_HIT, At::Ahead(2.5), 6, 1.5), ring(STRIKE_HIT, At::Ahead(2.5), 10, 3.5),
            fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.2),
            Wait(1.4), Aura(false),
        ],
        // ---- Luffy: stretching fists, fire and Gear Fourth
        "luffy_red_hawk" => vec![
            // 1.3.0 (the user's pick, rework round 2): the twinblade's second special: a coiled lunge that tears a streak of fire through them
            FaceTarget,
            Weapon { row: TWINBLADE, lua: HEAVY_SPECIAL2_B, secs: 2.4 },
            Wait(0.95), burst(STRIKE_HIT, At::Line(1.5, 2.0), Aim::Forward, 4, 0.0), fire(STRIKE_HIT, At::Target, Aim::Forward),
            HitStop(0.12), Wait(1.0),
        ],
        "luffy_grizzly_magnum" => vec![
            // (approved) 1.3.0 (researched, One Piece wiki: "activates Gear 3 on both his arms ... runs towards the target with both his
            // arms still static behind him before leaping ... implements Armament Haki into his arms, turning them black ...
            // launches his attack", a Bazooka: both together): he lunges in with both arms driven forward (the fists' second
            // heavy, type 1) and two giant haki-black fists (HAKI_FISTS: 90 cm fists on Gear 3 arms, real models) slam into
            // them together, the air blast behind them; forward only, never up
            go(Dest::Target { gap: 5.0 }, 0.14), FaceTarget,
            Weapon { row: FISTS, lua: RIGHT_HEAVY2_B, secs: 1.6 },
            Wait(0.22), RubberFist(1, 0.24, (0.2, 0.05)), RubberFist(1, -0.24, (-0.2, -0.05)),
            Wait(0.08), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(STRIKE_HIT, At::Target, Aim::Forward),
            fire(STORM_STOMP, At::Target, Aim::Forward), HitStop(0.15), Wait(0.8),
        ],
        "luffy_gatling_battle_axe" => {
            // 1.3.0 (user: "just gum gum Gatling", as in the show: "a barrage of punches so fast that it looks like there
            // are multiple hands"; "create or find attacks to match", not drawings over them): his body throws Carian Combo
            // Warriors' paired-fists flurry (the user's pointer: its fists blur into spiralling wind), twice over, feet
            // planted; out of it a burst of real fists (RUBBER_FISTS: oversized fists on stretched arms, lit models) shoot
            // from alternating shoulders, about five out at once, fanned across and around the target like the anime's
            // many arms; every other one lands a blow
            // (1.3.1 round 2, the user: Repeating Thrust "only punches with one arm" while both stretchy arms fire: Carian Combo
            // Warriors' paired-fists flurry back, twice, its shoulders square to the arms)
            let mut v = vec![go(Dest::Target { gap: 3.6 }, 0.12), FaceTarget, Plant(1.8), Weapon { row: FISTS, lua: DUAL_LIGHT2, secs: 2.0 }];
            for k in 0..24u32 {
                if k == 13 {
                    v.push(Weapon { row: FISTS, lua: DUAL_LIGHT2, secs: 1.2 });
                }
                // a golden-angle spiral: spots spread evenly over a disc 0.35..0.9 m out round their chest
                let a = k as f32 * 2.399_963;
                let r = 0.35 + 0.55 * ((k * 7 % 24) as f32 / 23.0);
                // alternating his right shoulder and his left (user)
                let side = if k % 2 == 0 { 0.2 } else { -0.2 };
                v.push(RubberFist(0, side, (r * a.cos(), r * a.sin() * 0.8)));
                if k % 2 == 0 {
                    v.push(fire(STRIKE_HIT, At::Target, Aim::Forward));
                }
                v.push(Wait(0.055));
            }
            v.push(HitStop(0.1));
            v
        }
        "luffy_gear_fourth_attack" => vec![
            // (approved) 1.3.0 round 3 (researched, One Piece wiki; the user: Gear Fourth "needs to alter his appearance"): he bites his
            // arm and blows himself up into Boundman: his body swaps to the Boundman model (huge upper body and arms,
            // haki-black with crimson flame lines: work/rigport/luffy_gear4.py) and he grows; King Kong Gun: he springs at
            // them and his arm, blown up giant, fires a 2.2 m haki fist into them (KING_KONG_FISTS, a real model), the
            // shock wave and rubble after; then he deflates. No leap into the sky
            Aura(true), Voice, act(PowerUp), Gear4(true), Grow(1.5), Wait(0.9),
            go(Dest::Target { gap: 4.5 }, 0.18), FaceTarget,
            Weapon { row: FISTS, lua: HEAVY_SPECIAL1_CHARGED_B, secs: 2.2 },
            Wait(0.8), RubberFist(2, 0.3, (0.0, 0.1)), Wait(0.08), fire(STRIKE_HIT, At::Target, Aim::Forward),
            ring(STORM_STOMP, At::Target, 6, 3.0), fire(STORM_STOMP, At::Target, Aim::Forward), fire(GROUND_SHATTER, At::Target, Aim::Forward),
            HitStop(0.35), Wait(1.1), Grow(1.0), Gear4(false), Aura(false),
        ],
        // ---- Deku: One For All, green lightning
        "deku_detroit_smash" => vec![
            aow(BLINKBOLT), go(Dest::Target { gap: 1.3 }, 0.12), act(Heavy), fire(LIGHTNING_SLASH, At::Hand, Aim::Forward),
            fire(FORTISSAX, At::Ahead(1.3), Aim::Forward), fire(ONYX_REPULSION, At::Hand, Aim::Forward), HitStop(0.08),
        ],
        // 1.2.0 (user: "a weird blue projectile"): a flick of pure air pressure, white wind only (the
        // Frozen Lightning Spear it fired was a blue bolt)
        "deku_delaware_smash" => vec![act(Cast), fire(STORM_BLADE, At::Hand, Aim::AtTarget), burst(VACUUM_SLICE, At::Hand, Aim::AtTarget, 2, 0.08),
            Wait(0.15), fire(STORM_STOMP, At::Target, Aim::Forward)],
        "deku_shoot_style" => vec![
            // (approved 2026-10-17) (MHA: Full Cowl: Shoot Style: springing kicks off the ground and the air in his green lightning, and the Manchester
            // Smash: a forward somersault that drives his heel down on them): the kicks as before, green, then the finisher
            // (the user, Oct 17: Lion's Claw "looks like he is doing a two handed slam rather than a kick"): his flying kick into them
            go(Dest::Target { gap: 1.2 }, 0.11), act(FlyingKick), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(DEKU_ZAP, At::Target, Aim::Forward),
            leap(Dest::Through { past: 1.8, side: 1.8 }, 0.16, 1.2), act(FlyingKick), fire(STORM_BLADE, At::Target, Aim::AtTarget), fire(DEKU_ZAP, At::Target, Aim::Forward),
            leap(Dest::Through { past: 1.8, side: -1.8 }, 0.16, 1.2), aow(STORM_KICK), fire(VACUUM_SLICE, At::Target, Aim::AtTarget), fire(DEKU_ZAP, At::Target, Aim::Forward),
            go(Dest::Target { gap: 3.0 }, 0.1), FaceTarget, act(FlyingKick), go(Dest::Target { gap: 0.9 }, 0.28), Wait(0.08),
            fire(STRIKE_HIT, At::Target, Aim::Forward), fire(DEKU_SMASH, At::Target, Aim::Forward), fire(GROUND_SHATTER, At::Ahead(1.5), Aim::Forward),
            HitStop(0.12), Wait(0.4),
        ],
        "deku_delaware_detroit_smash" => vec![
            // (approved 2026-10-17) (MHA: Delaware Detroit Smash: a Delaware flick's air pressure launches him, and he lands one full-power Detroit Smash,
            // green lightning tearing round him; against Muscular it left a crater): the flick, he rockets in, the fists' charged
            // punch; a green lightning burst at the contact and the ground broken in a ring round them
            Aura(true), Voice, FaceTarget, act(Cast), fire(STORM_BLADE, At::Hand, Aim::AtTarget), burst(VACUUM_SLICE, At::Hand, Aim::AtTarget, 2, 0.08), Wait(0.3),
            fire(STORM_STOMP, At::Feet, Aim::Back), go(Dest::Target { gap: 1.6 }, 0.12), FaceTarget,
            Weapon { row: FISTS, lua: HEAVY_SPECIAL1_CHARGED, secs: 2.4 },
            burst(DEKU_ZAP, At::Feet, Aim::Forward, 3, 0.12), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(DEKU_SMASH, At::Target, Aim::Forward),
            fire(STORM_STOMP, At::Target, Aim::Forward), fire(GROUND_SHATTER, At::Target, Aim::Forward), ring(GROUND_SHATTER, At::Target, 5, 2.2),
            HitStop(0.3), Wait(0.6), Aura(false),
        ],
        "deku_black_whip" => vec![
            // the tendril yanks him in, and he lands a Shoot Style flying kick
            act(Whip), go(Dest::Target { gap: 1.2 }, 0.22), act(FlyingKick), fire(LIGHTNING_SLASH, At::Ahead(1.0), Aim::Forward),
            fire(STORM_BLADE, At::Target, Aim::AtTarget), HitStop(0.08),
        ],
        "luffy_gum_gum_rocket" => vec![
            // 0.14.3: his stretched arm grabs the enemy and yanks him in for a Gum-Gum Pistol
            act(LuffySwing), go(Dest::Target { gap: 1.3 }, 0.22), act(Pistol), fire(STORM_BLADE, At::Target, Aim::AtTarget),
            fire(FELL_GOD, At::Ahead(1.2), Aim::Forward), HitStop(0.08),
        ],
        // ---- All Might: the Symbol of Peace
        "allmight_detroit_smash" => vec![
            go(Dest::Target { gap: 1.5 }, 0.12), act(Heavy), fire(WAVE_OF_DESTRUCTION, At::Hand, Aim::Forward),
            fire(ONYX_REPULSION, At::Hand, Aim::Forward), HitStop(0.1),
        ],
        "allmight_plus_ultra_barrage" => vec![
            // (approved 2026-10-17) (MHA: against the Nomu he punches past his limit, "Plus Ultra!", every blow a shock wave; Texas Smash: a straight punch
            // whose wind pressure blasts the foe away): the jabs, the cross-chop and uppercut as before, then the Texas Smash: the
            // fists' heavy special punch and its blast of wind
            go(Dest::Target { gap: 1.3 }, 0.12), aow(REPEATING_THRUST), burst(STRIKE_HIT, At::Target, Aim::Forward, 4, 0.12), Step::Flash,
            fan(STORM_BLADE, At::Hand, Aim::Forward, 2, 0.0, 0.7),
            aow(GIANT_HUNT), Wait(0.3), fire(WAVE_OF_DESTRUCTION, At::Hand, Aim::Forward),
            go(Dest::Target { gap: 1.5 }, 0.1), FaceTarget, Weapon { row: FISTS, lua: HEAVY_SPECIAL1, secs: 2.2 },
            Wait(0.55), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(WAVE_OF_DESTRUCTION, At::Hand, Aim::Forward), fire(ONYX_REPULSION, At::Hand, Aim::Forward),
            fire(STORM_BLADE, At::Hand, Aim::AtTarget), HitStop(0.14), Wait(0.5),
        ],
        "allmight_oklahoma_smash" => vec![aow(DRYLEAF_WHIRLWIND), fire(STORMCALLER, At::Feet, Aim::Forward), fire(STORM_STOMP, At::Feet, Aim::Forward), fire(WRATH_OF_GOLD, At::Feet, Aim::Forward)],
        "allmight_united_states_of_smash" => vec![
            // (approved) 1.3.0 round 3 (researched, MHA wiki: his last and strongest punch, driven down with "enough force to generate shock
            // waves that disrupt an entire city, creating a tornado when the target collides with the ground"; the user: no
            // flinging him into the sky): the fists' charged leaping punch drives into them; where it lands, shock waves race
            // out across the ground and a tornado tears up from the spot (Stormcaller's whirlwind, 2x)
            Aura(true), Voice, FaceTarget,
            Weapon { row: FISTS, lua: HEAVY_SPECIAL1_CHARGED_B, secs: 2.2 },
            Wait(0.85), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(SMASH_TORNADO, At::Target, Aim::Forward),
            ring(STORM_STOMP, At::Target, 6, 2.5), fire(STORM_STOMP, At::Target, Aim::Forward),
            HitStop(0.35), Wait(1.0), Aura(false),
        ],
        // ---- Bakugo: explosions
        "bakugo_ap_shot_autocannon" => vec![act(Cast), fan(EXPLOSIVE_STONE, At::Hand, Aim::AtTarget, 6, 0.08, 0.06)],
        "bakugo_burst_speed" => vec![
            // explosion-propelled: blast in, a right hook that explodes, rocket over their head, blast them
            // from behind and finish with a point-blank explosion
            fire(EXPLOSIVE_STONE, At::Feet, Aim::Back), go(Dest::Target { gap: 1.4 }, 0.16),
            aow(RAGING_BEAST), fire(EXPLOSIVE_CLUMP, At::Ahead(1.0), Aim::Forward),
            fire(EXPLOSIVE_STONE, At::Feet, Aim::Down), leap(Dest::Through { past: 2.2, side: 0.0 }, 0.28, 2.6),
            act(Strike2), fire(EXPLOSIVE_STONE, At::Target, Aim::AtTarget),
            act(Heavy), fire(EXPLOSIVE_CLUMP, At::Target, Aim::AtTarget), HitStop(0.1),
        ],
        "bakugo_stun_grenade" => vec![
            // (approved 2026-10-17) (MHA: Stun Grenade: an explosion from his palms that is a blinding white flash, leaving the foe unable to see)
            // (the user, Oct 17: the first one was "underwhelming for how intense his character is", and never that emote for an
            // attack): he lunges with both palms driven forward (the fists' second heavy), a huge white flash bursts from them with
            // his explosions all round it and a blast at the contact; everyone close by stands blinded (bosses half as long)
            FaceTarget, Weapon { row: FISTS, lua: RIGHT_HEAVY2_B, secs: 1.8 }, Wait(0.5),
            // (1.3.1 round 2, the user: it reached "way too far": a close-range blast about 6 m across: its blast 3 m round the palms'
            // flash, the blow on each enemy within 3 m of it (it was on the target wherever it stood), the stun within 4.5 m of him)
            fire(STUN_FLASH, At::Ahead(2.0), Aim::Forward), Step::Flash, fire(STUN_BLAST, At::Ahead(2.0), Aim::Forward),
            ring(EXPLOSIVE_CLUMP, At::Ahead(2.0), 8, 3.0), ring(STRIKE_HIT, At::Ahead(2.0), 8, 2.2), Stun(2.0), HitStop(0.25),
            Wait(0.8),
        ],
        "bakugo_howitzer_impact" => vec![
            // (approved 2026-10-17) 1.3.0 round 3 (researched, MHA wiki: he "dashes into the air", spins himself round on explosions from both hands
            // and "fires an explosive tornado at his opponent"; the user: no straight-up teleports; then "a big explosion in the
            // center of it when he makes contact" and Todoroki's Dancing Flame animation in the air on the way in): a blast at
            // his feet throws him in an arc at them while he spins in Dancing Flame's flaming whirl, a tornado of fire round them
            // as he arrives, and at the contact a huge explosion (the Fell God burst 3.2x) ringed with his blasts
            Aura(true), Voice, FaceTarget, fire(EXPLOSIVE_STONE, At::Feet, Aim::Down),
            Weapon { row: TWINBLADE, lua: BOTH_HEAVY_FIRE, secs: 3.0 }, leap(Dest::Target { gap: 1.0 }, 0.6, 2.4),
            fire(HOWITZER_TORNADO, At::Target, Aim::Forward), Wait(0.55),
            // (then: "I didn't see a big explosion ... on the first take but I saw it from a side view ... really short or it gets
            // cut off early": one 3.2x burst lasts about half a second, and centred on them its shell is round the camera
            // behind him, so it can't be seen. Now three of them back to back, centred 3 m beyond them, about 1.2 s of
            // explosion seen from anywhere, the 1.6x burst right at the contact and fire left burning. Then "it doesnt look
            // fiery enough": the burst was copied from the Fell God's travelling fireball (pink-white rings), so the centre is
            // now Giantsflame Take Thee's billowing explosion 4x and the contact blast 2x, both in Bakugo's orange)
            fire(HOWITZER_BLAST, At::Target, Aim::Forward), burst(HOWITZER_BIG, At::NearTarget(-3.0), Aim::Forward, 3, 0.35),
            ring(EXPLOSIVE_CLUMP, At::Target, 6, 3.0), fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.35),
            Wait(0.35), fire(HOWITZER_BLAST, At::NearTarget(-1.5), Aim::Forward), fire(FIRES_SIN, At::Target, Aim::Forward), Wait(1.0), Aura(false),
        ],
        // ---- Todoroki: half ice, half fire
        "todoroki_jet_burn" => vec![
            // Flashfire Fist: Jet Kindling (MHA wiki: his own ultimate move, not his father's: flames condensed into a white-hot point
            // on his fist, a punch, and on contact a violent explosion of flame bursts out of his arm): ice races along the ground to
            // pin them, he rides it in, and the fists' lunging blow ends in a white-hot burst with fire blasting out past them
            aow(HOARFROST_STOMP), burst(HOARFROST, At::Line(2.0, 2.5), Aim::Forward, 3, 0.07),
            go(Dest::Target { gap: 2.0 }, 0.18), FaceTarget, Weapon { row: FISTS, lua: HEAVY_SPECIAL1_B, secs: 1.8 },
            Wait(0.7), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(WHITE_HOT, At::Target, Aim::Forward),
            fire(HOWITZER_BLAST, At::NearTarget(-1.5), Aim::Forward), HitStop(0.2), Wait(0.5),
        ],
        "todoroki_dancing_flame" => vec![
            // 1.2.0 (the user's pick, from Carian Combo Warriors' twinblade, two-handed heavy): a flaming
            // spin that bursts into a great explosion of fire. From bare hands (no blade shown); our strike
            // lands with the burst so the ledger makes it the listed damage, and the script lasts as long
            // as the fire so its later hits count within it too
            // (1.3.1, the user: in the playtest it was "a spam kick with lightning": cast in the air (after a dive), the fall cut the
            // spin off after 0.4 s and the landing played instead; on the ground first, then the spin)
            Aura(true), leap(Dest::Drop, 0.12, 0.0), Wait(0.05),
            Weapon { row: TWINBLADE, lua: BOTH_HEAVY_FIRE, secs: 3.2 },
            // (the user: it hit no one) the burst reaches all round him when the fire explodes (1.2 s in):
            // unseen strikes in rings out to 5 m and on the target, the ledger making it the listed damage
            Wait(1.2), ring(STRIKE_HIT, At::Feet, 6, 1.5), ring(STRIKE_HIT, At::Feet, 10, 3.2), ring(STRIKE_HIT, At::Feet, 14, 5.0),
            fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.1),
            Wait(1.4),
            Aura(false),
        ],
        "todoroki_heaven_piercing_ice_wall" => vec![aow(HOARFROST_STOMP), burst(ICECRAG, At::Line(2.0, 2.5), Aim::Forward, 6, 0.07), fire(ZAMOR_STORM, At::Feet, Aim::Forward)],
        "todoroki_flashfreeze_heatwave" => vec![
            Aura(true), Voice, aow(HOARFROST_STOMP), ring(ICECRAG, At::Feet, 8, 4.0), fire(ZAMOR, At::Feet, Aim::Forward), Wait(0.5),
            burst(ICECRAG, At::Line(2.0, 2.5), Aim::Forward, 6, 0.06), Wait(0.3),
            act(Cast), fire(FELL_GOD, At::Hand, Aim::AtTarget), fire(FIRES_SIN, At::Target, Aim::Forward), ring(EXPLOSIVE_CLUMP, At::Target, 5, 3.0),
            HitStop(0.3), Aura(false),
        ],
        // ---- Asta: anti-magic swords, black (and white) flames
        "asta_black_divider" => vec![
            // (approved 2026-10-17) (Black Clover: Black Divider: he packs anti-magic onto the Demon-Slayer Sword's edges until the blade is bigger than
            // buildings, and one slash cuts the foe in two): the Bull Thrust and slashes as before, then he draws back and his heavy
            // swing sends a giant black blade through them
            go(Dest::Target { gap: 1.2 }, 0.14), act(Strike), Wait(0.12), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(BLACK_FLAME, At::Ahead(1.0), Aim::Forward), Wait(0.2),
            act(Strike2), Wait(0.12), fire(BLACK_BLADE, At::Hand, Aim::Forward), Wait(0.22),
            Aura(true), go(Dest::Target { gap: 3.0 }, 0.12), FaceTarget, act(Heavy), Wait(0.3),
            fire(BLACK_DIVIDER, At::Hand, Aim::AtTarget), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(GROUND_SHATTER, At::Target, Aim::Forward),
            HitStop(0.2), Wait(0.5), Aura(false),
        ],
        // 0.13.0: the slash lands, then the wave leaves the blade
        "asta_demon_dweller_sword" => vec![act(Strike), Wait(0.18), fire(WAVES_OF_DARKNESS, At::Hand, Aim::Forward), Wait(0.15), act(Strike2), Wait(0.15), burst(BLACK_BLADE, At::Hand, Aim::AtTarget, 2, 0.15)],
        // Black Hurricane: the straight sword's great spinning slash that leaves a ring (Heavy Special 2,
        // second kind), with his black flames
        "asta_black_hurricane" => vec![
            Weapon { row: LONGSWORD, lua: HEAVY_SPECIAL2_B, secs: 2.8 },
            // (1.3.1 round 3, the user: "should be 10m AOE": its blow on everyone within 5 m of him, not on the target wherever it was)
            Wait(0.5), ring(BLACK_FLAME, At::Feet, 6, 1.5), ring(STRIKE_HIT, At::Feet, 12, crate::combat::BLACK_HURRICANE_R - 0.8),
            ring(BLACK_FLAME, At::Feet, 8, 3.0), Wait(0.6),
        ],
        "asta_black_meteorite" => vec![
            // Black Asta: blitz back and forth through them trailing black and white flames, then one huge slash
            Aura(true), act(PowerUp), Voice,
            go(Dest::Through { past: 5.0, side: 2.5 }, 0.1), act(Strike), fire(BLACK_FLAME, At::Feet, Aim::Back), fire(GHOSTFLAME_SOFT, At::Target, Aim::Forward),
            go(Dest::Through { past: 5.0, side: -2.5 }, 0.1), act(Strike2), fire(GHOSTFLAME_SOFT, At::Target, Aim::Forward),
            go(Dest::Through { past: 5.0, side: 2.5 }, 0.1), act(Strike), fire(BLACK_FLAME, At::Feet, Aim::Back), fire(DESTINED_DEATH, At::Target, Aim::Forward),
            go(Dest::Through { past: 5.0, side: -2.5 }, 0.1), act(Strike2), fire(GHOSTFLAME_SOFT, At::Target, Aim::Forward),
            go(Dest::Up { height: 5.0, toward: 0.3 }, 0.28), Hover(0.25), go(Dest::Down, 0.18), act(Heavy),
            fire(BLACK_FLAME_RITUAL, At::Feet, Aim::Forward), fire(DESTINED_DEATH, At::Feet, Aim::Forward),
            fire(WAVE_OF_DESTRUCTION, At::Feet, Aim::Forward), ring(GHOSTFLAME_SOFT, At::Feet, 5, 3.0),
            HitStop(0.35), Aura(false),
        ],
        // ---- JUMP FORCE avatar abilities (Your Tarnished)
        "avatar_destructo_disc" => vec![act(Throw), fire(GLINT_ARC, At::Hand, Aim::AtTarget)],
        "avatar_fire_pillar" => vec![act(Cast), burst(REDMANES, At::Line(2.0, 2.5), Aim::Forward, 5, 0.08)],
        "avatar_gatotsu" => vec![go(Dest::Target { gap: 1.0 }, 0.12), act(Heavy), fire(ROCK_BLASTER, At::Ahead(1.0), Aim::Forward)],
        "avatar_one_handed_spirit_bomb" => vec![act(Raise), Wait(0.2), act(Throw), fire(SPIRIT_BOMB, At::Above(1.5), Aim::AtTarget)],
        "avatar_finger_pistol" => vec![go(Dest::Target { gap: 1.0 }, 0.1), act(Strike), burst(SWIFT_SHARD, At::Hand, Aim::AtTarget, 3, 0.06)],
        "avatar_futae_no_kiwami" => vec![go(Dest::Target { gap: 1.2 }, 0.12), act(Heavy), fire(GRAVITY_WELL, At::Ahead(1.0), Aim::Forward), fire(ROCK_BLASTER, At::Ahead(1.2), Aim::Forward)],
        "avatar_aurora_thunder_attack" => vec![Aura(true), act(PowerUp), Wait(0.4), act(Cast), streams(FROZEN_SPEAR, At::Hand, Aim::AtTarget, 6, 0.1, 0.4), Aura(false)],
        "avatar_meteor_shower" => vec![act(Raise), fire(COLLAPSING_STARS, At::Hand, Aim::AtTarget), fire(STARS_OF_RUIN, At::Hand, Aim::AtTarget)],
        // ================================================================ 1.3.0: ten new fighters
        // ---- Ichigo (Bleach): Zangetsu (the Uchigatana's katana moveset, 1.3.1 round 2; his abilities keep the greatsword swings), Getsuga Tensho
        "ichigo_getsuga_tensho" => vec![
            // 1.3.0 (researched, Bleach wiki: at the instant of the slash his reiatsu is released from the blade's tip as a crescent
            // wave that flies forward, blue-white in Shikai): the greatsword's heavy swing and Moonveil's crescent, blue-white, 2x
            FaceTarget, Weapon { row: ZANGETSU_GREATSWORD, lua: R_HEAVY1, secs: 2.0 }, Wait(0.35), fire(GETSUGA, At::Hand, Aim::AtTarget), HitStop(0.08),
        ],
        "ichigo_flash_step" => vec![
            // Shunpo: through them, back through from the other side, then a cleaving finish
            go(Dest::Through { past: 2.0, side: 0.0 }, 0.12), Weapon { row: ZANGETSU_GREATSWORD, lua: R_LIGHT1, secs: 1.4 }, fire(STRIKE_HIT, At::Target, Aim::Forward), Wait(0.22),
            go(Dest::Through { past: 2.0, side: 1.5 }, 0.12), Weapon { row: ZANGETSU_GREATSWORD, lua: R_LIGHT2, secs: 1.4 }, fire(STRIKE_HIT, At::Target, Aim::Forward), Wait(0.22),
            go(Dest::Target { gap: 1.5 }, 0.1), Weapon { row: ZANGETSU_GREATSWORD, lua: R_HEAVY1, secs: 2.0 }, Wait(0.3), fire(DESTINED_DEATH, At::Ahead(1.0), Aim::Forward), HitStop(0.1),
        ],
        "ichigo_tensa_zangetsu" => vec![
            // 1.3.0 (researched: Bankai's speed, and the Getsuga turns black edged in red): his reiatsu bursts, three quick cuts and
            // two black-red Getsugas point-blank
            Aura(true), Voice, go(Dest::Target { gap: 1.3 }, 0.12), Weapon { row: ZANGETSU_GREATSWORD, lua: R_LIGHT1, secs: 1.4 }, fire(STRIKE_HIT, At::Target, Aim::Forward), Wait(0.18),
            Weapon { row: ZANGETSU_GREATSWORD, lua: R_LIGHT2, secs: 1.4 }, fire(STRIKE_HIT, At::Target, Aim::Forward), Wait(0.18), Weapon { row: ZANGETSU_GREATSWORD, lua: R_HEAVY1, secs: 2.0 }, Wait(0.3),
            fire(GETSUGA_BANKAI, At::Hand, Aim::AtTarget), Wait(0.25), fire(GETSUGA_BANKAI, At::Hand, Aim::AtTarget), HitStop(0.12), Aura(false),
        ],
        "ichigo_mugetsu" => vec![
            // 1.3.0 (researched, Bleach wiki: the Final Getsuga Tensho: he becomes the technique and releases a wall of darkness):
            // the dark aura, one slash: a black-red Getsuga and a wall of black darkness racing out along its line
            Aura(true), Voice, FaceTarget, Wait(0.6), Step::Flash, Weapon { row: ZANGETSU_GREATSWORD, lua: R_HEAVY1, secs: 2.0 }, Wait(0.35),
            fire(GETSUGA_BANKAI, At::Hand, Aim::AtTarget), burst(MUGETSU, At::Line(1.5, 2.2), Aim::Forward, 7, 0.05), fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.35), Wait(0.6), Aura(false),
        ],
        // ---- Sasuke (Naruto): the Kusanagi (a katana's moveset stands in), Chidori, the Sharingan and Rinnegan
        "sasuke_chidori" => vec![
            // 1.3.1 (the user: "Chidori electricity lands as his sword swing connects, blade infused with electricity"; researched,
            // Narutopedia: Chidori Katana / Chidori Current, the lightning chakra run through the Kusanagi so it cuts through
            // anything): the blade crackles blue-white while it charges, he closes in with a katana slash, and the discharge bursts
            // on them as the cut lands
            Aura(true), Blade(crate::aura::CHIDORI_BLADE, true), Wait(0.45), go(Dest::Target { gap: 1.6 }, 0.18), act(Strike), Wait(CUT_LIGHT),
            fire(STRIKE_HIT, At::Target, Aim::Forward), burst(KILLUA_ZAP, At::Target, Aim::Forward, 3, 0.07), HitStop(0.12), Wait(0.35),
            Blade(crate::aura::CHIDORI_BLADE, false), Aura(false),
        ],
        "sasuke_great_fireball" => vec![
            // 1.3.0 (researched, Narutopedia: chakra turned to fire and breathed out of his mouth as a giant orb that craters the
            // ground): a fireball 3x Giantsflame's, from his face
            FaceTarget, act(Cast), Wait(0.3), fire(GREAT_FIREBALL, At::Above(0.0), Aim::AtTarget),
        ],
        "sasuke_amaterasu" => vec![
            // 1.3.1 (the user: Amaterasu as a combo move with its own animation, not Fire Style's cast; researched, Narutopedia: Blaze
            // Release, Kagutsuchi's Sword: Amaterasu's black flames shaped round his blade): the Kusanagi wreathed in black flame
            // (Black Flame Blade's coating), three katana cuts, each setting black fire on them as it lands, the last engulfing them
            Blade(crate::aura::AMATERASU_BLADE, true), FaceTarget, go(Dest::Target { gap: 1.6 }, 0.14),
            act(Strike), Wait(CUT_LIGHT), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(BLACK_FLAME, At::Target, Aim::Forward), Wait(0.2),
            FaceTarget, act(Strike2), Wait(CUT_LIGHT), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(BLACK_FLAME, At::Target, Aim::Forward), Wait(0.2),
            FaceTarget, act(Heavy), Wait(CUT_HEAVY), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(BLACK_FLAME_RITUAL, At::Target, Aim::Forward),
            HitStop(0.12), Wait(0.4), Blade(crate::aura::AMATERASU_BLADE, false),
        ],
        "sasuke_kirin" => vec![
            // 1.3.1 (the user: replace Indra's Arrow; researched, Narutopedia: Kirin: the storm his fire heated into the sky, its natural
            // lightning brought down on the target in an instant in the shape of a great beast, too fast to dodge): the Chidori in
            // his blade, the Kusanagi raised to the sky, and a huge blue-white bolt crashing down on them with the discharge round them
            Aura(true), Voice, Blade(crate::aura::CHIDORI_BLADE, true), FaceTarget, aow(THUNDERBOLT_RAISE), Wait(0.9), fire(KIRIN, At::AboveTarget(14.0), Aim::Down),
            Wait(0.25), fire(STRIKE_HIT, At::Target, Aim::Forward), burst(KILLUA_ZAP, At::Target, Aim::Forward, 3, 0.08),
            ring(KILLUA_ZAP, At::Target, 6, 3.0), HitStop(0.3), Wait(0.4), Blade(crate::aura::CHIDORI_BLADE, false), Aura(false),
        ],
        // ---- Zoro (One Piece): Three Sword Style (the Uchigatana's katana moveset stands in)
        "zoro_tora_gari" => vec![
            // 1.3.1 (the user: replace Oni Giri entirely; researched, One Piece wiki: Three Sword Style: Tora Gari (Tiger Hunt), he leaps
            // and brings all three blades down together in one crushing diagonal cut): Lion's Claw's flip with his katanas, and as
            // the blades land, three cuts tear across them (lab: the ground shatter's golden pillars read as magic: left out)
            FaceTarget, go(Dest::Target { gap: 3.0 }, 0.12), aow(LIONS_CLAW), Wait(TORA_GARI_LAND), fire(STRIKE_HIT, At::Target, Aim::Forward),
            burst(VACUUM_SLICE, At::Target, Aim::Forward, 3, 0.06), Step::Flash, HitStop(0.15), Wait(0.4),
        ],
        "zoro_pound_cannon" => vec![
            // 36 Pound Cannon: flying slashes like cannon shot
            // (1.3.1, the user: it made his swords disappear: the katana heavy (29030500) is a draw from the sheath that puts the blades
            // away; his second cut keeps them out, the slashes flying off it as it lands)
            FaceTarget, act(Strike2), Wait(CUT_LIGHT), fan(STORM_BLADE, At::Hand, Aim::AtTarget, 3, 0.05, 0.12), Wait(0.5),
        ],
        "zoro_tatsumaki" => vec![
            // 1.3.0 (researched, One Piece wiki: he spins into a dragon-like tornado that cuts and blows away everyone caught in it):
            // the spinning slash and a big cutting whirlwind
            // he closes to 2.5 m first so the whirlwind is on them, and its cuts land where they stand
            FaceTarget, go(Dest::Target { gap: 2.5 }, 0.15), aow(SPINNING_SLASH), Wait(0.25), fire(ZORO_TORNADO, At::Target, Aim::Forward),
            ring(VACUUM_SLICE, At::Feet, 6, 2.0), fire(STRIKE_HIT, At::Target, Aim::Forward), ring(STRIKE_HIT, At::Target, 6, 1.2),
            Wait(0.3), fire(STRIKE_HIT, At::Target, Aim::Forward), ring(STRIKE_HIT, At::Target, 8, 2.2), HitStop(0.1),
        ],
        "zoro_asura" => vec![
            // Nine Sword Style: Asura, three heads and nine blades, cutting from every side
            Aura(true), Voice, act(PowerUp), Wait(0.6),
            go(Dest::Through { past: 2.0, side: 1.5 }, 0.12), act(Strike), fire(STRIKE_HIT, At::Target, Aim::Forward), Wait(0.15),
            go(Dest::Through { past: 2.0, side: -1.5 }, 0.12), act(Strike2), fire(STRIKE_HIT, At::Target, Aim::Forward), Wait(0.15),
            go(Dest::Through { past: 2.0, side: 0.0 }, 0.12), act(Heavy), ring(DESTINED_DEATH, At::Target, 9, 2.0), HitStop(0.3), Aura(false),
        ],
        // ---- Yami (Black Clover): Dark Magic in the katana (the Nagakiba's moveset stands in)
        "yami_black_moon" => vec![
            // 1.3.0 (researched, BC wiki: a black hole created overhead that absorbs everything in the area): a dark moon over them that
            // drags them in
            // (1.3.1, the user: black, not purple: the moon and its pull recoloured)
            FaceTarget, act(Cast), Wait(0.3), fire(BLACK_MOON, At::AboveTarget(4.0), Aim::Forward), fire(BLACK_PULL, At::Target, Aim::Forward),
            Wait(0.6), fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.08),
        ],
        "yami_death_thrust" => vec![
            // Death Thrust: darkness driven out along the blade's line
            FaceTarget, act(Strike), Wait(0.15), burst(WAVES_OF_DARKNESS, At::Line(2.0, 2.5), Aim::Forward, 5, 0.05),
        ],
        "yami_avidya_slash" => vec![
            // Dark Cloaked Avidya Slash: the cut that splits the space in front of him
            // (1.3.1 round 2, the user: he swung his empty hand: the katana heavy sheathes the blade first; his second cut keeps it out)
            FaceTarget, act(Strike2), Wait(CUT_LIGHT), burst(DESTINED_DEATH, At::Line(2.0, 3.0), Aim::Forward, 6, 0.04), HitStop(0.1),
        ],
        "yami_dimension_slash" => vec![
            // Dark Cloaked Dimension Slash: the realm itself cut in two (1.3.1, the user: no fist-raise emote: darkness wraps his katana
            // instead (Dark Cloak), then the one cut) (round 2: the second cut, blade out, as Avidya Slash)
            Aura(true), Voice, Blade(crate::aura::DARK_BLADE, true), FaceTarget, Wait(0.7), act(Strike2), Wait(CUT_LIGHT),
            burst(BLACK_BLADE, At::Line(2.0, 3.0), Aim::Forward, 8, 0.03), ring(BLACK_FLAME, At::Target, 6, 3.0), HitStop(0.3), Wait(0.3),
            Blade(crate::aura::DARK_BLADE, false), Aura(false),
        ],
        // ---- Gojo (Jujutsu Kaisen): Limitless
        "gojo_lapse_blue" => vec![
            // 1.3.0 (researched, JJK wiki: space converged to a point, a blue sphere whose attraction pulls everything in): a bright blue
            // gravity point on them
            // (1.3.1, the user: Blue a projectile like Red: the sphere flies from his fingers and bursts on them)
            FaceTarget, act(Cast), Wait(0.3), fire(GOJO_BLUE_SHOT, At::Hand, Aim::AtTarget), HitStop(0.08),
        ],
        "gojo_reversal_red" => vec![
            // 1.3.0 (researched: a small red orb that bursts with immense repelling force on impact): a red orb from his fingers
            FaceTarget, act(Cast), Wait(0.3), fire(GOJO_RED, At::Hand, Aim::AtTarget), HitStop(0.1),
        ],
        "gojo_infinity" => vec![
            // Infinity (JJK wiki: the Limitless slows anything approaching him until it never arrives; nothing touches him)
            // (the user, 2026-10-17: "it should only last for about 5 seconds and have a minute long recharge"): he raises
            // his hand and for 5 s nothing reaches him, while he moves and fights as he likes
            FaceTarget, act(Cast), Guard(5.0), Wait(0.4),
        ],
        "gojo_hollow_purple" => vec![
            // 1.3.0 (researched: Blue and Red collided, an imaginary mass that rushes forth and erases everything in its path): a big
            // purple mass from his hands that passes through them, erasing as it goes
            // (the user, 2026-10-17: he takes off his blindfold for it: his unmasked body, swapped in with no load like Gear Fourth)
            Aura(true), Voice, Gear4(true), act(TwoHand), Wait(1.0), fire(GOJO_PURPLE, At::Hand, Aim::AtTarget), Wait(0.6), HitStop(0.2), Wait(0.4), Gear4(false), Aura(false),
        ],
        // ---- Yuji (Jujutsu Kaisen): cursed energy in the fists
        "yuji_piercing_blood" => vec![
            // 1.3.1 (the user: replace Divergent Fist with a move with some range that fits his kit; researched, JJK wiki: with Choso's
            // brothers' wombs taken in, Yuji uses Blood Manipulation: Piercing Blood, his blood compressed between his palms and
            // shot out as a needle-thin beam faster than sound): the beam from his palm, straight through them, and a red burst
            FaceTarget, act(Cast), Wait(0.3), fire(PIERCING_BLOOD, At::Hand, Aim::AtTarget), Wait(0.12), fire(STRIKE_HIT, At::Target, Aim::Forward),
            HitStop(0.08),
        ],
        "yuji_black_flash" => vec![
            // 1.3.0 (researched, JJK wiki: cursed energy within a millionth of a second of the blow distorts space in black-and-red
            // lightning): black flame and a red lightning crack on the blow
            go(Dest::Target { gap: 1.2 }, 0.12), act(Heavy), Wait(0.2), Step::Flash, fire(BLACK_FLAME, At::Target, Aim::Forward), fire(BLACK_FLASH, At::Target, Aim::Forward), HitStop(0.25),
        ],
        "yuji_manji_kick" => vec![
            // Manji Kick: a spinning kick, then the follow-through
            go(Dest::Target { gap: 1.4 }, 0.12), aow(STORM_KICK), Wait(0.3), fire(STRIKE_HIT, At::Target, Aim::Forward),
            act(Kick), Wait(0.15), fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.08),
        ],
        "yuji_black_flash_barrage" => vec![
            // 1.3.0 Sukuna's Vessel (the user, 2026-10-17: Yuji becomes Sukuna for it; researched, JJK wiki: Sukuna's own
            // techniques: Dismantle, invisible slashes that cut whatever is in front of him; Cleave, one cut up close that adapts to
            // its target; and "Open": Fuga, a flame arrow drawn between his hands): Sukuna's body swapped in with no load, a fan of
            // cutting slashes, the step in and the Cleave, then the flame arrow
            Aura(true), Voice, Gear4(true), act(PowerUp), Wait(0.8),
            FaceTarget, act(Strike), fan(VACUUM_SLICE, At::Hand, Aim::AtTarget, 5, 0.06, 0.35), Wait(0.15), act(Strike2), fan(VACUUM_SLICE, At::Hand, Aim::AtTarget, 5, 0.06, 0.35),
            Wait(0.3), go(Dest::Target { gap: 1.3 }, 0.12), act(Heavy), fire(STRIKE_HIT, At::Target, Aim::Forward), burst(VACUUM_SLICE, At::Target, Aim::Forward, 3, 0.05), HitStop(0.15),
            Wait(0.25), FaceTarget, act(Cast), Wait(0.5), fire(GIANTSFLAME, At::Hand, Aim::AtTarget), fire(FLAME_SLING, At::Hand, Aim::AtTarget), HitStop(0.25),
            Wait(0.6), Gear4(false), Aura(false),
        ],
        // ---- Gon (Hunter x Hunter): Jajanken
        "gon_jajanken_rock" => vec![
            // 1.3.0 (researched, Hunterpedia: aura focused in his fist while he chants, then his strongest punch): his golden aura, the
            // punch and a burst of yellow aura
            Aura(true), Wait(0.7), go(Dest::Target { gap: 1.2 }, 0.12), act(Heavy), fire(GON_BURST, At::Target, Aim::Forward),
            fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.2), Aura(false),
        ],
        "gon_jajanken_scissors" => vec![
            // 1.3.1 (the user: in the show Scissors "is a thrusting dash that cuts enemies in half", a sword arc in his colour): two
            // fingers held out as a blade of aura, he lunges through them with the straight sword's long thrust (Carian Combo
            // Warriors' Deep Stinger, no blade shown) and a yellow-orange crescent cuts across them as he passes
            Aura(true), FaceTarget, go(Dest::Through { past: 2.0, side: 0.0 }, 0.16), Weapon { row: LONGSWORD, lua: BACKSTEP_THRUST, secs: 1.4 },
            Wait(0.3), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(GON_CUT, At::Target, Aim::Forward), HitStop(0.12), Wait(0.5), Aura(false),
        ],
        "gon_jajanken_paper" => vec![
            // 1.3.1 (the user: Paper is a ranged attack: a yellow-orange ball of nen shot at them, like Naruto's Rasengan in his
            // colour): his hand thrust out and the spinning ball of aura flying from it, bursting on them (lab: out of the palm
            // blast Ash of War the ball started high above him and drifted off)
            FaceTarget, act(Cast), Wait(0.3), fire(GON_PAPER, At::Hand, Aim::AtTarget), HitStop(0.06),
        ],
        "gon_adult_form" => vec![
            // everything he has, for this one moment: grown, his aura overwhelming, and one Rock
            // (1.3.0: his adult body swapped in, Jump Force's Adult Gon with the hair to match, no load)
            // (1.3.1, the user: the adult form could last longer: he stays grown for 15 s after the blow, fighting on in it)
            Aura(true), Voice, Gear4(true), Grow(1.25), KeepForm(ADULT_SECS), Wait(1.0), go(Dest::Target { gap: 1.6 }, 0.18), act(Heavy), fire(GON_BURST, At::Target, Aim::Forward),
            ring(GON_BURST, At::Target, 6, 3.0), fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.3), Wait(0.4), Aura(false),
        ],
        // ---- Killua (Hunter x Hunter): Transmutation into lightning
        "killua_thunderbolt" => vec![
            // 1.3.0 (researched, Hunterpedia: Thunderbolt (Narukami): from above them he projects a bolt of lightning from his fingers
            // that paralyses): a blue-white bolt brought down on them, the discharge round them
            FaceTarget, act(Cast), Wait(0.25), fire(KILLUA_BOLT, At::AboveTarget(5.0), Aim::Down), Wait(0.1), fire(KILLUA_ZAP, At::Target, Aim::Forward),
            fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.08),
        ],
        "killua_lightning_palm" => vec![
            // 1.3.0 (researched: both palms on them, electrocuting and pushing them back)
            // (1.3.1, the user: the lightning should hit as the thrust finishes: the palms reach full extension 0.85 s in (Kamehameha's
            // timing on the same Ash of War))
            go(Dest::Target { gap: 1.2 }, 0.12), aow(PALM_BLAST), Wait(0.8), fire(KILLUA_ZAP, At::Ahead(1.0), Aim::Forward), fire(KILLUA_ZAP, At::Target, Aim::Forward),
            fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.1),
        ],
        "killua_narukami" => vec![
            // 1.3.0 (researched: Narukami is Thunderbolt's own name, so this is now Rhythm Echo: an assassin's step that leaves
            // afterimages and strikes out of them): he darts round them side to side, a blow and a spark of his lightning each time
            go(Dest::Through { past: 1.6, side: 1.4 }, 0.07), act(Strike), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(KILLUA_ZAP, At::Target, Aim::Forward), Wait(0.1),
            go(Dest::Through { past: 1.6, side: -1.4 }, 0.07), act(Strike2), fire(STRIKE_HIT, At::Target, Aim::Forward), Wait(0.1),
            go(Dest::Through { past: 1.6, side: 1.4 }, 0.07), act(Kick), fire(STRIKE_HIT, At::Target, Aim::Forward), fire(KILLUA_ZAP, At::Target, Aim::Forward), HitStop(0.1),
        ],
        "killua_godspeed" => vec![
            // (1.3.0: his Godspeed hair, from the Nen Impact model, swapped in for it, no load)
            // 1.3.1 (the user: more electricity in Godspeed, and glowing white hair; the round 3 version, blue-white lightning on
            // every blow, made live): researched (HxH wiki: Killua's aura turned to electricity, his hair standing white with it;
            // Whirlwind: he flashes through them from every side faster than they can see): his body crackling blue-white
            // (Lightning Fortification's, his aura) and his hair alight with it, every blow bursting in lightning, sparks left where
            // he flashes through, and the last a lunging blow with a great discharge. Sideways steps only, nothing straight up
            Aura(true), Voice, Gear4(true), HeadFx(true),
            go(Dest::Through { past: 2.0, side: 1.5 }, 0.08), fire(KILLUA_ZAP, At::Feet, Aim::Forward), act(Strike), fire(STRIKE_HIT, At::Target, Aim::Forward), burst(KILLUA_ZAP, At::Target, Aim::Forward, 2, 0.05), Wait(0.14),
            go(Dest::Through { past: 2.0, side: -1.5 }, 0.08), fire(KILLUA_ZAP, At::Feet, Aim::Forward), act(Strike2), fire(STRIKE_HIT, At::Target, Aim::Forward), burst(KILLUA_ZAP, At::Target, Aim::Forward, 2, 0.05), Wait(0.14),
            go(Dest::Through { past: 2.0, side: 0.0 }, 0.08), fire(KILLUA_ZAP, At::Feet, Aim::Forward), act(Kick), fire(STRIKE_HIT, At::Target, Aim::Forward), burst(KILLUA_ZAP, At::Target, Aim::Forward, 2, 0.05), Wait(0.14),
            go(Dest::Through { past: 2.0, side: 1.2 }, 0.08), fire(KILLUA_ZAP, At::Feet, Aim::Forward), act(Strike), fire(STRIKE_HIT, At::Target, Aim::Forward), burst(KILLUA_ZAP, At::Target, Aim::Forward, 2, 0.05), Wait(0.14),
            go(Dest::Target { gap: 2.5 }, 0.08), FaceTarget,
            Weapon { row: FISTS, lua: HEAVY_SPECIAL1_B, secs: 1.8 },
            Wait(0.7), fire(STRIKE_HIT, At::Target, Aim::Forward), burst(KILLUA_ZAP, At::Target, Aim::Forward, 4, 0.08), ring(KILLUA_ZAP, At::Target, 6, 2.0), HitStop(0.3),
            Wait(0.5), HeadFx(false), Gear4(false), Aura(false),
        ],
        // ---- Jotaro (JoJo's Bizarre Adventure): Star Platinum
        // (1.3.0, the user: "make the stands appear like they do in the show": Star Platinum comes out of him and throws the
        // blows while he stands back and points at his enemy; its look, poses and afterimages: stands.rs)
        "jotaro_ora_rush" => vec![
            // ORA ORA ORA: Star Platinum's barrage, a blur of fists, then one big ORA! that sends them flying
            go(Dest::Target { gap: 2.2 }, 0.12), FaceTarget, Step::Stand(1.6, crate::stands::ORA), Wait(0.15 + crate::stands::POINT_LEAD),
            burst(STRIKE_HIT, At::Target, Aim::Forward, 7, 0.11), Wait(0.3), fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.12),
        ],
        "jotaro_star_finger" => vec![
            // Star Finger: Star Platinum's two fingers shoot out from its fist and pierce
            FaceTarget, Step::Stand(0.9, crate::stands::THRUST), Wait(0.25 + crate::stands::POINT_LEAD), fire(SWIFT_SHARD, At::Ahead(1.8), Aim::AtTarget),
        ],
        "jotaro_the_world" => vec![
            // Star Platinum: The World: time stops (the sphere bursts out of him); Star Platinum looms over his shoulder, then
            // steps in with one heavy blow
            // (1.3.1 round 2, the user liked the old animation: the power-up stance back while Star Platinum looms)
            TimeStop(3.0), Step::Stand(1.0, crate::stands::ROAR), act(PowerUp), Wait(0.9),
            go(Dest::Target { gap: 2.0 }, 0.12), FaceTarget, Step::Stand(1.0, crate::stands::HEAVY), Wait(0.3), fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.1),
        ],
        "jotaro_time_stop_ora" => vec![
            // time stopped, Star Platinum rises over him, then everything it has: the longest barrage and a last blow
            Aura(true), Voice, TimeStop(4.0), Step::Stand(0.8, crate::stands::ROAR), Wait(0.7), go(Dest::Target { gap: 2.2 }, 0.12), FaceTarget,
            Step::Stand(2.1, crate::stands::ORA), Wait(0.15 + crate::stands::POINT_LEAD), burst(STRIKE_HIT, At::Target, Aim::Forward, 12, 0.1), Wait(0.3),
            fire(ROCK_BLASTER, At::Target, Aim::Forward), HitStop(0.3), Aura(false),
        ],
        // ---- DIO (JoJo's Bizarre Adventure): The World
        "dio_muda_rush" => vec![
            // MUDA MUDA MUDA: The World's barrage, a blur of fists, ended with a kick (as it does in the anime)
            go(Dest::Target { gap: 2.2 }, 0.12), FaceTarget, Step::Stand(1.6, crate::stands::MUDA), Wait(0.15 + crate::stands::POINT_LEAD),
            burst(STRIKE_HIT, At::Target, Aim::Forward, 7, 0.11), Wait(0.3), fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.12),
        ],
        "dio_vaporization_freezing" => vec![
            // 1.3.1 round 2 (the user: Space Ripper redesigned as a whole new move, their pick; researched, JoJo wiki: Phantom Blood's
            // Vaporization Freezing Technique: DIO draws the heat out of his body so the moisture where he touches evaporates and
            // freezes, and whatever his hand holds freezes solid, Zeppeli's arm frozen and shattered): he closes in, his open palm
            // lands on them, a burst of frost bursts from the touch (the Freezing Pot's: real frost build-up, the game's frostbite
            // ice on them) and they stand frozen solid for 2 s while the frost spreads over them
            FaceTarget, go(Dest::Target { gap: 1.3 }, 0.12), aow(PALM_BLAST), Wait(0.45),
            fire(FREEZING_TOUCH, At::Target, Aim::Forward), fire(STRIKE_HIT, At::Target, Aim::Forward), Stun(2.0), HitStop(0.15),
            Wait(0.25), burst(FREEZING_TOUCH, At::Target, Aim::Forward, 2, 0.35), Wait(0.4),
        ],
        "dio_the_world" => vec![
            // ZA WARUDO: time stops (the golden sphere bursts out of him), The World looms over his shoulder, then steps in
            // with one heavy blow
            TimeStop(3.0), Step::Stand(1.0, crate::stands::ROAR), act(PowerUp), Wait(0.9),
            go(Dest::Target { gap: 2.0 }, 0.12), FaceTarget, Step::Stand(1.0, crate::stands::HEAVY), Wait(0.3), fire(STRIKE_HIT, At::Target, Aim::Forward), HitStop(0.1),
        ],
        "dio_road_roller" => vec![
            // (greenlit 2026-10-17, to be approved in the user's playtest) 1.3.0 round 3 (researched, JoJo wiki: DIO stops time, comes back with a mustard-yellow road roller and drops it
            // on Jotaro, then pounds it with a MUDA MUDA barrage; it ends in an explosion in the games). The user: his animation
            // finishes as the roller begins to drop, the ground shatters when it hits, and it falls facing the enemy. Time stops,
            // DIO hammers away (the paired fists' flurry), and as it ends a road roller (a real model) drops out of the sky onto
            // them, level and front first; where it lands the ground shatters, and it explodes as time comes back. DIO stays on
            // the ground
            Aura(true), Voice, TimeStop(4.5), Step::Stand(0.9, crate::stands::ROAR), FaceTarget, Plant(1.4), Weapon { row: FISTS, lua: DUAL_LIGHT2, secs: 1.6 },
            burst(STRIKE_HIT, At::Target, Aim::Forward, 5, 0.2), Wait(0.2),
            Lob { b: ROAD_ROLLER_FALL, at: At::OverTarget(2.9, 8.5) }, Wait(0.68),
            fire(ROAD_ROLLER, At::NearTarget(0.0), Aim::Forward), fire(STRIKE_HIT, At::Target, Aim::Forward),
            fire(GROUND_SHATTER, At::Target, Aim::Forward), ring(GROUND_SHATTER, At::Target, 5, 2.2), fire(STORM_STOMP, At::Target, Aim::Forward),
            HitStop(0.2), Wait(0.9), fire(HOWITZER_BLAST, At::Target, Aim::Forward), HitStop(0.3), Wait(0.4), Aura(false),
        ],
        _ => return None,
    }};
    Some(Script { steps })
}

impl Script {
    /// Every bullet the script fires (for damage) and the total number of shots.
    pub fn bullets(&self) -> (Vec<i32>, u32) {
        let mut ids = Vec::new();
        let mut shots = 0;
        for s in &self.steps {
            let (b, n) = match *s {
                Fire { b, n, .. } => (b, n),
                Ring { b, n, .. } => (b, n),
                _ => continue,
            };
            shots += n;
            if !ids.contains(&b) {
                ids.push(b);
            }
        }
        (ids, shots)
    }

    /// How many of the script's shots land on one enemy it is aimed at (an estimate: shots at the
    /// target count fully, wide fans and ground waves partly, rings around a spot by a fraction).
    /// abilities.hits overrides it with the number measured in the lab.
    pub fn expected_hits(&self) -> f32 {
        let mut hits = 0.0;
        for s in &self.steps {
            hits += match *s {
                Fire { at, n, spread, .. } => {
                    let each = match at {
                        At::Line(..) => 0.4,
                        At::Feet => 0.7,
                        _ if spread > 0.2 => 0.5,
                        _ => 1.0,
                    };
                    each * n as f32
                }
                Ring { at, n, r, .. } => {
                    let each = match at {
                        At::Target | At::AboveTarget(_) if r <= 4.0 => 0.3,
                        _ => 0.15,
                    };
                    each * n as f32
                }
                _ => 0.0,
            };
        }
        hits.max(1.0)
    }
}

/// Every bullet row any script fires (the mod's own bullets in the shared pool).
pub fn all_bullets() -> &'static [i32] {
    static ALL: std::sync::OnceLock<Vec<i32>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        let mut all = Vec::new();
        for a in crate::sheets::ABILITIES.iter() {
            if let Some(s) = script(a.id) {
                for b in s.bullets().0 {
                    if !all.contains(&b) {
                        all.push(b);
                    }
                }
            }
        }
        all.push(SHATTER_EARTH);
        for b in crate::combos::all_bullets() {
            if !all.contains(&b) {
                all.push(b);
            }
        }
        all
    })
}
