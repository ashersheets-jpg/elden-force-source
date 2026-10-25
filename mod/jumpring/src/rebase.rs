//! 0.15.0: a position the mod writes can land after Elden Ring re-bases its physics coordinates.
//!
//! The game shifts every havok coordinate by a few dozen metres now and then as the player crosses the
//! map (the swing log calls it "a coordinate shift", e.g. (24, -32, 8)). The mod's own moves (swings,
//! flights, dashes, the fighter-change hold) put the body at a havok position every frame. When the game
//! re-based between our write and its physics step, our numbers were read in the new frame: the body
//! landed that shift away, up to 32 m inside the ground or under a floor. That is a fighter vanishing
//! mid-swing (and the void falls that followed). Such a write is seen the next frame: the havok <-> block
//! offset jumped while the body's numbers stayed exactly where we wrote them (in a real re-base the
//! body's numbers move with the offset). The body is then moved by the shift, to where it was meant to be.

use eldenring::cs::{BlockId, PlayerIns};
use eldenring::position::HavokPosition;

use crate::log;

type V3 = (f32, f32, f32);

#[derive(Default)]
pub struct Rebase {
    /// body (havok) and offset when this frame began
    start: Option<(V3, V3, BlockId)>,
    /// body, offset and block at the end of last frame, and whether the mod moved the body in it
    end: Option<(V3, V3, BlockId, bool)>,
}

fn read(player: &PlayerIns) -> (V3, V3, BlockId) {
    let h = player.chr_ins.modules.physics.position;
    let b = &player.block_position;
    ((h.0, h.1, h.2), (h.0 - b.x, h.1 - b.y, h.2 - b.z), player.current_block_id)
}

/// 1.2.0: a real re-base moves the havok origin by whole map cells: every logged one was a multiple of
/// 8 m on each axis ((24, 0, -32), (88, 8, -40)...). In the tutorial's Cave of Knowledge the block
/// position flickers by odd amounts instead ((0.1, -10.5, 0), then (0.7, 20.5, -0.2) and back): taken
/// for re-bases, they "put right" a body that was where it belonged, 10-20 m down through the floor
/// (the user's clip: Goku fell out of the world after a landing and was sent to the grace).
pub fn on_grid(shift: V3) -> bool {
    let near = |v: f32| (v - (v / 8.0).round() * 8.0).abs() < 0.25;
    near(shift.0) && near(shift.1) && near(shift.2)
}

fn dist(a: V3, b: V3) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
}

impl Rebase {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// First thing every frame in the world.
    pub fn begin(&mut self, player: &mut PlayerIns) {
        let (h, off, block) = read(player);
        if let Some((put, off0, block0, wrote)) = self.end.take() {
            let shift = (off.0 - off0.0, off.1 - off0.1, off.2 - off0.2);
            let jump = dist(shift, (0.0, 0.0, 0.0));
            if wrote && block == block0 && jump > 6.0 && jump < 200.0 && dist(h, put) < 1.5 && on_grid(shift) {
                let to = (put.0 + shift.0, put.1 + shift.1, put.2 + shift.2);
                log!("rebase: a position written as the coordinates shifted by ({:.1}, {:.1}, {:.1}) landed off; put right", shift.0, shift.1, shift.2);
                let ph = &mut player.chr_ins.modules.physics;
                ph.position = HavokPosition(to.0, to.1, to.2, ph.position.3);
                ph.chr_proxy_pos_update_requested = true;
                self.start = Some((to, off, block));
                return;
            }
        }
        self.start = Some((h, off, block));
    }

    /// Last thing every frame in the world (after every move of ours).
    pub fn end(&mut self, player: &PlayerIns) {
        let (h, off, block) = read(player);
        let wrote = self.start.is_some_and(|(h0, _, _)| dist(h, h0) > 0.0005);
        self.end = Some((h, off, block, wrote));
    }
}

/// 0.16.0: the drawn model and the body. Elden Ring eases the model toward the body's position; after
/// one of our moves puts the body somewhere (a swing step, a re-base put right), the model could trail
/// metres behind for a moment: the fighter "vanished" for under a second mid-swing (user, 0.16.0 test).
/// While our flight or swing moves the body, a model more than MODEL_SNAP_M from it is put on it.
const MODEL_SNAP_M: f32 = 2.5;

pub fn model_guard(player: &mut PlayerIns, flown: bool) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST_LOG: AtomicU64 = AtomicU64::new(0);
    let ctrl = &mut *player.chr_ins.chr_ctrl;
    let (m, p) = (ctrl.model_matrix.3, ctrl.physics_model_matrix.3);
    let d = dist((m.0, m.1, m.2), (p.0, p.1, p.2));
    if d <= MODEL_SNAP_M || d > 500.0 {
        return;
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.as_millis() as u64).unwrap_or(0);
    if now.saturating_sub(LAST_LOG.load(Ordering::Relaxed)) > 500 {
        LAST_LOG.store(now, Ordering::Relaxed);
        log!("rebase: model {d:.1} m from the body{}", if flown { "; put on it" } else { "" });
    }
    if flown {
        ctrl.model_matrix.3 = fromsoftware_shared::F32Vector4(p.0, p.1, p.2, m.3);
    }
}

