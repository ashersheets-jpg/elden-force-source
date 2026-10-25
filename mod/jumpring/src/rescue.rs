//! Falling out of the world. 0.15.0 rewrite (the player's wish: jumping off the Divine Tower bridge
//! kills again; being dropped out of the world by a fighter change or a load never does).
//! - A fall into the void (airborne, nothing at all under them, a real drop) is the player's own doing
//!   when they went over an edge: they die, as in Elden Ring (the mod's behaviour script turned the
//!   game's lethal-fall death off, so the mod ends it itself once the fall is clearly bottomless). So
//!   does a drop of more than LETHAL_DROP onto ground (off the map's cliffs and bridges); ordinary falls
//!   stay harmless (0.12.0).
//! - The same fall right after a load or a fighter change, or under the map (solid ground overhead: they
//!   went through a floor), is not their doing: back to their Site of Grace (grace.rs), runes kept.
//! - Stuck inside rock (a body inside solid ground for a while): back to their grace too.
//!
//! 0.14.x popped bodies "out on top" and back to the last ground instead. Where the floor was one the
//! mod's rays can't see, that looped (a minute and more under a floor, out of sight), and void falls
//! never killed at all.

use std::time::{Duration, Instant};

use eldenring::cs::PlayerIns;
use eldenring::position::{HavokPosition, PositionDelta};

use crate::log;
use crate::motion;

/// A load counts as "just now" for this long (the game may still be placing the player).
const AFTER_LOAD: Duration = Duration::from_secs(20);
/// A drop this long ends a fighter where it lands (a swing's highest release is about 25 m up).
const LETHAL_DROP: f32 = 60.0;

#[derive(Default)]
pub struct Rescue {
    /// falling over the void since, and the drop then
    void_since: Option<(Instant, f32)>,
    /// metres dropped in this fall (summed frame by frame: the game re-bases its coordinates by tens
    /// of metres mid-fall, which a start-minus-now height would take for a climb or a plunge)
    drop: f32,
    last_y: Option<f32>,
    /// inside solid ground since
    inside_since: Option<Instant>,
    /// when the player appeared (after any load)
    since: Option<Instant>,
    rock_check: Option<Instant>,
    /// a warp back to the grace was asked for (once per load)
    warped: bool,
    /// the fall was judged the player's own: they die
    dying: bool,
}

impl Rescue {
    pub fn reset(&mut self) {
        if self.warped {
            crate::movement::set_no_dead(crate::movement::NO_DEAD_RESCUE, false);
        }
        *self = Self::default();
    }

    /// Kept for the callers: the guard never holds the player anymore.
    pub fn holding(&self) -> bool {
        false
    }

    /// A void fall now isn't the player's fault (just loaded): the fall guard keeps them alive.
    pub fn protected(&self) -> bool {
        self.since.is_none_or(|t| t.elapsed() < AFTER_LOAD)
    }

    /// Every frame in world. `look_holding`: a fighter change is holding them itself (look.rs).
    /// `after_change`: a fighter change's reload happened in this load.
    pub fn tick(&mut self, player: &mut PlayerIns, look_holding: bool) {
        let now = Instant::now();
        let since = *self.since.get_or_insert(now);
        if player.chr_ins.modules.data.hp <= 0 || look_holding || self.warped || now.duration_since(since) < Duration::from_secs(3) {
            self.void_since = None;
            self.inside_since = None;
            return;
        }
        let just_loaded = now.duration_since(since) < AFTER_LOAD;
        let ph = &player.chr_ins.modules.physics;
        let pos = ph.position;
        let flown = crate::movement::BODY_FLOWN.load(std::sync::atomic::Ordering::Relaxed);

        // inside solid ground (checked 4 times a second): for 2 s in a row, back to the grace
        if self.rock_check.is_none_or(|t| now.duration_since(t) > Duration::from_millis(250)) {
            self.rock_check = Some(now);
            let off_ground = !ph.is_touching_ground || flown;
            let inside = (off_ground && crate::movement::inside_solid(player).is_some()) || (!flown && crate::movement::inside_rock_standing(player).is_some());
            if !inside {
                self.inside_since = None;
            } else if self.inside_since.get_or_insert(now).elapsed() > Duration::from_secs(2) {
                self.back_to_grace(player, "stuck inside solid ground");
                return;
            }
        }

        let ph = &player.chr_ins.modules.physics;
        let falling = ph.is_falling && !ph.is_touching_ground && !flown;
        let landed = ph.is_touching_ground && !flown;
        // the drop of this fall (our own flights don't count: it starts when they end)
        if falling {
            if let Some(ly) = self.last_y {
                let dy = ly - pos.1;
                if dy.abs() < 5.0 {
                    self.drop += dy.max(0.0);
                }
            }
            self.last_y = Some(pos.1);
        } else {
            if landed && self.drop > LETHAL_DROP && !just_loaded && !self.dying {
                self.die(player, &format!("a {:.0} m fall", self.drop));
                return;
            }
            if landed || flown {
                self.drop = 0.0;
                self.last_y = None;
            }
        }
        let airborne = (ph.is_falling && !ph.is_touching_ground) || flown;
        let void = airborne && motion::ground_below(player, pos.0, pos.1 + 0.5, pos.2, 300.0).is_none();
        if !void {
            self.void_since = None;
            return;
        }
        let (t0, d0) = *self.void_since.get_or_insert((now, self.drop));
        let fell = self.drop - d0;
        // (under the map Elden Ring crashes about 2 s into a fall: everything here is decided well before)
        if now.duration_since(t0) < Duration::from_millis(400) || fell < 5.0 {
            return;
        }
        // through a floor: something solid over them, near enough to have stood on
        let overhead = motion::ray(player, &HavokPosition(pos.0, pos.1 + 1.0, pos.2, 0.0), PositionDelta(0.0, 500.0, 0.0)).is_some();
        if just_loaded || overhead {
            let why = if just_loaded { "fell into the void right after a load" } else { "fell through the floor out of the world" };
            self.back_to_grace(player, why);
            return;
        }
        // their own fall over an edge into nothing: it ends them (as in Elden Ring)
        if !self.dying {
            self.die(player, &format!("a fall into the void ({fell:.0} m and nothing below)"));
        }
    }

    fn die(&mut self, player: &mut PlayerIns, what: &str) {
        self.dying = true;
        log!("rescue: {what}: a fall to their death");
        crate::movement::allow_death();
        player.chr_ins.chr_flags1c5.set_is_invincible(false);
        player.chr_ins.modules.data.hp = 0;
    }

    fn back_to_grace(&mut self, player: &mut PlayerIns, why: &str) {
        if crate::grace::warp_back(why, player.current_block_id, (player.block_position.x, player.block_position.y, player.block_position.z)) {
            self.warped = true;
            // held still and safe until the warp's load takes them
            player.chr_ins.modules.physics.gravity_disabled = true;
            player.chr_ins.chr_flags1c5.set_is_invincible(true);
            player.chr_ins.modules.fall.fall_timer = 0.0;
            crate::movement::set_no_dead(crate::movement::NO_DEAD_RESCUE, true);
        } else {
            // no grace known at all: the game's own way back (death, at its last grace)
            self.warped = true;
            self.die(player, why);
        }
    }
}
