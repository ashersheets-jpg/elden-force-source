//! hooks.params: set each ability's damage in memory (abilities.dmg_mult x attack rating, shared
//! by every bullet its script fires).
//! The bullet row's AtkParam_Pc row (and its hit/interval child bullets) gets the per-hit value in the
//! ability's element. Rows are vanilla rows shared with Elden Ring's own spells, so the values are
//! rewritten whenever the fighter or the player's level changes. Nothing is written to disk.

use eldenring::cs::{AtkParam_Pc, Bullet, SoloParamRepository};
use fromsoftware_shared::FromStatic;

use crate::log;
use crate::sheets::{Ability, Slot};

/// Every ability's damage is scaled by this (0.7.0: 0.7, the moves made the game too easy).
pub const ABILITY_DAMAGE: f32 = 0.7;

/// Attack rating of a JUMP FORCE ability at a given Elden Ring level.
/// Share of a move's planned hits that delivers its full damage.
const HIT_SHARE: f32 = 0.6;

/// 1.3.0 (overnight balance: a level-1 Bakugo took Flying Dragon Agheel to a quarter of its health): 60 + 6 a
/// level (was 100 + 5): level 54 where the user approved it (269, was 259), level 1 at 46 (was 73.5), so
/// under-levelled fights are properly hard and the late game is unchanged.
pub fn attack_rating(level: u32) -> f32 {
    (60.0 + level as f32 * 6.0) * ABILITY_DAMAGE
}

fn set_atk(repo: &mut SoloParamRepository, atk_id: i32, element: &str, value: u16) -> bool {
    if atk_id <= 0 {
        return false;
    }
    let Some(atk) = repo.get_mut::<AtkParam_Pc>(atk_id as u32) else { return false };
    // (1.3.0: lightning for Sasuke's and Killua's moves)
    let (phys, mag, fire, thun) = match element {
        "physical" => (value, 0, 0, 0),
        "fire" => (0, 0, value, 0),
        "lightning" => (0, 0, 0, value),
        _ => (0, value, 0, 0), // magic and frost
    };
    atk.set_atk_phys(phys);
    atk.set_atk_mag(mag);
    atk.set_atk_fire(fire);
    atk.set_atk_thun(thun);
    atk.set_atk_dark(0);
    true
}

/// 0.15.0: the three skills (L3 + A / B / X) hit 15% harder, the Awakenings (L3 + Y) 20% softer.
pub const SKILL_BOOST: f32 = 1.15;
pub const ULTIMATE_BOOST: f32 = 0.8;

/// The whole move's damage to one enemy at player `level` (abilities.dmg_mult x attack rating): the
/// number the fighter select shows.
pub fn total(a: &Ability, level: u32) -> f32 {
    let boost = if a.slot == Slot::Ultimate { ULTIMATE_BOOST } else { SKILL_BOOST };
    attack_rating(level) * a.dmg_mult * boost
}

/// Write damage for every bullet of `a`'s script at player `level`: the move's total split over the
/// hits that land on one enemy (`hits`). Returns the per-hit value.
pub fn apply_bullets(a: &Ability, bullets: &[i32], hits: f32, level: u32) -> u16 {
    // generous per hit: the move reaches its damage when most of its hits land; the ledger (ledger.rs)
    // gives back anything past it
    let per_hit = (total(a, level) / (hits * HIT_SHARE).max(1.0)).round().clamp(1.0, 9999.0) as u16;
    for &b in bullets {
        write_bullet(a.id, a.element, b, per_hit);
        unguarded(b);
    }
    per_hit
}

/// 1.3.1 (the user: Goku's Meteor Combination "hits nothing but Margit"; lab: Gatefront's shield soldiers blocked every blow of it,
/// bosses never guard): an ability's hits go through a raised shield (the combo blow shares STRIKE_HIT's row: the combos set it
/// back their own way, set_strike)
fn unguarded(bullet: i32) {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    let mut queue = vec![bullet];
    let mut done = 0;
    while let Some(id) = queue.pop() {
        if id <= 0 || done > 6 {
            continue;
        }
        done += 1;
        let Some((atk, hit, interval)) = repo.get::<Bullet>(id as u32).map(|b| (b.atk_id_bullet(), b.hit_bullet_id(), b.interval_create_bullet_id())) else { continue };
        if atk > 0 {
            if let Some(a) = repo.get_mut::<AtkParam_Pc>(atk as u32) {
                a.set_disable_guard(true);
            }
        }
        queue.push(hit);
        queue.push(interval);
    }
}

/// 0.16.0: a move is over (or has dealt all it may to a boss): its bullets hurt no more. Lingering
/// ones (Black Flame Ritual, ghostflame, Destined Death) kept burning at the move's per-hit damage
/// after the ledger stopped counting: Asta's Black Meteorite one-shot bosses that way.
pub fn mute(bullets: &[i32], element: &str) {
    for &b in bullets {
        write_bullet("", element, b, 1);
    }
}

fn write_bullet(id_for_log: &str, element: &str, bullet: i32, per_hit: u16) -> bool {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
    // The bullet and up to two generations of children (hit / interval bullets).
    let mut queue = vec![bullet];
    let mut done = 0;
    let mut written = 0;
    while let Some(id) = queue.pop() {
        if id <= 0 || done > 6 {
            continue;
        }
        done += 1;
        let Some(b) = repo.get::<Bullet>(id as u32) else { continue };
        let (atk, hit, interval) = (b.atk_id_bullet(), b.hit_bullet_id(), b.interval_create_bullet_id());
        if set_atk(repo, atk, element, per_hit) {
            written += 1;
        }
        queue.push(hit);
        queue.push(interval);
    }
    if written == 0 {
        if !id_for_log.is_empty() {
            log!("damage {id_for_log}: bullet {bullet} has no AtkParam_Pc row");
        }
        return false;
    }
    true
}

/// Flat damage on one bullet (and its children) for moves that aren't abilities (the slam).
pub fn set_bullet(bullet: i32, element: &'static str, value: u16) {
    write_bullet("slam", element, bullet, value);
}

/// 1.2.0 round 4: the combo blow (scripts::STRIKE_HIT): physical damage, the enemy's own hit reaction
/// (AtkParam dmgLevel: combos.rs DMG_*) and the poise it breaks.
pub fn set_strike(bullet: i32, value: u16, dmg_level: u8, poise: f32, unblockable: bool) {
    if !write_bullet("strike", "physical", bullet, value) {
        return;
    }
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    let Some(atk) = repo.get::<Bullet>(bullet as u32).map(|b| b.atk_id_bullet()) else { return };
    if let Some(a) = repo.get_mut::<AtkParam_Pc>(atk.max(0) as u32) {
        a.set_dmg_level(dmg_level);
        a.set_atk_super_armor(poise);
        // 1.2.0 round 5: the launch and the air blows go through a raised shield (a guarding soldier played
        // its guard instead of the airborne reaction)
        a.set_disable_guard(unblockable);
    }
}
