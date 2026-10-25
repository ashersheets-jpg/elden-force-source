//! 1.2.x (user, after the 1.2.0 playtest: the Move List "is very confusing and cluttered ... make a tab for CCW
//! inputs and one for our own movement inputs ... add the movelist for every weapon covered by ccw and allow the
//! players to easily sort by characters or weapons"): Carian Combo Warriors' movesets for every weapon type it
//! covers, by grip, from its own Usage Instructions (by its author on Nexus Mods, used with permission and credit),
//! in the Move List's words. Letters in its list: L light (RB), H heavy (RT); a sequence's next move comes "with H".

use crate::combos::{Btn, Line};

/// A weapon type with a Carian Combo Warriors moveset (the Move List's "by weapon" pages, in this order).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weapon {
    Fist,
    StraightSword,
    Katana,
    Greatsword,
    CurvedGreatsword,
    Twinblade,
}

pub const WEAPONS: [Weapon; 6] = [Weapon::Fist, Weapon::StraightSword, Weapon::Katana, Weapon::Greatsword, Weapon::CurvedGreatsword, Weapon::Twinblade];

/// One part of a moveset page: a heading (a grip, the Ash of War, its special states) and its lines.
pub struct Section {
    pub title: &'static str,
    pub lines: Vec<Line>,
}

impl Weapon {
    pub fn name(self) -> &'static str {
        match self {
            Weapon::Fist => "Fists",
            Weapon::StraightSword => "Straight Sword",
            Weapon::Katana => "Katana",
            Weapon::Greatsword => "Greatsword",
            Weapon::CurvedGreatsword => "Curved Greatsword",
            Weapon::Twinblade => "Twinblade",
        }
    }

    /// Who fights with it, and the Ash of War its list is made for.
    /// Who fights with it (from the fighters sheet, so a new fighter is listed by itself) and its Ash of War.
    pub fn about(self) -> String {
        let who: Vec<&str> = crate::select_screen::ORDER.iter().filter(|id| **id != "custom" && Weapon::of_fighter(id) == self)
            .filter_map(|id| crate::sheets::fighter(id).map(|f| f.name)).collect();
        let theirs = match self {
            Weapon::Fist => "Your Tarnished bare-handed or with fists (Ash of War: Kick)",
            Weapon::StraightSword => "Your Tarnished's straight swords (Ash of War: Quickstep)",
            Weapon::Katana => "Your Tarnished's katanas (Ash of War: Unsheathe)",
            Weapon::Greatsword => "Your Tarnished's greatswords: Bastard Sword, Forked, Iron, Lordsworn's, Gargoyle's (Ash of War: Stamp, Upward Cut)",
            Weapon::CurvedGreatsword => "Your Tarnished's curved greatswords: Dismounter, Monk's Flameblade (Ash of War: Square Off)",
            Weapon::Twinblade => "Your Tarnished's twinblades: Twinblade, Eleonora's Poleblade (Ash of War: Bloodhound's Step)",
        };
        if who.is_empty() { theirs.to_string() } else { format!("{}, and {theirs}", who.join(", ")) }
    }

    /// The weapon a fighter's basic attacks use (Your Tarnished: whatever they hold; fists until the menu knows).
    pub fn of_fighter(fighter: &str) -> Weapon {
        // (1.3.0: by the weapon the fighter holds: Ichigo's greatsword, the katanas of Sasuke, Zoro and Yami)
        match crate::sheets::fighter(fighter).and_then(|f| f.move_weapon) {
            Some(3_000_000..=3_999_999) => Weapon::Greatsword,
            Some(9_000_000..=9_999_999) => Weapon::Katana,
            Some(_) => Weapon::StraightSword,
            None => Weapon::Fist,
        }
    }
}

fn line(buttons: &[Btn], name: &str, note: &'static str) -> Line {
    Line { buttons: buttons.to_vec(), name: name.to_string(), note }
}

static HEAVY: Btn = Btn::Heavy;
static DODGE: Btn = Btn::Dodge;
/// (1.3.0: the game's two-hand is the Two-Hand button held and an attack, not the button alone)
static TWO_HAND: Btn = Btn::TwoHand;

/// The moveset of one weapon type, by grip.
pub fn sections(w: Weapon) -> Vec<Section> {
    use Btn::*;
    let sprint = Hold(&DODGE);
    let two_hand = Hold(&TWO_HAND);
    match w {
        Weapon::Fist => vec![
            Section { title: "FIST STANCE (ONE-HANDED)", lines: vec![
                line(&[Light, Light, Light, Light, Light], "Punch string", "RT as a punch lands: sequence B; after it plays out: sequence A"),
                line(&[Light, Heavy, Heavy], "Right Hook  >  Left Hook", "sequence A: each hook slips a hit before it lands; hold RT for an Upward Punch"),
                line(&[Light, Heavy, Heavy], "Back Double Punch  >  Jump Double Punch", "sequence B, RT right as a punch lands: the second sets off the Martial state; held: Guiding Pole Vault (into the kick stance), Back Lance Stab"),
                line(&[Light, Heavy, Heavy, Heavy, Heavy], "Forward Double Punch  >  Jump Smash", "sequence C, after B; held: Thigh Lance Stab, Adula Fire Spear (into the kick stance)"),
                line(&[sprint, Light], "Triple Whip Slash", "from a sprint"),
                line(&[sprint, Heavy], "Long-range Jump Kick", "from a sprint: into the kick stance"),
                line(&[Dodge, Light], "Double Punch", "out of a roll"),
                line(&[Jump, Light], "Downward Hammer", ""),
                line(&[Jump, Heavy], "Star Fall", "into the kick stance"),
                line(&[Left], "Left punches", "a five-punch string with the left hand, in either stance"),
            ]},
            Section { title: "KICK STANCE (TWO-HANDED)", lines: vec![
                line(&[two_hand, Light], "Kick stance", "every attack becomes a kick; roll, backstep and jump attacks switch stance by themselves"),
                line(&[Light, Light, Light, Light, Light], "Kick string", "RT as a kick lands: sequence E; after it plays out: sequence D"),
                line(&[Light, Heavy], "Upward Star Fall", "sequence D, into a jump attack; held: Back Flip"),
                line(&[Light, Heavy, Heavy], "Chasing Kick  >  Consecutive Kicks", "sequence E, RT as a kick lands: the second sets off the Martial state; held: Flint Wheel, Rotary Chasing Kick"),
                line(&[Light, Heavy, Heavy, Heavy, Heavy], "Triple Whip Leg  >  Kick 13", "sequence F, after E; held: Kick 13 DT, Augmenting Drive"),
                line(&[Dodge, Light], "Jump Kick", "out of a roll"),
                line(&[Dodge, Light], "Punch Missile", "out of a backstep (dodge standing still): back to the fists"),
                line(&[Jump, Heavy], "Star Fall", "charged in the air, it's the flying kick"),
            ]},
            Section { title: "MARTIAL STATE", lines: vec![
                line(&[], "5 seconds of power", "harder hits (+50% physical, +20% poise damage), air cannons and a wind orbit on the light strings"),
            ]},
        ],
        Weapon::StraightSword => vec![
            Section { title: "ONE-HANDED", lines: vec![
                line(&[Light, Light, Light], "Sword string", "RT as a slash lands: sequence B; after it plays out: sequence A"),
                line(&[Light, Heavy, Heavy], "Triple Slash  >  Carian Pierce", "sequence A; hold the second for Million Stab, a blur of thrusts"),
                line(&[Light, Heavy, Heavy], "Guiding Double Slash  >  Vertical Sword Edge", "sequence B, RT as a slash lands; held: Guiding Shuttle Double Slash, Backstep Arrow Loose"),
                line(&[Light, Heavy, Heavy, Heavy, Heavy], "Cross Sword Edge  >  Horizontal Stay Sword Edge", "sequence C, after B"),
                line(&[sprint, Light], "Triple Slash", "from a sprint"),
                line(&[sprint, Heavy], "Double Upward Slash", "from a sprint"),
                line(&[Dodge, Light], "Double Horizontal Slash", "out of a roll"),
                line(&[Dodge, Light], "Deep Stinger", "out of a backstep"),
                line(&[Jump, Light], "Cross Double Slash", ""),
                line(&[Jump, Heavy], "Downward Double Slash", ""),
            ]},
            Section { title: "TWO-HANDED", lines: vec![
                line(&[two_hand, Light], "Two-handed", ""),
                line(&[Light, Light, Light], "Chasing Slash", "keep pressing"),
                line(&[Heavy, Heavy], "Stinger  >  Vertical Sword Edge", "sequence D; hold the second: Backstep Arrow Loose"),
                line(&[Light, Heavy, Heavy], "Guiding Rotary Slash  >  Backward Jump", "sequence E: the jump throws blades"),
                line(&[Light, Heavy, Heavy, Heavy], "Rotary Carian Greatsword  >  Double Rotary", "sequence F: spins that can't be staggered"),
            ]},
            Section { title: "ASH OF WAR: QUICKSTEP (YOUR TARNISHED)", lines: vec![
                line(&[Skill], "Parry", "without a lock-on, or forward"),
                line(&[Skill], "Loretta's Rain  /  Eochaid's Dancing Blade  /  Grandeur Caria", "locked on: left / right / back"),
            ]},
        ],
        Weapon::Katana => vec![
            Section { title: "ONE-HANDED", lines: vec![
                line(&[Light, Light, Light], "Nine Slashes", ""),
                line(&[Heavy, Heavy], "Parry  >  Back Slash", "sequence A; hold the second: Ryuusen"),
                line(&[Light, Heavy, Heavy], "Step Slash  >  Jump Slash", "sequence B; held: Gatsu, Heavy Slash"),
                line(&[Dodge, Heavy, Heavy], "Owl Dash  >  Triple Cut", "sequence C, out of a backstep; held: Souryuusen, Onmai"),
                line(&[sprint, Light], "Senpuukiri", "from a sprint"),
                line(&[sprint, Heavy], "Jump Sting", "from a sprint"),
                line(&[Dodge, Light], "Sabimaru", "out of a roll"),
                line(&[Dodge, Light], "Let's Get To It", "out of a backstep: uses up your FP, a light strike gets 40% back"),
                line(&[Jump, Light], "Triple Mirage", ""),
                line(&[Jump, Heavy], "Diving Slash", "lands fast, with thunder"),
            ]},
            Section { title: "TWO-HANDED", lines: vec![
                line(&[two_hand, Light], "Two-handed", ""),
                line(&[Light, Light, Light], "Rapid Slash", "FP comes back as you slash"),
                line(&[Light, Heavy, Heavy], "Air Dash Slash  /  Spirit Slash", "sequence D; held: Miyavi, Frozen Burst"),
                line(&[Heavy, Heavy], "Triple Cut  >  Spirit Slash", "sequence E; held: Heavy Raise, Isshin"),
                line(&[Light, Heavy, Heavy, Heavy], "Rising Star  >  Air Dash Slash", "sequence F; held: Waterfowl Dance, Setsunahyouka"),
                line(&[Jump, Light], "Air Judgement Cut", ""),
            ]},
            Section { title: "JUDGEMENT CUT AND ASH OF WAR: UNSHEATHE", lines: vec![
                line(&[Heavy], "Judgement Cut", "when the Carian emblem shows after a move on the ground (Light there: combo C)"),
                line(&[Heavy], "Hell's Gate", "when the emblem shows after a move in the air"),
                line(&[Skill, Light], "Space Slash", "hold the skill, with FP"),
                line(&[Skill, Heavy], "Frayed Blade", "hold the skill, with FP"),
                line(&[Skill, Light], "Octoslash", "hold the skill with no FP left"),
            ]},
        ],
        Weapon::Greatsword => vec![
            Section { title: "ONE-HANDED (ONE SWORD)", lines: vec![
                line(&[Light, Heavy], "Upward Cut  >  Upward Slash", "sequence A: RT after the first light attack"),
                line(&[Light, Light, Heavy], "Pierce  >  Horizontal Slash", "sequence B: RT after the second; held: Giant Hunt, Strong Horizontal Slash"),
                line(&[Light, Light, Light, Heavy], "Smash  >  The Queen's Black Flame", "sequence C: RT after the third; held: Twin Slash, Blackblade"),
                line(&[sprint, Light], "Wild Strikes", "from a sprint"),
                line(&[sprint, Heavy], "Double Thrust", "from a sprint: into the two-handed grip"),
                line(&[Dodge, Light], "Lion Slash", "out of a roll"),
                line(&[Dodge, Light], "Shouryuu Kiri", "out of a backstep: attacks follow in the air"),
                line(&[Jump, Light], "Smash", ""),
                line(&[Jump, Heavy], "Double Slash", "into the two-handed grip"),
            ]},
            Section { title: "TWO-HANDED (TWO SWORDS)", lines: vec![
                line(&[two_hand, Light], "Double sword mode", "RT when the storm emblem shimmers after a move: a guard counter that switches grip"),
                line(&[Light, Heavy, Heavy], "Double Smash  >  Double Smash", "sequence D: RT after the first light attack"),
                line(&[Light, Light, Heavy, Heavy], "Double Slash  >  Leap Double Smash", "sequence E: after the second"),
                line(&[Light, Light, Light, Heavy, Heavy], "Double Thrust  >  Leap Double Smash", "sequence F: after the third"),
                line(&[Dodge, Light], "Double Slash", "out of a roll"),
                line(&[Dodge, Light], "Upward Double Slash", "out of a backstep"),
            ]},
            Section { title: "RED FLAME AND ASH OF WAR: STAMP", lines: vec![
                line(&[], "Red Flame", "some one-handed heavies cost 10% FP and burn away the enemy's HP; no dodging out of moves (the skill cancels them)"),
                line(&[Skill], "Stamp", "avoids damage and parries; RT after it: Back Smash. With no FP: Soul of Cinder's slashes"),
            ]},
        ],
        Weapon::CurvedGreatsword => vec![
            Section { title: "ONE-HANDED", lines: vec![
                line(&[Light, Light], "Sword attack", ""),
                line(&[Heavy, Heavy], "Slash Gun Shot  >  Gunslinger Back", "sequence A; held: Showdown, Triple Shoot"),
                line(&[Light, Heavy, Heavy], "Sliding Slash  >  Rotary Slash", "sequence B: both avoid damage; held: Ex High Roller Spin (Exceed), Shuffle"),
                line(&[Light, Heavy, Heavy, Heavy], "Bloody Queen  >  Bloodhound's Finesse", "sequence C; held: Overdrive, Maximum Bet (Exceed)"),
                line(&[sprint, Light], "Punch Line", "from a sprint: board flight"),
                line(&[sprint, Heavy], "Hard Way", "from a sprint (also out of a roll)"),
                line(&[Dodge, Light], "Streak Chasing", "out of a backstep"),
                line(&[Jump, Light], "Payline", ""),
                line(&[Jump, Heavy], "Double Down", "into the two-handed grip"),
            ]},
            Section { title: "TWO-HANDED", lines: vec![
                line(&[two_hand, Light], "Two-handed", ""),
                line(&[Light, Light], "Ex Calibur  >  air slashes", ""),
                line(&[Heavy, Heavy], "Twin Rotary Slash  >  Triple Slash", "sequence D; held: Gunslinger Front, Ex High Roller"),
                line(&[sprint, Light, Heavy], "Board flight", "sequence E: hold RT to climb, tap to dive; attack or dodge any time"),
                line(&[Light, Heavy, Heavy], "Roulette Spin  >  Boost Knuckle", "sequence F; held: Air Provoke, Punch Line"),
            ]},
            Section { title: "EXCEED AND ASH OF WAR: SQUARE OFF", lines: vec![
                line(&[], "Exceed state", "30 seconds: fire on the attacks, stronger moves"),
                line(&[Skill, Light], "Shoot", "parries attacks within 10 m"),
                line(&[Skill, Heavy], "Ex Shuffle", "avoids damage before the slash"),
            ]},
        ],
        Weapon::Twinblade => vec![
            Section { title: "ONE-HANDED (TWINBLADE)", lines: vec![
                line(&[Light, Light, Light, Light], "Blade Dance", "RT during the first four: sequence B; after: sequence E"),
                line(&[Heavy, Heavy], "Jump-pierce  >  Kaisengou", "sequence A; held: Farewell"),
                line(&[Light, Heavy, Heavy], "Sabimaru  >  Another Time", "sequence B; held: Sword Blew"),
                line(&[Light, Heavy, Heavy, Heavy], "Bloodblade Dance  >  Horizontal Slash", "sequence C; held: Rankiri, Cursed-Blood Slice"),
                line(&[sprint, Light], "Thrusting Edge", "from a sprint"),
                line(&[sprint, Heavy], "Wind Split", "from a sprint: into the dual swords"),
                line(&[Dodge, Light], "Bloodhound Slash", "out of a roll"),
                line(&[Dodge, Light], "Bloody Claw", "out of a backstep: a throw (people and Malenia)"),
                line(&[Jump, Light], "Descending Thrust", ""),
                line(&[Jump, Heavy], "Jumping Double Slash", "into the dual swords"),
            ]},
            Section { title: "TWO-HANDED (DUAL SWORDS)", lines: vec![
                line(&[two_hand, Light], "Dual sword mode", ""),
                line(&[Heavy, Heavy], "Sabimaru  >  Dual Blood", "sequence D; held: Round Flame"),
                line(&[Light, Heavy, Heavy], "Sabimaru  >  Upward Cut", "sequence E; held: Greeting Shift, Back Slash"),
                line(&[Light, Heavy, Heavy, Heavy], "Dual Sting  >  Meteoroid", "sequence F; held: Fire Storm, Forgotten Rook"),
            ]},
            Section { title: "FIRE EMBLEM AND ASH OF WAR: BLOODHOUND'S STEP", lines: vec![
                line(&[Light], "Wind Split  /  Thrusting Edge", "when the fire emblem shows after a thrown-blade heavy, moving forward / back (RT: Blade Gate / Gift)"),
                line(&[Skill], "Pole vault, air jumps and an air dance", "keep pressing in the air; attack to land safely"),
            ]},
        ],
    }
}
