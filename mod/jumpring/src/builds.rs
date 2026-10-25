//! 0.15.0: two build sheets on one Elden Ring character.
//!
//! Your Tarnished keeps the character's own attributes, all eight, built however the player likes. The
//! eight JUMP FORCE fighters share a sheet of their own: points only in Vigor, Mind and Endurance, the
//! other five held at 10 (fighter damage follows level only: damage.rs, weapon.rs). Level and runes are
//! one, shared: a level gained on one sheet is a free point on the other, spent on the Build page of
//! the JUMP FORCE menu (or on the game's own level-up screen, for the sheet in use). A point a fighter
//! puts in a locked attribute at a grace comes back as a free point.
//!
//! Free points are not stored: they are what the level leaves over, level + 79 minus the sheet's sum
//! (Elden Ring's level is the attribute total minus 79). The sheets live per character in
//! %LOCALAPPDATA%/JumpRing/characters/<slot>-<name>.txt; the game's own attribute fields hold the sheet
//! in use (and are saved with it).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eldenring::cs::{GameMan, PlayerGameData};
use fromsoftware_shared::FromStatic;

use crate::log;

/// The attributes, in Elden Ring's order.
pub const NAMES: [&str; 8] = ["Vigor", "Mind", "Endurance", "Strength", "Dexterity", "Intelligence", "Faith", "Arcane"];
/// What a fighter's five locked attributes stay at.
pub const LOCKED: u32 = 10;
const MAX: u32 = 99;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sheet {
    Tarnished,
    Fighters,
}

#[derive(Default)]
pub struct Builds {
    /// whose character the sheets are (the file's key)
    key: Option<String>,
    tarnished: Option<[u32; 8]>,
    fighters: Option<[u32; 3]>,
    /// the sheet the game's attribute fields hold
    applied: Option<Sheet>,
    last_check: Option<Instant>,
}

fn attrs(p: &PlayerGameData) -> [u32; 8] {
    [p.vigor, p.mind, p.endurance, p.strength, p.dexterity, p.intelligence, p.faith, p.arcane]
}

fn set_attrs(p: &mut PlayerGameData, a: [u32; 8]) {
    p.vigor = a[0];
    p.mind = a[1];
    p.endurance = a[2];
    p.strength = a[3];
    p.dexterity = a[4];
    p.intelligence = a[5];
    p.faith = a[6];
    p.arcane = a[7];
    recompute(p);
}

/// Lab (2.0.3 trailer shoot): the character at `level` with the fighters' Vigor, Mind and Endurance (tick takes them
/// into the fighters' sheet when it is the one on).
pub fn lab_level(p: &mut PlayerGameData, level: u32, vme: [u32; 3]) {
    p.level = level;
    p.vigor = vme[0];
    p.mind = vme[1];
    p.endurance = vme[2];
    recompute(p);
}

/// Elden Ring's base HP / FP / stamina for the attributes (its CalcCorrectGraph curves), as the
/// level-up screen sets them.
fn recompute(p: &mut PlayerGameData) {
    let v = p.vigor.clamp(1, 99) as f32;
    let hp = if v <= 25.0 {
        300.0 + 500.0 * ((v - 1.0) / 24.0).powf(1.5)
    } else if v <= 40.0 {
        800.0 + 650.0 * ((v - 25.0) / 15.0).powf(1.1)
    } else if v <= 60.0 {
        1450.0 + 450.0 * (1.0 - (1.0 - (v - 40.0) / 20.0).powf(1.2))
    } else {
        1900.0 + 200.0 * (1.0 - (1.0 - (v - 60.0) / 39.0).powf(1.2))
    };
    let m = p.mind.clamp(1, 99) as f32;
    let fp = if m <= 15.0 {
        50.0 + 45.0 * ((m - 1.0) / 14.0)
    } else if m <= 35.0 {
        95.0 + 105.0 * ((m - 15.0) / 20.0)
    } else if m <= 60.0 {
        200.0 + 150.0 * (1.0 - (1.0 - (m - 35.0) / 25.0).powf(1.2))
    } else {
        350.0 + 100.0 * ((m - 60.0) / 39.0)
    };
    let e = p.endurance.clamp(1, 99) as f32;
    let st = if e <= 15.0 {
        80.0 + 25.0 * ((e - 1.0) / 14.0)
    } else if e <= 30.0 {
        105.0 + 25.0 * ((e - 15.0) / 15.0)
    } else if e <= 50.0 {
        130.0 + 25.0 * ((e - 30.0) / 20.0)
    } else {
        155.0 + 15.0 * ((e - 50.0) / 49.0)
    };
    p.base_max_hp = hp.floor() as u32;
    p.base_max_fp = fp.floor() as u32;
    p.base_max_stamina = st.floor() as u32;
}

/// Points the level leaves over on a sheet with this sum.
fn free_of(level: u32, sum: u32) -> u32 {
    (level + 79).saturating_sub(sum)
}

fn path(key: &str) -> PathBuf {
    crate::log::dir().join("characters").join(format!("{key}.txt"))
}

/// A per-character file next to the build sheets (`ext`: "grace", ...).
pub fn char_path(p: &PlayerGameData, ext: &str) -> PathBuf {
    crate::log::dir().join("characters").join(format!("{}.{ext}", key_of(p)))
}

fn key_of(p: &PlayerGameData) -> String {
    let slot = unsafe { GameMan::instance() }.map(|g| g.save_slot).unwrap_or(-1);
    let name: String = String::from_utf16_lossy(&p.character_name).chars().take_while(|c| *c != '\0').filter(|c| c.is_alphanumeric()).collect();
    format!("{slot}-{name}")
}

impl Builds {
    /// The player object went away (a load): read the sheets again on the way back.
    pub fn on_unload(&mut self) {
        self.key = None;
        self.last_check = None;
    }

    fn load(&mut self, p: &PlayerGameData) {
        let key = key_of(p);
        self.tarnished = None;
        self.fighters = None;
        self.applied = None;
        let text = std::fs::read_to_string(path(&key)).ok().filter(|t| t.contains("applied=")).or_else(|| std::fs::read_to_string(path(&key).with_extension("bak")).ok());
        if let Some(text) = text {
            for line in text.lines() {
                let nums = |v: &str| v.split(',').filter_map(|x| x.trim().parse::<u32>().ok()).collect::<Vec<_>>();
                if let Some(v) = line.strip_prefix("tarnished=") {
                    self.tarnished = nums(v).try_into().ok();
                } else if let Some(v) = line.strip_prefix("fighters=") {
                    self.fighters = nums(v).try_into().ok();
                } else if let Some(v) = line.strip_prefix("applied=") {
                    self.applied = match v.trim() {
                        "fighters" => Some(Sheet::Fighters),
                        "tarnished" => Some(Sheet::Tarnished),
                        _ => None,
                    };
                }
            }
        }
        let cur = attrs(p);
        // first time for this character: what it has now is Your Tarnished's build, and the fighters
        // start from its Vigor, Mind and Endurance (the rest of its points are theirs to place)
        if self.tarnished.is_none() || self.applied.is_none() {
            self.tarnished = Some(cur);
            self.applied = Some(Sheet::Tarnished);
            log!("builds: {key}: Your Tarnished's build taken from the character ({cur:?})");
        }
        if self.fighters.is_none() {
            let t = self.tarnished.unwrap();
            self.fighters = Some([t[0], t[1], t[2]]);
        }
        self.key = Some(key);
        self.save();
    }

    fn save(&self) {
        let (Some(key), Some(t), Some(f), Some(a)) = (&self.key, self.tarnished, self.fighters, self.applied) else { return };
        let j = |v: &[u32]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
        let text = format!("tarnished={}\nfighters={}\napplied={}\n", j(&t), j(&f), if a == Sheet::Fighters { "fighters" } else { "tarnished" });
        let p = path(key);
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        // written beside and swapped in, with the last good copy kept (the save holds only the sheet in
        // use: this file is the other one's only home)
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::copy(&p, p.with_extension("bak"));
            let _ = std::fs::rename(&tmp, &p);
        }
    }

    fn sheet_attrs(&self, s: Sheet) -> [u32; 8] {
        match s {
            Sheet::Tarnished => self.tarnished.unwrap_or([10; 8]),
            Sheet::Fighters => {
                let f = self.fighters.unwrap_or([10; 3]);
                [f[0], f[1], f[2], LOCKED, LOCKED, LOCKED, LOCKED, LOCKED]
            }
        }
    }

    /// Twice a second in the world: put the right sheet on the character, and take in what the game's
    /// own level-up screen changed. `want`: the sheet the current fighter uses.
    pub fn tick(&mut self, p: &mut PlayerGameData, want: Sheet) {
        if self.last_check.is_some_and(|t| t.elapsed() < Duration::from_millis(500)) {
            return;
        }
        self.last_check = Some(Instant::now());
        if p.level == 0 || p.vigor == 0 {
            return;
        }
        if self.key.as_deref() != Some(key_of(p).as_str()) {
            self.load(p);
        }
        let Some(applied) = self.applied else { return };
        // what the game holds now belongs to the applied sheet (a level-up, a rebirth)
        let cur = attrs(p);
        let mut changed = false;
        match applied {
            Sheet::Tarnished => {
                if self.tarnished != Some(cur) {
                    self.tarnished = Some(cur);
                    changed = true;
                }
            }
            Sheet::Fighters => {
                let f = [cur[0], cur[1], cur[2]];
                if self.fighters != Some(f) {
                    self.fighters = Some(f);
                    changed = true;
                }
                // a point put in a locked attribute comes back as a free point
                if cur[3..].iter().any(|x| *x != LOCKED) {
                    log!("builds: a fighter's point in a locked attribute ({:?}) is a free point again", &cur[3..]);
                    set_attrs(p, self.sheet_attrs(Sheet::Fighters));
                    changed = true;
                }
            }
        }
        if applied != want {
            set_attrs(p, self.sheet_attrs(want));
            self.applied = Some(want);
            changed = true;
            log!("builds: {} build on (free points {})", if want == Sheet::Fighters { "the fighters'" } else { "Your Tarnished's" }, self.free(p.level, want));
        }
        if changed {
            self.save();
        }
    }

    /// Free points on a sheet.
    pub fn free(&self, level: u32, s: Sheet) -> u32 {
        free_of(level, self.sheet_attrs(s).iter().sum())
    }

    /// The sheet's attributes (all eight; a fighter's locked five at 10).
    pub fn view(&self, s: Sheet) -> [u32; 8] {
        self.sheet_attrs(s)
    }

    /// Spend a free point on attribute `i` of sheet `s` (fighters: Vigor, Mind, Endurance only).
    pub fn spend(&mut self, p: &mut PlayerGameData, s: Sheet, i: usize) -> bool {
        if self.free(p.level, s) == 0 {
            return false;
        }
        match s {
            Sheet::Tarnished => {
                let Some(t) = self.tarnished.as_mut() else { return false };
                if i >= 8 || t[i] >= MAX {
                    return false;
                }
                t[i] += 1;
            }
            Sheet::Fighters => {
                let Some(f) = self.fighters.as_mut() else { return false };
                if i >= 3 || f[i] >= MAX {
                    return false;
                }
                f[i] += 1;
            }
        }
        if self.applied == Some(s) {
            set_attrs(p, self.sheet_attrs(s));
        }
        self.save();
        true
    }
}
