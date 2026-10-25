//! 0.16.0: Training (fighter select: T / RT). The enemy you lock onto becomes a punching bag: it stands
//! still, never attacks, and is healed back to full after every hit, so it never dies. Every hit's
//! damage is shown on screen, grouped by the move that dealt it (an ability by name, or the fighter's
//! own RB / RT / LB attacks), and written to %LOCALAPPDATA%\JumpRing\training.csv, for balancing every
//! fighter's every move against the same target. Damage is what the game really dealt, after the
//! move's cap (ledger.rs) and the target's defenses.

use std::io::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use eldenring::cs::{ChrIns, PlayerIns, WorldChrMan};
use fromsoftware_shared::FromStatic;

use crate::log;

/// A move's hits end a row after this long with nothing landing.
const GAP: Duration = Duration::from_millis(3000);
const ROWS: usize = 8;

/// One move's damage to the punching bag.
#[derive(Clone)]
pub struct Row {
    pub what: String,
    pub damage: i32,
    pub hits: u32,
}

/// What the panel shows (overlay.rs draws it).
pub struct Panel {
    pub on: bool,
    pub target: Option<String>,
    pub rows: Vec<Row>,
}

pub static PANEL: Mutex<Panel> = Mutex::new(Panel { on: false, target: None, rows: Vec::new() });

/// 0.16.0 fix: the bag is healed only when it runs low, and then the ledger (move caps) and bosses.rs
/// (boss health) are told its new HP, so they never take the heal for a missed hit. (Healing it every
/// frame made both lose track: some hits showed uncapped, some up to 2.7x too big.) (generation, bag
/// ChrIns address, its handle, HP it was set to)
pub static REFILL: Mutex<(u64, usize, Option<eldenring::cs::FieldInsHandle>, i32)> = Mutex::new((0, 0, None, 0));

fn refill(key: usize, c: &mut ChrIns) {
    c.modules.data.hp = c.modules.data.max_hp;
    let mut r = REFILL.lock().unwrap();
    *r = (r.0 + 1, key, Some(c.field_ins_handle), c.modules.data.hp);
}

/// The bag is healed when it falls under this share of its health (it can't die while Training is on).
const REFILL_BELOW: f32 = 0.4;

#[derive(Default)]
pub struct Training {
    /// the punching bag (ChrIns address), whether it could move and act before, and its HP at the end of
    /// last frame (after the move caps and boss health had their say)
    bag: Option<(usize, (bool, bool), i32)>,
    /// the move being counted and when it last landed
    current: Option<(String, Instant)>,
    /// the fighter's own attack last pressed, and when
    pressed: Option<(&'static str, Instant)>,
    /// the game's "enemies can't die" switch is ours
    no_dead: bool,
}

/// Lab: the enemy with the most health within `r` m (the bag; lab scripts can't lock on).
#[cfg(feature = "lab")]
pub fn biggest_near(player: &PlayerIns, r: f32) -> Option<(f32, f32, f32)> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let me = &player.chr_ins as *const _ as usize;
    let p = player.chr_ins.modules.physics.position;
    let mut best: Option<(i32, (f32, f32, f32))> = None;
    for e in crate::chars::near(wcm, 64).iter() {
        let c = unsafe { e.chr_ins.as_ref() };
        if e.chr_ins.as_ptr() as usize == me || c.modules.data.hp <= 0 {
            continue;
        }
        let q = c.modules.physics.position;
        if ((q.0 - p.0).powi(2) + (q.2 - p.2).powi(2)).sqrt() < r && best.is_none_or(|(m, _)| c.modules.data.max_hp > m) {
            best = Some((c.modules.data.max_hp, (q.0, q.1, q.2)));
        }
    }
    best.map(|(_, q)| q)
}

fn find_locked(player: &PlayerIns) -> Option<usize> {
    // lab scripts can't lock on: the nearest enemy within 8 m stands in
    #[cfg(feature = "lab")]
    let t = if player.chr_ins.is_locked_on {
        player.chr_ins.lock_on_target_position
    } else {
        let near = biggest_near(player, 15.0)?;
        fromsoftware_shared::F32Vector4(near.0, near.1, near.2, 0.0)
    };
    #[cfg(not(feature = "lab"))]
    let t = {
        if !player.chr_ins.is_locked_on {
            return None;
        }
        player.chr_ins.lock_on_target_position
    };
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let me = &player.chr_ins as *const _ as usize;
    let mut best: Option<(f32, usize)> = None;
    for e in crate::chars::near(wcm, 64).iter() {
        let key = e.chr_ins.as_ptr() as usize;
        let c = unsafe { e.chr_ins.as_ref() };
        if key == me || c.modules.data.max_hp <= 0 || c.modules.data.hp <= 0 {
            continue;
        }
        let p = c.modules.physics.position;
        let d = ((p.0 - t.0).powi(2) + (p.2 - t.2).powi(2)).sqrt();
        if d < 4.0 && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, key));
        }
    }
    best.map(|(_, k)| k)
}

fn alive(key: usize) -> Option<&'static mut ChrIns> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    crate::chars::near(wcm, 96).iter().find(|e| e.chr_ins.as_ptr() as usize == key).map(|e| unsafe { &mut *e.chr_ins.as_ptr() })
}

fn csv_line(line: &str) {
    let Some(dir) = std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("JumpRing")) else { return };
    let path = dir.join("training.csv");
    let new = !path.exists();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        if new {
            let _ = writeln!(f, "time,fighter,level,move,damage,hits,target_max_hp");
        }
        let _ = writeln!(f, "{line}");
    }
}

impl Training {
    fn release(&mut self) {
        if let Some((key, (movement, actions), _)) = self.bag.take() {
            if let Some(c) = alive(key) {
                c.debug_flags.set_disabled_movement(movement);
                c.debug_flags.set_disabled_secondary_actions(actions);
            }
            log!("training: punching bag released");
        }
        PANEL.lock().unwrap().target = None;
    }

    /// Every frame in the world, after the moves and bosses. `on`: the switch; `ability`: the move
    /// playing now (name), if any.
    pub fn tick(&mut self, player: &PlayerIns, on: bool, ability: Option<&'static str>, fighter: &str) {
        PANEL.lock().unwrap().on = on;
        // while Training is on no enemy dies (a big hit on a small bag would kill it): the game's own
        // "enemies can't die" switch, put back when Training goes off
        if on != self.no_dead {
            if let Ok(f) = unsafe { eldenring::cs::WorldChrManDbgFlags::instance_mut() } {
                f.all_no_dead = on;
            }
            self.no_dead = on;
        }
        if !on {
            self.release();
            self.current = None;
            return;
        }
        // the fighter's own attacks, by the button pressed
        let p = player.chr_ins.modules.action_request.new_action_presses;
        let now = Instant::now();
        if p.r1() {
            self.pressed = Some(("RB attack", now));
        } else if p.r2() {
            self.pressed = Some(("RT heavy attack", now));
        } else if p.l1() {
            self.pressed = Some(("LB attack", now));
        }
        // the bag: the enemy locked onto (kept while it lives)
        if let Some(key) = find_locked(player) {
            if self.bag.is_none_or(|(k, _, _)| k != key) {
                self.release();
                if let Some(c) = alive(key) {
                    let before = (c.debug_flags.disabled_movement(), c.debug_flags.disabled_secondary_actions());
                    c.debug_flags.set_disabled_movement(true);
                    c.debug_flags.set_disabled_secondary_actions(true);
                    refill(key, c);
                    self.bag = Some((key, before, c.modules.data.max_hp));
                    let max = c.modules.data.max_hp;
                    // a hit bigger than the bag's health only shows as its health
                    PANEL.lock().unwrap().target = Some(if max < 1500 { format!("{max} HP (small: pick a bigger enemy)") } else { format!("{max} HP") });
                    log!("training: punching bag set ({} HP)", c.modules.data.max_hp);
                }
            }
        }
        let Some((key, flags, last)) = self.bag else { return };
        let Some(c) = alive(key) else {
            self.bag = None;
            PANEL.lock().unwrap().target = None;
            return;
        };
        let max = c.modules.data.max_hp;
        let hp = c.modules.data.hp;
        c.debug_flags.set_disabled_movement(true);
        c.debug_flags.set_disabled_secondary_actions(true);
        if max <= 0 {
            return;
        }
        // heal it when low, telling the ledger and bosses.rs
        let hp = if (hp as f32) < max as f32 * REFILL_BELOW {
            refill(key, c);
            self.bag = Some((key, flags, max));
            if hp < last { hp } else { return }
        } else {
            self.bag = Some((key, flags, hp));
            hp
        };
        if hp >= last {
            if self.current.as_ref().is_some_and(|(_, t)| t.elapsed() > GAP) {
                self.current = None;
            }
            return;
        }
        // a boss took 1/mult of the hit (bosses.rs, the difficulty's boss health): the move's own damage
        // is shown, and the header says what a boss takes of it
        let boss = crate::ledger::bosses().contains(&c.field_ins_handle);
        let mult = if boss { crate::bosses::boss_hp_mult() } else { 1.0 };
        let dmg = ((last - hp) as f32 * mult).round() as i32;
        if boss {
            let mut p = PANEL.lock().unwrap();
            if p.target.as_ref().is_some_and(|t| !t.contains("boss")) {
                p.target = Some(format!("{max} HP boss (takes 1/{mult} of each hit)"));
            }
        }
        // which move: the ability playing, else the attack button pressed last (within 2 s)
        // (a move's beam or flames still landing after it ended count for it, unless an attack
        // button was pressed since)
        let pressed_recent = self.pressed.is_some_and(|(_, t)| t.elapsed() < Duration::from_millis(700));
        let lingering = self.current.as_ref().filter(|(w, t)| t.elapsed() < GAP && !w.ends_with("attack")).map(|(w, _)| w.clone());
        let what = match (ability, lingering) {
            (Some(a), _) => a.to_string(),
            (None, Some(w)) if !pressed_recent => w,
            _ => match self.pressed {
                Some((b, t)) if t.elapsed() < Duration::from_secs(2) => b.to_string(),
                _ => "other".to_string(),
            },
        };
        let same = self.current.as_ref().is_some_and(|(w, t)| *w == what && t.elapsed() < GAP)
            // each press of an attack button is its own row
            && !(ability.is_none() && self.pressed.is_some_and(|(_, t)| t.elapsed() < Duration::from_millis(60)));
        let level = unsafe { player.player_game_data.as_ref().level };
        let mut panel = PANEL.lock().unwrap();
        if same {
            if let Some(r) = panel.rows.first_mut() {
                r.damage += dmg;
                r.hits += 1;
            }
        } else {
            panel.rows.insert(0, Row { what: what.clone(), damage: dmg, hits: 1 });
            panel.rows.truncate(ROWS);
        }
        self.current = Some((what.clone(), now));
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        csv_line(&format!("{secs},{fighter},{level},\"{what}\",{dmg},1,{max}{}", if boss { ",boss" } else { "" }));
        log!("training: {what} hit for {dmg}");
    }
}
