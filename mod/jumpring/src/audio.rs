//! hooks.audio: Elden Force's own sounds, all shipped in <modengine2>/jumpring (1.0: nothing is read
//! from any other game):
//! - voices.bin: the fighters' battle shouts (bank "pack:<fighter>/<kind>");
//! - audio/menu: the menu themes (looped while our menu is open);
//! - audio/music: the exploration pieces (a quiet playlist with pauses, silent in fights);
//! - audio/boss: the boss themes (one loops through a boss fight, fading in and out);
//! - audio/moment: pieces for one scene (the Burning of the Erdtree), faded in, played once.
//! Bank names "file:<path under audio>" (a file, or a folder to pick from).

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use rand::RngExt;
use rand::seq::SliceRandom;
use rodio::Source;

use crate::log;

pub enum Pick {
    Index(usize),
    Random,
}

enum Job {
    Play { bank: String, pick: Pick, volume: f32 },
    /// Loop a piece on the menu channel until StopMusic.
    Music { bank: String, volume: f32 },
    StopMusic,
    /// exploring (true) or in a fight / a load (false)
    Explore(bool),
    /// a boss fight started (true) or ended (false)
    Boss(bool),
    /// a scene's piece: faded in over `fade_in` seconds, played once
    Moment { bank: String, volume: f32, fade_in: f32 },
    /// fade the scene's piece out
    EndMoment,
}

const EXPLORE_VOLUME: f32 = 0.28;
const BOSS_VOLUME: f32 = 0.5;
const FADE_S: f32 = 3.0;

static TX: OnceLock<Sender<Job>> = OnceLock::new();

/// 1.1.0: the player's volumes from the Elden Force settings, 0..=10 (ui.rs saves them): all of our
/// music (menu, exploring, boss themes, scenes) and the fighters' voice quips.
static MUSIC_VOL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(10);
static VOICE_VOL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(10);

pub fn set_volumes(music: u8, voice: u8) {
    MUSIC_VOL.store(music.min(10), std::sync::atomic::Ordering::Relaxed);
    VOICE_VOL.store(voice.min(10), std::sync::atomic::Ordering::Relaxed);
}

/// The music volume as a gain (0..1; squared, so the low steps are really quiet).
fn music_gain() -> f32 {
    let v = MUSIC_VOL.load(std::sync::atomic::Ordering::Relaxed) as f32 / 10.0;
    v * v
}

fn voice_gain() -> f32 {
    let v = VOICE_VOL.load(std::sync::atomic::Ordering::Relaxed) as f32 / 10.0;
    v * v
}
/// 0.17.0: the fighters' battle shouts, packed in <modengine2>/jumpring/voices.bin (the free voice
/// materials' terms forbid handing them out as loose files): name ("deku/attack/01") -> OGG bytes.
static PACK: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();

/// <modengine2>/jumpring/audio.
fn own_dir() -> Option<PathBuf> {
    Some(crate::recolor::mod_dir()?.parent()?.join("jumpring").join("audio"))
}

/// voices.bin: b"EFV1", u32 count, then per entry u16 name length, name (UTF-8), u32 data length,
/// data (OGG Vorbis). Little-endian.
fn load_pack() -> HashMap<String, Vec<u8>> {
    let mut out = HashMap::new();
    let Some(path) = own_dir().and_then(|d| d.parent().map(|p| p.join("voices.bin"))) else { return out };
    let Ok(b) = std::fs::read(&path) else { return out };
    if b.len() < 8 || &b[..4] != b"EFV1" {
        log!("audio: {} is not a voice pack", path.display());
        return out;
    }
    let n = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    let mut i = 8;
    for _ in 0..n {
        if i + 2 > b.len() { break; }
        let nl = u16::from_le_bytes(b[i..i + 2].try_into().unwrap()) as usize;
        i += 2;
        if i + nl + 4 > b.len() { break; }
        let name = String::from_utf8_lossy(&b[i..i + nl]).into_owned();
        i += nl;
        let dl = u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as usize;
        i += 4;
        if i + dl > b.len() { break; }
        out.insert(name, b[i..i + dl].to_vec());
        i += dl;
    }
    out
}

/// The shout pack has lines for `group` ("deku/attack").
pub fn has_pack(group: &str) -> bool {
    let prefix = format!("{group}/");
    PACK.get().is_some_and(|p| p.keys().any(|k| k.starts_with(&prefix)))
}

struct Pcm {
    channels: u16,
    rate: u32,
    data: Vec<f32>,
}

impl Pcm {
    fn secs(&self) -> f32 {
        self.data.len() as f32 / self.channels as f32 / self.rate as f32
    }
    fn source(self) -> rodio::buffer::SamplesBuffer {
        rodio::buffer::SamplesBuffer::new(NonZero::new(self.channels).unwrap(), NonZero::new(self.rate).unwrap(), self.data)
    }
    /// 2.0.7: the track over and over from the samples already decoded (rodio's repeat_infinite kept a second copy of the
    /// whole track as it played, in 128 KB pieces: about 100 MB more per looping track; lab memprobe)
    fn looped(self) -> Looped {
        Looped { channels: NonZero::new(self.channels).unwrap(), rate: NonZero::new(self.rate).unwrap(), data: self.data, pos: 0 }
    }
}

/// A decoded track played on a loop (Pcm::looped).
struct Looped {
    channels: rodio::ChannelCount,
    rate: rodio::SampleRate,
    data: Vec<f32>,
    pos: usize,
}

impl Iterator for Looped {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.data.is_empty() {
            return None;
        }
        if self.pos >= self.data.len() {
            self.pos = 0;
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Some(v)
    }
}

impl Source for Looped {
    fn current_span_len(&self) -> Option<usize> {
        // (a span never splits a frame: up to the track's end, where the loop starts again with a whole track)
        let left = self.data.len().saturating_sub(self.pos);
        Some(if left == 0 { self.data.len() } else { left })
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.channels
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.rate
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

struct Sounds {
    own: Option<PathBuf>,
    /// per pack group: the line played last
    last_pick: HashMap<String, String>,
}

impl Sounds {
    fn load(&mut self, bank: &str, pick: &Pick) -> Result<(String, Pcm), String> {
        if let Some(group) = bank.strip_prefix("pack:") {
            let pack = PACK.get().ok_or("no voice pack")?;
            let prefix = format!("{group}/");
            let mut names: Vec<&String> = pack.keys().filter(|k| k.starts_with(&prefix)).collect();
            if names.is_empty() {
                return Err("no such lines".into());
            }
            names.sort();
            // never the same line twice in a row
            let last = self.last_pick.get(group).cloned();
            let mut i = rand::rng().random_range(0..names.len());
            if names.len() > 1 && last.as_deref() == Some(names[i].as_str()) {
                i = (i + 1 + rand::rng().random_range(0..names.len() - 1)) % names.len();
            }
            let name = names[i].clone();
            self.last_pick.insert(group.to_string(), name.clone());
            return Ok((name.clone(), decode(Cursor::new(pack[&name].clone()))?));
        }
        let rel = bank.strip_prefix("file:").ok_or("unknown bank")?;
        let path = self.own.as_ref().ok_or("audio folder not found")?.join(rel);
        let file = if path.is_dir() {
            let mut files = oggs(&path);
            if files.is_empty() {
                return Err("no sounds".into());
            }
            let i = match pick {
                Pick::Index(i) => (*i).min(files.len() - 1),
                Pick::Random => rand::rng().random_range(0..files.len()),
            };
            files.swap_remove(i)
        } else {
            path
        };
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok((name, decode(BufReader::new(File::open(&file).map_err(|e| e.to_string())?))?))
    }
}

fn oggs(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "ogg")).collect()).unwrap_or_default();
    files.sort();
    files
}

fn decode<R: std::io::Read + std::io::Seek + Send + Sync + 'static>(r: R) -> Result<Pcm, String> {
    let dec = rodio::Decoder::new(r).map_err(|e| e.to_string())?;
    let channels = dec.channels().get();
    let rate = dec.sample_rate().get();
    Ok(Pcm { channels, rate, data: dec.collect() })
}

/// The exploration playlist: every exploration piece, shuffled.
fn playlist(s: &Sounds) -> Vec<String> {
    let mut out: Vec<String> = s
        .own
        .as_ref()
        .map(|d| oggs(&d.join("music")).iter().filter_map(|p| p.file_name()).map(|n| format!("file:music/{}", n.to_string_lossy())).collect())
        .unwrap_or_default();
    out.shuffle(&mut rand::rng());
    out
}

/// A player whose volume moves toward a target at a fixed rate (fades).
struct Fader {
    player: rodio::Player,
    vol: f32,
    target: f32,
    /// volume units per second
    rate: f32,
}

impl Fader {
    fn new(mixer: &rodio::mixer::Mixer, src: impl Source<Item = f32> + Send + 'static, target: f32, fade_in: f32) -> Self {
        let player = rodio::Player::connect_new(mixer);
        player.set_volume(0.0);
        player.append(src);
        Fader { player, vol: 0.0, target, rate: target / fade_in.max(0.05) }
    }
    fn fade_to(&mut self, target: f32, secs: f32) {
        self.target = target;
        self.rate = (self.vol - target).abs().max(0.01) / secs.max(0.05);
    }
    /// Advance by dt; false once it has faded out (or run out) and can be dropped.
    fn tick(&mut self, dt: f32) -> bool {
        let step = self.rate * dt;
        self.vol = if self.vol < self.target { (self.vol + step).min(self.target) } else { (self.vol - step).max(self.target) };
        self.player.set_volume(self.vol * music_gain());
        !(self.player.empty() || (self.target <= 0.0 && self.vol <= 0.0))
    }
}

/// Start the audio thread.
pub fn start() {
    let pack = PACK.get_or_init(load_pack);
    log!("audio: voice pack: {} lines", pack.len());
    let (tx, rx) = channel::<Job>();
    if TX.set(tx).is_err() {
        return;
    }
    std::thread::spawn(move || {
        let sink = match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(s) => s,
            Err(e) => {
                log!("audio: no audio device: {e}");
                return;
            }
        };
        let mut sounds = Sounds { own: own_dir(), last_pick: HashMap::new() };
        log!("audio: ready (own sounds: {})", sounds.own.as_ref().is_some_and(|d| d.exists()));
        let mut menu: Option<(rodio::Player, f32)> = None;
        let mut boss: Option<Fader> = None;
        let mut moment: Option<Fader> = None;
        // exploration music
        let mut list: Vec<String> = Vec::new();
        let mut list_i = 0;
        let mut explore: Option<rodio::Player> = None;
        let mut exploring = false;
        let mut vol = 0.0f32;
        let mut next_at = Instant::now() + Duration::from_secs(8);
        let mut last = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Job::Play { bank, pick, volume }) if voice_gain() <= 0.0 => {
                    let _ = (bank, pick, volume);
                }
                Ok(Job::Play { bank, pick, volume }) => match sounds.load(&bank, &pick) {
                    Ok((name, pcm)) => {
                        let volume = volume * voice_gain();
                        let secs = pcm.secs();
                        sink.mixer().add(pcm.source().amplify(volume));
                        log!("audio: playing {name} ({secs:.1}s)");
                    }
                    Err(e) => log!("audio: {bank}: {e}"),
                },
                Ok(Job::Music { bank, volume }) => match sounds.load(&bank, &Pick::Random) {
                    Ok((name, pcm)) => {
                        if let Some((old, _)) = menu.take() {
                            old.stop();
                        }
                        let player = rodio::Player::connect_new(sink.mixer());
                        player.set_volume(volume * music_gain());
                        player.append(pcm.looped());
                        menu = Some((player, volume));
                        log!("audio: menu music {name}");
                    }
                    Err(e) => log!("audio: music {bank}: {e}"),
                },
                Ok(Job::StopMusic) => {
                    if let Some((old, _)) = menu.take() {
                        old.stop();
                    }
                }
                Ok(Job::Explore(on)) => exploring = on,
                Ok(Job::Boss(true)) => {
                    if let Some(b) = &mut boss {
                        b.fade_to(BOSS_VOLUME, 2.0);
                    } else {
                        match sounds.load("file:boss", &Pick::Random) {
                            Ok((name, pcm)) => {
                                boss = Some(Fader::new(sink.mixer(), pcm.looped(), BOSS_VOLUME, 2.0));
                                log!("audio: boss music {name}");
                            }
                            Err(e) => log!("audio: boss music: {e}"),
                        }
                    }
                }
                Ok(Job::Boss(false)) => {
                    if let Some(b) = &mut boss {
                        b.fade_to(0.0, 4.0);
                    }
                }
                Ok(Job::Moment { bank, volume, fade_in }) => match sounds.load(&bank, &Pick::Random) {
                    Ok((name, pcm)) => {
                        moment = Some(Fader::new(sink.mixer(), pcm.source(), volume, fade_in));
                        log!("audio: moment {name} (fading in over {fade_in}s)");
                    }
                    Err(e) => log!("audio: moment {bank}: {e}"),
                },
                Ok(Job::EndMoment) => {
                    if let Some(m) = &mut moment {
                        m.fade_to(0.0, 4.0);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }

            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f32();
            last = now;
            // 1.1.0: the settings' music volume reaches a menu theme that is already playing
            if let Some((p, v)) = &menu {
                p.set_volume(*v * music_gain());
            }
            if boss.as_mut().is_some_and(|b| !b.tick(dt)) {
                boss = None;
            }
            if moment.as_mut().is_some_and(|m| !m.tick(dt)) {
                moment = None;
            }

            // exploration music: fade toward the wanted volume, next track after a pause; quiet while
            // the menu, a boss or a scene has music of its own
            let want = if exploring && menu.is_none() && boss.is_none() && moment.is_none() { EXPLORE_VOLUME } else { 0.0 };
            let step = EXPLORE_VOLUME * dt / FADE_S;
            vol = if vol < want { (vol + step).min(want) } else { (vol - step).max(want) };
            if let Some(p) = &explore {
                p.set_volume(vol * music_gain());
                if p.empty() {
                    explore = None;
                    // Elden Ring's own ambience between tracks
                    next_at = now + Duration::from_secs(rand::rng().random_range(25..60));
                }
            } else if want > 0.0 && now >= next_at {
                if list_i >= list.len() {
                    list = playlist(&sounds);
                    list_i = 0;
                }
                if let Some(bank) = list.get(list_i).cloned() {
                    list_i += 1;
                    match sounds.load(&bank, &Pick::Index(0)) {
                        Ok((name, pcm)) => {
                            let player = rodio::Player::connect_new(sink.mixer());
                            vol = 0.0;
                            player.set_volume(0.0);
                            player.append(pcm.source());
                            explore = Some(player);
                            log!("audio: exploring to {name}");
                        }
                        Err(e) => {
                            log!("audio: explore {bank}: {e}");
                            next_at = now + Duration::from_secs(5);
                        }
                    }
                } else {
                    next_at = now + Duration::from_secs(60);
                }
            }
            if vol <= 0.0 && want <= 0.0 {
                // faded all the way out (a fight): drop the track, a new one starts when it's calm again
                if explore.take().is_some() {
                    next_at = now + Duration::from_secs(10);
                }
            }
        }
    });
}

fn send(job: Job) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(job);
    }
}

/// Queue a sound; silently does nothing when it isn't there.
pub fn play(bank: &str, pick: Pick, volume: f32) {
    send(Job::Play { bank: bank.to_string(), pick, volume });
}

/// Loop a piece as menu music (replaces any current menu music; exploration music fades out).
pub fn music(bank: &str, volume: f32) {
    send(Job::Music { bank: bank.to_string(), volume });
}

pub fn stop_music() {
    send(Job::StopMusic);
}

/// Exploring (calm) or not: the exploration playlist plays only while calm.
pub fn explore(on: bool) {
    send(Job::Explore(on));
}

/// A boss fight started or ended: a boss theme fades in (one per fight) or out.
pub fn boss(on: bool) {
    send(Job::Boss(on));
}

/// A scene's piece, faded in over `fade_in` seconds and played once.
pub fn moment(bank: &str, volume: f32, fade_in: f32) {
    send(Job::Moment { bank: bank.to_string(), volume, fade_in });
}

pub fn end_moment() {
    send(Job::EndMoment);
}
