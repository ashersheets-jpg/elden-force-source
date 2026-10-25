//! Each ability's damage is exactly what the fighter select shows (abilities.dmg_mult x attack
//! rating), at most, to every enemy it hits. While a move plays (and a moment after), every HP loss of
//! every enemy around is counted; what goes past the move's damage is given back. Elden Ring's bullets
//! hit a varying number of times (some tick many times a second) and the attack animations the moves
//! play also swing the player's own weapon, so without this the same move would deal very different
//! damage from fighter to fighter and weapon to weapon.
//!
//! 0.16.0: when the counting stops the move's bullets are muted (lingering flames and fields burned on
//! at full damage, uncounted: Asta's Black Meteorite one-shot bosses), an Awakening takes at most
//! BOSS_ULT_SHARE of a boss's health (and its bullets go quiet once it has), and the swing grabs
//! (Deku's Black Whip kick, Luffy's Gum-Gum Rocket) always land their damage on the enemy they pulled
//! in.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eldenring::cs::{CSFeManImp, FieldInsHandle, PlayerIns, WorldChrMan};
use fromsoftware_shared::FromStatic;


/// After the move's script ends, its bullets are still out for a moment.
const AFTER: Duration = Duration::from_millis(1500);

/// 0.16.0: an Awakening takes at most this share of a boss's (difficulty-raised) health.
pub const BOSS_ULT_SHARE: f32 = 0.3;

/// A swing grab's damage reaches its enemy (within this many metres when the move ends) at least
/// this share of the full move.
const SURE_HIT_M: f32 = 8.0;
const SURE_HIT_SHARE: f32 = 0.9;

#[derive(Default)]
pub struct Ledger {
    /// the move's damage to one enemy, and when counting stops (None = idle)
    cap: i32,
    until: Option<Instant>,
    /// per enemy (ChrIns address): damage counted so far, HP last frame
    dealt: HashMap<usize, i32>,
    last_hp: HashMap<usize, i32>,
    pub refunded: i32,
    label: &'static str,
    /// the move's bullets and element (muted when it's over)
    bullets: Vec<i32>,
    element: &'static str,
    ultimate: bool,
    sure_hit: bool,
    muted: bool,
    /// the last training-bag heal seen (training.rs)
    refill_seen: u64,
    /// 0.16.0: a punch: each enemy it touches takes exactly `cap` at once
    exact: bool,
}

/// 0.16.0: every character but the player is counted. Only team types 6 and 7 were before, and many
/// bosses (Wormface, in the user's test) have others: their damage went uncounted and uncapped, and a
/// single Spirit Bomb killed him.
fn countable(c: &eldenring::cs::ChrIns) -> bool {
    c.modules.data.max_hp > 0
}

/// Lab and tuning: JR_MEASURE=1 logs every move's real damage and never caps or mutes it.
fn measuring() -> bool {
    static M: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *M.get_or_init(|| std::env::var_os("JR_MEASURE").is_some())
}

/// The enemies with a boss health bar on screen.
pub fn bosses() -> Vec<FieldInsHandle> {
    let Ok(fe) = (unsafe { CSFeManImp::instance() }) else { return Vec::new() };
    fe.boss_health_displays.iter().filter(|b| b.fmg_id > 0 && !b.field_ins_handle.is_empty()).map(|b| b.field_ins_handle).collect()
}

/// 1.3.0: one of the player's moves or punches is being counted (set by any ledger this frame; villains.rs reads and clears
/// it): a hit with no attacker then may be the player's (their blows and the ledger's top-ups leave none)
pub static PLAYER_ATTACKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Ledger {
    /// 0.16.0: one of the fighter's own attacks (RB / LB / RT): every enemy it touches in the next
    /// moment takes exactly `damage`, whatever its defenses (the user's targets: RB and LB about 8% of
    /// Elemer's health on Hard at level 54, RT about 14%).
    pub fn start_punch(&mut self, label: &'static str, damage: i32) {
        self.start(label, damage, Vec::new(), "physical", false, false);
        self.exact = true;
        self.until = Some(Instant::now() + Duration::from_millis(1600));
    }

    /// 1.2.0: a punch's window stays open while its attack still plays (Carian Combo Warriors' long moves,
    /// a Kick 13 string or a late explosion, outlast the 1.6 s: their later hits counted at Elden Ring's own
    /// damage, beside the balance)
    pub fn hold(&mut self, secs: f32) {
        if self.exact {
            if let Some(u) = self.until {
                self.until = Some(u.max(Instant::now() + Duration::from_secs_f32(secs)));
            }
        }
    }

    /// A move begins: `cap` damage to each enemy at most. `bullets` carry its damage (already written).
    pub fn start(&mut self, label: &'static str, cap: i32, bullets: Vec<i32>, element: &'static str, ultimate: bool, sure_hit: bool) {
        // the move it cuts short: its bullets that this one doesn't use go quiet
        if !self.muted && !self.bullets.is_empty() {
            let old: Vec<i32> = self.bullets.iter().copied().filter(|b| !bullets.contains(b)).collect();
            crate::damage::mute(&old, self.element);
        }
        self.label = label;
        self.cap = cap.max(1);
        self.until = Some(Instant::now() + Duration::from_secs(10));
        self.dealt.clear();
        self.refunded = 0;
        self.bullets = bullets;
        self.element = element;
        self.ultimate = ultimate;
        self.sure_hit = sure_hit;
        self.muted = false;
        self.exact = false;
        self.snapshot_all();
    }

    /// The move's script ended: keep counting for a moment. A swing grab lands its damage now, and
    /// every enemy the move hit gets what it lists (0.16.0, below).
    pub fn ending(&mut self, player: &PlayerIns) {
        if let Some(u) = self.until {
            self.until = Some(u.min(Instant::now() + AFTER));
            if std::mem::take(&mut self.sure_hit) && !measuring() {
                self.land_grab(player);
            }
            if !measuring() && !self.label.starts_with("attack") {
                self.land_listed(SURE_HIT_SHARE);
            }
        }
    }

    /// 0.16.0: every enemy a move hit takes the damage the fighter select lists (SURE_HIT_SHARE of it
    /// when the script ends; its last hits make up the rest, up to the cap). Elden Ring's defense
    /// formula cuts a hit that is small next to the target's defense to as little as a tenth, so a move
    /// of many small hits (Kamehameha, the Gatling, Jet Burn) dealt a fraction of a one-hit move of the
    /// same listing (Oklahoma Smash, Rasengan): the user's punching-bag test. Now the listing is the
    /// damage. Bosses: at most BOSS_ULT_SHARE for an Awakening, and bosses.rs scales it by the
    /// difficulty like any hit.
    fn land_listed(&mut self, share: f32) {
        let Ok(wcm) = (unsafe { WorldChrMan::instance() }) else { return };
        let bosses = bosses();
        let mult = crate::bosses::boss_hp_mult();
        let hit: Vec<(usize, i32)> = self.dealt.iter().filter(|(_, d)| **d > 0).map(|(k, d)| (*k, *d)).collect();
        for (key, dealt) in hit {
            let Some(e) = crate::chars::near(wcm, 64).into_iter().find(|e| e.chr_ins.as_ptr() as usize == key) else { continue };
            let c = unsafe { &mut *e.chr_ins.as_ptr() };
            let hp = c.modules.data.hp;
            if hp <= 0 {
                continue;
            }
            let boss = bosses.contains(&c.field_ins_handle);
            let cap = if boss && self.ultimate { self.cap.min((c.modules.data.max_hp as f32 * BOSS_ULT_SHARE * mult) as i32) } else { self.cap };
            let want = (cap as f32 * share) as i32;
            if dealt >= want {
                continue;
            }
            let add = want - dealt;
            if boss {
                // 1.1.0: through the difficulty, and never a kill by itself (bosses.rs decides)
                crate::bosses::hit_boss(c, add);
                *self.dealt.entry(key).or_insert(0) += add;
            } else {
                c.modules.data.hp = (hp - add).max(0);
                *self.dealt.entry(key).or_insert(0) += hp - c.modules.data.hp;
            }
            self.last_hp.insert(key, c.modules.data.hp);
            crate::log!("damage {}: {add} more to reach its listed damage ({dealt} landed)", self.label);
        }
    }

    /// The most this move deals one enemy.
    pub fn cap(&self) -> i32 {
        self.cap
    }

    /// 1.1.0: what the move may still deal the enemy it has hurt most (all of it when idle).
    pub fn room(&self) -> i32 {
        if !self.active() {
            return i32::MAX / 4;
        }
        (self.cap - self.dealt.values().max().copied().unwrap_or(0)).max(0)
    }

    pub fn active(&self) -> bool {
        self.until.is_some_and(|u| Instant::now() < u)
    }

    fn mute(&mut self) {
        if !self.muted && !measuring() {
            self.muted = true;
            crate::damage::mute(&self.bullets, self.element);
        }
    }

    /// The enemy the grab pulled the fighter to (the nearest living one within SURE_HIT_M) takes
    /// what the move's bullets didn't deal it (Luffy's Gum-Gum Rocket dealt 10: its strike bullets
    /// mostly missed the enemy right in front of him).
    fn land_grab(&mut self, player: &PlayerIns) {
        let Ok(wcm) = (unsafe { WorldChrMan::instance() }) else { return };
        let me = &player.chr_ins as *const _ as usize;
        let p = player.chr_ins.modules.physics.position;
        let mut best: Option<(f32, usize)> = None;
        for e in crate::chars::near(wcm, 32).iter() {
            let key = e.chr_ins.as_ptr() as usize;
            let c = unsafe { e.chr_ins.as_ref() };
            if key == me || !countable(c) || c.modules.data.hp <= 0 {
                continue;
            }
            let q = c.modules.physics.position;
            let d = ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2) + (q.2 - p.2).powi(2)).sqrt();
            if d < SURE_HIT_M && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, key));
            }
        }
        let Some((_, key)) = best else { return };
        let dealt = self.dealt.get(&key).copied().unwrap_or(0);
        let want = (self.cap as f32 * SURE_HIT_SHARE) as i32;
        if dealt >= want {
            return;
        }
        let c = unsafe { &mut *(key as *mut eldenring::cs::ChrIns) };
        let add = want - dealt;
        let hp = c.modules.data.hp;
        if bosses().contains(&c.field_ins_handle) {
            crate::bosses::hit_boss(c, add);
            *self.dealt.entry(key).or_insert(0) += add;
        } else {
            c.modules.data.hp = (hp - add).max(0);
            *self.dealt.entry(key).or_insert(0) += hp - c.modules.data.hp;
        }
        self.last_hp.insert(key, c.modules.data.hp);
        crate::log!("damage {}: grab landed {add} on its enemy ({dealt} from the move itself)", self.label);
    }

    fn snapshot_all(&mut self) {
        self.last_hp.clear();
        let Ok(wcm) = (unsafe { WorldChrMan::instance() }) else { return };
        for e in crate::chars::near(wcm, 64).iter() {
            let c = unsafe { e.chr_ins.as_ref() };
            if countable(c) {
                self.last_hp.insert(e.chr_ins.as_ptr() as usize, c.modules.data.hp);
            }
        }
    }

    pub fn tick(&mut self, player: &PlayerIns) {
        if self.active() {
            PLAYER_ATTACKING.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if !self.active() {
            if self.until.is_some() {
                // hits that landed after the script ended (a slow projectile, a beam's tail) count too; then
                // (1.3.0, the damage check: a third of the moves ended on exactly 90% of their listing, their last
                // hits already spent) every enemy the move hit takes the whole listing
                if !measuring() && !self.label.starts_with("attack") {
                    self.land_listed(1.0);
                }
            }
            if self.until.take().is_some() {
                self.mute();
                let best = self.dealt.values().max().copied().unwrap_or(0);
                crate::log!("damage {}: {} enemies hit, most {best} of {} (gave back {})", self.label, self.dealt.len(), self.cap, self.refunded);
            }
            return;
        }
        // 1.1.0: what bosses.rs counted beyond a blow the "can't die" switch stopped at 1 HP
        {
            let mut ex = crate::bosses::FLOOR_EXTRA.lock().unwrap();
            ex.retain(|(k, v)| match self.dealt.get_mut(k) {
                Some(d) => {
                    *d += *v;
                    false
                }
                None => true,
            });
            ex.truncate(16);
        }
        // the training bag was healed: its new HP is no hit taken back
        {
            let r = crate::training::REFILL.lock().unwrap();
            if r.0 != self.refill_seen {
                self.refill_seen = r.0;
                if self.last_hp.contains_key(&r.1) {
                    self.last_hp.insert(r.1, r.3);
                }
            }
        }
        let Ok(wcm) = (unsafe { WorldChrMan::instance() }) else { return };
        let bosses = bosses();
        let me = &player.chr_ins as *const _ as usize;
        for e in crate::chars::near(wcm, 64).iter() {
            let key = e.chr_ins.as_ptr() as usize;
            if key == me {
                continue;
            }
            let c = unsafe { &mut *e.chr_ins.as_ptr() };
            if !countable(c) {
                continue;
            }
            // an Awakening on a boss: at most BOSS_ULT_SHARE of its health
            let is_boss = bosses.contains(&c.field_ins_handle);
            let boss = is_boss && self.ultimate;
            // (bosses take 1/boss_hp_mult of each hit after this: the share is of their raised health)
            let share = c.modules.data.max_hp as f32 * BOSS_ULT_SHARE * crate::bosses::boss_hp_mult();
            let cap = if boss { self.cap.min(share as i32).max(1) } else { self.cap };
            let hp = c.modules.data.hp;
            if let Some(&old) = self.last_hp.get(&key) {
                // an enemy that died keeps its death; a living one gets back what went past the cap
                if hp < old && hp > 0 {
                    let d = self.dealt.entry(key).or_insert(0);
                    *d += old - hp;
                    // 0.16.0 punches: the first touch on an enemy deals the whole fixed damage at once
                    if self.exact && *d < cap && !measuring() {
                        let more = cap - *d;
                        let now = c.modules.data.hp;
                        if is_boss {
                            // 1.1.0: through the difficulty (bosses.rs)
                            crate::bosses::hit_boss(c, more);
                            *d += more;
                        } else {
                            c.modules.data.hp = (now - more).max(1);
                            *d += now - c.modules.data.hp;
                        }
                    }
                    if *d > cap && !measuring() {
                        let back = (*d - cap).min(old - hp);
                        c.modules.data.hp = (hp + back).min(c.modules.data.max_hp);
                        *d -= back;
                        self.refunded += back;
                    }
                    if boss && *d >= cap && !self.muted && !measuring() {
                        crate::log!("damage {}: a boss took its share ({cap}): the move goes quiet", self.label);
                        self.mute();
                    }
                } else if hp < old {
                    *self.dealt.entry(key).or_insert(0) += old - hp;
                }
            }
            self.last_hp.insert(key, c.modules.data.hp);
        }
    }
}
