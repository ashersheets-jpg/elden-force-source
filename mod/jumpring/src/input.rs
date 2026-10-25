//! The movement stick, turned into a direction in the world through the camera (side-dashes go where
//! the stick points, not where the fighter faces).

use eldenring::cs::{CSCamExt, CSCamera, UserInputKey};
use eldenring::fd4::FD4PadManager;
use fromsoftware_shared::FromStatic;

use crate::ability::V3;

/// Stick (or WASD) as (right, forward), each -1..1. 0.10.0: read from the controller and keyboard
/// themselves (binds.rs hooks), the game's own pad state as a last resort: in 0.9.0 the game's analog
/// values could read zero, and air dashes lost their direction.
pub fn stick() -> (f32, f32) {
    use std::sync::atomic::Ordering;
    let (x, y) = (crate::binds::STICK_X.load(Ordering::Relaxed) as f32 / 32767.0, crate::binds::STICK_Y.load(Ordering::Relaxed) as f32 / 32767.0);
    if x * x + y * y > 0.06 {
        return (x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0));
    }
    let k = crate::binds::WASD.load(Ordering::Relaxed);
    if k != 0 {
        let b = |bit: u8| if k & bit != 0 { 1.0 } else { 0.0 };
        return (b(8) - b(2), b(1) - b(4));
    }
    let Some(pad) = (unsafe { FD4PadManager::instance() }).ok().and_then(|m| m.get_in_game_pad()) else { return (0.0, 0.0) };
    let x = pad.poll_analog_input(UserInputKey::MoveRight).abs() - pad.poll_analog_input(UserInputKey::MoveLeft).abs();
    let y = pad.poll_analog_input(UserInputKey::MoveForwards).abs() - pad.poll_analog_input(UserInputKey::MoveBackwards).abs();
    (x, y)
}

/// Camera forward and right on the ground plane.
pub fn camera_axes() -> Option<(V3, V3)> {
    let cam = unsafe { CSCamera::instance() }.ok()?;
    let f = cam.pers_cam_1.forward();
    let r = cam.pers_cam_1.right();
    let fl = (f.0 * f.0 + f.2 * f.2).sqrt();
    let rl = (r.0 * r.0 + r.2 * r.2).sqrt();
    if fl < 1e-3 || rl < 1e-3 {
        return None;
    }
    Some(((f.0 / fl, 0.0, f.2 / fl), (r.0 / rl, 0.0, r.2 / rl)))
}

/// Where the stick points in the world (unit, ground plane), or None when it is centred.
pub fn stick_world() -> Option<V3> {
    let (x, y) = stick();
    if x * x + y * y < 0.09 {
        return None;
    }
    let (f, r) = camera_axes()?;
    let (dx, dz) = (f.0 * y + r.0 * x, f.2 * y + r.2 * x);
    let len = (dx * dx + dz * dz).sqrt();
    (len > 1e-3).then(|| (dx / len, 0.0, dz / len))
}

/// Where `p` (havok) sits on screen, as (x, y) in -1..1 from the centre (|x|, |y| <= 1: in frame), and
/// its distance in front of the camera; None behind it.
pub fn on_screen(p: V3) -> Option<(f32, f32, f32)> {
    let cam = unsafe { CSCamera::instance() }.ok()?;
    let c = &cam.pers_cam_1;
    let (o, f, r, u) = (c.position(), c.forward(), c.right(), c.up());
    let d = (p.0 - o.0, p.1 - o.1, p.2 - o.2);
    let z = d.0 * f.0 + d.1 * f.1 + d.2 * f.2;
    if z < 0.5 || !(0.2..3.0).contains(&c.fov) {
        return None;
    }
    let ty = (c.fov * 0.5).tan();
    let tx = ty * c.aspect_ratio.clamp(1.0, 4.0);
    let x = (d.0 * r.0 + d.1 * r.1 + d.2 * r.2) / z / tx;
    let y = (d.0 * u.0 + d.1 * u.1 + d.2 * u.2) / z / ty;
    Some((x, y, z))
}
