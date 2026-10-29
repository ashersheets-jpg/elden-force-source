//! Lab-only probes (feature "lab"): animation/movement trace, bullet and animation auditions, and a
//! dump of every ability bullet's lifetime fields. Lists live in lab/audition.txt and lab/anims.txt
//! ("<id> <name>" per line).

use eldenring::cs::{Bullet, PlayerIns, SoloParamRepository};
use fromsoftware_shared::FromStatic;

use crate::ability;
use crate::log;

#[derive(Default)]
pub struct Lab {
    last_anim: i32,
    bullets: Vec<(i32, String)>,
    bullet_at: Option<usize>,
    anims: Vec<(i32, String)>,
    anim_at: Option<usize>,
    dumped: bool,
    del_type: i8,
    /// audition linger check: (bullet, fired at, effect count before)
    check: Option<(i32, std::time::Instant, u32)>,
    /// automatic audition (Home+Insert... here: key '=' starts it): next index and when to fire it
    auto: Option<(usize, std::time::Instant)>,
    /// family of the bullet under test and when to judge it
    judge: Option<(i32, Vec<i32>, std::time::Instant)>,
    /// movement experiments: 0 off, 1 motion_multiplier 1.5, 2 hks_root_motion_mult 1.5, 3 animation_speed 1.5
    speed_mode: u8,
    speed_probe: Option<(std::time::Instant, (f32, f32))>,
    last_alpha: [f32; 4],
    cue_at: Option<usize>,
}

/// A bullet and every row it can create (hit / interval children), with the longest chain life.
pub fn family(id: i32) -> (Vec<i32>, f32) {
    let Ok(repo) = (unsafe { SoloParamRepository::instance() }) else { return (vec![id], 1.0) };
    let mut out = Vec::new();
    let mut stack = vec![(id, 0.0f32)];
    let mut longest = 0.0f32;
    while let Some((i, t)) = stack.pop() {
        if i <= 0 || out.contains(&i) || out.len() > 12 {
            continue;
        }
        out.push(i);
        let Some(b) = repo.get::<Bullet>(i as u32) else { continue };
        let end = t + b.life().max(0.0);
        longest = longest.max(end);
        stack.push((b.hit_bullet_id(), end));
        stack.push((b.interval_create_bullet_id(), t + b.interval_create_time_max()));
    }
    (out, longest)
}

pub fn counts() -> (u32, u32) {
    unsafe { eldenring::cs::CSBulletManager::instance() }.map(|m| (m.bullets_count, m.sfx_related_count)).unwrap_or((0, 0))
}

fn list(name: &str) -> Vec<(i32, String)> {
    let path = format!("C:\\Users\\asher\\JumpRing\\lab\\{name}");
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let (id, rest) = l.split_once(|c: char| c.is_whitespace()).unwrap_or((l, ""));
            Some((id.parse().ok()?, rest.trim().to_string()))
        })
        .collect()
}

pub fn bullet_info(id: i32) -> String {
    let Ok(repo) = (unsafe { SoloParamRepository::instance() }) else { return "-".into() };
    let Some(b) = repo.get::<Bullet>(id as u32) else { return format!("{id}: no row") };
    format!(
        "{id}: life {:.2} dist {:.1} v0 {:.1} radius {:.2}->{:.2} shots {} hit {} interval {} ({:.2}-{:.2}s) atk {} | sfx {} del hit {} life {} force {}/{} attach {} inherit {}",
        b.life(), b.dist(), b.init_vellocity(), b.hit_radius(), b.hit_radius_max(), b.num_shoot(),
        b.hit_bullet_id(), b.interval_create_bullet_id(), b.interval_create_time_min(), b.interval_create_time_max(), b.atk_id_bullet(),
        b.sfx_id_bullet(), b.bullet_sfx_delete_type_by_hit(), b.bullet_sfx_delete_type_by_life_dead(), b.sfx_id_force_erase(), b.bullet_sfx_delete_type_by_force_erase(), b.attach_effect_type(), b.is_inherit_sfx_to_child()
    )
}

impl Lab {
    pub fn tick(&mut self, player: &mut PlayerIns) {
        use eldenring::util::input::is_key_pressed;
        if !self.dumped {
            self.dumped = true;
            for a in crate::sheets::ABILITIES.iter() {
                let mut ids = vec![a.er_bullet];
                let mut seen = 0;
                while let Some(id) = ids.pop() {
                    if id <= 0 || seen > 6 {
                        continue;
                    }
                    seen += 1;
                    log!("lab bullet [{}] {}", a.id, bullet_info(id));
                    if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                        if let Some(b) = repo.get::<Bullet>(id as u32) {
                            ids.push(b.hit_bullet_id());
                            ids.push(b.interval_create_bullet_id());
                        }
                    }
                }
            }
        }
        // 1.3.0 (villain balance): "meter on": every hit the player takes is logged with who dealt it (its NpcParam row:
        // villains are 9990200 + their index, their other forms 9990215..) and its share of the bar, then the bar refills
        if METER.load(std::sync::atomic::Ordering::Relaxed) {
            // (something else may hold the player invincible: say so once a second, and take it off)
            if player.chr_ins.chr_flags1c5.is_invincible() {
                static SAID: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
                let mut said = SAID.lock().unwrap();
                if said.is_none_or(|t| t.elapsed().as_secs_f32() > 1.0) {
                    *said = Some(std::time::Instant::now());
                    log!("meter: the player was invincible (taken off)");
                }
                player.chr_ins.chr_flags1c5.set_is_invincible(false);
            }
            // the villains' animations as they change (are their basic attacks thrown at all?)
            if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                static VANIM: std::sync::Mutex<Vec<(i32, i32)>> = std::sync::Mutex::new(Vec::new());
                let mut seen = VANIM.lock().unwrap();
                for e in wcm.chr_inses_by_distance.iter().take(24) {
                    let c = unsafe { e.chr_ins.as_ref() };
                    if crate::villains::is_villain_npc(c.npc_param_id) && e.distance < 30.0 {
                        let ta = &c.modules.time_act;
                        let a = ta.anim_queue[(ta.read_idx as usize) % ta.anim_queue.len()].anim_id;
                        match seen.iter_mut().find(|(n, _)| *n == c.npc_param_id) {
                            Some(x) if x.1 == a => {}
                            Some(x) => { x.1 = a; log!("meter: villain {} anim {a} at {:.1} m", c.npc_param_id, e.distance); }
                            None => {
                                seen.push((c.npc_param_id, a));
                                // a human enemy is a PlayerIns: what it really has equipped
                                let p = unsafe { &*(c as *const eldenring::cs::ChrIns as *const PlayerIns) };
                                let asm = &p.chr_asm.equipment_param_ids;
                                let pgd = unsafe { p.player_game_data.as_ref() };
                                log!("meter: villain {} chr_asm weapons {:?} pgd weapons {:?} arm style {:?}", c.npc_param_id, &asm[..6], &pgd.equipment.chr_asm.equipment_param_ids[..6], pgd.equipment.chr_asm.equipment.arm_style);
                            }
                        }
                    }
                }
            }
            // the boss (its bar) losing health, and to whom (villains must never hurt their boss)
            if let (Ok(fe), Ok(wcm)) = (unsafe { eldenring::cs::CSFeManImp::instance() }, unsafe { eldenring::cs::WorldChrMan::instance_mut() }) {
                // (per boss: a new bar, a second phase or the next fight starts over)
                static BOSS_LAST: std::sync::Mutex<Option<(eldenring::cs::FieldInsHandle, i32)>> = std::sync::Mutex::new(None);
                let h = fe.boss_health_displays[0].field_ins_handle;
                if let Some(b) = wcm.chr_ins_by_handle_mut(&h) {
                    let (bhp, by) = (b.modules.data.hp, b.last_hit_by);
                    let mut last = BOSS_LAST.lock().unwrap();
                    if let Some((lh, lhp)) = *last {
                        if lh == h && bhp < lhp && lhp - bhp < b.modules.data.max_hp / 3 {
                            log!("meter: boss lost {} (hp {bhp}) from npc {}", lhp - bhp, who_is(wcm, by));
                        }
                    }
                    *last = Some((h, bhp));
                }
            }
            let (hp, max) = (player.chr_ins.modules.data.hp, player.chr_ins.modules.data.max_hp);
            // (read a frame late: the villains' and bosses' damage caps run after this, in the same frame)
            static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            let ready = hp < max && SEEN.swap(true, std::sync::atomic::Ordering::Relaxed);
            if hp >= max {
                SEEN.store(false, std::sync::atomic::Ordering::Relaxed);
            }
            if ready {
                SEEN.store(false, std::sync::atomic::Ordering::Relaxed);
                let by = player.chr_ins.last_hit_by;
                let who = unsafe { eldenring::cs::WorldChrMan::instance_mut() }.ok().map(|w| who_is(w, by)).unwrap_or(-1);
                log!("meter: took {} ({:.1}% of {max}) from npc {who} c0000", max - hp, (max - hp) as f32 / max.max(1) as f32 * 100.0);
                player.chr_ins.modules.data.hp = max;
            }
        }
        // JR_GOD (lab runs only): the test character can't die (the test spot has a dragon).
        if std::env::var_os("JR_GOD").is_some() {
            let d = &mut player.chr_ins.modules.data;
            if d.hp < d.max_hp {
                log!("lab: took {} damage", d.max_hp - d.hp);
            }
            d.hp = d.max_hp;
        }
        let presses = player.chr_ins.modules.action_request.new_action_presses.0;
        if presses != 0 {
            log!("lab presses {presses:#x} anim {}", crate::movement::current_anim(player));
        }
        // Visibility trace: any change of the player's transparency multipliers.
        {
            let c = &player.chr_ins;
            let a = [c.opacity_keyframes_multiplier, c.tint_alpha_multiplier, c.camouflage_transparency, c.base_transparency];
            if a.iter().zip(self.last_alpha.iter()).any(|(x, y)| (x - y).abs() > 0.05) {
                log!("lab alpha opacity {:.2} tint {:.2} (mod {:.2}) camo {:.2} base {:.2} (mod {:.2})", a[0], a[1], c.tint_alpha_multiplier_modifier, a[2], a[3], c.base_transparency_modifier);
                self.last_alpha = a;
            }
        }
        // Animation trace: every change of the playing animation, with the movement state.
        let ta = &player.chr_ins.modules.time_act;
        let cur = ta.anim_queue[(ta.read_idx as usize) % 10].anim_id;
        if cur != self.last_anim {
            let ph = &player.chr_ins.modules.physics;
            let ar = &player.chr_ins.modules.action_request;
            log!(
                "lab anim {} -> {} falling {} ground {} presses {:#x} speed {:.2} motion {:.2} rm {:.2}",
                self.last_anim, cur, ph.is_falling, ph.is_touching_ground, ar.new_action_presses.0,
                player.chr_ins.modules.behavior.animation_speed, ph.motion_multiplier, player.chr_ins.modules.behavior_data.hks_root_motion_mult
            );
            self.last_anim = cur;
        }
        // Insert / Delete: next / previous bullet from lab/audition.txt, fired 4 m ahead.
        let step = if is_key_pressed(0x2D) { 1 } else if is_key_pressed(0x2E) { -1 } else { 0 };
        if step != 0 {
            if self.bullets.is_empty() {
                self.bullets = list("audition.txt");
            }
            if !self.bullets.is_empty() {
                let n = self.bullets.len() as i64;
                let i = match self.bullet_at { Some(i) => (i as i64 + step).rem_euclid(n) as usize, None => 0 };
                self.bullet_at = Some(i);
                let (id, name) = &self.bullets[i];
                let pos = ability::position(player);
                let dir = ability::forward(player);
                let before = counts().1;
                let r = ability::fire_at(player, *id, (pos.0 + dir.0 * 2.0, pos.1 + 1.4, pos.2 + dir.2 * 2.0), dir);
                self.check = Some((*id, std::time::Instant::now(), before));
                log!("lab audition {}/{} {name} -> {r:?} | {}", i + 1, n, bullet_info(*id));
            }
        }
        if let Some((id, at, before)) = self.check {
            if at.elapsed().as_secs_f32() > 4.5 {
                let (b, sfx) = counts();
                log!("lab linger {id}: bullets {b} effects {before} -> {sfx}{}", if sfx > before { "  LINGERS" } else { "" });
                self.check = None;
            }
        }
        // '=' (0xBB): automatic audition of lab/audition.txt: fire, wait past the chain's life, judge.
        if is_key_pressed(0xBB) {
            self.bullets = list("audition.txt");
            self.auto = Some((0, std::time::Instant::now()));
            log!("lab auto audition: {} bullets", self.bullets.len());
        }
        if let Some((fam_id, fam, at)) = &self.judge {
            if std::time::Instant::now() >= *at {
                let alive = unsafe { eldenring::cs::CSBulletManager::instance() }
                    .map(|m| m.bullets().filter(|b| fam.contains(&b.fly_state.base.param.param_id)).count())
                    .unwrap_or(0);
                let name = self.bullets.iter().find(|b| b.0 == *fam_id).map(|b| b.1.clone()).unwrap_or_default();
                let alive = if alive == 0 && counts().0 == 0 && fam.len() > 0 { 0 } else { alive };
                log!("lab judge {fam_id} {name}: {}", if alive == 0 { "OK".to_string() } else { format!("FROZEN ({alive} still alive)") });
                self.judge = None;
            }
        }
        if let Some((i, at)) = self.auto {
            if std::time::Instant::now() >= at && self.judge.is_none() {
                if i >= self.bullets.len() {
                    log!("lab auto audition done");
                    self.auto = None;
                } else if counts().0 > 6 {
                    // crowded by earlier frozen bullets (new spawns may silently fail): die and respawn to clear them
                    log!("lab auto audition: {} live bullets, respawning to clear", counts().0);
                    player.chr_ins.modules.data.hp = 0;
                    self.auto = Some((i, std::time::Instant::now() + std::time::Duration::from_secs(20)));
                } else {
                    let (id, _) = self.bullets[i].clone();
                    let (fam, life) = family(id);
                    let pos = ability::position(player);
                    let dir = ability::forward(player);
                    let _ = ability::fire_at(player, id, (pos.0 + dir.0 * 2.0, pos.1 + 1.4, pos.2 + dir.2 * 2.0), dir);
                    let wait = std::time::Duration::from_secs_f32(life.min(12.0) + 2.0);
                    self.judge = Some((id, fam, std::time::Instant::now() + wait));
                    self.auto = Some((i + 1, std::time::Instant::now() + wait + std::time::Duration::from_millis(300)));
                }
            }
        }
        // Numpad 7: cycle the movement experiment; the horizontal speed is logged every second.
        if is_key_pressed(0x67) {
            self.speed_mode = (self.speed_mode + 1) % 4;
            log!("lab speed mode {}", self.speed_mode);
        }
        {
            let m = if self.speed_mode == 0 { 1.0 } else { 1.5 };
            player.chr_ins.modules.physics.motion_multiplier = if self.speed_mode == 1 { m } else { 1.0 };
            player.chr_ins.modules.behavior_data.hks_root_motion_mult = if self.speed_mode == 2 { m } else { 1.0 };
            player.chr_ins.modules.behavior.animation_speed = if self.speed_mode == 3 { m } else { 1.0 };
            let p = ability::position(player);
            let now = std::time::Instant::now();
            match self.speed_probe {
                Some((t, (x, z))) if now.duration_since(t).as_secs_f32() >= 1.0 => {
                    let v = ((p.0 - x).powi(2) + (p.2 - z).powi(2)).sqrt() / now.duration_since(t).as_secs_f32();
                    if v > 0.3 {
                        log!("lab speed {:.2} m/s (mode {})", v, self.speed_mode);
                    }
                    self.speed_probe = Some((now, (p.0, p.2)));
                }
                None => self.speed_probe = Some((now, (p.0, p.2))),
                _ => {}
            }
        }
        // Num Lock: list live bullets by param id (fly-state row), with the oldest age.
        if is_key_pressed(0x90) {
            if let Ok(m) = unsafe { eldenring::cs::CSBulletManager::instance() } {
                let mut by: std::collections::BTreeMap<i32, (u32, f32, f32, f32)> = Default::default();
                for b in m.bullets() {
                    let id = b.fly_state.base.param.param_id;
                    let e = by.entry(id).or_insert((0, 0.0, 0.0, 0.0));
                    e.0 += 1;
                    if b.time_alive >= e.1 {
                        e.1 = b.time_alive;
                        e.2 = b.life;
                        e.3 = b.life_max;
                    }
                }
                log!("lab live bullets {}: {:?}", m.bullets_count, by);
            }
        }
        // Scroll Lock: end every live bullet (life 0) and report the count a moment later.
        if is_key_pressed(0x91) {
            if let Ok(m) = unsafe { eldenring::cs::CSBulletManager::instance_mut() } {
                let mut n = 0;
                for b in m.bullets_mut() {
                    b.life = 0.0;
                    n += 1;
                }
                log!("lab: ended {n} bullets");
            }
        }
        // ';' (0xBA): play the next line of lab/cues.txt (Lua for the behaviour script, anim.rs).
        if is_key_pressed(0xBA) {
            let lines: Vec<String> = std::fs::read_to_string(r"C:\Users\asher\JumpRing\lab\cues.txt").unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
            if !lines.is_empty() {
                let i = self.cue_at.map(|i| (i + 1) % lines.len()).unwrap_or(0);
                self.cue_at = Some(i);
                crate::anim::play(player, &lines[i]);
                log!("lab cue {}/{}: {}", i + 1, lines.len(), lines[i]);
            }
        }
        // Pause: request the next animation from lab/anims.txt.
        if is_key_pressed(0x13) {
            if self.anims.is_empty() {
                self.anims = list("anims.txt");
            }
            if !self.anims.is_empty() {
                let i = self.anim_at.map(|i| (i + 1) % self.anims.len()).unwrap_or(0);
                self.anim_at = Some(i);
                let (id, name) = &self.anims[i];
                player.chr_ins.modules.event.request_animation_id = *id;
                log!("lab anim request {id} {name}");
            }
        }
    }
}

/// A lab script (env JR_LAB_SCRIPT = a text file): one command per line, run in order, so a lab run
/// needs no keyboard focus. Before the world loads it taps Enter through the title screens.
///   sleep <ms> | fighter <id> | closemenu | cue <lua> | key <scan hex> <ms> | down <scan> | up <scan>
///   ki | awaken | log <text> | quit
pub struct Script {
    lines: Vec<String>,
    at: usize,
    wait_until: std::time::Instant,
    holds: Vec<(u8, std::time::Instant)>,
    next_enter: std::time::Instant,
    /// 2.0.3 "waitkey <vk hex>": the script waits for the player to press this key (the user's villain tour: they play each
    /// fight as long as they like, F9 for the next)
    wait_key: Option<i32>,
    /// 2.0.3 trailer: "noenter": no more Enter presses out of the world (the user's own title screen, character creation,
    /// cutscenes)
    no_enter: bool,
    /// 2.0.3 "waitboss": the script waits until a boss's health bar is up (the player started its fight themselves: the
    /// tour's teleport into Malenia's arena once they were in it put them into the trees and through the ground), or F9
    wait_boss: bool,
    /// 2.0.6 "waitworld <ms>": the script waits for the next arrival in the world (a load ending), then ms more (the freeze
    /// hunt: a fighter picked moments after a respawn)
    wait_world: Option<(u64, u64)>,
}

/// 2.0.6: arrivals in the world so far, and when the last one was (lib.rs "player: in world")
static WORLD_ENTRIES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static WORLD_AT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

pub fn world_entered() {
    WORLD_ENTRIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *WORLD_AT.lock().unwrap() = Some(std::time::Instant::now());
}

static SCRIPT: std::sync::Mutex<Option<Script>> = std::sync::Mutex::new(None);
static HOME: std::sync::Mutex<Option<eldenring::position::HavokPosition>> = std::sync::Mutex::new(None);
/// script "goto": the lab's F1 (teleport to the nearest hostile and face it) next frame
pub static GOTO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// script "airdash": an air dash as if dodge were pressed (movement.rs)
pub static FORCE_AIRDASH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// script "warp": the lab's Numpad 9 (Raya Lucaria's first grace: walls all around)
/// Lab: log every change of the player's animation (script "watch on").
pub static WATCH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 1.2.0 round 5: log the nearest hostile's animations (id, length) as they change
static FOEWATCH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 1.2.0 round 5: hold the nearest hostile up in the air (metres, 0 off), tilted (deg) and spinning (deg/s)
/// "foefloat <height> <gravity multiplier>": lifted once, then left to really fall, slowly (lab)
static FOEFLOAT: std::sync::Mutex<Option<f32>> = std::sync::Mutex::new(None);
/// "foehang <height> <every n frames>": held near a height by putting it back only every n frames, in
/// between it really falls (lab: does the game's own fall motion play?)
static FOEHANG: std::sync::Mutex<Option<(f32, u32, Option<f32>, u32)>> = std::sync::Mutex::new(None);
static FOEFALL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static FOEHOLD: std::sync::Mutex<Option<(f32, f32, f32, f32, Option<(f32, f32, f32)>, f32)>> = std::sync::Mutex::new(None);

/// The nearest hostile within `r` m (lab).
fn nearest_foe(player: &PlayerIns, r: f32) -> Option<&'static mut eldenring::cs::ChrIns> {
    let wcm = unsafe { eldenring::cs::WorldChrMan::instance() }.ok()?;
    let me = &player.chr_ins as *const _ as usize;
    let p = player.chr_ins.modules.physics.position;
    let mut best: Option<(f32, *mut eldenring::cs::ChrIns)> = None;
    for e in wcm.chr_inses_by_distance.iter().take(64) {
        if e.chr_ins.as_ptr() as usize == me {
            continue;
        }
        let c = unsafe { e.chr_ins.as_ref() };
        if !crate::combat::hostile(c.team_type) || c.modules.data.hp <= 0 {
            continue;
        }
        let q = c.modules.physics.position;
        let d = ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2) + (q.2 - p.2).powi(2)).sqrt();
        if d <= r && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, e.chr_ins.as_ptr()));
        }
    }
    best.map(|(_, c)| unsafe { &mut *c })
}
static LAST_ANIM: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// 1.0: lab: no exploration music from the mod (to measure the game's own music alone)
pub static NO_MUSIC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static BOSS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Lab: Deku's Black Whip button held (script "whip on" / "whip off").
pub static WHIP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub static WARP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn hold_key(sc: u8, on: bool) {
    use std::sync::atomic::Ordering;
    // 0xF0 + n: mouse button n (script "mouse")
    if sc >= 0xF0 {
        let bit = 1u8 << (sc - 0xF0);
        if on {
            crate::binds::LAB_MOUSE.fetch_or(bit, Ordering::Relaxed);
        } else {
            crate::binds::LAB_MOUSE.fetch_and(!bit, Ordering::Relaxed);
        }
        return;
    }
    let keys = &crate::binds::LAB_KEYS;
    if on {
        if keys.iter().any(|k| k.load(Ordering::Relaxed) == sc) {
            return;
        }
        if let Some(k) = keys.iter().find(|k| k.load(Ordering::Relaxed) == 0) {
            k.store(sc, Ordering::Relaxed);
        }
    } else {
        for k in keys {
            let _ = k.compare_exchange(sc, 0, Ordering::Relaxed, Ordering::Relaxed);
        }
    }
}

fn script_lock() -> std::sync::MutexGuard<'static, Option<Script>> {
    let mut g = SCRIPT.lock().unwrap();
    if g.is_none() {
        if let Ok(path) = std::env::var("JR_LAB_SCRIPT") {
            let lines: Vec<String> = std::fs::read_to_string(&path).unwrap_or_default().lines()
                .map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
            log!("lab script {path}: {} lines", lines.len());
            let now = std::time::Instant::now();
            // (2.0.7: "noenter" as the first line holds from the start: the script's commands only run in the world, after the
            // title screen's Enter presses; titleidle needs the title left alone)
            let no_enter = lines.first().is_some_and(|l| l == "noenter");
            *g = Some(Script { lines, at: 0, wait_until: now, holds: Vec::new(), next_enter: now + std::time::Duration::from_secs(12), wait_key: None, wait_boss: false, no_enter, wait_world: None });
        }
    }
    g
}

fn release_holds(s: &mut Script) {
    let now = std::time::Instant::now();
    s.holds.retain(|(sc, until)| {
        if now >= *until {
            hold_key(*sc, false);
            false
        } else {
            true
        }
    });
}

/// Every frame while not in the world: Enter through the title screens.
pub fn script_boot() {
    let mut g = script_lock();
    let Some(s) = g.as_mut() else { return };
    release_holds(s);
    let now = std::time::Instant::now();
    if !s.no_enter && now >= s.next_enter {
        hold_key(0x1C, true);
        s.holds.push((0x1C, now + std::time::Duration::from_millis(120)));
        s.next_enter = now + std::time::Duration::from_millis(2500);
    }
}

/// Lab "foeat": where the enemy it brought stands (the lock follows it)
static FOEAT_AT: std::sync::Mutex<Option<(f32, f32, f32)>> = std::sync::Mutex::new(None);
/// Lab "lock on|off": hold a lock-on on the biggest enemy near (script_tick).
static LOCK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Every frame in the world: the next command when its time comes.
/// Lab: a villain to summon now (villains.rs).
pub static VILLAIN: std::sync::Mutex<Option<usize>> = std::sync::Mutex::new(None);
/// 2.0.3 trailer shoot: "nolabkeys": the lab's own keys (F1-F12, Home: lib.rs lab_keys) off while the user plays (F9 marked a
/// take and turned them into All Might, F10 started the waves as Bakugo, Home for cinematic mode also killed them)
pub static NO_LAB_KEYS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 1.3.0: vilpool <share>: every live villain's pool set to that share of its fight health (form changes, defeats)
pub static VILPOOL: std::sync::Mutex<Option<f32>> = std::sync::Mutex::new(None);
/// 1.3.0: "vilteam <n>": villains kept on this team (AI tests), "vilteam off"
pub static VILTEAM: std::sync::Mutex<Option<u8>> = std::sync::Mutex::new(None);
/// 1.3.0: vilpath on: villains with paths (Pain) change path after every special, and use one every 2 s
/// the NpcParam row of the character with handle `h` (searched among the nearby ones: the handle lookup misses map
/// characters of other blocks); -1 when none
fn who_is(wcm: &mut eldenring::cs::WorldChrMan, h: eldenring::cs::FieldInsHandle) -> i32 {
    if let Some(c) = wcm.chr_ins_by_handle_mut(&h) {
        return c.npc_param_id;
    }
    wcm.chr_inses_by_distance.iter().take(96).map(|e| unsafe { e.chr_ins.as_ref() }).find(|c| c.field_ins_handle == h)
        .map(|c| c.npc_param_id).unwrap_or_else(|| { log!("meter: unknown attacker {h:?}"); -1 })
}

/// 1.3.0: the damage meter (lab "meter on")
pub static METER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 2.0.2: "standoff on": the Stand sent away and kept away (stands.rs)
pub static STAND_OFF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static VILPATH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// vilmoves <NAME,NAME|off>: villains pick only these specials (by name; the ultimate still comes when low)
/// armslot: the fighter's outfit left alone meanwhile (it dresses a changed slot's row in the fighter's look)
pub static OUTFIT_HOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static VILMOVES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// 1.3.0: buddyhold on: the summon manager told every frame that the player is within a Rebirth Monument's range
pub static BUDDY_HOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// buddykeep on: the active summon's goods and "alive" kept set (the summon was cleaned up 3 s after it formed)
pub static BUDDY_KEEP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// buddyforce on: and the activation-range flags forced on every frame
pub static BUDDY_FORCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn script_tick(player: &mut PlayerIns, combat: &mut crate::combat::Combat) {
    if BUDDY_HOLD.load(std::sync::atomic::Ordering::Relaxed) {
        if let Some(wcm) = BUDDY_FORCE.load(std::sync::atomic::Ordering::Relaxed).then(|| unsafe { eldenring::cs::WorldChrMan::instance_mut() }.ok()).flatten() {
            let m = &mut wcm.summon_buddy_manager;
            m.is_within_activation_range = true;
            m.prev_is_within_activation_range = true;
            m.is_within_warn_range = true;
            m.prev_is_within_warn_range = true;
        }
        if BUDDY_KEEP.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance_mut() } {
                let m = &mut wcm.summon_buddy_manager;
                m.active_summon_goods_id = 207000;
                m.player_has_alive_summon = true;
            }
        }
        static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
        let mut last = LAST.lock().unwrap();
        if last.is_none_or(|t| t.elapsed().as_secs_f32() > 1.0) {
            *last = Some(std::time::Instant::now());
            if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                let m = &wcm.summon_buddy_manager;
                let n = wcm.summon_buddy_chr_set.characters().count();
                log!("lab buddy watch: {n} in the summon set; active {} alive {} disappear {} stone {} goods {} cooldown {:.1}", m.active_summon_speffect_id as i32, m.player_has_alive_summon, m.disappear_requested, m.active_summmon_buddy_stone_entity_id, m.active_summon_goods_id, m.item_use_cooldown_timer);
                for c in wcm.summon_buddy_chr_set.characters() {
                    let p = c.modules.physics.position;
                    log!("lab buddy watch: chr hp {}/{} team {} npc {} chr {} entity {} at ({:.1}, {:.1}, {:.1})", c.modules.data.hp, c.modules.data.max_hp, c.team_type, c.npc_param_id, c.character_id, c.event_entity_id, p.0, p.1, p.2);
                    if c.team_type == 47 {
                        let ids: Vec<i32> = c.special_effect.entries().map(|e| e.param_id).collect();
                        log!("lab buddy watch: its effects {ids:?}");
                    }
                }
            }
        }
    }
    let mut g = script_lock();
    let Some(s) = g.as_mut() else { return };
    // 1.1.0 (user: lock on while fighting in the lab, hits land more consistently): "lock on" keeps the
    // biggest enemy within 40 m locked as the abilities see it (the game's own Q is the swing key here)
    if LOCK.load(std::sync::atomic::Ordering::Relaxed) {
        // (1.2.x) the enemy "foeat" brought, while it lives; else the biggest near
        let placed = FOEAT_AT.lock().unwrap().and_then(|at| {
            let wcm = unsafe { eldenring::cs::WorldChrMan::instance() }.ok()?;
            wcm.chr_inses_by_distance.iter().take(64).map(|e| unsafe { e.chr_ins.as_ref() })
                .filter(|c| crate::combat::hostile(c.team_type) && c.modules.data.hp > 0)
                .map(|c| { let q = c.modules.physics.position; (q.0, q.1, q.2) })
                .find(|q| (q.0 - at.0).powi(2) + (q.2 - at.2).powi(2) < 4.0)
        });
        if let Some(q) = placed {
            *FOEAT_AT.lock().unwrap() = Some(q);
        }
        if let Some(e) = placed.or_else(|| crate::training::biggest_near(player, 40.0)) {
            player.chr_ins.is_locked_on = true;
            player.chr_ins.lock_on_target_position = fromsoftware_shared::F32Vector4(e.0, e.1 + 1.5, e.2, 1.0);
        }
    }
    release_holds(s);
    if WATCH.load(std::sync::atomic::Ordering::Relaxed) {
        static LASTK: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
        let k = crate::binds::WASD.load(std::sync::atomic::Ordering::Relaxed);
        if LASTK.swap(k, std::sync::atomic::Ordering::Relaxed) != k {
            log!("lab watch: wasd {k:#x} stick {:?}", crate::input::stick());
        }
        let a = crate::movement::current_anim(player);
        if LAST_ANIM.swap(a, std::sync::atomic::Ordering::Relaxed) != a {
            log!("lab watch: anim {a}");
        }
    }
    if let Some((h, tilt, spin, yaw, at, speed)) = FOEHOLD.lock().unwrap().as_mut() {
        if let Some(c) = nearest_foe(player, 12.0) {
            if *speed >= 0.0 {
                c.modules.behavior.animation_speed = *speed;
            }
            c.modules.data.hp = c.modules.data.max_hp;
            // turned about the body's middle (1 m up), not the feet: the model is put back by what the turn
            // moves its middle (Rodrigues, axis in the body's frame, then the body's own yaw)
            let (sy, cy) = ((*yaw).to_radians()).sin_cos();
            let th = (*tilt).to_radians();
            let k = (cy, 0.0f32, sy);
            let v = (0.0f32, 1.0f32, 0.0f32);
            let kxv = (k.1 * v.2 - k.2 * v.1, k.2 * v.0 - k.0 * v.2, k.0 * v.1 - k.1 * v.0);
            let kv = k.0 * v.0 + k.1 * v.1 + k.2 * v.2;
            let rv = (v.0 * th.cos() + kxv.0 * th.sin() + k.0 * kv * (1.0 - th.cos()), v.1 * th.cos() + kxv.1 * th.sin() + k.1 * kv * (1.0 - th.cos()), v.2 * th.cos() + kxv.2 * th.sin() + k.2 * kv * (1.0 - th.cos()));
            let local = (v.0 - rv.0, v.1 - rv.1, v.2 - rv.2);
            let q = c.modules.physics.orientation;
            let qv = (q.0, q.1, q.2);
            let t = (2.0 * (qv.1 * local.2 - qv.2 * local.1), 2.0 * (qv.2 * local.0 - qv.0 * local.2), 2.0 * (qv.0 * local.1 - qv.1 * local.0));
            let wl = (local.0 + q.3 * t.0 + (qv.1 * t.2 - qv.2 * t.1), local.1 + q.3 * t.1 + (qv.2 * t.0 - qv.0 * t.2), local.2 + q.3 * t.2 + (qv.0 * t.1 - qv.1 * t.0));
            let ph = &mut c.modules.physics;
            let base = *at.get_or_insert((ph.position.0, ph.position.1, ph.position.2));
            ph.position = eldenring::position::HavokPosition(base.0 + wl.0, base.1 + *h + wl.1, base.2 + wl.2, ph.position.3);
            ph.chr_proxy_pos_update_requested = true;
            // FOEFALL: the game is left to think it is falling (its own fall motion plays); the height is
            // still held, and the fall timer kept short of fall damage
            let falling = FOEFALL.load(std::sync::atomic::Ordering::Relaxed);
            ph.gravity_disabled = !falling;
            if falling {
                ph.is_falling = true;
                ph.is_touching_ground = false;
            }
            *tilt += *spin / 60.0;
            // the model only (ChrCtrl's additional orientation, after the body's own): pitch about its
            // right axis, `yaw` picks another axis (90: roll)
            let (sy, cy) = ((*yaw).to_radians()).sin_cos();
            let (sp, cp) = ((*tilt).to_radians() / 2.0).sin_cos();
            if FOEFALL.load(std::sync::atomic::Ordering::Relaxed) {
                c.modules.fall.fall_timer = c.modules.fall.fall_timer.min(0.9);
            } else {
                c.modules.fall.fall_timer = 0.0;
            }
            c.chr_ctrl.additional_orientation_quat = fromsoftware_shared::F32Vector4(cy * sp, 0.0, sy * sp, cp);
        }
    }
    if let Some((h, every, base, n)) = FOEHANG.lock().unwrap().as_mut() {
        if let Some(c) = nearest_foe(player, 12.0) {
            c.modules.data.hp = c.modules.data.max_hp;
            let ph = &mut c.modules.physics;
            let y0 = *base.get_or_insert(ph.position.1);
            *n += 1;
            ph.gravity_disabled = false;
            if *n % (*every).max(1) == 0 || ph.position.1 < y0 + *h - 0.6 {
                ph.position = eldenring::position::HavokPosition(ph.position.0, y0 + *h, ph.position.2, ph.position.3);
                ph.chr_proxy_pos_update_requested = true;
            }
            let (py, fall, touch) = (ph.position.1, ph.is_falling, ph.is_touching_ground);
            if *n % 6 == 0 {
                let ta = &c.modules.time_act;
                log!("lab hang: y {:.2} falling {} touching {} fall_timer {:.2} anim {}", py - y0, fall, touch, c.modules.fall.fall_timer, ta.anim_queue[(ta.read_idx as usize) % 10].anim_id);
            }
        }
    }
    if let Some(gm) = *FOEFLOAT.lock().unwrap() {
        if let Some(c) = nearest_foe(player, 12.0) {
            c.modules.physics.gravity_multiplier = gm;
            c.modules.physics.gravity_disabled = false;
        }
    }
    if FOEWATCH.load(std::sync::atomic::Ordering::Relaxed) {
        // (no health top-ups: they fooled the damage ledger into finishing the enemy; LAB_DMG_SCALE instead)
        if false {
        if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
            let me = &player.chr_ins as *const _ as usize;
            for e in wcm.chr_inses_by_distance.iter().take(32) {
                if e.chr_ins.as_ptr() as usize == me {
                    continue;
                }
                let c = unsafe { &mut *e.chr_ins.as_ptr() };
                if crate::combat::hostile(c.team_type) && c.modules.data.hp > 0 {
                    if c.modules.data.max_hp < 50000 {
                        c.modules.data.max_hp = 99999;
                    }
                    c.modules.data.hp = c.modules.data.max_hp;
                }
            }
        }
        }
        static LASTF: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
        if let Some(c) = nearest_foe(player, 15.0) {
            // (alive thanks to LAB_DMG_SCALE, not top-ups)
            let ta = &c.modules.time_act;
            let a = &ta.anim_queue[(ta.read_idx as usize) % 10];
            // every animation in its queue, whenever the set changes (a hit reaction may not be the one read)
            static LASTQ: std::sync::Mutex<Vec<i32>> = std::sync::Mutex::new(Vec::new());
            let q: Vec<i32> = ta.anim_queue.iter().map(|x| x.anim_id).collect();
            let mut lq = LASTQ.lock().unwrap();
            if *lq != q {
                log!("lab foe queue: read {} write {} {:?} times {:?}", ta.read_idx, ta.write_idx, q, ta.anim_queue.iter().map(|x| (x.play_time * 100.0).round() / 100.0).collect::<Vec<f32>>());
                *lq = q;
            }
            drop(lq);
            if LASTF.swap(a.anim_id, std::sync::atomic::Ordering::Relaxed) != a.anim_id {
                let q = c.modules.physics.position;
                log!("lab foe: npc {} chr {} anim {} len {:.2} y {:.1} speed {:.2}", c.npc_param_id, c.character_id, a.anim_id, a.anim_length, q.1, c.modules.behavior.animation_speed);
            }
        }
    }
    let now = std::time::Instant::now();
    if let Some(vk) = s.wait_key {
        if unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk) } as u16 & 0x8000 != 0 {
            s.wait_key = None;
            log!("lab script: key {vk:#x} pressed");
        }
    }
    if s.wait_boss {
        let bar = unsafe { eldenring::cs::CSFeManImp::instance() }.is_ok_and(|fe| fe.boss_health_displays.iter().any(|b| b.fmg_id > 0));
        let f9 = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x78) } as u16 & 0x8000 != 0;
        if bar || f9 {
            s.wait_boss = false;
            log!("lab script: the boss fight is on ({})", if bar { "its bar is up" } else { "F9" });
        }
    }
    if let Some((seen, ms)) = s.wait_world {
        let at = *WORLD_AT.lock().unwrap();
        if WORLD_ENTRIES.load(std::sync::atomic::Ordering::Relaxed) > seen && at.is_some_and(|t| now >= t + std::time::Duration::from_millis(ms)) {
            s.wait_world = None;
            log!("lab script: in the world {ms} ms after the load");
        }
    }
    while now >= s.wait_until && s.at < s.lines.len() && s.wait_key.is_none() && !s.wait_boss && s.wait_world.is_none() {
        let line = s.lines[s.at].clone();
        s.at += 1;
        let (cmd, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let num = |t: &str| t.trim().parse::<u64>().unwrap_or(0);
        let sc = |t: &str| u8::from_str_radix(t.trim().trim_start_matches("0x"), 16).unwrap_or(0);
        match cmd {
            "sleep" => s.wait_until = now + std::time::Duration::from_millis(num(rest)),
            "fighter" => combat.set_fighter(rest.trim()),
            "closemenu" => {
                crate::ui::UI.lock().unwrap().menu = None;
                crate::binds::BLOCK_ALL.store(false, std::sync::atomic::Ordering::Relaxed);
            }
            "cue" => crate::anim::play(player, rest),
            "turn" => {
                // turn the fighter by <deg> (positive: to their left), e.g. side-on to the camera
                let f = crate::ability::forward(player);
                let th = (-f.0).atan2(-f.2) + rest.trim().parse::<f32>().unwrap_or(90.0).to_radians();
                let phys = &mut player.chr_ins.modules.physics;
                phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
                phys.interpolated_orientation = phys.orientation;
            }
            "speed" => player.chr_ins.modules.behavior.animation_speed = rest.trim().parse().unwrap_or(1.0),
            "anim" => player.chr_ins.modules.event.request_animation_id = rest.trim().parse().unwrap_or(-1),
            // 2.0.3 trailer shoot: "level <n> <vigor> <mind> <endurance>": the character at that level (the user's boss fights)
            "level" => {
                let v: Vec<u32> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if let [n, vg, md, en] = v[..] {
                    crate::builds::lab_level(unsafe { player.player_game_data.as_mut() }, n, [vg, md, en]);
                    log!("lab script: level {n} (vigor {vg} mind {md} endurance {en})");
                }
            }
            // 1.3.0: "armslot <row>": the fighter's arms slot shows that armour row (a villain's held weapon piece, villains.rs HELD)
            "armslot" => {
                let row = rest.trim().parse::<i32>().unwrap_or(-1);
                OUTFIT_HOLD.store(row != 870200, std::sync::atomic::Ordering::Relaxed);
                let pgd = unsafe { player.player_game_data.as_mut() };
                pgd.equipment.chr_asm.equipment_param_ids[14] = row;
                player.chr_asm.equipment_param_ids[14] = row;
                log!("lab script: arms slot {row}");
            }
            // 1.2.0 Carian Combo Warriors reel: a caption on screen, and the same spot for every move
            "caption" => crate::world_fx::banner(rest.trim(), [1.0, 1.0, 1.0, 1.0], 3.0),
            "waitboss" => s.wait_boss = true,
            "waitworld" => s.wait_world = Some((WORLD_ENTRIES.load(std::sync::atomic::Ordering::Relaxed), num(rest))),
            "noenter" => s.no_enter = true,
            "nolabkeys" => NO_LAB_KEYS.store(true, std::sync::atomic::Ordering::Relaxed),
            "waitkey" => s.wait_key = Some(i32::from_str_radix(rest.trim().trim_start_matches("0x"), 16).unwrap_or(0x78)),
            "home" => *HOME.lock().unwrap() = Some(player.chr_ins.modules.physics.position.clone()),
            "back" => {
                if let Some(h) = HOME.lock().unwrap().clone() {
                    let ph = &mut player.chr_ins.modules.physics;
                    ph.position = h;
                    ph.chr_proxy_pos_update_requested = true;
                }
            }
            "key" => {
                let mut it = rest.split_whitespace();
                let (k, ms) = (sc(it.next().unwrap_or("0")), num(it.next().unwrap_or("100")));
                hold_key(k, true);
                s.holds.push((k, now + std::time::Duration::from_millis(ms)));
            }
            "mouse" => {
                // mouse <0 left | 1 right> <ms>
                let mut it = rest.split_whitespace();
                let (b, ms) = (it.next().unwrap_or("0").parse::<u8>().unwrap_or(0).min(1), num(it.next().unwrap_or("100")));
                hold_key(0xF0 + b, true);
                s.holds.push((0xF0 + b, now + std::time::Duration::from_millis(ms)));
            }
            "pad" => {
                // pad <rt|lt|a|b|x|y|rb|lb> <ms>: a controller button held (works with the window in the background)
                let mut it = rest.split_whitespace();
                let bit: u32 = match it.next().unwrap_or("") {
                    "rt" => 0x10000, "lt" => 0x20000, "a" => 0x1000, "b" => 0x2000, "x" => 0x4000, "y" => 0x8000,
                    "rb" => 0x0200, "lb" => 0x0100, "l3" => 0x0040, "r3" => 0x0080,
                    // (1.3.1: the d-pad, XInput's bits: right switches the right hand's weapon)
                    "dup" => 0x0001, "ddown" => 0x0002, "dleft" => 0x0004, "dright" => 0x0008, _ => 0,
                };
                let ms = num(it.next().unwrap_or("100"));
                crate::binds::LAB_PAD.fetch_or(bit, std::sync::atomic::Ordering::Relaxed);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                    crate::binds::LAB_PAD.fetch_and(!bit, std::sync::atomic::Ordering::Relaxed);
                });
            }
            "hands" => {
                // hands <1|2>: one- or two-handed, toggled (E + left click) only when it isn't already (Carian
                // Combo Warriors moves often drop back to one hand on their own)
                let want_two = rest.trim() == "2";
                let two = unsafe { eldenring::cs::GameDataMan::instance() }
                    .map(|g| g.main_player_game_data.equipment.chr_asm.equipment.is_two_handing())
                    .unwrap_or(false);
                log!("lab script: hands {} (two-handed now {two})", if want_two { 2 } else { 1 });
                if two != want_two {
                    hold_key(0x12, true);
                    s.holds.push((0x12, now + std::time::Duration::from_millis(300)));
                    hold_key(0xF0, true);
                    s.holds.push((0xF0, now + std::time::Duration::from_millis(110)));
                }
            }
            "rowarts" => {
                // rowarts <SwordArtsParam row>: the weapon in the right hand (and the bare hands) get that Ash of
                // War, e.g. Carian Combo Warriors' own skills for its movesets
                use eldenring::cs::EquipParamWeapon;
                let sa = rest.trim().parse::<i32>().unwrap_or(0);
                let id = unsafe { eldenring::cs::GameDataMan::instance() }.ok().map(|g| {
                    let asm = &g.main_player_game_data.equipment.chr_asm;
                    asm.equipment_param_ids[2 * asm.equipment.active_right_weapon_slot() as usize + 1]
                });
                if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                    for row in [id.map(|i| (i / 100 * 100) as u32).unwrap_or(0), crate::weapon::UNARMED] {
                        if let Some(w) = repo.get_mut::<EquipParamWeapon>(row) {
                            w.set_sword_arts_param_id(sa);
                        }
                    }
                }
                log!("lab script: rowarts {sa} on {id:?}");
            }
            "glidepose" => crate::movement::LAB_GLIDE_POSE.store(num(rest) as u32, std::sync::atomic::Ordering::Relaxed),
            "boardbody" => crate::movement::LAB_BOARD_BODY.store(num(rest) as i32, std::sync::atomic::Ordering::Relaxed),
            "mutebullets" | "mutevfx" => {
                // lab: every bullet's look (mutebullets) or every special effect's look (mutevfx) taken away
                // live, to see which one a move's effect comes from
                use eldenring::cs::{Bullet, SpEffectParam};
                let mut n = 0;
                if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                    let mut i = 0;
                    if cmd == "mutebullets" {
                        while let Some(r) = repo.get_row_by_index_mut::<Bullet>(i) {
                            r.set_sfx_id_bullet(-1);
                            r.set_sfx_id_hit(-1);
                            r.set_sfx_id_flick(-1);
                            i += 1;
                        }
                    } else {
                        while let Some(r) = repo.get_row_by_index_mut::<SpEffectParam>(i) {
                            r.set_vfx_id(-1);
                            r.set_vfx_id1(-1);
                            r.set_vfx_id2(-1);
                            r.set_vfx_id3(-1);
                            r.set_vfx_id4(-1);
                            r.set_vfx_id5(-1);
                            r.set_vfx_id6(-1);
                            r.set_vfx_id7(-1);
                            i += 1;
                        }
                    }
                    n = i;
                }
                log!("lab script: {cmd}: {n} rows");
            }
            "gliderow" => crate::movement::LAB_GLIDE_ROW.store(num(rest) as u32, std::sync::atomic::Ordering::Relaxed),
            "moverow" => {
                // moverow <weapon row> <secs>: the bare hands play that weapon's moveset, no model in hand
                let mut it = rest.split_whitespace();
                let row = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let secs = it.next().and_then(|v| v.parse().ok()).unwrap_or(600.0);
                crate::weapon::set_move(row, secs);
                log!("lab script: bare hands play row {row} for {secs} s");
            }
            "fxview" => {
                // fxview <effect id>: that effect alone, 3 m in front of the fighter (the invisible strike's
                // bullet carries it for 1.5 s), to see what an effect id looks like
                use eldenring::cs::Bullet;
                let id = rest.trim().parse::<i32>().unwrap_or(-1);
                if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                    if let Some(b) = repo.get_mut::<Bullet>(crate::scripts::STRIKE_HIT as u32) {
                        b.set_sfx_id_bullet(id);
                        b.set_sfx_id_hit(-1);
                        b.set_life(1.5);
                    }
                }
                let p = crate::ability::position(player);
                let f = crate::ability::forward(player);
                let at = (p.0 + f.0 * 3.0, p.1 + 1.2, p.2 + f.2 * 3.0);
                let _ = crate::ability::fire_at(player, crate::scripts::STRIKE_HIT, at, (f.0, 0.0, f.2));
                log!("lab script: fxview {id}");
            }
            "fpzero" => player.chr_ins.modules.data.fp = 0,
            "fpfull" => {
                let d = &mut player.chr_ins.modules.data;
                d.fp = d.max_fp;
            }
            "slot" => { crate::anims::play(player, rest.trim().parse().unwrap_or(0)); }
            "look" => {
                // look <dx per poll> <polls>: the mouse moves sideways, the camera turns
                let mut it = rest.split_whitespace();
                let dx = it.next().and_then(|v| v.parse().ok()).unwrap_or(20);
                let n = it.next().and_then(|v| v.parse().ok()).unwrap_or(30);
                crate::binds::LAB_LOOK[0].store(dx, std::sync::atomic::Ordering::Relaxed);
                crate::binds::LAB_LOOK[1].store(n, std::sync::atomic::Ordering::Relaxed);
            }
            // 1.3.0: "wset <row> <secs>": the hands become that weapon for a while (scripts' Weapon step), for "cue <lua>"
            "wset" => {
                let mut it = rest.split_whitespace();
                let row = it.next().and_then(|t| t.parse::<u32>().ok()).unwrap_or(0);
                let secs = it.next().and_then(|t| t.parse::<f32>().ok()).unwrap_or(4.0);
                crate::weapon::set_move(row, secs);
                log!("lab script: wset {row} {secs}");
            }
            "swordrow" => crate::weapon::LAB_SWORD_ROW.store(rest.trim().parse().unwrap_or(0), std::sync::atomic::Ordering::Relaxed),
            "provar" => crate::scripts::LAB_PROTO_VARIANT.store(rest.trim().parse().unwrap_or(1), std::sync::atomic::Ordering::Relaxed),
            "proto" => crate::scripts::LAB_PROTO.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            "notrees" => crate::combos::LAB_NO_TREES.store(true, std::sync::atomic::Ordering::Relaxed),
            "vk" => {
                // vk <hex> <ms>: a real key press for other mods (QuestPath reads the keyboard itself)
                use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP};
                let mut it = rest.split_whitespace();
                let vk = sc(it.next().unwrap_or("0"));
                let ms = num(it.next().unwrap_or("100"));
                std::thread::spawn(move || unsafe {
                    keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY, 0);
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                    keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
                });
            }
            "down" => hold_key(sc(rest), true),
            "up" => hold_key(sc(rest), false),
            "cast" => {
                let d = &mut player.chr_ins.modules.data;
                d.fp = d.max_fp;
                if rest.trim() == "4" {
                    combat.awakening = crate::combat::AWAKENING_FULL;
                }
                combat.clear_cooldowns();
                combat.cast(player, rest.trim().parse::<usize>().unwrap_or(1).saturating_sub(1));
            }
            "fill" => {
                // the shared bullet pool filled with lasting bullets, as in a crowded fight
                let pos = crate::ability::position(player);
                let mut n = 0;
                for _ in 0..rest.trim().parse::<u32>().unwrap_or(140) {
                    if crate::ability::fire_at(player, 2070, (pos.0 + 6.0, pos.1 + 0.2, pos.2), (0.0, -1.0, 0.0)).is_ok() {
                        n += 1;
                    }
                }
                log!("lab script: filled {n}");
            }
            "castraw" => combat.cast(player, rest.trim().parse::<usize>().unwrap_or(1).saturating_sub(1)),
            "parryalways" => crate::parry::LAB_ALWAYS.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            "airdash" => FORCE_AIRDASH.store(true, std::sync::atomic::Ordering::Relaxed),
            "warp" => WARP.store(true, std::sync::atomic::Ordering::Relaxed),
            "goto" => GOTO.store(true, std::sync::atomic::Ordering::Relaxed),
            "ki" => {
                let d = &mut player.chr_ins.modules.data;
                d.fp = d.max_fp;
            }
            "awaken" => combat.awakening = crate::combat::AWAKENING_FULL,
            "attack" => combat.measure_attack(player, rest.trim() == "heavy"),
            // 1.2.0: "combo l" / "combo h": a press into the fighter's combo tree
            "combo" => combat.lab_press(rest.trim().starts_with('h')),
            // 0.17.0: shout attack|hurt (always, as if the dice said yes)
            "shout" => combat.bark(rest.trim(), 1.0, 1.0),
            "training" => crate::ui::UI.lock().unwrap().training = rest.trim() != "off",
            // 1.2.0: "touchfoe [m]": like touch, but the nearest hostile (soldiers, not the biggest thing around)
            "touchfoe" => {
                let gap = rest.trim().parse::<f32>().unwrap_or(1.6);
                let p = player.chr_ins.modules.physics.position;
                if let Some(e) = crate::combat::enemy_near(player, (p.0, p.1, p.2), 40.0, None) {
                    let (dx, dz) = (p.0 - e.0, p.2 - e.2);
                    let l = (dx * dx + dz * dz).sqrt().max(0.01);
                    let (ux, uz) = (dx / l, dz / l);
                    let phys = &mut player.chr_ins.modules.physics;
                    phys.position = eldenring::position::HavokPosition(e.0 + ux * gap, e.1 + 0.2, e.2 + uz * gap, p.3);
                    phys.chr_proxy_pos_update_requested = true;
                    let th = ux.atan2(uz);
                    phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
                    phys.interpolated_orientation = phys.orientation;
                    log!("lab script: touching a hostile ({gap} m, it was {l:.1} m away)");
                } else {
                    let mut seen = Vec::new();
                    if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                        for e in wcm.chr_inses_by_distance.iter().take(20) {
                            let c = unsafe { e.chr_ins.as_ref() };
                            let q = c.modules.physics.position;
                            seen.push(format!("team {} hp {} at {:.0} m", c.team_type, c.modules.data.hp, ((q.0 - p.0).powi(2) + (q.2 - p.2).powi(2)).sqrt()));
                        }
                    }
                    log!("lab script: no hostile within 40 m; around: {}", seen.join(", "));
                }
            }
            // 1.3.0: "entchr <entity> [m]": the character with this map entity id (villain slots): logged, and with
            // [m] the fighter stands that far from it, facing it
            // (2.0.3 "entchr <entity> <m> front": in front of it, where it faces, not on the fighter's side: the user had walked
            // up past Malenia before it ran, and was put behind her over the drop; a boss waiting for its fight faces the way
            // in. (Fixed points don't do: positions are relative to the loaded block, which moved as the user walked))
            "entchr" => {
                struct Dist { distance: f32 }
                let mut it = rest.split_whitespace();
                let id = it.next().and_then(|t| t.parse::<u32>().ok()).unwrap_or(0);
                let gap = it.next().and_then(|t| t.parse::<f32>().ok());
                let front = it.next() == Some("front");
                let mut facing = None;
                let mut at = None;
                let me = player.chr_ins.modules.physics.position;
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                    for c in wcm.chr_sets.iter().flatten().flat_map(|set| set.characters()) {
                        if c.event_entity_id == id {
                            let q = c.modules.physics.position;
                            let e = (((q.0 - me.0).powi(2) + (q.1 - me.1).powi(2) + (q.2 - me.2).powi(2)).sqrt(),);
                            let e = Dist { distance: e.0 };
                            log!("lab script: entchr {id}: {:.1} m, npc {} team {} hp {}/{} load {} render {} anim {} at ({:.1},{:.1},{:.1})", e.distance, c.npc_param_id, c.team_type, c.modules.data.hp, c.modules.data.max_hp, c.load_state.0, c.chr_flags1c5.enable_render(), { let ta = &c.modules.time_act; ta.anim_queue[(ta.read_idx as usize) % 10].anim_id }, q.0, q.1, q.2);
                            at = Some((q.0, q.1, q.2));
                            let o = c.modules.physics.orientation;
                            let th = 2.0 * o.1.atan2(o.3);
                            facing = Some((th.sin(), th.cos()));
                        }
                    }
                }
                match (at, gap) {
                    (None, _) => {
                        let mut ids = Vec::new();
                        if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                            for c in wcm.chr_sets.iter().flatten().flat_map(|set| set.characters()) {
                                ids.push((c.event_entity_id, c.npc_param_id));
                            }
                        }
                        log!("lab script: entchr {id}: not found among {} characters; block {} ids {:?}", ids.len(), player.current_block_id, &ids[..ids.len().min(60)]);
                    }
                    (Some(e), Some(gap)) => {
                        let p = player.chr_ins.modules.physics.position;
                        let (dx, dz) = match facing.filter(|_| front) { Some(f) => f, None => (p.0 - e.0, p.2 - e.2) };
                        let l = (dx * dx + dz * dz).sqrt().max(0.01);
                        let (ux, uz) = (dx / l, dz / l);
                        let phys = &mut player.chr_ins.modules.physics;
                        phys.position = eldenring::position::HavokPosition(e.0 + ux * gap, e.1 + 0.3, e.2 + uz * gap, p.3);
                        phys.chr_proxy_pos_update_requested = true;
                        let th = (-ux).atan2(-uz);
                        phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
                        phys.interpolated_orientation = phys.orientation;
                    }
                    _ => {}
                }
            }
            "touch" => {
                // touch [m]: stand right against the nearest enemy (default 2 m from its centre), facing it
                let gap = rest.trim().parse::<f32>().unwrap_or(2.0);
                let p = player.chr_ins.modules.physics.position;
                if let Some(e) = crate::training::biggest_near(player, 40.0) {
                    let (dx, dz) = (p.0 - e.0, p.2 - e.2);
                    let l = (dx * dx + dz * dz).sqrt().max(0.01);
                    let (ux, uz) = (dx / l, dz / l);
                    let phys = &mut player.chr_ins.modules.physics;
                    phys.position = eldenring::position::HavokPosition(e.0 + ux * gap, e.1 + 0.2, e.2 + uz * gap, p.3);
                    phys.chr_proxy_pos_update_requested = true;
                    let th = ux.atan2(uz);
                    phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
                    phys.interpolated_orientation = phys.orientation;
                    log!("lab script: touching the enemy ({gap} m)");
                }
            }
            // 1.2.x: "foeat <m>": the nearest hostile (60 m) brought to <m> metres straight ahead of the fighter,
            // facing them, held still (no moving, no attacks): a clean, repeatable target for videos
            // 1.3.0: "vilat [gap]": the villain brought in front of the fighter, facing them (it keeps fighting)
            "vilat" => {
                // vilat <gap> [side]: that far ahead (and to the side: out from behind the fighter, in the camera's view)
                let mut a = rest.split_whitespace().map(|t| t.parse::<f32>().ok());
                let gap = a.next().flatten().unwrap_or(1.8);
                let side = a.next().flatten().unwrap_or(0.0);
                let p = player.chr_ins.modules.physics.position;
                let f = crate::ability::forward(player);
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                    if let Some(e) = wcm.chr_inses_by_distance.iter().find(|e| crate::villains::is_villain_npc(unsafe { e.chr_ins.as_ref() }.npc_param_id) && e.distance < 80.0) {
                        let c = unsafe { &mut *e.chr_ins.as_ptr() };
                        let (x, z) = (p.0 + f.0 * gap - f.2 * side, p.2 + f.2 * gap + f.0 * side);
                        // (on the ground there: put at the fighter's height on a slope it fell through the hill)
                        let y = crate::motion::ground_below(player, x, p.1 + 6.0, z, 14.0).unwrap_or(p.1) + 0.3;
                        let phys = &mut c.modules.physics;
                        phys.position = eldenring::position::HavokPosition(x, y, z, p.3);
                        phys.chr_proxy_pos_update_requested = true;
                        let th = (-f.0).atan2(-f.2);
                        phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
                        phys.interpolated_orientation = phys.orientation;
                        log!("lab script: villain {} brought {gap} m ahead", c.npc_param_id);
                    }
                }
            }
            "foeat" => {
                let gap = rest.trim().parse::<f32>().unwrap_or(2.0);
                let p = player.chr_ins.modules.physics.position;
                let f = crate::ability::forward(player);
                if let Some(c) = nearest_foe(player, 60.0) {
                    let (x, z) = (p.0 + f.0 * gap, p.2 + f.2 * gap);
                    let phys = &mut c.modules.physics;
                    phys.position = eldenring::position::HavokPosition(x, p.1 + 0.3, z, p.3);
                    phys.chr_proxy_pos_update_requested = true;
                    // facing the fighter
                    let th = f.0.atan2(f.2);
                    phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
                    phys.interpolated_orientation = phys.orientation;
                    c.debug_flags.set_disabled_movement(true);
                    c.debug_flags.set_disabled_secondary_actions(true);
                    *FOEAT_AT.lock().unwrap() = Some((x, p.1 + 0.3, z));
                    log!("lab script: a hostile brought {gap} m ahead");
                }
            }
            // 1.2.x: "facecam": the fighter turned to look where the camera looks (his back to it)
            // 1.3.0: "facevil": the fighter turned toward the nearest villain (hidden too: its entrance spot); with "pad r3" after,
            // the camera looks there as well
            // 1.3.0: "dropat <bullet> <metres ahead> <height>": a bullet dropped straight down onto a spot ahead
            "dropat" => {
                let v: Vec<f32> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if v.len() >= 3 {
                    let pos = crate::ability::position(player);
                    let f = crate::ability::forward(player);
                    let at = (pos.0 + f.0 * v[1], pos.1 + v[2], pos.2 + f.2 * v[1]);
                    let r = crate::ability::spawn_ex(player.chr_ins.field_ins_handle, -1, -1, -1, v[0] as i32, at, (0.0, -1.0, 0.0));
                    log!("lab dropat {} {} m ahead, {} m up: {r:?}", v[0], v[1], v[2]);
                }
            }
            "facevil" => {
                let p = player.chr_ins.modules.physics.position;
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                    let v = wcm.chr_sets.iter().flatten().flat_map(|set| set.characters()).filter(|c| crate::villains::is_villain_npc(c.npc_param_id))
                        .map(|c| c.modules.physics.position).min_by(|a, b| { let d = |q: &eldenring::position::HavokPosition| (q.0 - p.0).powi(2) + (q.2 - p.2).powi(2); d(a).total_cmp(&d(b)) });
                    if let Some(q) = v {
                        let th = (-(q.0 - p.0)).atan2(-(q.2 - p.2));
                        let phys = &mut player.chr_ins.modules.physics;
                        phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
                        phys.interpolated_orientation = phys.orientation;
                        log!("lab script: facing the villain at ({:.1}, {:.1}, {:.1})", q.0, q.1, q.2);
                    }
                }
            }
            // 1.3.0: "vilside <m>": the fighter put this far from the nearest villain, off to the side of the line from the boss
            // to it (a clear view of its entrance), facing it
            "vilside" => {
                let m = rest.trim().parse::<f32>().unwrap_or(16.0);
                let p = player.chr_ins.modules.physics.position;
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                    let d2 = |q: &eldenring::position::HavokPosition| (q.0 - p.0).powi(2) + (q.2 - p.2).powi(2);
                    let all: Vec<_> = wcm.chr_sets.iter().flatten().flat_map(|set| set.characters()).map(|c| (c.npc_param_id, c.modules.data.max_hp, c.modules.data.hp, c.team_type, c.modules.physics.position)).collect();
                    let vil = all.iter().filter(|c| crate::villains::is_villain_npc(c.0)).map(|c| c.4).min_by(|a, b| d2(a).total_cmp(&d2(b)));
                    let boss = all.iter().filter(|c| !crate::villains::is_villain_npc(c.0) && c.2 > 0 && c.3 != player.chr_ins.team_type && d2(&c.4) < 60.0 * 60.0).max_by_key(|c| c.1).map(|c| c.4);
                    if let (Some(q), Some(b)) = (vil, boss) {
                        let (dx, dz) = (q.0 - b.0, q.2 - b.2);
                        let l = (dx * dx + dz * dz).sqrt().max(0.01);
                        let (sx, sz) = (-dz / l, dx / l);
                        let to = (q.0 + sx * m, q.1 + 2.0, q.2 + sz * m);
                        let y = crate::motion::ground_below(player, to.0, to.1 + 6.0, to.2, 30.0).unwrap_or(q.1);
                        let th = (-(q.0 - to.0)).atan2(-(q.2 - to.2));
                        let phys = &mut player.chr_ins.modules.physics;
                        phys.position.0 = to.0;
                        phys.position.1 = y + 0.2;
                        phys.position.2 = to.2;
                        phys.chr_proxy_pos_update_requested = true;
                        phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
                        phys.interpolated_orientation = phys.orientation;
                        log!("lab script: vilside {m} m from the villain at ({:.1}, {:.1}, {:.1}), the boss at ({:.1}, {:.1}, {:.1})", q.0, q.1, q.2, b.0, b.1, b.2);
                    }
                }
            }
            "facecam" => {
                if let Some((f, _)) = crate::input::camera_axes() {
                    let th = (-f.0).atan2(-f.2);
                    let phys = &mut player.chr_ins.modules.physics;
                    phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
                    phys.interpolated_orientation = phys.orientation;
                    log!("lab script: facing away from the camera");
                }
            }
            // 1.2.x: "handfx <sfx> [sfx2] [dmy]" / "handfx off": effects held on a dummy poly of the player (palm charge tests)
            "handfx" => {
                if rest.trim() == "off" {
                    crate::aura::off(player, crate::aura::PALM);
                } else {
                    let v: Vec<i32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    let (a, b, d) = (v.first().copied().unwrap_or(-1), v.get(1).copied().unwrap_or(-1), v.get(2).copied().unwrap_or(26) as i16);
                    crate::aura::off(player, crate::aura::PALM);
                    let ok = crate::aura::palm_prepare([a, b], d);
                    crate::aura::on(player, crate::aura::PALM);
                    log!("lab script: handfx {a} {b} on dummy {d}: {ok}");
                }
            }
            "grab" => {
                let ok = combat.whip_strike(player);
                log!("lab script: grab {ok}");
            }
            "log" => log!("lab script: {rest}"),
            // (2.0.7: the fault log's memory dump, on an address: "hexdump er+<hex>" or "hexdump <hex>")
            "hexdump" => {
                let t = rest.trim();
                let er = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) } as usize;
                let a = match t.strip_prefix("er+") { Some(o) => er + usize::from_str_radix(o.trim_start_matches("0x"), 16).unwrap_or(0), None => usize::from_str_radix(t.trim_start_matches("0x"), 16).unwrap_or(0) };
                let sleep = windows_sys::Win32::System::Threading::Sleep as usize;
                log!("lab hexdump: (Sleep is {:?}){}", crate::veh::module_of(sleep), crate::veh::hexdump(t, a));
            }
            // "foehold <height> <tilt deg> <spin deg/s> <yaw deg>" / "foehold off"
            "foehold" => {
                let v: Vec<f32> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                let mut g = FOEHOLD.lock().unwrap();
                if rest.trim() == "off" || v.is_empty() {
                    if let Some(c) = nearest_foe(player, 12.0) {
                        c.modules.physics.gravity_disabled = false;
                        c.chr_ctrl.additional_orientation_quat = fromsoftware_shared::F32Vector4(0.0, 0.0, 0.0, 1.0);
                        c.modules.behavior.animation_speed = 1.0;
                    }
                    *g = None;
                } else {
                    let at = g.as_ref().and_then(|x| x.4);
                    *g = Some((v[0], v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0), v.get(3).copied().unwrap_or(0.0), at, v.get(4).copied().unwrap_or(-1.0)));
                }
            }
            "strikelevel" => crate::combos::LAB_LEVEL.store(rest.trim().parse().unwrap_or(0), std::sync::atomic::Ordering::Relaxed),
            "impactstyle" => crate::juggle::IMPACT_STYLE.store(rest.trim().parse().unwrap_or(0), std::sync::atomic::Ordering::Relaxed),
            "dmgscale" => crate::combat::LAB_DMG_SCALE.store((rest.trim().parse::<f32>().unwrap_or(1.0) * 1000.0) as u32, std::sync::atomic::Ordering::Relaxed),
            "launchswitch" => crate::combat::LAB_SWITCHES.store(u32::from_str_radix(rest.trim(), 2).unwrap_or(0b111), std::sync::atomic::Ordering::Relaxed),
            "launchpose" => crate::juggle::POSE.store(match rest.trim() { "a" => 1, "b" => 2, "mix" => 3, "impact" => 4, _ => 0 }, std::sync::atomic::Ordering::Relaxed),
            "foehang" => {
                let v: Vec<f32> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                *FOEHANG.lock().unwrap() = if v.is_empty() { None } else { Some((v[0], v.get(1).copied().unwrap_or(4.0) as u32, None, 0)) };
            }
            // "foeragdoll <state> [revive s]": the nearest hostile's ragdoll state (lab: a live ragdoll?)
            "foeragdoll" => {
                let v: Vec<f32> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if let Some(c) = nearest_foe(player, 12.0) {
                    let before = (c.chr_ctrl.chr_ragdoll_state, c.chr_ctrl.ragdoll_revive_time);
                    c.chr_ctrl.chr_ragdoll_state = v.first().copied().unwrap_or(0.0) as u8;
                    if let Some(t) = v.get(1) {
                        c.chr_ctrl.ragdoll_revive_time = *t;
                    }
                    log!("lab script: foe {} ragdoll {:?} -> ({}, {})", c.npc_param_id, before, c.chr_ctrl.chr_ragdoll_state, c.chr_ctrl.ragdoll_revive_time);
                }
            }
            "foeragdollwatch" => {
                if let Some(c) = nearest_foe(player, 12.0) {
                    log!("lab script: foe {} ragdoll state {} revive {:.2} hp {} y {:.2}", c.npc_param_id, c.chr_ctrl.chr_ragdoll_state, c.chr_ctrl.ragdoll_revive_time, c.modules.data.hp, c.modules.physics.position.1);
                }
            }
            "foefloat" => {
                let v: Vec<f32> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if let Some(c) = nearest_foe(player, 12.0) {
                    if v.is_empty() {
                        c.modules.physics.gravity_multiplier = 1.0;
                        *FOEFLOAT.lock().unwrap() = None;
                    } else {
                        let ph = &mut c.modules.physics;
                        ph.position = eldenring::position::HavokPosition(ph.position.0, ph.position.1 + v[0], ph.position.2, ph.position.3);
                        ph.chr_proxy_pos_update_requested = true;
                        ph.gravity_disabled = false;
                        *FOEFLOAT.lock().unwrap() = Some(v.get(1).copied().unwrap_or(0.05));
                    }
                }
            }
            "foefall" => FOEFALL.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            "foewatch" => FOEWATCH.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            // "foeanim <id> [speed]": the nearest hostile plays one of its own animations
            "foeanim" => {
                let mut it = rest.split_whitespace();
                let id = it.next().and_then(|t| t.parse::<i32>().ok()).unwrap_or(0);
                let sp = it.next().and_then(|t| t.parse::<f32>().ok());
                if let Some(c) = nearest_foe(player, 15.0) {
                    c.modules.event.request_animation_id = id;
                    if let Some(sp) = sp {
                        c.modules.behavior.animation_speed = sp;
                    }
                    log!("lab script: foe {} asked to play {id}", c.npc_param_id);
                }
            }
            "foespeed" => {
                if let Some(c) = nearest_foe(player, 15.0) {
                    c.modules.behavior.animation_speed = rest.trim().parse::<f32>().unwrap_or(1.0);
                }
            }
            "boss" => BOSS.store(true, std::sync::atomic::Ordering::Relaxed),
            "art" => crate::bosses::FORCE_ART.store(true, std::sync::atomic::Ordering::Relaxed),
            "lockstate" => log!("lab script: locked on {} {rest}", player.chr_ins.is_locked_on),
            "php" => {
                let d = &mut player.chr_ins.modules.data;
                d.hp = ((d.max_hp as f32) * rest.trim().parse::<f32>().unwrap_or(0.5)).max(1.0) as i32;
                log!("lab script: player hp {}/{}", d.hp, d.max_hp);
            }
            "lock" => LOCK.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            "warnall" => crate::ui::UI.lock().unwrap().warnings = crate::ui::Warnings::All,
            // 1.3.0: "bossover": every boss on screen 300 HP above its max for a frame (what a mid-fight max HP drop
            // looks like to bosses.rs: the game's clamp back must not read as a hit)
            "bossover" => {
                if let (Ok(fe), Ok(wcm)) = (unsafe { eldenring::cs::CSFeManImp::instance() }, unsafe { eldenring::cs::WorldChrMan::instance_mut() }) {
                    for b in fe.boss_health_displays.iter().filter(|b| b.fmg_id > 0) {
                        if let Some(c) = wcm.chr_ins_by_handle_mut(&b.field_ins_handle) {
                            log!("lab script: bossover {} -> {} (max {})", c.modules.data.hp, c.modules.data.max_hp + 300, c.modules.data.max_hp);
                            c.modules.data.hp = c.modules.data.max_hp + 300;
                        }
                    }
                }
            }
            "bosshp" => {
                // bosshp <fraction>: every boss on screen to that share of its health (1.1.0 kill tests)
                let frac = rest.trim().parse::<f32>().unwrap_or(0.5);
                if let (Ok(fe), Ok(wcm)) = (unsafe { eldenring::cs::CSFeManImp::instance() }, unsafe { eldenring::cs::WorldChrMan::instance() }) {
                    for b in fe.boss_health_displays.iter().filter(|b| b.fmg_id > 0) {
                        for c in wcm.chr_sets.iter().flatten().flat_map(|set| set.characters()) {
                            let c = unsafe { &mut *(c as *const eldenring::cs::ChrIns as *mut eldenring::cs::ChrIns) };
                            if c.field_ins_handle == b.field_ins_handle {
                                let want = (c.modules.data.max_hp as f32 * frac) as i32;
                                // (counted as already scaled, so bosses.rs leaves it as set)
                                crate::bosses::DIRECT.lock().unwrap().push((c as *const _ as usize, c.modules.data.hp - want));
                                c.modules.data.hp = want;
                                log!("lab script: boss hp {}/{}", c.modules.data.hp, c.modules.data.max_hp);
                            }
                        }
                    }
                }
            }
            // 2.0.2: "bosshit <hp>": every boss on screen hurt as by the game's own blow (not counted as scaled; held at 1 HP
            // as the boss fight's "can't die" switch holds it)
            "bosshit" => {
                let n = rest.trim().parse::<i32>().unwrap_or(1);
                if let (Ok(fe), Ok(wcm)) = (unsafe { eldenring::cs::CSFeManImp::instance() }, unsafe { eldenring::cs::WorldChrMan::instance() }) {
                    for b in fe.boss_health_displays.iter().filter(|b| b.fmg_id > 0) {
                        for c in wcm.chr_sets.iter().flatten().flat_map(|set| set.characters()) {
                            let c = unsafe { &mut *(c as *const eldenring::cs::ChrIns as *mut eldenring::cs::ChrIns) };
                            if c.field_ins_handle == b.field_ins_handle && c.modules.data.hp > 0 {
                                c.modules.data.hp = (c.modules.data.hp - n).max(1);
                                log!("lab script: boss hit for {n}: hp {}/{}", c.modules.data.hp, c.modules.data.max_hp);
                            }
                        }
                    }
                }
            }
            // 2.0.2: "bossq": every boss on screen's health
            "bossq" => {
                if let (Ok(fe), Ok(wcm)) = (unsafe { eldenring::cs::CSFeManImp::instance() }, unsafe { eldenring::cs::WorldChrMan::instance() }) {
                    for b in fe.boss_health_displays.iter().filter(|b| b.fmg_id > 0) {
                        for c in wcm.chr_sets.iter().flatten().flat_map(|set| set.characters()) {
                            if c.field_ins_handle == b.field_ins_handle {
                                log!("lab script: boss hp {}/{} dead {}", c.modules.data.hp, c.modules.data.max_hp, c.chr_flags1c5.death_flag());
                            }
                        }
                    }
                }
            }
            "whip" => WHIP.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            "fire" => {
                // fire <bullet> [goods]: one bullet 3 m ahead (effect pool leak tests)
                let mut it = rest.split_whitespace();
                let id = it.next().and_then(|x| x.parse::<i32>().ok()).unwrap_or(0);
                let goods = it.next().and_then(|x| x.parse::<i32>().ok()).unwrap_or(1700);
                let magic = it.next().and_then(|x| x.parse::<i32>().ok()).unwrap_or(-1);
                let behavior = it.next().and_then(|x| x.parse::<i32>().ok()).unwrap_or(-1);
                let pos = crate::ability::position(player);
                let dir = crate::ability::forward(player);
                let r = crate::ability::spawn_ex(player.chr_ins.field_ins_handle, goods, magic, behavior, id, (pos.0 + dir.0 * 3.0, pos.1 + 1.2, pos.2 + dir.2 * 3.0), dir);
                log!("lab fire {id} goods {goods} magic {magic} behavior {behavior}: {r:?} | {}", bullet_info(id));
            }
            "firedir" => {
                // 1.3.0: firedir <bullet> <metres ahead> <yaw deg, + to the left> <pitch deg, + up>: a bullet at chest height
                // that far ahead, flying the given way (how a bullet turns its effect's model)
                let mut it = rest.split_whitespace();
                let id = it.next().and_then(|x| x.parse::<i32>().ok()).unwrap_or(0);
                let d = it.next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(3.0);
                let yaw = it.next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0).to_radians();
                let pitch = it.next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0).to_radians();
                let pos = crate::ability::position(player);
                let f = crate::ability::forward(player);
                // turned `yaw` to the left about the up axis (left of (x, z) is (z, -x)... the character's left is (-fz, fx))
                let (c, s) = (yaw.cos(), yaw.sin());
                let h = (f.0 * c - f.2 * s, f.2 * c + f.0 * s);
                let dir = (h.0 * pitch.cos(), pitch.sin(), h.1 * pitch.cos());
                let r = crate::ability::spawn_ex(player.chr_ins.field_ins_handle, 1700, -1, -1, id, (pos.0 + f.0 * d, pos.1 + 1.2, pos.2 + f.2 * d), dir);
                log!("lab firedir {id} {d} m yaw {:.0} pitch {:.0}: {r:?}", yaw.to_degrees(), pitch.to_degrees());
            }
            "fireup" => {
                // 1.3.0: fireup <bullet> <metres ahead> <metres up>: a bullet that far ahead and up, facing ahead (drop tests)
                let mut it = rest.split_whitespace();
                let id = it.next().and_then(|x| x.parse::<i32>().ok()).unwrap_or(0);
                let d = it.next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(5.0);
                let up = it.next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(8.0);
                let pos = crate::ability::position(player);
                let f = crate::ability::forward(player);
                let r = crate::ability::spawn_ex(player.chr_ins.field_ins_handle, 1700, -1, -1, id, (pos.0 + f.0 * d, pos.1 + up, pos.2 + f.2 * d), (f.2, 0.0, -f.0));
                log!("lab fireup {id}: {r:?}");
            }
            "spawnflags" => crate::ability::SPAWN_FLAGS.store(rest.trim().parse().unwrap_or(0), std::sync::atomic::Ordering::Relaxed),
            // 2.0.1 crash hunt: "cuehammer": the cue files rewritten every 2 ms (short and long by turns), as the DLL writes
            // them, so a script's loadfile meets a half-written file far more often than in play
            "cuehammer" => {
                if let Some(dir) = crate::recolor::mod_dir().map(|d| d.join("action").join("script")) {
                    std::thread::spawn(move || {
                        let long = format!("-- {}\n", "x".repeat(600));
                        let mut n = 0u64;
                        loop {
                            let text = if n % 2 == 0 { "-- hammer\n" } else { long.as_str() };
                            let _ = std::fs::write(dir.join("jr_cue.lua"), text);
                            let _ = std::fs::write(dir.join("jr_npc_cue.lua"), text);
                            n += 1;
                            std::thread::sleep(std::time::Duration::from_millis(2));
                        }
                    });
                    log!("lab script: cuehammer on");
                }
            }
            // 2.0.6: "edgedrop x y z": the body to that havok position, gravity left on (off an edge: rescue.rs FIGHT_DROP)
            "edgedrop" => {
                let v: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if v.len() == 3 {
                    // (block coordinates: the world's are re-based from load to load)
                    let b = (player.block_position.x, player.block_position.y, player.block_position.z);
                    let ph = &mut player.chr_ins.modules.physics;
                    let p = ph.position;
                    ph.position = eldenring::position::HavokPosition(p.0 + v[0] - b.0, p.1 + v[1] - b.1, p.2 + v[2] - b.2, p.3);
                    ph.chr_proxy_pos_update_requested = true;
                    ph.gravity_disabled = false;
                    log!("lab script: edgedrop to block ({:.1}, {:.1}, {:.1}) from block ({:.1}, {:.1}, {:.1})", v[0], v[1], v[2], b.0, b.1, b.2);
                }
            }
            "lift" => {
                // lift <m>: put the fighter that far up (fall tests)
                let p = player.chr_ins.modules.physics.position;
                let up = rest.trim().parse::<f32>().unwrap_or(30.0);
                player.chr_ins.modules.physics.position = eldenring::position::HavokPosition(p.0, p.1 + up, p.2, p.3);
                player.chr_ins.modules.physics.chr_proxy_pos_update_requested = true;
            }
            "rays" => {
                // rays: which collision filters see the floor under the player (straight down, 6 m)
                let p = player.chr_ins.modules.physics.position;
                if let Ok(h) = unsafe { eldenring::cs::CSHavokMan::instance() } {
                    let mut hits = Vec::new();
                    for hi in [0u32, 0x1000000, 0x2000000, 0x3000000, 0x4000000] {
                        for layer in 0..128u32 {
                            let f = hi | layer;
                            if let Some(g) = h.phys_world.cast_ray(f, &eldenring::position::HavokPosition(p.0, p.1 + 1.0, p.2, 0.0), eldenring::position::PositionDelta(0.0, -6.0, 0.0), player) {
                                hits.push(format!("{f:#x}:{:.2}", g.1 - p.1));
                            }
                        }
                    }
                    log!("lab script: rays {rest} at y {:.2}: {}", p.1, hits.join(" "));
                }
            }
            "vfx" => {
                // vfx: the visual settings behind every fighter's aura SpEffect
                use eldenring::cs::{SpEffectParam, SpEffectVfxParam};
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    for f in crate::sheets::FIGHTERS.iter() {
                        let Some(sp) = repo.get::<SpEffectParam>(f.aura_speffect as u32) else { continue };
                        let ids = [sp.vfx_id(), sp.vfx_id1(), sp.vfx_id2(), sp.vfx_id3(), sp.vfx_id4(), sp.vfx_id5(), sp.vfx_id6(), sp.vfx_id7()];
                        for id in ids.into_iter().filter(|i| *i > 0) {
                            if let Some(v) = repo.get::<SpEffectVfxParam>(id as u32) {
                                log!("lab script: vfx {} sp {} vfx {id}: camo {:.1}..{:.1} min {} transform {} phantom {}/{} material {} {:.2}->{:.2} type {} midst {}", f.id, f.aura_speffect, v.camouflage_begin_dist(), v.camouflage_end_dist(), v.camouflage_min_alpha(), v.transform_protector_id(), v.phantom_param_overwrite_type(), v.phantom_param_overwrite_id(), v.material_param_id(), v.material_param_init_value(), v.material_param_target_value(), v.effect_type(), v.midst_sfx_id());
                            }
                        }
                    }
                }
            }
            "hrays" => {
                // hrays: what each solid filter hits ahead (facing) at the feet, middle and head, 8 m
                let p = player.chr_ins.modules.physics.position;
                let f = crate::ability::forward(player);
                if let Ok(h) = unsafe { eldenring::cs::CSHavokMan::instance() } {
                    let mut out = Vec::new();
                    for flt in crate::motion::SOLID_FILTERS {
                        for hh in [0.3f32, 1.0, 1.7] {
                            if let Some(g) = h.phys_world.cast_ray(flt, &eldenring::position::HavokPosition(p.0, p.1 + hh, p.2, 0.0), eldenring::position::PositionDelta(f.0 * 8.0, 0.0, f.2 * 8.0), player) {
                                out.push(format!("{flt:#x}@{hh}:{:.2}m", ((g.0 - p.0).powi(2) + (g.2 - p.2).powi(2)).sqrt()));
                            }
                        }
                    }
                    log!("lab script: hrays {rest}: {}", out.join(" "));
                }
            }
            "hp" => log!("lab script: no-dead {:#x} hp {}/{} {rest}", crate::movement::no_dead_mask(), player.chr_ins.modules.data.hp, player.chr_ins.modules.data.max_hp),
            "pools" => log!("lab script: pools {} {rest}", crate::ability::pool_stats()),
            "fp" => log!("lab script: fp {}/{} stamina {}/{} airborne {} {rest}", player.chr_ins.modules.data.fp, player.chr_ins.modules.data.max_fp, player.chr_ins.modules.data.stamina, player.chr_ins.modules.data.max_stamina, !player.chr_ins.modules.physics.is_touching_ground),
            "probe" => {
                use eldenring::cs::EquipParamWeapon;
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    for row in [4000000u32, 110000, 9000000] {
                        if let Some(w) = repo.get::<EquipParamWeapon>(row) {
                            log!("lab probe: weapon {row} model {} category {} gender {}", w.equip_model_id(), w.equip_model_category(), w.equip_model_gender());
                        }
                    }
                }
                if let Ok(m) = unsafe { eldenring::cs::CSLuaEventManImp::instance() } {
                    log!("lab probe: lua bonfire {:?}", m.lua_event_script_imitation.as_ref().map(|s| s.bonfire_entity_id));
                }
                if let Ok(m) = unsafe { eldenring::cs::CSBulletManager::instance() } {
                    log!("lab probe: bullets iter {} count field {} sfx {}", m.bullets().count(), m.bullets_count, m.sfx_related_count);
                }
            }
            "watch" => WATCH.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            // myhands: the player's weapon slots (chr_asm [L1,R1,L2,R2,L3,R3]) and each row's model and moveset
            // 1.3.1: rslot <0..2>: hold that right-hand slot (as the user's character held its empty third one)
            "rslot" => {
                let n = rest.trim().parse::<u32>().unwrap_or(0).min(2);
                let pgd = unsafe { player.player_game_data.as_mut() };
                pgd.equipment.chr_asm.equipment.selected_slots.right_weapon_slot = n;
                log!("lab script: right-hand slot {n} held");
            }
            "myhands" => {
                use eldenring::cs::EquipParamWeapon;
                let ids = player.chr_asm.equipment_param_ids;
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    let desc: Vec<String> = ids[..6].iter().map(|&id| {
                        let row = if id <= 0 { 0 } else { (id - id.rem_euclid(100)) as u32 };
                        repo.get::<EquipParamWeapon>(row).map(|w| format!("{id}(model {} motion {})", w.equip_model_id(), w.wepmotion_category())).unwrap_or(format!("{id}"))
                    }).collect();
                    log!("lab myhands: {} style {:?} active right {} left {} level {}", desc.join(" "), player.chr_asm.equipment.arm_style,
                        player.chr_asm.equipment.active_right_weapon_slot() as usize, player.chr_asm.equipment.active_left_weapon_slot() as usize, unsafe { player.player_game_data.as_ref() }.level);
                }
            }
            "wpn" => {
                // a weapon row's Ash of War, its arts id (the animation section is a<600 + id>) and FP use
                use eldenring::cs::{EquipParamWeapon, SwordArtsParam};
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    let row = rest.trim().parse::<u32>().unwrap_or(110000);
                    if let Some(w) = repo.get::<EquipParamWeapon>(row) {
                        let sa = w.sword_arts_param_id();
                        let t = repo.get::<SwordArtsParam>(sa.max(0) as u32).map(|a| (a.sword_arts_type(), a.use_point_l2())).unwrap_or((0, 0));
                        log!("lab wpn {row}: swordArtsParam {sa} arts type {} use_l2 {} wepType {} model {} motion {} atk {}/{}/{}/{}/{} behavior {}", t.0, t.1, w.wep_type(),
                            w.equip_model_id(), w.wepmotion_category(), w.attack_base_physics(), w.attack_base_magic(), w.attack_base_fire(), w.attack_base_thunder(), w.attack_base_dark(), w.behavior_variation_id());
                    }
                }
            }
            "arts" => {
                // the bare hands play Ash of War <SwordArtsParam row> (weapon.rs, anim.rs Cue::Arts)
                let sa = rest.trim().parse::<i32>().unwrap_or(0);
                crate::anim::cue(player, crate::anim::Cue::Arts(sa));
                log!("lab script: arts {sa} fp {}", player.chr_ins.modules.data.fp);
            }
            "setarts" => {
                use eldenring::cs::EquipParamWeapon;
                if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                    let sa = rest.trim().parse::<i32>().unwrap_or(0);
                    if let Some(w) = repo.get_mut::<EquipParamWeapon>(110000) {
                        w.set_sword_arts_param_id(sa);
                    }
                }
            }
            "facewall" => {
                // the nearest wall around (chest-high rays in 32 directions): stand 3 m from it, facing it
                use eldenring::position::{HavokPosition, PositionDelta};
                if let Ok(h) = unsafe { eldenring::cs::CSHavokMan::instance() } {
                    let p = player.chr_ins.modules.physics.position;
                    let mut best: Option<(f32, f32, f32)> = None;
                    for i in 0..32 {
                        let a = i as f32 * std::f32::consts::TAU / 32.0;
                        let (dx, dz) = (a.sin(), a.cos());
                        if let Some(hit) = h.phys_world.cast_ray(crate::motion::MAP_FILTER, &HavokPosition(p.0, p.1 + 1.0, p.2, 0.0), PositionDelta(dx * 40.0, 0.0, dz * 40.0), player) {
                            let d = ((hit.0 - p.0).powi(2) + (hit.2 - p.2).powi(2)).sqrt();
                            if best.is_none_or(|b| d < b.0) {
                                best = Some((d, dx, dz));
                            }
                        }
                    }
                    if let Some((d, dx, dz)) = best {
                        let want = rest.trim().parse::<f32>().unwrap_or(3.0);
                        let back = (d - want).max(0.0);
                        let phys = &mut player.chr_ins.modules.physics;
                        phys.position = HavokPosition(p.0 + dx * back, p.1, p.2 + dz * back, p.3);
                        phys.chr_proxy_pos_update_requested = true;
                        let th = (-dx).atan2(-dz);
                        phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
                        phys.interpolated_orientation = phys.orientation;
                        log!("lab script: wall {d:.1} m away at ({dx:.2}, {dz:.2}); now 3 m from it");
                    } else {
                        log!("lab script: no wall within 40 m");
                    }
                }
            }
            "reloadat" => {
                // reload in place with GameMan.last_load_position set to where the player stands
                // (2.0.6: never toward an unloaded block, as look.rs: run during a fighter change's load, m255 sent the
                // game to the title screen)
                let area = player.current_block_id.area();
                // (and just after a load the tile isn't known yet for a moment: asked again a quarter second later, for up to 15 s;
                // skipped outright, a "waitworld" after it waited for good: bossloads 2026-10-27)
                static TRIES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                if area == 0 || area == 0xFF {
                    if TRIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 60 {
                        s.at -= 1;
                        s.wait_until = now + std::time::Duration::from_millis(250);
                    } else {
                        TRIES.store(0, std::sync::atomic::Ordering::Relaxed);
                        log!("lab script: reloadat skipped: tile {} isn't loaded", player.current_block_id);
                    }
                } else if let Ok(gm) = unsafe { eldenring::cs::GameMan::instance_mut() } {
                    let b = &player.block_position;
                    gm.last_load_position = fromsoftware_shared::F32Vector4(b.x, b.y, b.z, 1.0);
                    if rest.trim() == "join" {
                        gm.load_target_block_id = player.current_block_id;
                        gm.multiplay_join_block_pos = eldenring::position::BlockPosition { x: b.x, y: b.y, z: b.z, yaw: b.yaw };
                    }
                    gm.move_map_target = player.current_block_id;
                    gm.initial_area_entity_id = 0;
                    gm.warp_requested = true;
                    TRIES.store(0, std::sync::atomic::Ordering::Relaxed);
                    log!("lab script: reloadat ({:.1}, {:.1}, {:.1}) tile {}", b.x, b.y, b.z, player.current_block_id);
                }
            }
            "puth" => {
                // puth x y z: the body to that havok position, gravity off (held)
                let v: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if v.len() == 3 {
                    let ph = &mut player.chr_ins.modules.physics;
                    ph.position = eldenring::position::HavokPosition(v[0], v[1], v[2], ph.position.3);
                    ph.chr_proxy_pos_update_requested = true;
                    ph.gravity_disabled = true;
                    log!("lab script: puth {v:?}");
                }
            }
            "graceinfo" => {
                use eldenring::cs::{BonfireWarpParam, SoloParamRepository};
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    // "graceinfo <area>": every grace in that area (with its row id), else the old Limgrave set
                    let area: Option<u8> = rest.trim().parse().ok();
                    for r in (0..3000).filter_map(|i| repo.get_row_by_index::<BonfireWarpParam>(i)).filter(|r| match area {
                        Some(a) => r.area_no() == a,
                        None => r.area_no() == 60 && (r.grid_x_no() == 41 || r.grid_x_no() == 42) && (36..=38).contains(&r.grid_z_no()),
                    }) {
                        log!("lab script: grace {} tile {}_{}_{} pos ({:.1}, {:.1}, {:.1})", r.bonfire_entity_id(), r.area_no(), r.grid_x_no(), r.grid_z_no(), r.pos_x(), r.pos_y(), r.pos_z());
                    }
                }
            }
            // spawnnpc <npc param> <think param> <chara init> [metres ahead] [chr id]: 1.3.0 villain summons
            // prototype: a character made by the game's debug creator in front of the player
            "spawnnpc" => {
                let v: Vec<i32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                let (npc, think, init) = (v.first().copied().unwrap_or(0), v.get(1).copied().unwrap_or(0), v.get(2).copied().unwrap_or(-1));
                let ahead = v.get(3).copied().unwrap_or(5) as f32;
                let chr = v.get(4).copied().unwrap_or(0);
                let blockc = if rest.contains("block") { 1.0 } else { 0.0 };
                let p = crate::ability::position(player);
                let f = crate::ability::forward(player);
                let (hv, bl) = (player.chr_ins.modules.physics.position, &player.block_position);
                let off = (hv.0 - bl.x, hv.1 - bl.y, hv.2 - bl.z);
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance_mut() } {
                    crate::villains::prepare_creator(wcm);
                    wcm.spawn_debug_character(&eldenring::cs::ChrDebugSpawnRequest {
                        chr_id: chr, chara_init_param_id: init, npc_param_id: npc, npc_think_param_id: think,
                        event_entity_id: 0, talk_id: 0, pos_x: p.0 + f.0 * ahead - off.0 * blockc, pos_y: p.1 + 0.5 - off.1 * blockc, pos_z: p.2 + f.2 * ahead - off.2 * blockc,
                    });
                    log!("lab script: spawnnpc c{chr:04} npc {npc} think {think} init {init} {ahead} m ahead");
                }
            }
            // npcanim <anim id>: the last spawned npc asked to play an animation (does a human npc take requests?)
            "npcanim" => {
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance_mut() } {
                    if let Some(mut c) = wcm.debug_chr_creator.last_created_chr {
                        let c = unsafe { c.as_mut() };
                        c.modules.event.request_animation_id = rest.trim().parse().unwrap_or(-1);
                        log!("lab script: npcanim {} asked", rest.trim());
                    }
                }
            }
            // npcfire <bullet>: a bullet owned by the last spawned npc, from its chest toward the player
            "npcfire" => {
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                    if let Some(c) = wcm.debug_chr_creator.last_created_chr {
                        let c = unsafe { c.as_ref() };
                        let from = c.modules.physics.position;
                        let to = crate::ability::position(player);
                        let (dx, dy, dz) = (to.0 - from.0, to.1 + 1.0 - (from.1 + 1.3), to.2 - from.2);
                        let l = (dx * dx + dy * dy + dz * dz).sqrt().max(0.01);
                        let r = crate::ability::spawn(c.field_ins_handle, -1, rest.trim().parse().unwrap_or(0), (from.0, from.1 + 1.3, from.2), (dx / l, dy / l, dz / l));
                        log!("lab script: npcfire {} -> {r:?}", rest.trim());
                    }
                }
            }
            // villainlook <row> <helm> <body> <arms> <legs> <right weapon> <left weapon> [hp]: dress a villain's rows
            // (CharaInitParam / NpcParam 9990200..) before spawning it
            "villainlook" => {
                let v: Vec<i32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if v.len() >= 7 {
                    if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                        if let Some(c) = repo.get_mut::<eldenring::cs::CharaInitParam>(v[0] as u32) {
                            c.set_equip_helm(v[1]);
                            c.set_equip_armer(v[2]);
                            c.set_equip_gaunt(v[3]);
                            c.set_equip_leg(v[4]);
                            c.set_equip_wep_right(v[5]);
                            c.set_equip_wep_left(v[6]);
                        }
                        if let (Some(hp), Some(n)) = (v.get(7), repo.get_mut::<eldenring::cs::NpcParam>(v[0] as u32)) {
                            n.set_hp(*hp as u32);
                        }
                        log!("lab script: villainlook {v:?}");
                    }
                }
            }
            // npcshow [on|off]: the last spawned npc drawn (render group, render on, opaque), and its flags logged
            "npcshow" => {
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance_mut() } {
                    if let Some(mut c) = wcm.debug_chr_creator.last_created_chr {
                        let c = unsafe { c.as_mut() };
                        log!("lab script: npc draw before: group {} render {} onscreen {} base {:.2} camo {:.2} opacity {:.2} tint {:.2} load {:?}", c.chr_flags1c4.is_render_group_enabled(), c.chr_flags1c5.enable_render(), c.chr_flags1c4.is_onscreen(), c.base_transparency, c.camouflage_transparency, c.opacity_keyframes_multiplier, c.tint_alpha_multiplier, c.load_state);
                        if rest.trim() != "off" {
                            crate::villains::make_visible(c);
                        }
                    }
                }
            }
            // chrdiag: the last npc's render-related fields next to the nearest drawn enemy's; "copy" also takes the
            // drawn enemy's block, render range, distances and omission onto the npc
            "chrdiag" => {
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance_mut() } {
                    fn info(c: &eldenring::cs::ChrIns) -> String {
                        let ph = c.modules.physics.position;
                        let bb = c.chr_model_ins.model_ins.model_item.location_aabb_exporter.aabb;
                        let m = unsafe { std::slice::from_raw_parts(c.render_group_mask.as_ptr(), 0x60) };
                        let bits: Vec<u32> = m.chunks(0x20).map(|k| k.iter().map(|b| b.count_ones()).sum()).collect();
                        let first: Vec<String> = m.chunks(0x20).map(|k| k[..8].iter().map(|b| format!("{b:02x}")).collect()).collect();
                        let raw = unsafe { std::slice::from_raw_parts((c as *const _ as *const u8).add(0x1b0), 0x20) };
                        log!("lab script: chrdiag masks bits {bits:?} first {first:?} raw1b0 {}", raw.iter().map(|b| format!("{b:02x}")).collect::<String>());
                        format!("phys ({:.1},{:.1},{:.1}) chunk ({:.1},{:.1},{:.1}) aabb ({:.1},{:.1},{:.1})-({:.1},{:.1},{:.1}) | ", ph.0, ph.1, ph.2,
                            c.chunk_position.0, c.chunk_position.1, c.chunk_position.2, bb.min.0, bb.min.1, bb.min.2, bb.max.0, bb.max.1, bb.max.2)
                        + &format!("block {:?} ovr {:?} origin {:?} ovr {:?} omis {:?}/{:?} range {:.1} deact {:.1} fade {:.1} deact_o {:.1} fade_o {:.1} act {:#x} thr {:.1} prio {:.1} load {} type {:?} backread {}",
                            c.block_id.0, c.block_id_override.0, c.block_origin.0, c.block_origin_override.0, c.omission_mode, c.omission_mode_override,
                            c.max_render_range, c.squared_deactivation_distance, c.squared_fade_out_start_distance, c.squared_deactivation_distance_override,
                            c.squared_fade_out_start_distance_override, unsafe { *(&c.chr_activation_flags as *const _ as *const u32) }, c.chr_activate_threshold, c.update_priority, c.load_state.0, c.chr_type, c.backread_state)
                    }
                    let mut donor: Option<std::ptr::NonNull<eldenring::cs::ChrIns>> = None;
                    for e in wcm.chr_inses_by_distance.iter().take(12) {
                        let c = unsafe { e.chr_ins.as_ref() };
                        if c.team_type != 0 && c.chr_flags1c4.is_render_group_enabled() && donor.is_none() {
                            donor = Some(e.chr_ins);
                            log!("lab script: chrdiag map {:.1} m: {}", e.distance, info(c));
                        }
                    }
                    if let Some(mut n) = wcm.debug_chr_creator.last_created_chr {
                        let n = unsafe { n.as_mut() };
                        log!("lab script: chrdiag npc: {}", info(n));
                        if rest.trim() == "copy" {
                            if let Some(d) = donor {
                                let d = unsafe { d.as_ref() };
                                n.block_id = d.block_id;
                                n.block_id_override = d.block_id_override;
                                n.max_render_range = d.max_render_range;
                                n.squared_deactivation_distance = d.squared_deactivation_distance;
                                n.squared_fade_out_start_distance = d.squared_fade_out_start_distance;
                                n.squared_deactivation_distance_override = d.squared_deactivation_distance_override;
                                n.squared_fade_out_start_distance_override = d.squared_fade_out_start_distance_override;
                                n.omission_mode_override = d.omission_mode_override;
                                n.backread_state = d.backread_state;
                                unsafe { std::ptr::copy_nonoverlapping(d.render_group_mask.as_ptr(), n.render_group_mask.as_mut_ptr(), 0x60) };
                                unsafe { *(&mut n.chr_activation_flags as *mut _ as *mut u32) = *(&d.chr_activation_flags as *const _ as *const u32) };
                                log!("lab script: chrdiag copied -> {}", info(n));
                            }
                        }
                    }
                }
            }
            // npcpoke <offset hex> <or hex> [<clear hex>]: one byte of the last npc's ChrIns set/cleared (diagnostics)
            "npcpoke" => {
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance_mut() } {
                    if let Some(c) = wcm.debug_chr_creator.last_created_chr {
                        let v: Vec<usize> = rest.split_whitespace().filter_map(|t| usize::from_str_radix(t.trim_start_matches("0x"), 16).ok()).collect();
                        if v.len() >= 2 && v[0] < 0x580 {
                            let b = unsafe { &mut *(c.as_ptr() as *mut u8).add(v[0]) };
                            let before = *b;
                            *b = (*b | v[1] as u8) & !(v.get(2).copied().unwrap_or(0) as u8);
                            log!("lab script: npcpoke {:#x}: {before:#04x} -> {:#04x}", v[0], *b);
                        }
                    }
                }
            }
            // chrstate: the nearest characters' load state and draw flags (what a drawn enemy has)
            "chrstate" => {
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                    for e in wcm.chr_inses_by_distance.iter().take(8) {
                        let c = unsafe { e.chr_ins.as_ref() };
                        log!("lab script: chr at {:.1} m team {} load {} group {} render {} onscreen {} hp {}", e.distance, c.team_type, c.load_state.0, c.chr_flags1c4.is_render_group_enabled(), c.chr_flags1c5.enable_render(), c.chr_flags1c4.is_onscreen(), c.modules.data.hp);
                    }
                }
            }
            // npcload <value>: the last npc's load state written
            "npcload" => {
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance_mut() } {
                    if let Some(mut c) = wcm.debug_chr_creator.last_created_chr {
                        let c = unsafe { c.as_mut() };
                        let v: u32 = rest.trim().parse().unwrap_or(0);
                        log!("lab script: npc load {} -> {v}", c.load_state.0);
                        c.load_state.0 = v;
                    }
                }
            }
            "lastnpc" => {
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                    match wcm.debug_chr_creator.last_created_chr {
                        Some(c) => {
                            let c = unsafe { c.as_ref() };
                            let np = c.modules.physics.position;
                            let pp = player.chr_ins.modules.physics.position;
                            let bp = &player.block_position;
                            let cr = &wcm.debug_chr_creator.init_data.spawn_position;
                            log!("lab script: last npc hp {}/{} team {} anim {} | npc havok ({:.1},{:.1},{:.1}) asked ({:.1},{:.1},{:.1}) | player havok ({:.1},{:.1},{:.1}) block ({:.1},{:.1},{:.1})", c.modules.data.hp, c.modules.data.max_hp, c.team_type, c.modules.time_act.anim_queue[(c.modules.time_act.read_idx as usize) % 10].anim_id, np.0, np.1, np.2, cr.0, cr.1, cr.2, pp.0, pp.1, pp.2, bp.x, bp.y, bp.z);
                        }
                        None => log!("lab script: no npc made yet"),
                    }
                }
            }
            // vilwep <villain id> <weapon row>: that villain's right-hand weapon (to see a lent weapon on a villain whose arena the
            // test save reaches: Aizen's Kyoka Suigetsu on Madara)
            "vilwep" => {
                let mut it = rest.split_whitespace();
                let (v, w) = (it.next().unwrap_or(""), it.next().and_then(|x| x.parse::<i32>().ok()).unwrap_or(110000));
                if let (Some(i), Ok(repo)) = (crate::villains::VILLAINS.iter().position(|x| x.id == v), unsafe { SoloParamRepository::instance_mut() }) {
                    if let Some(c) = repo.get_mut::<eldenring::cs::CharaInitParam>(crate::villains::villain_row(i) as u32) {
                        c.set_equip_wep_right(w);
                        log!("lab script: {v} holds weapon {w}");
                    }
                }
            }
            // buddyprobe: every spirit summon (BuddyParam) row and the summon manager's state (1.3.0, the Mimic Tear shadow clone)
            "buddyprobe" => {
                use eldenring::cs::{BuddyParam, EquipParamGoods, WorldChrMan};
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    let triggers: Vec<(u32, i32)> = repo.rows::<BuddyParam>().map(|(id, b)| (id, b.trigger_sp_effect_id())).collect();
                    for (id, b) in repo.rows::<BuddyParam>() {
                        log!("lab buddy {id}: trigger {} npc {} think {} playerinit {} anim {}", b.trigger_sp_effect_id(), b.npc_param_id(), b.npc_think_param_id(), b.npc_player_init_param_id(), b.generate_anim_id());
                    }
                    for (id, g) in repo.rows::<EquipParamGoods>() {
                        if triggers.iter().any(|(_, t)| *t > 0 && *t == g.ref_id_default()) {
                            log!("lab buddy goods {id}: ref {} type {} refcat {}", g.ref_id_default(), g.goods_type(), g.ref_category());
                        }
                    }
                }
                if let Ok(wcm) = unsafe { WorldChrMan::instance() } {
                    let m = &wcm.summon_buddy_manager;
                    log!("lab buddy mgr: request {} active {} stone {} active stone {} in range {} warn {} goods {} / {} alive {}", m.request_summon_speffect_id, m.active_summon_speffect_id, m.buddy_stone_entity_id, m.active_summmon_buddy_stone_entity_id, m.is_within_activation_range, m.is_within_warn_range, m.requested_summon_goods_id, m.active_summon_goods_id, m.player_has_alive_summon);
                }
            }
            // buddysummon <trigger speffect> <goods id>: ask the summon manager for that spirit here, as if its item had been used
            // within a Rebirth Monument's range
            "buddysummon" => {
                use eldenring::cs::WorldChrMan;
                let mut it = rest.split_whitespace();
                let sp = it.next().and_then(|x| x.parse::<i32>().ok()).unwrap_or(0);
                let goods = it.next().and_then(|x| x.parse::<i32>().ok()).unwrap_or(-1);
                if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
                    let pos = player.chr_ins.modules.physics.position;
                    let m = &mut wcm.summon_buddy_manager;
                    m.is_within_activation_range = true;
                    m.spawn_origin = pos;
                    m.request_summon_speffect_id = sp;
                    m.requested_summon_goods_id = goods;
                    log!("lab script: disappear delay was {:.1} s", m.buddy_disappear_delay_sec);
                    if rest.contains("nodelay") {
                        m.buddy_disappear_delay_sec = 1.0e7;
                    }
                    log!("lab script: summon requested (speffect {sp}, goods {goods})");
                }
            }
            "standoff" => STAND_OFF.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            "buddyoff" => {
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance_mut() } {
                    wcm.summon_buddy_manager.disappear_requested = true;
                    log!("lab script: summon dismissed");
                }
            }
            // stonerange: every Rebirth Monument's activation and return range made huge (what Nexus 5641 does in the params)
            "stonerange" => {
                use eldenring::cs::BuddyStoneParam;
                if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                    let mut n = 0;
                    for (_, r) in repo.rows_mut::<BuddyStoneParam>() {
                        r.set_activate_range(60000);
                        r.set_overwrite_return_range(30000);
                        if rest.trim() == "noregion" {
                            r.set_overwrite_activate_region_entity_id(0);
                        }
                        n += 1;
                    }
                    log!("lab script: {n} buddy stones' range made huge");
                }
            }
            "buddykeep" => BUDDY_KEEP.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            "buddyforce" => BUDDY_FORCE.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            // playerstone: the player made a Rebirth Monument: the first entry of the game's monument list (sorted by entity id;
            // the player's id is smaller than any monument's) points at the player, its row's range huge
            "playerstone" => {
                use eldenring::cs::BuddyStoneParam;
                // (the player's own entity id is 0, which a placeholder entry already has: "playerstone <entity>" names another
                // character, e.g. Torrent, always loaded in the summon set)
                let me = rest.trim().parse::<u32>().unwrap_or(player.chr_ins.event_entity_id);
                if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                    let k = repo.buddy_stone_entity_ids.iter().position(|e| e.talk_chr_entity_id != 0).unwrap_or(0);
                    let (old, idx) = {
                        let e = &mut repo.buddy_stone_entity_ids[k];
                        let old = e.talk_chr_entity_id;
                        e.talk_chr_entity_id = me;
                        (old, e.buddy_stone_param_index)
                    };
                    repo.buddy_stone_entity_ids.sort_by_key(|e| e.talk_chr_entity_id);
                    if let Some(r) = repo.get_row_by_index_mut::<BuddyStoneParam>(idx as usize) {
                        r.set_talk_chr_entity_id(me);
                        r.set_activate_range(60000);
                        r.set_overwrite_return_range(30000);
                        r.set_overwrite_activate_region_entity_id(0);
                        r.set_warn_region_entity_id(0);
                    }
                    log!("lab script: monument {old} (row index {idx}) is now the player ({me})");
                }
            }
            // clonedress <CharaInitParam row>: a spirit's equipment made the player's worn armour rows (which carry the fighter's
            // models while a fighter is active) and bare hands: the shadow clone looks like the fighter
            "clonedress" => {
                let row = rest.trim().parse::<u32>().unwrap_or(23342);
                let ids = unsafe { player.player_game_data.as_ref() }.equipment.chr_asm.equipment_param_ids;
                if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                    if let Some(c) = repo.get_mut::<eldenring::cs::CharaInitParam>(row) {
                        c.set_equip_helm(ids[12]);
                        c.set_equip_armer(ids[13]);
                        c.set_equip_gaunt(ids[14]);
                        c.set_equip_leg(ids[15]);
                        c.set_equip_wep_right(110000);
                        c.set_equip_wep_left(110000);
                        log!("lab script: CharaInit {row} dressed as the player ({:?})", &ids[12..16]);
                    }
                }
            }
            // clonelook <NpcParam row> [shader]: a spirit's phantom shader (the blue spirit glow) logged and set (default -1: none)
            "clonelook" => {
                let mut it = rest.split_whitespace();
                let row = it.next().and_then(|x| x.parse::<u32>().ok()).unwrap_or(100000020);
                let shader = it.next().and_then(|x| x.parse::<i32>().ok()).unwrap_or(-1);
                if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                    if let Some(n) = repo.get_mut::<eldenring::cs::NpcParam>(row) {
                        log!("lab script: NpcParam {row} phantom shader {} draw type {} ghost model {}", n.phantom_shader_id(), n.draw_type(), n.ghost_model_id());
                        n.set_phantom_shader_id(shader);
                    }
                }
            }
            // spiritvfx: the effects on every summoned spirit, their visual rows, and the phantom shaders those use
            "spiritvfx" => {
                use eldenring::cs::{PhantomParam, SpEffectParam, SpEffectVfxParam};
                let (Ok(wcm), Ok(repo)) = (unsafe { eldenring::cs::WorldChrMan::instance() }, unsafe { SoloParamRepository::instance() }) else { return };
                for c in wcm.summon_buddy_chr_set.characters() {
                    let ids: Vec<i32> = c.special_effect.entries().map(|e| e.param_id).collect();
                    log!("lab script: spirit npc {} effects {ids:?}", c.npc_param_id);
                    for id in ids {
                        let Some(sp) = repo.get::<SpEffectParam>(id as u32) else { continue };
                        for v in [sp.vfx_id(), sp.vfx_id1(), sp.vfx_id2(), sp.vfx_id3(), sp.vfx_id4(), sp.vfx_id5(), sp.vfx_id6(), sp.vfx_id7()].into_iter().filter(|i| *i > 0) {
                            let Some(x) = repo.get::<SpEffectVfxParam>(v as u32) else { continue };
                            log!("lab script:  sp {id} vfx {v}: phantom {}/{} midst sfx {} dmy {} init sfx {} type {} material {} camo {:.1}", x.phantom_param_overwrite_type(), x.phantom_param_overwrite_id(), x.midst_sfx_id(), x.midst_dmy_id(), x.init_sfx_id(), x.effect_type(), x.material_param_id(), x.camouflage_begin_dist());
                            let p = x.phantom_param_overwrite_id();
                            if p > 0 {
                                if let Some(ph) = repo.get::<PhantomParam>(p as u32) {
                                    log!("lab script:   phantom {p}: edge {},{},{} a{:.2} front {},{},{} a{:.2} diff {},{},{} a{:.2} spec a{:.2} light {},{},{} a{:.2} alpha {:.2} blend {:.2}/{} edgepow {:.2} glow {:.2} sub {}/{} no2 {}",
                                        ph.edge_color_r(), ph.edge_color_g(), ph.edge_color_b(), ph.edge_color_a(), ph.front_color_r(), ph.front_color_g(), ph.front_color_b(), ph.front_color_a(),
                                        ph.diff_mul_color_r(), ph.diff_mul_color_g(), ph.diff_mul_color_b(), ph.diff_mul_color_a(), ph.spec_mul_color_a(), ph.light_color_r(), ph.light_color_g(), ph.light_color_b(), ph.light_color_a(),
                                        ph.alpha(), ph.blend_rate(), ph.blend_type(), ph.edge_power(), ph.glow_scale(), ph.is_edge_subtract(), ph.is_front_subtract(), ph.is_no2_pass());
                                }
                            }
                        }
                    }
                }
            }
            // standkeep <speffect ...>: the Stand keeps these spirit effects (stands.rs), to see which look each gives
            // standafter <side back lag> <side back lag> <opacity> <glow 0/1>: the afterimages' layout
            "standafter" => {
                let v: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if v.len() >= 8 {
                    *crate::stands::AFTER.lock().unwrap() = Some(([(v[0], v[1], v[2]), (v[3], v[4], v[5])], v[6], v[7] > 0.5));
                    log!("lab script: afterimages {v:?}");
                }
            }
            "standmode" => crate::stands::MODE.store(rest.trim().parse().unwrap_or(0), std::sync::atomic::Ordering::Relaxed),
            "standkeep" => {
                *crate::stands::KEEP.lock().unwrap() = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                log!("lab script: the Stand keeps {rest}");
            }
            // artsanim <row ...>: each Ash of War's animation section (SwordArtsParam swordArtsTypeNew)
            "artsanim" => {
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    for row in rest.split_whitespace().filter_map(|x| x.parse::<u32>().ok()) {
                        if let Some(a) = repo.get::<eldenring::cs::SwordArtsParam>(row) {
                            log!("lab script: arts {row}: type_new {} type {}", a.sword_arts_type_new(), a.sword_arts_type());
                        }
                    }
                }
            }
            // buddyrows <trigger>: the BuddyParam rows a spirit item calls (several = several spirits)
            "buddyrows" => {
                let trig: i32 = rest.trim().parse().unwrap_or(259000);
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    let mut n = 0;
                    for (id, b) in repo.rows::<eldenring::cs::BuddyParam>() {
                        if n < 400 && (b.trigger_sp_effect_id() == trig || trig == 0) {
                            n += 1;
                            log!("lab script: buddy {id}: trigger {} npc {} think {} offset {:.1},{:.1} angle {:.0} init {}", b.trigger_sp_effect_id(), b.npc_param_id(), b.npc_think_param_id(), b.x_offset(), b.z_offset(), b.y_angle(), b.npc_player_init_param_id());
                        }
                    }
                }
            }
            // gparam: the scene's draw-param lerpers and the area blocks' overrides
            "gparam" => {
                if let Ok(m) = unsafe { eldenring::cs::CSWorldSceneDrawParamManager::instance() } {
                    let l: Vec<String> = m.scene_draw_param.lerpers.iter().map(|l| format!("{}>{} t{:.2}", l.begin_id, l.destination_id, l.timer)).collect();
                    log!("lab script: gparam lerpers [{}] main {}>{} t{:.2}", l.join(", "), m.scene_draw_param.lerper.begin_id, m.scene_draw_param.lerper.destination_id, m.scene_draw_param.lerper.timer);
                    for b in m.world_area_blocks.iter() {
                        log!("lab script: gparam block {}_{} override {} to {} dur {:.2}", b.area.area(), b.area.block(), b.override_gparam, b.transition_to_override, b.override_transition_duration);
                    }
                }
            }
            // gparamset <id> <secs> | gparamset off: every area block's gparam overridden (EMEVD's own way)
            "gparamset" => {
                if let Ok(m) = unsafe { eldenring::cs::CSWorldSceneDrawParamManager::instance_mut() } {
                    let mut it = rest.split_whitespace();
                    let first = it.next().unwrap_or("off");
                    let secs: f32 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0.2);
                    for b in m.world_area_blocks.iter_mut() {
                        match first.parse::<u32>() {
                            Ok(id) => { b.override_gparam = id; b.transition_to_override = true; b.override_transition_duration = secs; }
                            Err(_) => { b.transition_to_override = false; b.override_transition_duration = secs; }
                        }
                    }
                    log!("lab script: gparamset {rest}");
                }
            }
            // gparamlerp <index> <dest> <secs>: one scene lerper sent to another gparam id
            "gparamlerp" => {
                let v: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if let (Some(&i), Some(&d), Ok(m)) = (v.first(), v.get(1), unsafe { eldenring::cs::CSWorldSceneDrawParamManager::instance_mut() }) {
                    let l = if i < 0.0 { &mut m.scene_draw_param.lerper } else { &mut m.scene_draw_param.lerpers[i as usize % 15] };
                    l.begin_id = l.destination_id;
                    l.destination_id = d as i32;
                    l.timer = v.get(2).copied().unwrap_or(0.0);
                    log!("lab script: gparamlerp {rest}");
                }
            }
            // standout <secs> <anim>: the Stand comes out and plays <anim> (to try its poses)
            "standout" => {
                let mut it = rest.split_whitespace();
                let secs = it.next().and_then(|x| x.parse().ok()).unwrap_or(2.0);
                let anim = it.next().and_then(|x| x.parse().ok()).unwrap_or(crate::stands::ORA);
                *crate::stands::REQUEST.lock().unwrap() = Some((secs, anim));
            }
            // phantom <row> <edge r g b a> <front r g b a> <alpha> <edge power> <glow>: a phantom shader's colours set
            "phantom" => {
                let v: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if v.len() >= 12 {
                    if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                        if let Some(ph) = repo.get_mut::<eldenring::cs::PhantomParam>(v[0] as u32) {
                            ph.set_edge_color_r(v[1] as u8); ph.set_edge_color_g(v[2] as u8); ph.set_edge_color_b(v[3] as u8); ph.set_edge_color_a(v[4]);
                            ph.set_front_color_r(v[5] as u8); ph.set_front_color_g(v[6] as u8); ph.set_front_color_b(v[7] as u8); ph.set_front_color_a(v[8]);
                            ph.set_alpha(v[9]); ph.set_edge_power(v[10]); ph.set_glow_scale(v[11]);
                            log!("lab script: phantom {} set {:?}", v[0], &v[1..]);
                        }
                    }
                }
            }
            // cloneclean: every effect on the summoned spirits removed (to find the one that draws them blue)
            "cloneclean" => {
                use eldenring::cs::ChrInsExt;
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance_mut() } {
                    for c in wcm.summon_buddy_chr_set.characters() {
                        if c.team_type == 47 {
                            let ids: Vec<i32> = c.special_effect.entries().map(|e| e.param_id).collect();
                            for id in &ids {
                                c.remove_speffect(*id);
                            }
                            log!("lab script: removed {ids:?}");
                        }
                    }
                }
            }
            "buddyhold" => BUDDY_HOLD.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            "buddyrange" => {
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance_mut() } {
                    wcm.summon_buddy_manager.is_within_activation_range = true;
                }
            }
            "vilpath" => VILPATH.store(rest.trim() != "off", std::sync::atomic::Ordering::Relaxed),
            // 1.3.0: "bossbar <entity>": that character's boss health bar shown, as when its fight begins (a remembrance boss
            // waits for its fog gate's event, which a lab warp skips): the villains read the fight from this bar
            "bossbar" => {
                let id = rest.trim().parse::<u32>().unwrap_or(0);
                if let (Ok(fe), Ok(wcm)) = (unsafe { eldenring::cs::CSFeManImp::instance_mut() }, unsafe { eldenring::cs::WorldChrMan::instance() }) {
                    // (a big boss far out in its arena, Radahn's, isn't on the by-distance list: every character set next)
                    let found = wcm.chr_inses_by_distance.iter().map(|e| unsafe { &*e.chr_ins.as_ptr() }).find(|c| c.event_entity_id == id)
                        .or_else(|| wcm.chr_sets.iter().flatten().flat_map(|set| set.characters()).find(|c| c.event_entity_id == id).map(|c| unsafe { &*(c as *const eldenring::cs::ChrIns) }));
                    match found {
                        Some(c) => {
                            let name = unsafe { SoloParamRepository::instance() }.ok()
                                .and_then(|r| r.get::<eldenring::cs::NpcParam>(c.npc_param_id as u32).map(|n| n.name_id())).filter(|n| *n > 0).unwrap_or(1);
                            fe.boss_health_displays[0].fmg_id = name;
                            fe.boss_health_displays[0].field_ins_handle = c.field_ins_handle;
                            log!("lab script: bossbar {id}: npc {} hp {}/{} name {name}", c.npc_param_id, c.modules.data.hp, c.modules.data.max_hp);
                        }
                        None => log!("lab script: bossbar {id}: not found"),
                    }
                }
            }
            "atkprobe" => crate::villains::probe_atk(),
            // 1.3.1: "atkrow <id> ...": a player attack row's damage and guard fields
            "atkrow" => {
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    for id in rest.split_whitespace().filter_map(|t| t.parse::<u32>().ok()) {
                        match repo.get::<eldenring::cs::AtkParam_Pc>(id) {
                            Some(a) => log!("lab atkrow {id}: phys {} mag {} fire {} thun {} dark {} disable_guard {} dmg_level {}", a.atk_phys(), a.atk_mag(), a.atk_fire(), a.atk_thun(), a.atk_dark(), a.disable_guard(), a.dmg_level()),
                            None => log!("lab atkrow {id}: none"),
                        }
                    }
                }
            }
            // 1.3.0: "protrow <id> ...": armour rows' model, category and gender
            "protrow" => {
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    for id in rest.split_whitespace().filter_map(|t| t.parse::<u32>().ok()) {
                        match repo.get::<eldenring::cs::EquipParamProtector>(id) {
                            Some(r) => log!("lab protrow {id}: model {} category {} gender {} protector cat {}", r.equip_model_id(), r.equip_model_category(), r.equip_model_gender(), r.protector_category()),
                            None => log!("lab protrow {id}: none"),
                        }
                    }
                }
            }
            // 1.3.0: "npcrow <id>": an NpcParam row's behavior and animation fields
            "npcrow" => {
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    for id in rest.split_whitespace().filter_map(|t| t.parse::<u32>().ok()) {
                        if let Some(c) = repo.get::<eldenring::cs::CharaInitParam>(id) {
                            log!("lab charainit {id}: wep R {} subR {} L {} subL {} think {} str {} dex {}", c.equip_wep_right(), c.equip_subwep_right(), c.equip_wep_left(), c.equip_subwep_left(), c.npc_player_think_id(), c.base_str(), c.base_dex());
                        }
                        if let Some(n) = repo.get::<eldenring::cs::NpcParam>(id) {
                            log!("lab npcrow {id}: behavior_variation {} anim_offset {} anim_offset2 {} r1 {} l1 {} r2 {} l2 {} think? team {}", n.behavior_variation_id(), n.anim_id_offset(), n.anim_id_offset2(),
                                n.dbg_behavior_r1(), n.dbg_behavior_l1(), n.dbg_behavior_r2(), n.dbg_behavior_l2(), n.team_type());
                        }
                    }
                }
            }
            // 1.3.0: "wpnscan": every weapon row's type and motion category (finding fist weapons for the villains)
            "wpnscan" => {
                use eldenring::cs::EquipParamWeapon;
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    let mut seen = std::collections::BTreeMap::new();
                    for (id, w) in repo.rows::<EquipParamWeapon>() {
                        if id % 10000 == 0 {
                            seen.entry((w.wep_type(), w.wepmotion_category())).or_insert_with(Vec::new).push(id);
                        }
                    }
                    for ((t, m), ids) in seen {
                        log!("lab wpnscan: type {t} motion {m}: {:?}", &ids[..ids.len().min(8)]);
                    }
                }
            }
            // 1.3.0: "ownfire <entity> <bullet>": that character fires the bullet at the fighter (whose hits count: villains)
            "ownfire" => {
                let mut it = rest.split_whitespace();
                // ownfire <entity | npc:<NpcParam row>> <bullet> [metres away, default 1.2]
                let who = it.next().unwrap_or("0").to_string();
                let b = it.next().and_then(|t| t.parse::<i32>().ok()).unwrap_or(0);
                let away = it.next().and_then(|t| t.parse::<f32>().ok()).unwrap_or(1.2);
                let (by_npc, id) = match who.strip_prefix("npc:") { Some(n) => (true, n.parse::<u32>().unwrap_or(0)), None => (false, who.parse::<u32>().unwrap_or(0)) };
                let me = player.chr_ins.modules.physics.position;
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                    if let Some(c) = wcm.chr_inses_by_distance.iter().map(|e| unsafe { &*e.chr_ins.as_ptr() }).find(|c| if by_npc { c.npc_param_id as u32 == id } else { c.event_entity_id == id }) {
                        let p = c.modules.physics.position;
                        let (dx, dy, dz) = (me.0 - p.0, me.1 - p.1, me.2 - p.2);
                        let l = (dx * dx + dy * dy + dz * dz).sqrt().max(0.1);
                        let r = crate::ability::spawn(c.field_ins_handle, -1, b, (me.0 - dx / l * away, me.1 + 1.0, me.2 - dz / l * away), (dx / l, 0.0, dz / l));
                        log!("lab script: ownfire {b} by {id} (npc {} type {:?} team {}) at {l:.1} m: {r:?}", c.npc_param_id, c.chr_type, c.team_type);
                    } else {
                        log!("lab script: ownfire: {id} not found");
                    }
                }
            }
            // 1.3.0: "bossquiet [off]": the boss on the bar neither attacks nor moves (to measure its villain alone)
            "bossquiet" => {
                let on = rest.trim() != "off";
                if let (Ok(fe), Ok(wcm)) = (unsafe { eldenring::cs::CSFeManImp::instance() }, unsafe { eldenring::cs::WorldChrMan::instance_mut() }) {
                    let h = fe.boss_health_displays[0].field_ins_handle;
                    if let Some(b) = wcm.chr_ins_by_handle_mut(&h) {
                        b.debug_flags.set_disabled_secondary_actions(on);
                        b.debug_flags.set_disabled_movement(on);
                        log!("lab script: bossquiet {on} (npc {})", b.npc_param_id);
                    } else {
                        log!("lab script: bossquiet: no boss on the bar");
                    }
                }
            }
            "meter" => {
                let on = rest.trim() != "off";
                METER.store(on, std::sync::atomic::Ordering::Relaxed);
                player.chr_ins.chr_flags1c5.set_is_invincible(false);
                if let Ok(f) = unsafe { eldenring::cs::WorldChrManDbgFlags::instance_mut() } {
                    f.player_no_dead = on;
                    log!("lab script: meter dbg flags: no_hit {} no_attack {} no_damage {} all_no_dead {} auto_parry {} no_move {}", f.all_no_hit, f.all_no_attack, f.all_no_damage, f.all_no_dead, f.auto_parry, f.all_no_move);
                }
                log!("lab script: meter {on} (max hp {}, team {:?})", player.chr_ins.modules.data.max_hp, player.chr_ins.team_type);
                log!("lab script: meter player flags: dbg_no_hit {} ctrl_no_hit {} invincible {} 1c4 {:?} 1c5 {:?} 1c6 {:?}", player.chr_ins.debug_flags.disabled_hit(),
                    player.chr_ins.chr_ctrl.flags.disable_hit(), player.chr_ins.chr_flags1c5.is_invincible(), player.chr_ins.chr_flags1c4, player.chr_ins.chr_flags1c5, player.chr_ins.chr_flags1c6);
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                    for e in wcm.chr_inses_by_distance.iter().take(12) {
                        let c = unsafe { e.chr_ins.as_ref() };
                        log!("lab script: meter near {:.1} m: npc {} c{:04} team {:?} type {:?} hp {}/{} dbg_no_hit {} ctrl_no_hit {}", e.distance, c.npc_param_id, c.character_id, c.team_type, c.chr_type, c.modules.data.hp, c.modules.data.max_hp, c.debug_flags.disabled_hit(), c.chr_ctrl.flags.disable_hit());
                    }
                }
            }
            "vilmoves" => {
                *VILMOVES.lock().unwrap() = if rest.trim() == "off" { Vec::new() } else { rest.trim().split(',').map(|s| s.trim().to_string()).collect() };
                log!("lab script: vilmoves {}", rest.trim());
            }
            "vilteam" => {
                *VILTEAM.lock().unwrap() = rest.trim().parse().ok();
                log!("lab script: vilteam {}", rest.trim());
            }
            "vilpool" => {
                *VILPOOL.lock().unwrap() = rest.trim().parse().ok();
                log!("lab script: vilpool {}", rest.trim());
            }
            // villain <id or index>: summon that villain now (villains.rs)
            "villain" => {
                let t = rest.trim();
                let i = t.parse::<usize>().ok().or_else(|| crate::villains::VILLAINS.iter().position(|v| v.id == t));
                *VILLAIN.lock().unwrap() = i;
                log!("lab script: villain {t} -> {i:?}");
            }
            // menukey <input id>: as if that menu button were pressed (menu_left, menu_build, ...)
            "menukey" => crate::handle(rest.trim(), player, combat),
            // device pad|kb: the prompts for a controller or a keyboard
            "device" => crate::binds::lab_device(rest.trim() == "pad"),
            "menupage" => {
                let mut ui = crate::ui::UI.lock().unwrap();
                let page = match rest.trim() { "build" => crate::ui::Page::Build, "controls" => crate::ui::Page::Controls, "settings" => crate::ui::Page::Settings, "loadout" => crate::ui::Page::Loadout, "moves" => crate::ui::Page::Moves, "credits" => crate::ui::Page::Credits, _ => crate::ui::Page::Roster };
                match ui.menu.as_mut() {
                    Some(m) => { m.page = page; m.cursor = 0; }
                    None => ui.menu = Some(crate::ui::Menu { page, cursor: 0, at_start: false }),
                }
            }
            "spend" => crate::ui::UI.lock().unwrap().build_spend = rest.trim().parse().ok(),
            // 1.0: dumpgame <prefix>: every archive file whose dictionary path starts with <prefix>
            // (EldenRingDictionary.txt), decompressed, into work/dump_game (to read the event scripts)
            "dumpgame" => {
                use crate::ermario::{archive::Archives, dcx};
                let prefix = rest.trim().to_string();
                let dict = std::fs::read_to_string("C:/Users/asher/JumpRing/work/title/dl/EldenRingDictionary.txt").unwrap_or_default();
                match Archives::open() {
                    Ok(a) => {
                        let mut n = 0;
                        for path in dict.lines().map(str::trim).filter(|l| l.starts_with(&prefix)) {
                            if let Ok(d) = a.read(path) {
                                let d = dcx::decompress(&d).unwrap_or(d);
                                let out = format!("C:/Users/asher/JumpRing/work/dump_game{}", path.trim_end_matches(".dcx"));
                                if let Some(dir) = std::path::Path::new(&out).parent() {
                                    let _ = std::fs::create_dir_all(dir);
                                }
                                if std::fs::write(&out, &d).is_ok() {
                                    n += 1;
                                }
                            }
                        }
                        log!("lab script: dumpgame {prefix}: {n} files");
                    }
                    Err(e) => log!("lab script: dumpgame: {e}"),
                }
            }
            "titleprobe" => {
                use crate::ermario::{archive::Archives, dcx, tpf};
                match Archives::open() {
                    Ok(a) => {
                        for path in ["/menu/05_001_title_logo.gfx", "/menu/05_000_title.gfx", "/menu/hi/02_title.tpf.dcx", "/menu/05_020_titleinformation.gfx", "/menu/hi/01_common.tpf.dcx", "/menu/hi/00_solo.tpfbhd", "/menu/hi/00_solo.tpfbdt", "/menu/hi/01_title.tpf.dcx", "/menu/hi/00_title.tpf.dcx", "/menu/title.tpf.dcx", "/menu/hi/01_common.sblytbnd.dcx", "/menu/01_common.gfx", "/menu/hi/00_title.tpfbhd", "/menu/low/01_common.tpf.dcx", "/menu/hi/mapimage.tpfbhd", "/movie/00_title.bk2", "/movie/000000_title.bk2", "/menu/01_title.gfx", "/menu/02_title.gfx", "/menu/00_title.gfx", "/menu/title.gfx", "/menu/01_000_title.gfx", "/menu/hi/01_logo.tpf.dcx", "/menu/hi/00_logo.tpf.dcx"] {
                            match a.read(path) {
                                Ok(d) => {
                                    log!("lab script: titleprobe {path} {} bytes", d.len());
                                    if path.ends_with(".gfx") || path.contains("02_title") {
                                        let _ = std::fs::write(format!("C:/Users/asher/JumpRing/work/title/{}", path.rsplit('/').next().unwrap()), &d);
                                        if path.contains("02_title") {
                                            if let Ok(t) = dcx::decompress(&d) {
                                                for x in tpf::tpf_textures(&t) {
                                                    log!("lab script: titleprobe   02_title texture {} {} bytes", x.name, x.size);
                                                    let _ = std::fs::write(format!("C:/Users/asher/JumpRing/work/title/{}.dds", x.name), &t[x.off..x.off + x.size]);
                                                }
                                            }
                                        }
                                    }
                                    if path.ends_with("tpfbhd") || path.ends_with("sblytbnd.dcx") {
                                        let _ = std::fs::write(format!("C:/Users/asher/JumpRing/work/title/{}", path.rsplit('/').next().unwrap()), &d);
                                    }
                                    if path.ends_with("sblytbnd.dcx") {
                                        if let Ok(raw) = dcx::decompress(&d) {
                                            if let Ok(files) = crate::ermario::bnd4::read(&raw) {
                                                for f in files {
                                                    let n = f.name.rsplit(['\\', '/']).next().unwrap_or("x").to_string();
                                                    let _ = std::fs::create_dir_all("C:/Users/asher/JumpRing/work/title/sblyt");
                                                    let _ = std::fs::write(format!("C:/Users/asher/JumpRing/work/title/sblyt/{n}"), &f.data);
                                                }
                                            }
                                        }
                                    }
                                    if path.ends_with(".tpf.dcx") {
                                        if let Ok(t) = dcx::decompress(&d) {
                                            let tx = tpf::tpf_textures(&t);
                                            let names: Vec<String> = tx.iter().map(|x| x.name.clone()).filter(|n| { let l = n.to_lowercase(); l.contains("title") || l.contains("logo") || l.contains("back") || l.contains("bg") }).collect();
                                            log!("lab script: titleprobe   {} textures; title/logo/bg: {:?}", tx.len(), names);
                                            log!("lab script: titleprobe   all: {:?}", tx.iter().map(|x| x.name.clone()).collect::<Vec<_>>());
                                            if path.contains("/hi/") {
                                                for x in &tx {
                                                    if x.name.starts_with("SB_Title") {
                                                        let _ = std::fs::write(format!("C:/Users/asher/JumpRing/work/title/{}.dds", x.name), &t[x.off..x.off + x.size]);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => log!("lab script: titleprobe {path}: {e}"),
                            }
                        }
                    }
                    Err(e) => log!("lab script: titleprobe archives: {e}"),
                }
            }
            "grav" => player.chr_ins.modules.physics.gravity_disabled = rest.trim() == "off",
            // 1.2.0: "trip <grace entity>": travel there now (the load soak)
            "trip" => {
                if let Some(dir) = std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("JumpRing")) {
                    let _ = std::fs::write(dir.join("warp.txt"), format!("e{}", rest.trim()));
                    crate::grace::requested_trip();
                }
            }
            "gracewarp" => {
                log!("lab script: grace {:?} block {}", crate::grace::respawn_grace(), player.current_block_id);
                crate::grace::warp_back("lab", player.current_block_id, (player.block_position.x, player.block_position.y, player.block_position.z));
            }
            // 2.0.6: "kill" a lethal blow through the game (its own HP loss, as a real one; the guards then do what they do
            // with a real one); "kill0" HP set to 0 (which never starts the game's death)
            "kill" => crate::bosses::kill_player(&mut player.chr_ins),
            "kill0" => player.chr_ins.modules.data.hp = 0,
            // 2.0.7: "freeze <ms>": the frame task holds the game's thread (the watchdog's report, tested)
            "freeze" => std::thread::sleep(std::time::Duration::from_millis(rest.trim().parse().unwrap_or(30000))),
            // 2.0.6: "hurt <hp>": HP lost as from a hit (rescue.rs: a fall in a fight counts only after one)
            "hurt" => {
                let d = &mut player.chr_ins.modules.data;
                d.hp = (d.hp - rest.trim().parse::<i32>().unwrap_or(50)).max(1);
            }
            // 2.0.6: "hp1": what a lethal blow does under a "can't die" switch (the game holds HP at 1)
            "hp1" => player.chr_ins.modules.data.hp = 1,
            // 2.0.6: "deathrow <hp rate> <hp point> <endurance> <interval>": the death effect's row set (bosses::DEATH_EFFECT)
            "deathrow" => {
                let v: Vec<f32> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if v.len() >= 4 {
                    crate::bosses::lab_death_row(v[0], v[1] as i32, v[2], v[3]);
                }
            }
            // 1.3.0: "warpto <area> <block> <region> <x> <y> <z>": a trip to that spot of that map (a map's own coordinates,
            // as its .msb places things: flverport msbchr prints them)
            "warpto" => {
                let v: Vec<f32> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if v.len() >= 6 {
                    let tile = eldenring::cs::BlockId::from_parts(v[0] as u8, v[1] as u8, v[2] as u8, 0);
                    crate::grace::trip_to(tile, (v[3], v[4], v[5]));
                    log!("lab script: warpto {tile:?} ({}, {}, {})", v[3], v[4], v[5]);
                }
            }
            // 1.3.0: "echofire <bullet>": the nearest hostile fires a boss echo's bullet at the fighter (bosses.rs ECHOES)
            "echofire" => {
                let b = rest.trim().parse::<i32>().unwrap_or(0);
                let me = player.chr_ins.modules.physics.position;
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance() } {
                    if let Some(e) = wcm.chr_inses_by_distance.iter().find(|e| crate::combat::hostile(unsafe { e.chr_ins.as_ref() }.team_type) && e.distance < 40.0) {
                        let c = unsafe { e.chr_ins.as_ref() };
                        let p = c.modules.physics.position;
                        let (dx, dz) = (me.0 - p.0, me.2 - p.2);
                        let l = (dx * dx + dz * dz).sqrt().max(0.1);
                        let r = crate::ability::spawn(c.field_ins_handle, -1, b, (p.0 + dx / l * 1.5, p.1 + 1.2, p.2 + dz / l * 1.5), (dx / l, 0.0, dz / l));
                        log!("lab script: echofire {b} from {:.1} m: {r:?}", e.distance);
                    }
                }
            }
            // 1.3.0: "god": the fighter can't be hurt (long ability tests among soldiers), full health
            "god" => {
                player.chr_ins.chr_flags1c5.set_is_invincible(rest.trim() != "off");
                let d = &mut player.chr_ins.modules.data;
                d.hp = d.max_hp;
                log!("lab script: god {}", rest.trim() != "off");
            }
            // 1.3.0: "flagq <id> [...]": event flags read (the villain gate: a boss's defeat flag, its arena's waiting flag)
            // 2.0.2: "allocnow <tag>": the allocators read now (allocwatch.rs)
            "allocnow" => crate::allocwatch::sample(rest.trim()),
            // 2.0.2: "redress <entity> vil <i> | naked | rows a b c d"
            "redress" => {
                let (id, how) = rest.trim().split_once(' ').unwrap_or((rest.trim(), "naked"));
                crate::villains::lab_redress(id.parse().unwrap_or(0), how);
            }
            // 2.0.2: "vilchr": every villain slot with a flag on or a character loaded
            "vilchr" => {
                let p = player.chr_ins.modules.physics.position;
                crate::villains::slot_report((p.0, p.1, p.2));
            }
            "flagq" => {
                if let Ok(f) = unsafe { eldenring::cs::CSEventFlagMan::instance() } {
                    let v: Vec<String> = rest.split_whitespace().filter_map(|t| t.parse::<u32>().ok()).map(|id| format!("{id}={}", f.virtual_memory_flag.get_flag(id) as u8)).collect();
                    log!("lab script: flags {}", v.join(" "));
                }
            }
            // 1.0: flag <id> <0|1>: set an event flag (simulates the Burning of the Erdtree: 9001, 111)
            "flag" => {
                let mut it = rest.split_whitespace();
                if let (Some(id), Some(v)) = (it.next().and_then(|t| t.parse::<u32>().ok()), it.next()) {
                    if let Ok(f) = unsafe { eldenring::cs::CSEventFlagMan::instance_mut() } {
                        f.virtual_memory_flag.set_flag(id, v == "1");
                        log!("lab script: flag {id} = {v} (reads back {})", f.virtual_memory_flag.get_flag(id));
                    }
                }
            }
            // 1.0.1: bgmdump: every MapDefaultInfoParam row's place music (to learn where FromSoftware put
            // location music) into work/bgm_rows.tsv, and the current map
            "bgmdump" => {
                if let Ok(repo) = unsafe { eldenring::cs::SoloParamRepository::instance() } {
                    let mut out = String::from("row	bgm_place_info	env_place_info	bank
");
                    for (id, r) in repo.rows::<eldenring::cs::MapDefaultInfoParam>() {
                        out += &format!("{id}	{}	{}	{}
", r.bgm_place_info(), r.env_place_info(), r.map_additional_sound_bank_id());
                    }
                    let _ = std::fs::write("C:/Users/asher/JumpRing/work/bgm_rows.tsv", out);
                    log!("lab script: bgmdump written; here: {}", player.current_block_id);
                }
            }
            "nomusic" => NO_MUSIC.store(true, std::sync::atomic::Ordering::Relaxed),
            // 1.0: musicvol N: the game's own music volume option (0-10), to test muting it in boss fights
            "musicvol" => {
                if let Ok(gdm) = unsafe { eldenring::cs::GameDataMan::instance_mut() } {
                    let s = &mut *gdm.game_settings;
                    let before = s.music_volume;
                    s.music_volume = rest.trim().parse().unwrap_or(before);
                    log!("lab script: music volume {before} -> {}", s.music_volume);
                }
            }
            "gmprobe" => {
                if let Ok(gm) = unsafe { eldenring::cs::GameMan::instance() } {
                    let l = gm.last_load_position;
                    let b = &player.block_position;
                    log!("lab script: gmprobe last_load ({:.1}, {:.1}, {:.1}, {:.1}) block pos ({:.1}, {:.1}, {:.1})", l.0, l.1, l.2, l.3, b.x, b.y, b.z);
                    log!("lab script: gmprobe current {} move_map_target {} unk18 {} load_target {} initial_entity {} save_slot {}", player.current_block_id, gm.move_map_target, unsafe { *((&gm.move_map_target as *const eldenring::cs::BlockId).add(1)) }, gm.load_target_block_id, gm.initial_area_entity_id, gm.save_slot);
                }
            }
            "graceprobe" => {
                // which u32s in GameMan / GameDataMan / the lua imitation hold a grace's entity id
                use eldenring::cs::{BonfireWarpParam, GameDataMan, GameMan, SoloParamRepository};
                if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
                    let ids: std::collections::HashSet<u32> = (0..3000).filter_map(|i| repo.get_row_by_index::<BonfireWarpParam>(i)).map(|r| r.bonfire_entity_id()).filter(|x| *x > 0).collect();
                    let scan = |name: &str, base: usize, len: usize| {
                        for off in (0..len).step_by(4) {
                            let v = unsafe { std::ptr::read_unaligned((base + off) as *const u32) };
                            if ids.contains(&v) || (v > 1000 && ids.contains(&(v + 1000))) {
                                log!("lab script: graceprobe {name}+{off:#x} = {v}");
                            }
                        }
                    };
                    if let Ok(gm) = unsafe { GameMan::instance() } {
                        scan("GameMan", gm as *const _ as usize, 0x1000);
                    }
                    if let Ok(g) = unsafe { GameDataMan::instance() } {
                        scan("GameDataMan", g as *const _ as usize, 0x400);
                        scan("PlayerGameData", &*g.main_player_game_data as *const _ as usize, 0x1000);
                    }
                    log!("lab script: graceprobe done ({} graces)", ids.len());
                }
            }
            // away <m>: the fighter put <m> metres from the toughest nearby enemy that isn't a villain (the boss), facing away
            // from it (a villain then brought ahead with vilat stands clear of the boss: lab tests of ranged villains)
            "away" => {
                let m = rest.trim().parse::<f32>().unwrap_or(15.0);
                let p = player.chr_ins.modules.physics.position;
                let boss = unsafe { eldenring::cs::WorldChrMan::instance() }.ok().and_then(|wcm| {
                    wcm.chr_inses_by_distance
                        .iter()
                        .map(|e| unsafe { e.chr_ins.as_ref() })
                        .filter(|c| c.modules.data.hp > 0 && !crate::villains::is_villain_npc(c.npc_param_id) && c.team_type != player.chr_ins.team_type)
                        .filter(|c| { let q = c.modules.physics.position; ((q.0 - p.0).powi(2) + (q.2 - p.2).powi(2)).sqrt() < 40.0 })
                        .max_by_key(|c| c.modules.data.max_hp)
                        .map(|c| c.modules.physics.position)
                });
                if let Some(b) = boss {
                    let (dx, dz) = (p.0 - b.0, p.2 - b.2);
                    let l = (dx * dx + dz * dz).sqrt().max(0.01);
                    let (ux, uz) = (dx / l, dz / l);
                    let ph = &mut player.chr_ins.modules.physics;
                    ph.position = eldenring::position::HavokPosition(b.0 + ux * m, p.1 + 0.5, b.2 + uz * m, p.3);
                    ph.chr_proxy_pos_update_requested = true;
                    let th = (-ux).atan2(-uz);
                    ph.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
                    log!("lab script: away {m} m from the boss at ({:.1}, {:.1}, {:.1})", b.0, b.1, b.2);
                }
            }
            "pos" => {
                let p = player.chr_ins.modules.physics.position;
                log!("lab script: pos {rest} {:.2} {:.2} {:.2}", p.0, p.1, p.2);
            }
            "quit" => {
                crate::log::drain();
                log!("lab script: quit");
                std::process::exit(0);
            }
            // 2.0.1 crash hunt: "buddyinfo <row>...": a spirit's NpcParam and NpcThinkParam rows (whose AI works in any map)
            "buddyinfo" => {
                if let Ok(repo) = unsafe { eldenring::cs::SoloParamRepository::instance() } {
                    for row in rest.split_whitespace().filter_map(|t| t.parse::<u32>().ok()) {
                        match repo.get::<eldenring::cs::BuddyParam>(row) {
                            Some(b) => log!("lab script: buddy {row}: npc {} think {}", b.npc_param_id(), b.npc_think_param_id()),
                            None => log!("lab script: buddy {row}: none"),
                        }
                    }
                }
            }
            // 2.0.1 crash hunt: "wmclose": the game's window asked to close, as a player closes the game (the game's own
            // exit runs its static destructors, which "quit" (ExitProcess) skips)
            "wmclose" => {
                use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE};
                unsafe extern "system" fn each(h: windows_sys::Win32::Foundation::HWND, _: isize) -> i32 {
                    let mut pid = 0u32;
                    unsafe { GetWindowThreadProcessId(h, &mut pid) };
                    if pid == std::process::id() && unsafe { IsWindowVisible(h) } != 0 {
                        unsafe { PostMessageW(h, WM_CLOSE, 0, 0) };
                        log!("lab script: wmclose sent to window {h:?}");
                    }
                    1
                }
                crate::log::drain();
                unsafe { EnumWindows(Some(each), 0) };
            }
            _ => log!("lab script: unknown command {line}"),
        }
    }
}
