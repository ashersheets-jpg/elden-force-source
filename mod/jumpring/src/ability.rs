//! hooks.bullet_spawn: fire ability bullets so the engine does the hits.

use eldenring::cs::{BulletSpawnData, CSBulletManager, FieldInsHandle, PlayerIns};
use fromsoftware_shared::{F32Vector4, FromStatic};

/// Same layout as `eldenring::cs::BulletSpawnData` (its fields are private there).
#[repr(C)]
struct SpawnData {
    owner: FieldInsHandle,
    behavior_id: i32,
    magic_id: i32,
    unk10: u32,
    bullet_id: i32,
    goods_id: i32,
    dummy_poly_id: i32,
    target: [u8; 8],
    unk28: u32,
    unk2c: u32,
    unk30: F32Vector4,
    unk40: u32,
    unk44: u32,
    pad48: [u8; 8],
    acceleration_angle: F32Vector4,
    unk60: F32Vector4,
    angle: F32Vector4,
    position: F32Vector4,
    unk90: u64,
    unk98: u64,
    unka0: u64,
    pada8: [u8; 8],
    unkb0_struct: [u8; 0x50],
    unk100: u8,
    pad101: [u8; 15],
}
const _: () = assert!(size_of::<SpawnData>() == size_of::<BulletSpawnData>());

pub type V3 = (f32, f32, f32);

/// BulletSpawnData +0x44 (see spawn_ex); the lab can change it to compare.
pub static SPAWN_FLAGS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// EquipParamGoods row of the Throwing Dagger. Verified in game: flat damage applies with it.
const THROWN_GOODS: i32 = 1700;

/// Facing of the player as a unit vector on the ground plane, from the physics orientation.
pub fn forward(player: &PlayerIns) -> V3 {
    let q = player.chr_ins.modules.physics.orientation;
    let (x, y, z, w) = (q.0, q.1, q.2, q.3);
    // Elden Ring characters face -Z of their model matrix: rotate (0, 0, -1) by q.
    // Verified in game by walking straight: movement matched -(row 2) of physics_model_matrix.
    let fx = -2.0 * (x * z + w * y);
    let fz = -(1.0 - 2.0 * (x * x + y * y));
    let len = (fx * fx + fz * fz).sqrt().max(1e-6);
    (fx / len, 0.0, fz / len)
}

/// Turn the player to face `dir` (ground plane) at once.
pub fn face(player: &mut PlayerIns, dir: V3) {
    let l = (dir.0 * dir.0 + dir.2 * dir.2).sqrt();
    if l < 1e-3 {
        return;
    }
    let th = (-dir.0 / l).atan2(-dir.2 / l);
    let phys = &mut player.chr_ins.modules.physics;
    phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
    phys.interpolated_orientation = phys.orientation;
}

pub fn position(player: &PlayerIns) -> V3 {
    let p = player.chr_ins.modules.physics.position;
    (p.0, p.1, p.2)
}

/// Spawn one bullet row owned by the player at `pos` heading along `dir`.
pub fn fire_at(player: &PlayerIns, bullet_id: i32, pos: V3, dir: V3) -> Result<(), i32> {
    // Attributed to a thrown item (Throwing Dagger) so the engine uses the flat AtkParam_Pc values we set;
    // as a sorcery (goods -1, magic -1) damage comes only from a catalyst and floors at ~2.
    spawn(player.chr_ins.field_ins_handle, THROWN_GOODS, bullet_id, pos, dir)
}

/// Spawn one bullet row owned by any character (bosses' extra attacks use AtkParam_Npc).
pub fn spawn(owner: FieldInsHandle, goods_id: i32, bullet_id: i32, pos: V3, dir: V3) -> Result<(), i32> {
    spawn_ex(owner, goods_id, -1, -1, bullet_id, pos, dir)
}

pub fn spawn_ex(owner: FieldInsHandle, goods_id: i32, magic_id: i32, behavior_id: i32, bullet_id: i32, pos: V3, dir: V3) -> Result<(), i32> {
    let Ok(bullets) = (unsafe { CSBulletManager::instance_mut() }) else {
        return Err(-100);
    };
    let data = SpawnData {
        owner,
        behavior_id,
        magic_id,
        unk10: 0,
        bullet_id,
        goods_id,
        dummy_poly_id: -1,
        target: [0xFF; 8],
        unk28: 0,
        unk2c: 0,
        unk30: F32Vector4(0.0, 0.0, 0.0, 0.0),
        unk40: 0,
        // 0.11.0: "single-shot emitter": the game releases the spawn's emitter entry once its bullet is
        // out. Left at 0, every bullet kept one entry of a 256-slot pool forever, and after ~250 bullets
        // (a long fight) every ability failed to spawn until the area reloaded.
        unk44: SPAWN_FLAGS.load(std::sync::atomic::Ordering::Relaxed),
        pad48: [0; 8],
        acceleration_angle: F32Vector4(dir.0, dir.1, dir.2, 0.0),
        unk60: F32Vector4(0.0, 0.0, 0.0, 0.0),
        angle: F32Vector4(dir.0, dir.1, dir.2, 0.0),
        position: F32Vector4(pos.0, pos.1, pos.2, 1.0),
        unk90: 0,
        unk98: 0,
        unka0: 0,
        pada8: [0; 8],
        unkb0_struct: [0; 0x50],
        unk100: 0,
        pad101: [0; 15],
    };
    bullets.spawn_bullet(unsafe { &*(&data as *const SpawnData as *const BulletSpawnData) })
}

/// Elden Ring keeps at most 128 bullets alive at once, shared by everyone. In busy places (the Academy
/// of Raya Lucaria's sorcerers fill the air with glintstone) a new spawn then fails and an ability plays
/// without its effects (0.7.0). Before firing, when the pool is nearly full, end the oldest of the mod's
/// own bullets (rows the scripts use) that have been out for a while. Returns how many were ended.
pub fn make_room(ours: &[i32]) -> usize {
    let Ok(m) = (unsafe { CSBulletManager::instance_mut() }) else { return 0 };
    if (m.bullets().count() as u32) < POOL_BUSY {
        return 0;
    }
    let mut old: Vec<(f32, *mut eldenring::cs::CSBulletIns)> = m
        .bullets_mut()
        .filter(|b| b.time_alive > 0.5 && b.life > 0.0 && ours.contains(&b.fly_state.base.param.param_id))
        .map(|b| (b.time_alive, b as *mut _))
        .collect();
    old.sort_by(|a, b| b.0.total_cmp(&a.0));
    let n = old.len().min(12);
    for (_, b) in old.into_iter().take(n) {
        unsafe { (*b).life = 0.0 };
    }
    n
}

/// Bullets alive in the shared pool.
pub fn pool_count() -> u32 {
    unsafe { CSBulletManager::instance() }.map(|m| m.bullets().count() as u32).unwrap_or(0)
}

const POOL_BUSY: u32 = 110;

/// The bullet manager's three pools (bullets, bullet effects, emitters) as "alive/allocated(+heap)",
/// read raw from its BufferAndAllocLinkedList headers: logged when a spawn fails, so a player's log
/// says which one ran out.
pub fn pool_stats() -> String {
    let Ok(m) = (unsafe { CSBulletManager::instance() }) else { return "-".into() };
    let base = m as *const CSBulletManager as *const u8;
    let mut out = String::new();
    for (name, off) in [("bullets", 0x00usize), ("effects", 0x20), ("emitters", 0x40)] {
        let (head, empty, alloc, total) = unsafe {
            let p = base.add(off);
            (*(p.add(0x08) as *const usize), *(p.add(0x10) as *const usize), *(p.add(0x18) as *const u32), *(p.add(0x1C) as *const u32))
        };
        out.push_str(&format!("{name} heap {alloc} made {total} free-slot {} list {} | ", empty != 0, head != 0));
    }
    let raw = |o: usize| unsafe { *(base.add(o) as *const u32) };
    out.push_str(&format!("counts sfx {}/{} bullets {}/{}", raw(0x1D0), raw(0x1D4), raw(0x1D8), raw(0x1DC)));
    out
}
