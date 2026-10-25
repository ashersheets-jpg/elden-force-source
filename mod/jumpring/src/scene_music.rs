//! 1.0: music that follows what happens in the world.
//! - Boss fights: while a boss's health bar is up (not in Training), one of Elden Force's boss themes
//!   plays and Elden Ring's own music volume option is set to 0 (lab-measured: the game's music drops
//!   at once), then put back when the bar goes. The player's own setting is also written to
//!   %LOCALAPPDATA%\JumpRing\music_volume.txt while it's lowered, and put back at the next launch if
//!   the game closed mid-fight.
//! - The Burning of the Erdtree (common.emevd event 901/902): the game sets event flag 111 (or 112 in
//!   the other version) right before it plays the cutscenes 60540000/60540010 (60540001/60540011), and
//!   clears flag 9001 when they're over. Heart of a Hero starts the moment 111/112 turns on (user: from
//!   the second the cutscene starts, fading in over 5 s, softly) and fades out if the scene ends first.

use std::time::{Duration, Instant};

use eldenring::cs::{CSEventFlagMan, CSFeManImp, GameDataMan};
use fromsoftware_shared::FromStatic;

use crate::{audio, log};

const BURN_FLAGS: [u32; 2] = [111, 112];
const CUTSCENE_EVENT_FLAG: u32 = 9001;
const MOMENT: &str = "file:moment/elden-force-heart-of-a-hero.ogg";
const MOMENT_VOLUME: f32 = 0.45;
const MOMENT_FADE_IN_S: f32 = 5.0;
/// A boss bar must be up this long before the music starts (no flicker), and gone this long to end it.
const BOSS_ON: Duration = Duration::from_millis(800);
const BOSS_OFF: Duration = Duration::from_millis(2500);

#[derive(Default)]
pub struct SceneMusic {
    restored: bool,
    boss_on: bool,
    changed_at: Option<Instant>,
    saved_volume: Option<u8>,
    burn_seen: Option<bool>,
    moment_live: bool,
    /// when this stay in the world began (flag changes count only after it settles: a save that
    /// loads with the Erdtree already burnt must not look like the moment it burns)
    in_world_since: Option<Instant>,
}

fn volume_file() -> Option<std::path::PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("JumpRing").join("music_volume.txt"))
}

fn set_music_volume(v: u8) -> Option<u8> {
    let gdm = unsafe { GameDataMan::instance_mut() }.ok()?;
    let s = &mut *gdm.game_settings;
    let before = s.music_volume;
    s.music_volume = v;
    Some(before)
}

impl SceneMusic {
    /// Every frame in the world. `training`: the fighter menu's Training switch (no boss music then).
    pub fn tick(&mut self, training: bool) {
        let now = Instant::now();
        if !self.restored {
            self.restored = true;
            // the game closed during a boss fight last time: the player's own music volume back
            if let Some(path) = volume_file() {
                if let Some(v) = std::fs::read_to_string(&path).ok().and_then(|t| t.trim().parse::<u8>().ok()) {
                    if set_music_volume(v).is_some() {
                        log!("scene_music: music volume put back to {v} (the game closed mid-fight)");
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }

        // boss fights
        let bar = !training
            && (unsafe { CSFeManImp::instance() }.is_ok_and(|fe| fe.boss_health_displays.iter().any(|b| b.fmg_id > 0 && !b.field_ins_handle.is_empty()))
                // (1.3.0: the boss fallen, its villain still fighting: the fight goes on)
                || crate::villains::FIGHTING.load(std::sync::atomic::Ordering::Relaxed));
        if bar != self.boss_on {
            let since = *self.changed_at.get_or_insert(now);
            if now.duration_since(since) >= if bar { BOSS_ON } else { BOSS_OFF } {
                self.boss_on = bar;
                self.changed_at = None;
                if bar {
                    self.saved_volume = set_music_volume(0);
                    if let (Some(v), Some(path)) = (self.saved_volume, volume_file()) {
                        let _ = std::fs::write(path, v.to_string());
                    }
                    audio::boss(true);
                    log!("scene_music: boss fight: boss theme on, game music {:?} -> 0", self.saved_volume);
                } else {
                    audio::boss(false);
                    self.end_fight();
                }
            }
        } else {
            self.changed_at = None;
        }

        // the Burning of the Erdtree
        let settled = now.duration_since(*self.in_world_since.get_or_insert(now)) >= Duration::from_secs(3);
        if let Ok(flags) = unsafe { CSEventFlagMan::instance() } {
            let burning = BURN_FLAGS.iter().any(|&f| flags.virtual_memory_flag.get_flag(f));
            if settled && self.burn_seen == Some(false) && burning {
                audio::moment(MOMENT, MOMENT_VOLUME, MOMENT_FADE_IN_S);
                self.moment_live = true;
                log!("scene_music: the Erdtree burns: Heart of a Hero");
            }
            self.burn_seen = Some(burning);
            if self.moment_live && burning && !flags.virtual_memory_flag.get_flag(CUTSCENE_EVENT_FLAG) {
                // the scene is over (or was skipped) before the piece: let it go
                audio::end_moment();
                self.moment_live = false;
            }
        }
    }

    fn end_fight(&mut self) {
        if let Some(v) = self.saved_volume.take() {
            set_music_volume(v);
            if let Some(path) = volume_file() {
                let _ = std::fs::remove_file(path);
            }
            log!("scene_music: boss fight over: game music back to {v}");
        }
    }

    /// Leaving the world (a load, the title): no boss music, the player's volume back. The flag
    /// history starts over (a loaded save with the Erdtree already burnt never plays the piece).
    pub fn reset(&mut self) {
        if self.boss_on {
            audio::boss(false);
            self.boss_on = false;
        }
        self.changed_at = None;
        self.end_fight();
        self.burn_seen = None;
        self.in_world_since = None;
    }
}
