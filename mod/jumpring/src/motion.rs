//! hooks.player_move: moving the player ourselves (ability dashes, leaps, dives, air dashes). Every
//! frame moves the player by the step since the last frame, from where they are now: Havok coordinates
//! are re-based when the player crosses into another map block (the moment a place name appears), so a
//! motion kept as absolute start/end points would jump across the map there.

use std::time::Instant;

use eldenring::cs::{CSHavokMan, PlayerIns};
use eldenring::position::{HavokPosition, PositionDelta};
use fromsoftware_shared::FromStatic;

use crate::ability::V3;

/// Havok filter for map collision (from the CS2 field note).
pub const MAP_FILTER: u32 = 0x2000058;

/// 0.14.2: the collision layers the mod's own moves must not pass through. MAP_FILTER alone misses the
/// floors and walls of castle interiors and building pieces (lab probe in Stormveil: a lower floor seen
/// only by 0x2000066 / 0x2000069), so flights, swings and slams went straight through them: the body
/// half sunk into a floor, then gone under it with the camera stuck above.
/// (not 0x2000000: it also hits things that aren't solid, near a Site of Grace it ended a double jump
/// on its first frame)
pub const SOLID_FILTERS: [u32; 3] = [MAP_FILTER, 0x2000066, 0x2000069];

/// The nearest hit along a ray on any solid layer.
pub fn ray(player: &PlayerIns, from: &HavokPosition, d: PositionDelta) -> Option<HavokPosition> {
    let h = unsafe { CSHavokMan::instance() }.ok()?;
    let mut best: Option<(f32, HavokPosition)> = None;
    for f in SOLID_FILTERS {
        if let Some(hit) = h.phys_world.cast_ray(f, from, PositionDelta(d.0, d.1, d.2), player) {
            let dist = (hit.0 - from.0).powi(2) + (hit.1 - from.1).powi(2) + (hit.2 - from.2).powi(2);
            if best.as_ref().is_none_or(|b| dist < b.0) {
                best = Some((dist, hit));
            }
        }
    }
    best.map(|b| b.1)
}

pub struct Motion {
    /// whole displacement (x, y, z)
    total: V3,
    /// extra height at the middle (a leap)
    arc: f32,
    dur: f32,
    start: Instant,
    /// progress already applied (0..1) and arc height already applied
    done: f32,
    arc_done: f32,
    /// keep the feet on the ground under the path (dashes); off for anything airborne
    snap: bool,
    /// ease: fast start, soft stop (dashes) or even speed (dives)
    ease_out: bool,
    /// hit a wall
    blocked: bool,
}

impl Motion {
    /// Along the ground: `total` horizontal, feet kept on the ground.
    pub fn dash(total: V3, dur: f32) -> Self {
        Self::new((total.0, 0.0, total.2), 0.0, dur, true, true)
    }

    /// Through the air: displacement including height, with an optional arc on top.
    pub fn air(total: V3, arc: f32, dur: f32) -> Self {
        Self::new(total, arc, dur, false, true)
    }

    /// Straight at constant speed (dives).
    pub fn straight(total: V3, dur: f32) -> Self {
        Self::new(total, 0.0, dur, false, false)
    }

    fn new(total: V3, arc: f32, dur: f32, snap: bool, ease_out: bool) -> Self {
        Self { total, arc, dur: dur.max(0.01), start: Instant::now(), done: 0.0, arc_done: 0.0, snap, ease_out, blocked: false }
    }

    pub fn airborne(&self) -> bool {
        !self.snap
    }

    /// Move one frame. Returns true when the motion is over.
    pub fn step(&mut self, player: &mut PlayerIns) -> bool {
        let t = (self.start.elapsed().as_secs_f32() / self.dur).min(1.0);
        let p = if self.ease_out { 1.0 - (1.0 - t) * (1.0 - t) } else { t };
        let arc_now = 4.0 * self.arc * t * (1.0 - t);
        let dp = p - self.done;
        let (mut dx, mut dy, mut dz) = (self.total.0 * dp, self.total.1 * dp, self.total.2 * dp);
        dy += arc_now - self.arc_done;
        self.done = p;
        self.arc_done = arc_now;

        let havok = unsafe { CSHavokMan::instance() }.ok();
        let pos = player.chr_ins.modules.physics.position;
        // Stop at walls (feet, chest and head, across the body's width).
        if !self.blocked && wall_ahead(player, &pos, dx, dy, dz) {
            self.blocked = true;
        }
        if self.blocked {
            dx = 0.0;
            dz = 0.0;
        }
        let (nx, mut ny, nz) = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
        if let Some(h) = &havok {
            // Feet on the ground (dashes); in the air, never go below the ground under us. The ray starts
            // above both where we were and where we are going: a fast step (a dive, a dash up a slope or
            // into a rock) can put the new spot inside the ground, and a ray started in there finds no
            // surface, which once dropped fighters through the map (0.7.0: "Deku went invisible").
            let top = pos.1.max(ny) + 1.2;
            let depth = top - ny + if self.snap { 2.5 } else { 0.1 };
            let down = ray(player, &HavokPosition(nx, top, nz, 0.0), PositionDelta(0.0, -depth, 0.0));
            if let Some(g) = down {
                if self.snap || ny < g.1 {
                    ny = g.1;
                }
            }
            // Never into a ceiling.
            if dy > 0.0 && ray(player, &HavokPosition(pos.0, pos.1 + 1.6, pos.2, 0.0), PositionDelta(0.0, dy + 0.4, 0.0)).is_some() {
                ny = pos.1;
            }
            // Nothing solid under the new spot at all (we are inside or under the map): stay put.
            if ground_below(player, nx, ny + 0.5, nz, 200.0).is_none() && ground_below(player, pos.0, pos.1 + 0.5, pos.2, 200.0).is_some() {
                self.blocked = true;
                return t >= 1.0;
            }
        }
        let phys = &mut player.chr_ins.modules.physics;
        phys.position = HavokPosition(nx, ny, nz, phys.position.3);
        phys.chr_proxy_pos_update_requested = true;
        t >= 1.0
    }
}

/// Would moving the body by (dx, dy, dz) from `pos` go into a wall? Rays along the step (a body's width
/// further) from the feet, the chest and the head, at the body's middle and both its sides, so a low
/// ledge, an overhang or a corner met at an angle stops the body too. 0.8.0 checked the chest only, and a
/// jump plus an air dash into a wall could carry the fighter through it.
pub fn wall_ahead(player: &PlayerIns, pos: &HavokPosition, dx: f32, dy: f32, dz: f32) -> bool {
    let Ok(h) = (unsafe { CSHavokMan::instance() }) else { return false };
    let len = (dx * dx + dz * dz).sqrt();
    if len < 1e-4 {
        return false;
    }
    let (ux, uz) = (dx / len, dz / len);
    let reach = len + 0.45;
    // a rising step tilts the rays up with it (a slanted overhang above a jump)
    let rise = if dy > 0.0 { dy / len * reach } else { 0.0 };
    for height in [0.35, 1.0, 1.65] {
        for side in [-0.28f32, 0.0, 0.28] {
            let (ox, oz) = (pos.0 - uz * side, pos.2 + ux * side);
            let from = HavokPosition(ox, pos.1 + height, oz, 0.0);
            if let Some(hit) = ray(player, &from, PositionDelta(ux * reach, rise, uz * reach)) {
                // 0.10.0: the feet ray also meets rising ground; a slope the body can walk up (ground just
                // past the hit no higher than knee height) is no wall. 0.9.0 stopped dashes on slopes there.
                if height < 0.5 {
                    let top = ground_below(player, hit.0 + ux * 0.25, pos.1 + 1.4, hit.2 + uz * 0.25, 2.0);
                    if top.is_some_and(|g| g < pos.1 + 0.6) {
                        continue;
                    }
                }
                return true;
            }
        }
    }
    false
}

/// The ground height under a point, if there is ground within `depth` below `from`.
pub fn ground_below(player: &PlayerIns, x: f32, y: f32, z: f32, depth: f32) -> Option<f32> {
    let h = unsafe { CSHavokMan::instance() }.ok()?;
    ray(player, &HavokPosition(x, y + 1.0, z, 0.0), PositionDelta(0.0, -(depth + 1.0), 0.0)).map(|g| g.1)
}
