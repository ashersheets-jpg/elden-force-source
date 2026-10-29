//! JUMP FORCE Ã— Elden Ring. Loaded by ModEngine2 (external_dlls). Offline only; Melty starts the game.

mod ability;
mod chars;
mod perf;
mod anim;
mod anims;
mod appearance;
mod aura;
mod binds;
mod bosses;
mod combat;
mod damage;
mod ermario;
mod glyphs;
mod grace;
mod rebase;
mod combos;
mod juggle;
mod title;
mod builds;
mod hits;
mod input;
mod audio;
mod scene_music;
mod menu_pages;
mod ledger;
mod training;
mod questpath;
mod motion;
mod movelists;
mod villains;
mod clones;
mod stands;
mod credits;
mod timestop;
mod movement;
mod scripts;
#[cfg(feature = "lab")]
mod lab;
mod look;
mod log;
#[cfg(feature = "lab")]
mod oracle;
mod outfit;
mod overlay;
mod parry;
mod weapon;
mod world_fx;
mod profiles;
mod prepare;
mod regulation;
pub use prepare::prepare_main;
mod survey;
pub use survey::{ccw_dump, ccw_dump_many, ccw_reg, ccw_survey, dcx_file};
mod recolor;
mod steam;
mod watchdog;
mod rescue;
mod save;
#[cfg(feature = "lab")]
mod heapcheck;
#[cfg(feature = "lab")]
mod labheap;
#[cfg(feature = "lab")]
mod luahook;
mod veh;
#[cfg(feature = "lab")]
mod d3ddebug;
#[cfg(feature = "lab")]
mod steamoverlay_lab;
#[cfg(feature = "lab")]
mod allocwatch;
#[cfg(feature = "lab")]
mod exitwatch;
mod portraits;
mod select_screen;
mod sheets;
mod splash;
mod ui;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use eldenring::{
    cs::{CSLuaEventManImp, CSTaskGroupIndex, CSTaskImp, PlayerIns, WorldChrMan},
    fd4::FD4TaskData,
};
use fromsoftware_shared::{FromStatic, SharedTaskImpExt};
use sheets::When;
use ui::{Menu, Page, UI};

/// # Safety
/// Called by the loader. Do not call this yourself.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn DllMain(hmodule: usize, reason: u32) -> bool {
    if reason != 1 {
        return true;
    }
    watchdog::note_main_thread();
    #[cfg(feature = "lab")]
    steamoverlay_lab::install();
    steam::install();
    recolor::start_early();
    // (lab, 2.0.3: JR_EARLY_MAP=1 asks the library for the task runner at once, the earliest a player's PC could: does it
    // give up, as in the players' "CSTask never appeared" logs?)
    #[cfg(feature = "lab")]
    if std::env::var("JR_EARLY_MAP").is_ok() {
        std::thread::spawn(|| {
            // (the library's wait asked as early as a player's PC might: before the game has made any singleton)
            let r = CSTaskImp::wait_for_instance(Duration::MAX).map(|_| ());
            log::write_now(&format!("lab: early wait for the task runner: {r:?}"));
        });
    }
    std::thread::spawn(move || boot(hmodule));
    true
}

/// 2.0.3 (Melty players' logs: "boot: CSTask never appeared" within a second of starting, then nothing of the mod ran, while
/// every lab boot found it): the library finds the game's singletons by name once, and keeps that list for good, dropping any
/// singleton the game hasn't named yet (from_singleton::map: it settles as soon as any singleton exists). Started early
/// enough on a player's PC, the task runner wasn't in it and never could be. So the list is first built privately, again and
/// again, until the task runner and the character manager have their names; only then does the library build its own.
fn singletons_named() {
    let t0 = Instant::now();
    if let Err(e) = eldenring::util::system::wait_for_system_init(&fromsoftware_shared::Program::current(), Duration::from_secs(120)) {
        log!("boot: the game's start not seen ({e:?})");
        return;
    }
    let mut tries = 0;
    while t0.elapsed() < Duration::from_secs(30) {
        tries += 1;
        let pe = unsafe { pelite::pe::PeView::module(windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) as *const u8) };
        // (the library's two lists, as from_singleton::map joins them: the FD4 singletons by their reflection names, and the
        // derived ones, named from the code)
        let partial = from_singleton::find::fd4_singletons(pe);
        let derived = from_singleton::find::derived_singletons(pe);
        if !partial.all_null() || !derived.all_null() {
            // SAFETY: the game has started (its HINSTANCE global is set) and some singleton exists: its reflection is being
            // filled in; names not given yet read as null and are left out (find.rs finish)
            let mut named: std::collections::HashSet<String> = unsafe { partial.finish() }.keys().cloned().collect();
            named.extend(derived.keys().cloned());
            // (by the names the library looks them up by: the task runner's is "CSTask")
            let want = [<CSTaskImp as from_singleton::FromSingleton>::name(), <eldenring::cs::WorldChrMan as from_singleton::FromSingleton>::name()];
            if want.iter().all(|n| named.contains(n.as_ref())) {
                log!("boot: the game's singletons named ({} after {tries} looks, {:.2} s)", named.len(), t0.elapsed().as_secs_f32());
                // (lab, the 2.0.7 crash hunt: every singleton's name and its global slot's address in the game, to name the
                // globals players' crash addresses read through: logs/singletons.txt)
                #[cfg(feature = "lab")]
                {
                    let base = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) } as usize;
                    let mut rows: Vec<(usize, String)> = from_singleton::find::derived_singletons(pe).iter().map(|(k, v)| (v.as_ptr() as usize - base, k.clone())).collect();
                    rows.extend(unsafe { from_singleton::find::fd4_singletons(pe).finish() }.iter().map(|(k, v)| (v.as_ptr() as usize - base, k.clone())));
                    rows.sort();
                    let text: String = rows.iter().map(|(r, k)| format!("{r:#x} {k}\n")).collect();
                    let _ = std::fs::write(log::dir().join("logs").join("singletons.txt"), text);
                }
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    log!("boot: the game's singletons still unnamed after {tries} looks");
    // (lab: which of the two the lists do have, to see why)
    #[cfg(feature = "lab")]
    {
        let pe = unsafe { pelite::pe::PeView::module(windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) as *const u8) };
        let d = from_singleton::find::derived_singletons(pe);
        let f = unsafe { from_singleton::find::fd4_singletons(pe).finish() };
        log!("boot: derived {:?}", d.keys().filter(|k| k.contains("Task") || k.contains("WorldChr")).collect::<Vec<_>>());
        log!("boot: fd4 {:?}", f.keys().filter(|k| k.contains("Task") || k.contains("WorldChr")).collect::<Vec<_>>());
    }
}

/// hooks.boot: never touch game singletons from DllMain's thread; wait for the task runner.
fn boot(hmodule: usize) {
    log::init();
    // A panic aborts the game (panic = abort): write what and where to the log first.
    std::panic::set_hook(Box::new(|info| log::write_now(&format!("PANIC: {info}"))));
    // (lab, the DX12 crash hunt: d3d12debug.on: the debug layer and DRED, before the game makes its device)
    #[cfg(feature = "lab")]
    d3ddebug::enable();
    save::redirect();
    questpath::load();
    log!("JumpRing {} loading ({} fighters, {} abilities)", env!("CARGO_PKG_VERSION"), sheets::FIGHTERS.len(), sheets::ABILITIES.len());
    ui::load_profile();
    // (1.3.1 round 2: first, before the game makes its swap chain where it can: hudhook then finds the game's command queue
    // as it is created, not by searching the swap chain afterwards; it needs DirectX only, none of the game's singletons)
    overlay::install(hmodule);
    #[cfg(feature = "lab")]
    heapcheck::start();
    #[cfg(feature = "lab")]
    labheap::start();
    #[cfg(feature = "lab")]
    luahook::start();
    // (lab, 2.0.7 crash hunt: the graphics global players' crash at er+0x1a98b26 found null (er+0x47f6400, made by
    // CSGraphicsImp's setup, cleared by its destructor): when it is set and cleared)
    #[cfg(feature = "lab")]
    std::thread::spawn(|| {
        let base = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) } as usize;
        let slot = (base + 0x47f6400) as *const usize;
        let t0 = Instant::now();
        let mut last = usize::MAX;
        loop {
            let v = unsafe { slot.read_volatile() };
            if v != last {
                let heap = if v != 0 { unsafe { ((v + 0x5b8) as *const usize).read_volatile() } } else { 0 };
                log::write_now(&format!("lab gfxglobal: er+0x47f6400 = {v:#x} (heap at +0x5b8 {heap:#x}) {:.1} s after load", t0.elapsed().as_secs_f32()));
                last = v;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    #[cfg(feature = "lab")]
    if log::dir().join("olddeath.on").exists() {
        bosses::OLD_DEATH.store(true, Ordering::Relaxed);
        log!("lab: olddeath.on: 2.0.5's handling of the player's death");
    }
    veh::start();
    #[cfg(feature = "lab")]
    allocwatch::start();
    #[cfg(feature = "lab")]
    exitwatch::start();
    audio::start();

    singletons_named();
    let cs_task = match CSTaskImp::wait_for_instance(Duration::MAX) {
        Ok(t) => t,
        Err(e) => {
            log!("boot: CSTask never appeared ({e:?})");
            return;
        }
    };
    log!("boot: CSTask ready");
    binds::install();

    let mut last_report = Instant::now() - Duration::from_secs(10);
    let mut had_player = false;
    // 1.1.3: when the player last appeared after a load, until the character has settled (see below)
    let mut settling_since: Option<Instant> = None;
    let mut menu_shield = false;
    // 1.0: boss themes, the Burning of the Erdtree (scene_music.rs); the menu's first opening this session
    let mut scene = scene_music::SceneMusic::default();
    let mut menu_opened = false;
    let mut rows_written = false;
    let mut ccw_params = false;
    let mut recolor_shown = false;
    let mut recolor_started = false;
    let mut applied_profile: Option<&'static str> = None;
    let mut hp_fraction: Option<(f32, i32, Instant)> = None;
    let (start_fighter, start_loadout) = {
        let s = UI.lock().unwrap();
        (s.fighter, s.loadout)
    };
    let mut combat = combat::Combat::new(start_fighter);
    combat.loadout = start_loadout;
    let mut binds = binds::Bindings::default();
    #[cfg(feature = "lab")]
    let mut oracle = oracle::HitOracle::default();
    #[cfg(feature = "lab")]
    let mut lab = lab::Lab::default();
    let mut look = look::Look::default();
    let mut movement = movement::Movement::default();
    let mut bosses = bosses::Bosses::default();
    // 1.3.0: the anime villains who join boss fights (villains.rs)
    let mut villains = villains::Villains::default();
    let mut clones = clones::Clones::default();
    let mut stands = stands::Stands::default();
    let mut rescue = rescue::Rescue::default();
    let mut rebase = rebase::Rebase::default();
    let mut builds = builds::Builds::default();
    let mut hits = hits::Hits::default();
    let mut training = training::Training::default();
    #[cfg(feature = "lab")]
    let mut warp_check: Option<Instant> = None;
    let mut exploring: Option<bool> = None;
    let mut calm_since: Option<Instant> = None;
    let mut last_calm_check = Instant::now();

    cs_task.run_recurring(
        move |_: &FD4TaskData| {
            watchdog::beat();
            let _frame = perf::span("frame");
            // 2.0.3 cinematic mode: Home hides our HUD (overlay::CINEMATIC)
            if eldenring::util::input::is_key_pressed(0x24) {
                let on = !overlay::CINEMATIC.load(Ordering::Relaxed);
                overlay::CINEMATIC.store(on, Ordering::Relaxed);
                log!("cinematic mode {}", if on { "on" } else { "off" });
            }
            #[cfg(feature = "lab")]
            if !had_player {
                lab::script_boot();
            }
            #[cfg(feature = "lab")]
            {
                // 0.15.2 probe: the menu's UI element states (which say "title screen" vs "loading")
                static LAST: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
                if let Ok(m) = unsafe { eldenring::cs::CSMenuManImp::instance() } {
                    let hud = game_hud_state();
                    let now: String = format!("hud{hud} ") + &m.ui_states.iter().enumerate().filter(|(_, u)| u.created() || u.visible()).map(|(i, u)| format!("{i}{}", if u.visible() { "v" } else { "c" })).collect::<Vec<_>>().join(",");
                    let mut l = LAST.lock().unwrap();
                    if *l != now {
                        log!("lab ui: player {} states [{now}]", unsafe { WorldChrMan::instance() }.ok().is_some_and(|w| w.main_player.is_some()));
                        *l = now;
                    }
                }
            }
            let Ok(wcm) = (unsafe { WorldChrMan::instance_mut() }) else { return };
            // 1.3.1 lab (the game's 2.7.2.0 patch moved structures): JR_LAYOUT dumps the raw memory our code reads
            #[cfg(feature = "lab")]
            if std::env::var_os("JR_LAYOUT").is_some() {
                lab_layout_dump(wcm);
                return;
            }
            #[cfg(feature = "lab")]
            oracle.tick(wcm, combat.last_cast);
            if wcm.main_player.is_none() {
                // Loading: write the current fighter's look into the player's data so the character is
                // built with it (no reload needed after the load).
                if !recolor_started {
                    recolor_started = true;
                    // made before the game loaded: the look written below already uses the recoloured models
                    recolor_shown = recolor::READY.load(Ordering::Relaxed);
                }
                if let Ok(gdm) = unsafe { eldenring::cs::GameDataMan::instance_mut() } {
                    let pgd = &mut *gdm.main_player_game_data;
                    if outfit::Outfits::worn(pgd).iter().all(|id| *id > 0) {
                        look.write(pgd, combat.fighter);
                    }
                }
                villains.loading();
                if had_player {
                    log!("player: gone (loading or title)");
                    had_player = false;
                    settling_since = None;
                    let mut s = UI.lock().unwrap();
                    s.hud.in_world = false;
                    s.menu = None;
                    s.fx.wind = 0.0;
                    s.fx.slow_until = None;
                    s.fx.time_stop = None;
                    binds::BLOCK_ALL.store(false, Ordering::Relaxed);
                    applied_profile = None;
                    look.on_unload();
                    combat.on_unload();
                    movement.reset();
                    movement::end_death();
                    bosses.reset();
                    villains.reset();
                    clones.reset();
                    stands.reset();
                    timestop::reset();
                    rescue.reset();
                    rebase.reset();
                    builds.on_unload();
                    grace::forget();
                    hits.reset();
                    world_fx::clear();
                    audio::explore(false);
                    scene.reset();
                    exploring = None;
                }
                return;
            }
            if !had_player {
                log!("player: in world");
                #[cfg(feature = "lab")]
                lab::world_entered();
                had_player = true;
                settling_since = Some(Instant::now());
                #[cfg(feature = "lab")]
                {
                    warp_check = Some(Instant::now() + Duration::from_secs(3));
                }
                // Fighter select every time the player loads in (not after our own reload in place).
                let mut s = UI.lock().unwrap();
                if std::mem::take(&mut look.reloaded_by_us) {
                    drop(s);
                } else {
                let cursor = sheets::FIGHTERS.iter().position(|f| f.id == s.fighter).unwrap_or(0);
                s.menu = Some(Menu { page: Page::Roster, cursor, at_start: true });
                }
            }
            #[cfg(feature = "lab")]
            lab_keys(wcm, &mut oracle, &mut combat);
            // 0.14.3: the character camera (its death mode is undone below)
            let chr_cam = wcm.chr_cam;
            let Some(player) = wcm.main_player.as_mut() else { return };

            // 1.1.3: every crash players sent from 1.1.0 to 1.1.2 came during a load or within seconds
            // after one, while the game was still building the character (its load state still 0) and
            // ours was already changing its stats, attributes, size and outfit. Nothing of ours touches
            // the character until the game has marked it loaded and a moment has passed (8 s at most),
            // except a fighter change's hold at the old spot (without it the player falls under the map).
            // The fighter select opens at once as before; it answers once the character has settled.
            if let Some(since) = settling_since {
                let t = since.elapsed();
                let load_state = player.chr_ins.load_state.0;
                if (load_state != 0 && t >= Duration::from_millis(1000)) || t >= Duration::from_secs(8) {
                    log!("player: settled after {:.1}s (load state {load_state:#x})", t.as_secs_f32());
                    settling_since = None;
                } else {
                    if look.busy() {
                        look.tick(player, combat.fighter, combat.scale);
                    }
                    // (1.3.1 round 2, the user: "it waits a second before reading any inputs": on 2.7.2 the character settles in
                    // 3-4 s, under 2 before) the fighter select answers meanwhile: moving about it touches nothing of the
                    // character, and a pick made now is made the moment the character has settled
                    if UI.lock().unwrap().menu.is_some() {
                        binds::set_menu_open(true);
                        for id in binds.poll(When::Menu) {
                            match id {
                                "menu_confirm" => PICK_WAITING.store(true, Ordering::Relaxed),
                                "menu_up" | "menu_down" | "menu_left" | "menu_right" | "menu_difficulty" | "menu_training" | "menu_warnings" => {
                                    handle(id, player, &mut combat)
                                }
                                _ => {}
                            }
                        }
                    }
                    return;
                }
            }

            // (a fighter change's hold moves the body itself while the offset settles)
            if look.holding() {
                rebase.reset();
            } else {
                rebase.begin(player);
            }
            let grace_flag = unsafe { CSLuaEventManImp::instance() }
                .map(|m| m.lua_event_proxy.control_flags.bonfire_sitting_loop_active())
                .unwrap_or(false);
            // 0.15.2: the game's "sitting at a grace" flag can stay on after the player has left (a fighter
            // change made at a grace reloads the area mid-rest): the JUMP FORCE prompt stayed up and the
            // ability list stayed hidden while they ran about. Resting also needs the body in a grace
            // animation (a000_068xxx: sit, rest loop, menus, stand up) within the last second and a half.
            let at_grace = {
                static SEEN: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
                let mut seen = SEEN.lock().unwrap();
                if (68000..69000).contains(&movement::base_anim(movement::current_anim(player))) {
                    *seen = Some(Instant::now());
                }
                grace_flag && seen.is_some_and(|t| t.elapsed() < Duration::from_millis(1500))
            };
            grace::tick(at_grace, unsafe { player.player_game_data.as_ref() });
            {
                let sheet = if combat.fighter.id == "custom" { builds::Sheet::Tarnished } else { builds::Sheet::Fighters };
                let pgd = unsafe { player.player_game_data.as_mut() };
                {
                    let _p = perf::span("builds");
                    builds.tick(pgd, sheet);
                }
                let mut s = UI.lock().unwrap();
                if let Some(i) = s.build_spend.take() {
                    if builds.spend(pgd, sheet, i) {
                        log!("builds: a point on {}", builds::NAMES[i]);
                    }
                }
                s.build = ui::BuildView { tarnished: sheet == builds::Sheet::Tarnished, attrs: builds.view(sheet), free: builds.free(pgd.level, sheet), level: pgd.level };
            }
            let menu_open = UI.lock().unwrap().menu.is_some();
            binds::set_menu_open(menu_open);
            // Nothing can hurt the player while our menu is open (it blocks their input).
            if menu_open != menu_shield {
                player.chr_ins.chr_flags1c5.set_is_invincible(menu_open);
                menu_shield = menu_open;
                // 1.0: our own menu themes: the Main Theme on the session's first fighter select, Choose
                // Your Fighter at the graces after it
                if menu_open {
                    let piece = if menu_opened { "file:menu/elden-force-choose-your-fighter.ogg" } else { "file:menu/elden-force-main-theme.ogg" };
                    menu_opened = true;
                    audio::music(piece, 0.35);
                } else {
                    audio::stop_music();
                }
            }
            // 0.16.0: in Training the JUMP FORCE menu opens anywhere (switch fighters inside a boss room)
            let ctx = if menu_open { When::Menu } else if at_grace || UI.lock().unwrap().training { When::Grace } else { When::InWorld };
            let mut fired = binds.poll(ctx);
            if PICK_WAITING.swap(false, Ordering::Relaxed) && menu_open {
                fired.insert(0, "menu_confirm");
            }
            #[cfg(feature = "lab")]
            {
                static WAS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                let now = lab::WHIP.load(Ordering::Relaxed);
                if now && !WAS.swap(now, Ordering::Relaxed) {
                    fired.push("special");
                }
                WAS.store(now, Ordering::Relaxed);
            }
            #[cfg(feature = "lab")]
            let whip_held = binds.held("special") || lab::WHIP.load(Ordering::Relaxed);
            #[cfg(not(feature = "lab"))]
            let whip_held = binds.held("special");
            for id in fired {
                if look.holding() {
                    continue;
                }
                if id == "special" {
                    // Deku's Black Whip: an enemy locked on or close in front is pulled in for a kick,
                    // otherwise the tendril catches something ahead and above to swing from
                    if !menu_open && swings(combat.fighter.id) {
                        let near = grab_target(player).is_some();
                        if !(near && combat.whip_strike(player)) {
                            if combat.fighter.id == "killua" && !movement.grip_in_reach(player) {
                                // 1.3.0: nothing for Killua's yo-yo to catch: his Godspeed step instead
                                movement.blink(player, 7.0, crate::movement::LIGHTNING_FX);
                            } else {
                                movement.whip(player);
                            }
                        }
                    } else if !menu_open && combat.fighter.id == "asta" {
                        // 1.2.0 (prototype, not approved yet): Asta's glider, his blade ridden down
                        movement.glide(player);
                    } else if !menu_open && combat.fighter.id == "goku" {
                        // 1.2.0 (user): Goku's Instant Transmission is on this button (L3 + RB / Q)
                        movement.teleport(player);
                    } else if !menu_open {
                        // 1.3.0: the new fighters' steps: Ichigo's Shunpo, Killua's Godspeed, Gojo's Blue, DIO's The
                        // World step, Sasuke's Amenotejikara (behind the lock-on target when there is one)
                        match combat.fighter.id {
                            "ichigo" => { movement.blink(player, 9.0, crate::movement::SHUNPO_FX); }
                            "gojo" => { movement.blink(player, 12.0, crate::movement::SHUNPO_FX); }
                            // (1.3.0: DIO moves in stopped time: the world's colour drains for a moment as he steps)
                            "dio" => {
                                if movement.blink(player, 10.0, crate::movement::DARK_FX) {
                                    crate::ui::time_stop(0.3, [1.0, 0.82, 0.3, 1.0]);
                                }
                            }
                            "sasuke" => {
                                if !movement.teleport(player) {
                                    movement.blink(player, 10.0, crate::movement::DARK_FX);
                                }
                            }
                            _ => {}
                        }
                    }
                    continue;
                }
                handle(id, player, &mut combat);
            }
            // hooks.speffect: keep the active fighter's stat profile applied.
            // 1.2.0: Carian Combo Warriors' param rows (src/regulation.rs), before the first swing
            if !ccw_params {
                ccw_params = regulation::inject();
                if ccw_params {
                    // 1.3.0: the villains' rows, before any boss arena's map loads its villain
                    villains::dress_rows();
                    // 2.0.6: the player's death effect (bosses::kill_player)
                    bosses::prepare_death_row();
                }
            }
            if !rows_written && std::env::var_os("JUMPRING_NO_PROFILES").is_none() {
                rows_written = profiles::write_rows();
            }
            if rows_written && applied_profile != Some(combat.fighter.stat_profile) {
                let d = &player.chr_ins.modules.data;
                hp_fraction = Some(((d.hp as f32 / d.max_hp.max(1) as f32).clamp(0.0, 1.0), d.max_hp, Instant::now()));
                let old = applied_profile.map(sheets::profile);
                profiles::switch(player, old, sheets::profile(combat.fighter.stat_profile));
                applied_profile = Some(combat.fighter.stat_profile);
                combat.resync_hp();
            }
            // hooks.recolor: when the recoloured armor is ready, show it (one reload in place).
            if !recolor_shown && recolor::READY.load(Ordering::Relaxed) {
                recolor_shown = true;
                if combat.fighter.recolor.is_some() || combat.fighter.skin.is_some() {
                    look.force();
                }
            }
            // hooks.look: a new fighter's look needs the character rebuilt: reload in place.
            if look.write(unsafe { player.player_game_data.as_mut() }, combat.fighter) {
                // lab: show the new look from the front
                look.face_camera = cfg!(feature = "lab");
                look.reload_in_place(player);
            }
            { let _p = perf::span("look"); look.tick(player, combat.fighter, combat.scale); }
            // 0.14.3: THE vanishing. Past a lethal height Elden Ring switches the character camera to its
            // death mode: it stops following and watches the player fall away. Falls never kill here, so
            // the player lived on and walked about with the camera left behind, out of sight ("invisible",
            // half in frame), until a move's camera reset freed it. While the player is alive, the camera
            // goes straight back to following them.
            if let Some(mut cam) = chr_cam {
                let cam = unsafe { cam.as_mut() };
                static NORMAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                // (read and written as the raw number: the game may hold values the enum doesn't list)
                let ty = std::ptr::addr_of_mut!(cam.camera_type) as *mut u32;
                let cur = unsafe { *ty };
                if cur == 7 {
                    if player.chr_ins.modules.data.hp > 0 {
                        let back = NORMAL.load(Ordering::Relaxed);
                        log!("camera: death camera while alive; back to following (type {back})");
                        unsafe { *ty = back };
                        cam.death_cam_target = None;
                        cam.request_camera_reset = true;
                    }
                } else {
                    NORMAL.store(cur, Ordering::Relaxed);
                }
            }
            // 0.14.3 diag (the vanishing hunt): everything about how the game treats the player's body,
            // logged whenever it changes (at most 10 lines a second)
            {
                static LAST: std::sync::Mutex<(Option<String>, Option<Instant>)> = std::sync::Mutex::new((None, None));
                let c = &player.chr_ins;
                let ph = &c.modules.physics;
                let ctrl = &c.chr_ctrl;
                let mask: String = c.render_group_mask.iter().take(8).map(|b| format!("{b:02x}")).collect();
                let camst = chr_cam.map(|cc| { let cc = unsafe { cc.as_ref() }; format!("cam {} lock {} tgt {}", unsafe { *(std::ptr::addr_of!(cc.camera_type) as *const u32) }, cc.is_movement_locked as u8, cc.death_cam_target.is_some() as u8) }).unwrap_or_default();
                let now = format!(
                    "{camst} ctrl {:#x} f {:02x}{:02x}{:02x}{:02x}{:02x}{:02x} load {:#x} act {:#x} mask {mask} op {:.2}/{:.2} tint {:.2} camo {:.2} base {:.2} grav {} fall {} touch {} scale {:.2} anim {}",
                    ctrl.flags.0, c.chr_flags1c4.0, c.chr_flags1c5.0, c.chr_flags1c6.0, c.chr_flags1c7.0, c.chr_flags1c8.0, c.chr_flags1ca.0,
                    c.load_state.0, c.chr_activation_flags.0, c.opacity_keyframes_multiplier, c.opacity_keyframes_multiplier_previous,
                    c.tint_alpha_multiplier, c.camouflage_transparency, c.base_transparency,
                    ph.gravity_disabled as u8, ph.is_falling as u8, ph.is_touching_ground as u8, ctrl.scale_size_y, movement::base_anim(movement::current_anim(player))
                );
                let mut l = LAST.lock().unwrap();
                // the anim and the fall/touch flags change constantly: only the rest decides a new line, unless 1 s passed
                let key = now.split(" grav ").next().unwrap_or("").to_string() + &now.split(" scale ").nth(1).map(|_| String::new()).unwrap_or_default();
                let old_key = l.0.as_deref().map(|o| o.split(" grav ").next().unwrap_or("").to_string());
                let quiet = l.1.is_none_or(|t| t.elapsed() > Duration::from_millis(100));
                if quiet && old_key.as_deref() != Some(key.as_str()) {
                    let p = ph.position;
                    log!("diag: {now} pos ({:.1}, {:.1}, {:.1}) hp {}", p.0, p.1, p.2, c.modules.data.hp);
                    *l = (Some(now), Some(Instant::now()));
                }
            }
            #[cfg(feature = "lab")]
            {
                // 0.14.2 diag: the player's draw groups and load state, whenever they change
                static LAST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
                let c = &player.chr_ins;
                let hex = |m: &[u8; 0x20]| m.iter().map(|b| format!("{b:02x}")).collect::<String>();
                let now = format!("load {:#x} act {:#x} mask {} fade {:.0}/{:.0} deact {:.0}/{:.0}", c.load_state.0, c.chr_activation_flags.0, hex(&c.render_group_mask), c.squared_fade_out_start_distance, c.squared_fade_out_start_distance_override, c.squared_deactivation_distance, c.squared_deactivation_distance_override);
                let mut l = LAST.lock().unwrap();
                if l.as_deref() != Some(now.as_str()) {
                    log!("lab draw: {now}");
                    *l = Some(now);
                }
            }
            #[cfg(feature = "lab")]
            if eldenring::util::input::is_key_pressed(0x21) {
                // Page Up: next body armor in the suit survey
                const BODIES: [i32; 14] = [180100, 790100, 700100, 220100, 240100, 860100, 150100, 331100, 720100, 520100, 350100, 230100, 660100, 280100];
                let next = match look.body_override { Some(b) => BODIES.iter().position(|x| *x == b).map(|i| (i + 1) % BODIES.len()).unwrap_or(0), None => 0 };
                look.body_override = Some(BODIES[next]);
                look.face_camera = true;
                look.force();
                log!("lab: survey body {}", BODIES[next]);
            }
            #[cfg(feature = "lab")]
            if eldenring::util::input::is_key_pressed(0x22) {
                // Page Down: next hairstyle in the survey
                const HAIR_A: [u32; 37] = [0, 113, 112, 1, 3, 100, 5, 10, 101, 9, 8, 6, 7, 115, 114, 2, 4, 102, 103, 104, 105, 106, 107, 109, 108, 111, 110, 117, 119, 118, 116, 121, 125, 122, 120, 123, 124];
                let next = match look.hair_override { Some(h) => HAIR_A.iter().position(|x| *x == h).map(|i| (i + 1) % 37).unwrap_or(0), None => 0 };
                look.hair_override = Some(HAIR_A[next]);
                look.face_camera = true;
                look.force();
                log!("lab: survey hairstyle UI {} = PartsId {}", next + 1, HAIR_A[next]);
            }
            // Keep the health percentage when a stat profile changes max HP (it applies a frame or two later).
            if let Some((frac, old_max, since)) = hp_fraction {
                let d = &mut player.chr_ins.modules.data;
                // (2.0.6: never over a death: it would stand a dying player up again)
                if movement::dying() || d.hp <= 0 {
                    hp_fraction = None;
                } else if d.max_hp != old_max {
                    d.hp = ((d.max_hp as f32) * frac).round().max(1.0) as i32;
                    hp_fraction = None;
                } else if since.elapsed() > Duration::from_secs(2) {
                    hp_fraction = None;
                }
            }
            #[cfg(feature = "lab")]
            {
                let _p = perf::span("lab");
                lab.tick(player);
            }
            combat.airborne = movement.airborne();
            combat.dash_ctx = movement.dash_context();
            {
                let _p = perf::span("combat");
                combat.tick(player);
            }
            let difficulty = UI.lock().unwrap().difficulty;
            let parried = combat.parry.parried_at.take().is_some();
            let training_now = UI.lock().unwrap().training;
            {
                let _p = perf::span("bosses");
                bosses::follow_death(player);
                bosses.tick(player, difficulty.boss_hp(), parried, training_now);
            }
            #[cfg(feature = "lab")]
            if let Some(i) = lab::VILLAIN.lock().unwrap().take() {
                villains.force = Some(i);
            }
            // (2.0.1 crash hunt, lab: novillains.on beside the log folder leaves the villain slots to the game alone)
            #[cfg(feature = "lab")]
            let skip_villains = {
                static SKIP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
                *SKIP.get_or_init(|| crate::log::dir().join("novillains.on").exists())
            };
            #[cfg(not(feature = "lab"))]
            let skip_villains = false;
            if !skip_villains {
                let _p = perf::span("villains");
                villains.tick(player, combat.fighter.id, difficulty, training_now);
            }
            {
                let _p = perf::span("clones");
                clones.tick(player, combat.fighter.id);
            }
            {
                let _p = perf::span("stands");
                stands.tick(player, combat.fighter.id, clones.alive());
            }
            timestop::tick();
            {
                let _p = perf::span("scene");
                scene.tick(UI.lock().unwrap().training);
            }
            // Deku's Full Cowl: a second double jump and air dash; Goku and All Might fly
            // 1.2.0 (user): Asta an extra double jump
            movement.air_moves = if combat.fighter.passive == Some("full_cowl") || combat.fighter.id == "asta" { 2 } else { 1 };
            movement.flies = combat.fighter.flies;
            movement.rubber = combat.fighter.id == "luffy";
            movement.swinger = swings(combat.fighter.id);
            movement.fighter = combat.fighter.id;
            movement.dives = combat.fighter.id != "custom";
            combat.swinging = movement.whip_anchor().is_some();
            // 1.3.0: Gon holds his real fishing rod while he swings (look.rs SWING_PROP)
            crate::look::SWING_PROP.store(combat.swinging, Ordering::Relaxed);
            // 1.2.0 (user): every fighter without a swing has at least two air dashes, Goku and All Might three
            movement.air_dash_max = match combat.fighter.id {
                "goku" | "all_might" | "naruto" | "killua" => 3,
                "luffy" => 1,
                "deku" => 2,
                _ => 2,
            };
            // 0.14.0: nothing of ours moves the player while a fighter change holds them at their spot
            // (0.13.0's double jump fought the hold, and the fall it made took Deku to 1 HP)
            // 0.16.0: a trip to a Site of Grace asked for from outside (grace::requested_trip), once the
            // fighter select is closed and the load has settled
            // (lab builds only since 0.16.1: the user's Godskin Apostle tests used it; not for players)
            #[cfg(feature = "lab")]
            if warp_check.is_some_and(|t| Instant::now() >= t) && !look.holding() && UI.lock().unwrap().menu.is_none() {
                warp_check = None;
                grace::requested_trip();
            }
            let held = look.holding() || rescue.holding();
            if held && movement.flying() {
                movement.reset();
            }
            if !menu_open && !held {
                let mev = { let _p = perf::span("movement"); movement.tick(player, combat.moving(), whip_held) };
                if let Some(movement::Event::SlamLanded(at)) = mev {
                    combat.slam(player, at);
                }
            }
            // 1.1.0: Deku and Luffy swing without voice quips (and land without one)
            if movement.whip_anchor().is_some() {
                combat.swing_quiet_until = Instant::now() + Duration::from_millis(1500);
            }
            // 0.10.0: effects in the world (world_fx.rs): the fighter's particles, impacts, the whip
            {
                let f = combat.fighter;
                let pos = ability::position(player);
                let fx = (!menu_open).then(|| world_fx::FighterFx {
                    pos,
                    fwd: ability::forward(player),
                    style: if combat.particles_on() { f.particles.unwrap_or("") } else { "" },
                    power: if f.particles == Some("steam") { 1.0 } else { combat.fx_power() },
                    height: f.height * combat.scale,
                    flying: f.flies && movement.flying(),
                });
                world_fx::publish_fighter(fx, movement.speed());
                let chest = (pos.0, pos.1 + 1.35 * f.height, pos.2);
                let whip = match (movement.whip_anchor(), combat.run_target(player)) {
                    (Some(a), _) => Some(a),
                    (None, Some(("deku_black_whip" | "luffy_gum_gum_rocket", t))) => Some((t.0, t.1 + 1.0, t.2)),
                    _ => None,
                };
                // 0.13.0: a small marker where Deku's whip would catch (not while a pull-in kick would go)
                // 0.15.2: the marker only while L3 is held on a controller (it jumped about and distracted
                // in play); never on keyboard (the player's pick)
                let marker_on = binds::device() == binds::Device::Controller && binds::pad_held(binds::L3);
                let aim = movement.aim_tick(player, !menu_open && swings(f.id) && !combat.moving() && marker_on);
                // 0.15.0: the enemy the swing button would grab now (the same test as pressing it)
                let grab = (marker_on && !menu_open && swings(f.id) && !combat.moving() && movement.whip_anchor().is_none()).then(|| grab_target(player)).flatten();
                let mut w = world_fx::FX.lock().unwrap();
                w.aim = aim;
                w.aim_enemy = grab;
                w.rubber = f.id == "luffy";
                w.cord = match f.id {
                    "gon" => 1,
                    "killua" => 2,
                    _ => 0,
                };
                match (whip, w.whip.as_mut()) {
                    (Some(to), Some(cur)) => {
                        cur.from = chest;
                        cur.to = to;
                    }
                    (Some(to), None) => w.whip = Some(world_fx::Whip { from: chest, to, since: Instant::now() }),
                    (None, _) => {
                        // Luffy's arm snaps back when he lets go (the miss effect from its turn)
                        if let (Some(old), true) = (w.whip.take(), f.id == "luffy") {
                            let turn = Instant::now().checked_sub(Duration::from_millis(140)).unwrap_or_else(Instant::now);
                            w.whip_miss = Some((old.from, old.to, turn));
                        }
                    }
                }
                drop(w);
                { let _p = perf::span("hits"); hits.tick(player, world_fx::color_of(f.particles)); }
            }
            {
                let _p = perf::span("rescue");
                rescue.tick(player, look.holding());
            }
            movement::VOID_SAFE.store(rescue.protected() || look.holding(), Ordering::Relaxed);
            // Exploration music: on while nothing hostile is close (checked twice a second).
            if last_calm_check.elapsed() >= Duration::from_millis(500) {
                last_calm_check = Instant::now();
                let pos = ability::position(player);
                let fight = player.chr_ins.is_locked_on || combat::enemy_near(player, pos, 22.0, None).is_some();
                if fight {
                    calm_since = None;
                } else {
                    calm_since.get_or_insert_with(Instant::now);
                }
                let explore = calm_since.is_some_and(|t| t.elapsed() >= Duration::from_secs(6));
                // 1.0.1 (user: "I don't want our music to replace or play over any of fromsoft's music
                // they had put at certain locations"): FromSoftware's location music lives in its own
                // maps (Roundtable Hold, the legacy dungeons, Siofra, Nokron, Mohgwyn, Volcano Manor...),
                // so ours plays only out in the open world (m60 / m61 tiles) and stays quiet everywhere else
                let explore = explore && player.current_block_id.is_overworld();
                // (1.3.0: the user asked to remove all Elden Ring music, then on review: "Elden ring's music should still
                // play in areas that they distinctly chose to add music": this stays)
                #[cfg(feature = "lab")]
                let explore = explore && !lab::NO_MUSIC.load(Ordering::Relaxed);
                if exploring != Some(explore) {
                    exploring = Some(explore);
                    audio::explore(explore);
                }
            }
            rebase.end(player);
            rebase::model_guard(player, movement.flying() || combat.moving());
            let training_on = UI.lock().unwrap().training;
            if training_on {
                // Training: Ki stays full and the Awakening refills, so every move can be tried back to back
                let d = &mut player.chr_ins.modules.data;
                d.fp = d.max_fp;
                combat.awakening = combat::AWAKENING_FULL;
            }
            {
                let _p = perf::span("training");
                training.tick(player, training_on, combat.current_move(), combat.fighter.id);
            }
            {
                let _p = perf::span("hud");
                publish_hud(player, &combat, at_grace);
            }
            pause_moves(combat.fighter.id);
            moves_mouse(combat.fighter.id);

            if last_report.elapsed() >= Duration::from_secs(5) {
                last_report = Instant::now();
                let d = &player.chr_ins.modules.data;
                log!("player: hp {}/{} fp {}/{} awakening {:.0} fighter {} grace {at_grace} menu {menu_open}", d.hp, d.max_hp, d.fp, d.max_fp, combat.awakening, combat.fighter.id);
            }
        },
        CSTaskGroupIndex::FrameBegin,
    );
    log!("boot: frame task registered");
    watchdog::start();
}

/// The enemy the swing button grabs and pulls in for an attack: one locked on within 14 m (0.12.0: a
/// far lock-on yanked Deku 50 m across the map), or unlocked, the nearest within 12 m in front. Its
/// chest, for the marker.
fn grab_target(player: &PlayerIns) -> Option<ability::V3> {
    let pos = ability::position(player);
    if player.chr_ins.is_locked_on {
        let t = player.chr_ins.lock_on_target_position;
        let close = ((t.0 - pos.0).powi(2) + (t.2 - pos.2).powi(2)).sqrt() < 14.0 && (t.1 - pos.1).abs() < 6.0;
        close.then_some((t.0, t.1, t.2))
    } else {
        combat::enemy_near(player, pos, 12.0, Some(ability::forward(player))).map(|e| (e.0, e.1 + 1.2, e.2))
    }
}

/// 1.1.0: Elden Ring's HUD state (CSFeManHudState: 0 hidden, 1 the pause menu, 2 a popup menu, 3 normal
/// play), read as a raw byte (it may hold values the enum doesn't list); 9 when there is no HUD yet.
pub fn game_hud_state() -> u8 {
    unsafe { eldenring::cs::CSFeManImp::instance() }.map(|f| unsafe { *(std::ptr::addr_of!(f.hud_state) as *const u8) }).unwrap_or(9)
}

/// 1.3.0: Elden Ring's pause menu is up (its HUD state; lab-probed with Escape).
pub fn pause_menu_open() -> bool {
    PAUSE_STATES.contains(&game_hud_state())
}
/// (lab, Escape in the world: the main menu (Equipment, Inventory, Status ... System) sets the HUD state to 1; play is 3)
const PAUSE_STATES: &[u8] = &[1];

/// 1.3.0: the Move List sorted by weapon or by fighter (its dropdown), the dropdown closed.
fn set_moves_sort(v: &mut ui::MovesView, m: &ui::Menu, weapon: bool, fighter_now: &'static str) {
    if weapon && !v.by_weapon {
        // start at the weapon of the fighter on show
        let on = ui::combo_fighters().get(m.cursor).copied().unwrap_or(fighter_now);
        let w = movelists::Weapon::of_fighter(on);
        v.weapon = movelists::WEAPONS.iter().position(|x| *x == w).unwrap_or(0);
    }
    v.by_weapon = weapon;
    v.sort_open = false;
    v.scroll = 0.0;
}

/// 1.3.0: mouse clicks on the Move List's "sort by" dropdown.
fn moves_mouse(fighter_now: &'static str) {
    let click = ui::SORT_CLICK.swap(0, Ordering::Relaxed);
    if click == 0 {
        return;
    }
    let mut s = UI.lock().unwrap();
    let sh = &mut *s;
    let Some(m) = sh.menu.as_ref().filter(|m| m.page == Page::Moves) else { return };
    let v = &mut sh.moves;
    match click {
        1 => {
            v.sort_open = !v.sort_open;
            v.sort_pick = v.by_weapon as u8;
        }
        2 | 3 => set_moves_sort(v, m, click == 3, fighter_now),
        _ => {}
    }
}

/// 1.3.0: open the Move List from the pause menu when its button is clicked (the user, on review: no R3 / M
/// shortcut).
fn pause_moves(fighter_now: &'static str) {
    if ui::OPEN_MOVES.swap(false, Ordering::Relaxed) {
        let mut s = UI.lock().unwrap();
        if s.menu.is_none() {
            let list = ui::combo_fighters();
            let cursor = list.iter().position(|f| *f == fighter_now).unwrap_or(0);
            s.menu = Some(ui::Menu { page: Page::Moves, cursor, at_start: false });
            s.moves.from_pause = true;
            s.moves.scroll = 0.0;
            log!("menu: Move List from the pause menu");
        }
    }
}

/// The fighters who swing (Deku's Black Whip, Luffy's stretched arm).
fn swings(id: &str) -> bool {
    // (1.3.0: Gon swings on his fishing rod's line; the user: Killua on his yo-yo)
    matches!(id, "deku" | "luffy" | "gon" | "killua")
}

/// One binding (sheet: inputs) went down.
/// A fighter picked while the character was still settling (made once it has).
static PICK_WAITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn handle(id: &str, player: &mut PlayerIns, combat: &mut combat::Combat) {
    let slot = match id {
        "ability_1" => Some(0),
        "ability_2" => Some(1),
        "ability_3" => Some(2),
        "ability_4" => Some(3),
        _ => None,
    };
    let mut s = UI.lock().unwrap();
    if let Some(slot) = slot {
        if s.menu.is_none() {
            drop(s);
            combat.cast(player, slot);
        }
        return;
    }
    if id == "menu_difficulty" && s.menu.as_ref().is_some_and(|m| m.page == Page::Roster) {
        s.difficulty = s.difficulty.next();
        ui::save_profile(&s);
        log!("difficulty: {}", s.difficulty.label());
        return;
    }
    if id == "menu_training" && s.menu.as_ref().is_some_and(|m| m.page == Page::Roster) {
        s.training = !s.training;
        log!("training: {}", if s.training { "on" } else { "off" });
        return;
    }
    if id == "menu_warnings" && s.menu.as_ref().is_some_and(|m| m.page == Page::Roster) {
        s.warnings = s.warnings.next();
        ui::save_profile(&s);
        log!("boss warnings: {}", s.warnings.label());
        return;
    }
    let fighters = sheets::FIGHTERS.len();
    let fighter_now = s.fighter;
    let setting_rows = ui::setting_rows(&s);
    let build_tarnished = s.build.tarnished;
    // 1.2.x (user: the Move List "is very confusing and cluttered"): tabs (LB / RB: the moveset, movement),
    // X sorts by fighter or by weapon, left / right turns the page, up / down scrolls, back closes it
    {
        let sh = &mut *s;
        if let Some(m) = sh.menu.as_mut().filter(|m| m.page == Page::Moves) {
            let v = &mut sh.moves;
            let fighters = ui::combo_fighters().len().max(1);
            let weapons = movelists::WEAPONS.len();
            let by_weapon = v.tab == 0 && v.by_weapon;
            if v.sort_open {
                // the "sort by" dropdown: up / down picks, confirm (or X again) takes it, back closes it
                match id {
                    "menu_up" | "menu_down" => v.sort_pick = 1 - v.sort_pick.min(1),
                    "menu_confirm" | "menu_difficulty" => set_moves_sort(v, m, v.sort_pick == 1, fighter_now),
                    "menu_back" | "menu_moves" => v.sort_open = false,
                    _ => {}
                }
                return;
            }
            match id {
                "menu_left" | "menu_right" => {
                    let fwd = id == "menu_right";
                    if by_weapon {
                        v.weapon = if fwd { (v.weapon + 1) % weapons } else { (v.weapon + weapons - 1) % weapons };
                    } else {
                        m.cursor = if fwd { (m.cursor + 1) % fighters } else { (m.cursor + fighters - 1) % fighters };
                    }
                    v.scroll = 0.0;
                }
                "menu_up" => v.scroll = (v.scroll - 120.0).max(0.0),
                "menu_down" => v.scroll = (v.scroll + 120.0).min(menu_pages::MOVES_MAX_SCROLL.load(std::sync::atomic::Ordering::Relaxed) as f32),
                "menu_build" | "menu_warnings" => {
                    v.tab = 1 - v.tab.min(1);
                    v.scroll = 0.0;
                }
                "menu_difficulty" if v.tab == 0 => {
                    v.sort_open = true;
                    v.sort_pick = v.by_weapon as u8;
                }
                "menu_back" | "menu_moves" | "menu_confirm" => {
                    let list = ui::combo_fighters();
                    let shown = list.get(m.cursor).copied().unwrap_or(fighter_now);
                    m.page = Page::Roster;
                    m.cursor = sheets::FIGHTERS.iter().position(|f| f.id == shown).unwrap_or(0);
                    if v.from_pause {
                        // (opened from the pause menu: back to the game, not the fighter select)
                        v.from_pause = false;
                        sh.menu = None;
                        binds::BLOCK_ALL.store(false, Ordering::Relaxed);
                    }
                }
                _ => {}
            }
            return;
        }
    }
    let Some(m) = s.menu.as_mut() else {
        if id == "menu_open" {
            let cursor = sheets::FIGHTERS.iter().position(|f| f.id == s.fighter).unwrap_or(0);
            s.menu = Some(Menu { page: Page::Roster, cursor, at_start: false });
            log!("menu: opened at grace");
        }
        return;
    };
    let rows = match m.page {
        Page::Roster => fighters,
        Page::Loadout => 4,
        Page::Controls => ui::control_rows().len() + 1,
        Page::Build => 8,
        Page::Settings => setting_rows.len(),
        Page::Moves => ui::combo_fighters().len().max(1),
        Page::Credits => credits::LINES.len(),
    };
    // 1.2.0: the Move List, from the fighter select (its fighter) or the settings (the one playing)
    if id == "menu_moves" && m.page == Page::Roster {
        let list = ui::combo_fighters();
        let on = sheets::FIGHTERS.get(m.cursor).map(|f| f.id).unwrap_or(fighter_now);
        m.page = Page::Moves;
        m.cursor = list.iter().position(|f| *f == on).or_else(|| list.iter().position(|f| *f == fighter_now)).unwrap_or(0);
        return;
    }
    // 1.1.0: the fighter select's Settings button opens the Elden Force settings (the Controls page
    // is one of its rows)
    if id == "menu_settings" && m.page == Page::Roster {
        m.page = Page::Settings;
        m.cursor = 0;
        return;
    }
    if m.page == Page::Settings {
        let row = setting_rows[m.cursor.min(rows - 1)];
        let step: i32 = match id {
            "menu_left" => -1,
            "menu_right" => 1,
            "menu_confirm" => 1,
            _ => 0,
        };
        match id {
            "menu_up" => m.cursor = (m.cursor + rows - 1) % rows,
            "menu_down" => m.cursor = (m.cursor + 1) % rows,
            "menu_back" | "menu_settings" => {
                m.page = Page::Roster;
                m.cursor = sheets::FIGHTERS.iter().position(|f| f.id == fighter_now).unwrap_or(0);
            }
            "menu_confirm" if row == ui::SettingRow::Controls => {
                m.page = Page::Controls;
                m.cursor = 0;
            }
            "menu_confirm" if row == ui::SettingRow::Credits => {
                m.page = Page::Credits;
                m.cursor = 0;
            }
            "menu_confirm" if row == ui::SettingRow::Moves => {
                let list = ui::combo_fighters();
                m.page = Page::Moves;
                m.cursor = list.iter().position(|f| *f == fighter_now).unwrap_or(0);
            }
            _ if step != 0 => {
                let vol = |v: u8| (v as i32 + step).clamp(0, 10) as u8;
                match row {
                    ui::SettingRow::Music => s.music_vol = vol(s.music_vol),
                    ui::SettingRow::Voices => {
                        s.voice_vol = vol(s.voice_vol);
                        // a sample of the quips at the new volume
                        audio::set_volumes(s.music_vol, s.voice_vol);
                        let group = format!("{}/attack", if fighter_now == "custom" { "goku" } else { fighter_now });
                        audio::play(&format!("pack:{group}"), audio::Pick::Random, 0.85);
                    }
                    ui::SettingRow::Difficulty => s.difficulty = if step > 0 { s.difficulty.next() } else { s.difficulty.next().next() },
                    ui::SettingRow::Warnings => s.warnings = if step > 0 { s.warnings.next() } else { s.warnings.next().next().next() },
                    ui::SettingRow::Opacity => s.warn_opacity = (s.warn_opacity as i32 + step).clamp(1, 10) as u8,
                    ui::SettingRow::Training => s.training = !s.training,
                    ui::SettingRow::Controls | ui::SettingRow::Moves | ui::SettingRow::Credits => {}
                }
                ui::save_profile(&s);
            }
            _ => {}
        }
        return;
    }
    if id == "menu_build" && m.page == Page::Roster {
        m.page = Page::Build;
        m.cursor = 0;
        return;
    }
    if m.page == Page::Build {
        let n = if build_tarnished { 8 } else { 3 };
        let back_to = sheets::FIGHTERS.iter().position(|f| f.id == fighter_now).unwrap_or(0);
        let mut spend = None;
        match id {
            "menu_up" | "menu_left" => m.cursor = (m.cursor + n - 1) % n,
            "menu_down" | "menu_right" => m.cursor = (m.cursor + 1) % n,
            "menu_confirm" => spend = Some(m.cursor.min(n - 1)),
            "menu_back" | "menu_build" => {
                m.page = Page::Roster;
                m.cursor = back_to;
            }
            _ => {}
        }
        if spend.is_some() {
            s.build_spend = spend;
        }
        return;
    }
    // 1.3.0: the Credits page scrolls a line at a time; back returns to the settings
    if m.page == Page::Credits {
        let last = credits::LINES.len().saturating_sub(credits::PAGE_LINES);
        match id {
            "menu_up" => m.cursor = m.cursor.saturating_sub(1),
            "menu_down" => m.cursor = (m.cursor + 1).min(last),
            "menu_back" | "menu_confirm" => {
                m.page = Page::Settings;
                m.cursor = setting_rows.iter().position(|r| *r == ui::SettingRow::Credits).unwrap_or(0);
            }
            _ => {}
        }
        return;
    }
    if m.page == Page::Controls {
        match id {
            "menu_up" => m.cursor = (m.cursor + rows - 1) % rows,
            "menu_down" => m.cursor = (m.cursor + 1) % rows,
            "menu_confirm" => match ui::control_rows().get(m.cursor) {
                Some(i) => binds::CAPTURE.store(*i as i32, Ordering::Relaxed),
                None => {
                    binds::reset();
                    s.notice = Some(("Controls reset to defaults".to_string(), Instant::now()));
                }
            },
            "menu_back" => {
                // 1.1.0: back to the settings it was opened from
                m.page = Page::Settings;
                m.cursor = setting_rows.iter().position(|r| *r == ui::SettingRow::Controls).unwrap_or(0);
            }
            _ => {}
        }
        return;
    }
    // The roster is two rows of portrait tiles (select_screen::ORDER): left / right move along it, up / down change row.
    if m.page == Page::Roster && matches!(id, "menu_up" | "menu_down" | "menu_left" | "menu_right") {
        let n = select_screen::ORDER.len();
        let i = select_screen::index_of(sheets::FIGHTERS[m.cursor].id);
        let j = match id {
            "menu_left" => (i + n - 1) % n,
            "menu_right" => (i + 1) % n,
            _ => select_screen::grid_step(i, id == "menu_down"),
        };
        let want = select_screen::ORDER[j];
        m.cursor = sheets::FIGHTERS.iter().position(|f| f.id == want).unwrap_or(0);
        return;
    }
    match id {
        "menu_up" => m.cursor = (m.cursor + rows - 1) % rows,
        "menu_down" => m.cursor = (m.cursor + 1) % rows,
        "menu_left" | "menu_right" if m.page == Page::Loadout => {
            let slot = m.cursor;
            let pool = ui::pool(slot);
            let i = pool.iter().position(|a| a.id == s.loadout[slot]).unwrap_or(0);
            let n = pool.len();
            let j = if id == "menu_right" { (i + 1) % n } else { (i + n - 1) % n };
            s.loadout[slot] = pool[j].id;
        }
        "menu_confirm" if m.page == Page::Roster && sheets::FIGHTERS[m.cursor].id == "custom" => {
            m.page = Page::Loadout;
            m.cursor = 0;
        }
        "menu_confirm" => {
            let chosen = if m.page == Page::Loadout { "custom" } else { sheets::FIGHTERS[m.cursor].id };
            s.fighter = sheets::fighter(chosen).map(|f| f.id).unwrap_or("goku");
            s.menu = None;
            ui::save_profile(&s);
            let (f, loadout) = (s.fighter, s.loadout);
            drop(s);
            combat.loadout = loadout;
            combat.set_fighter(f);
            ui::notice(format!("{} enters the Lands Between", combat.fighter.name));
        }
        "menu_back" if m.page == Page::Loadout => {
            m.page = Page::Roster;
            m.cursor = sheets::FIGHTERS.iter().position(|f| f.id == "custom").unwrap_or(0);
        }
        "menu_back" => {
            // Closing keeps the current fighter.
            let start = m.at_start;
            let f = s.fighter;
            s.menu = None;
            drop(s);
            if start {
                combat.set_fighter(f);
            }
        }
        _ => {}
    }
}

fn publish_hud(player: &PlayerIns, combat: &combat::Combat, at_grace: bool) {
    let mut s = UI.lock().unwrap();
    let h = &mut s.hud;
    h.in_world = true;
    h.at_grace = at_grace;
    // (3 is normal play; anything else is one of the game's menus, the map, a popup or a cutscene)
    h.game_menu = !matches!(game_hud_state(), 3 | 9);
    h.fighter = combat.fighter.name;
    h.fighter_id = combat.fighter.id;
    h.level = unsafe { player.player_game_data.as_ref().level };
    h.series = combat.fighter.series;
    h.fp = player.chr_ins.modules.data.fp;
    h.awakening = combat.awakening;
    for slot in 0..4 {
        if let Some(a) = combat.ability(slot) {
            h.abilities[slot] = ui::HudAbility {
                name: a.name,
                cost: a.fp_cost,
                cooldown: combat.cooldown_left(a),
                ultimate: a.slot == sheets::Slot::Ultimate,
                combo: a.combo,
            };
        }
    }
    s.fx.finisher_cd = crate::combos::tree(combat.fighter.id).map(|_| combat.finisher_cooldown());
}

/// 1.3.1 lab: the raw memory and field checks behind the 2.7.2.0 layout fix (JR_LAYOUT)
#[cfg(feature = "lab")]
fn lab_layout_dump(wcm: &WorldChrMan) {
    use std::io::Write;
    static DONE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = DONE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if n % 300 != 0 || n > 300 * 20 {
        return;
    }
    let dir = std::path::PathBuf::from(r"C:\Users\asher\JumpRing\lab\layout");
    let _ = std::fs::create_dir_all(&dir);
    let base = wcm as *const WorldChrMan as usize;
    let len = std::mem::size_of::<WorldChrMan>() + 0x4000;
    let bytes = unsafe { std::slice::from_raw_parts(base as *const u8, len) };
    if let Ok(mut f) = std::fs::File::create(dir.join(format!("wcm_{n}.bin"))) {
        let _ = f.write_all(&(base as u64).to_le_bytes());
        let _ = f.write_all(bytes);
    }
    let mp = std::ptr::addr_of!(wcm.main_player) as usize;
    let exe = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) } as usize;
    log!("layout {n}: exe {exe:#x} wcm {base:#x} size {:#x} main_player field +{:#x} = {:#x}", std::mem::size_of::<WorldChrMan>(), mp - base, unsafe { *(mp as *const usize) });
    // safe reads (VirtualQuery: committed and readable)
    let q = |a: usize| -> Option<u64> {
        use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_NOACCESS, PAGE_GUARD};
        if a < 0x10000 || a % 4 != 0 {
            return None;
        }
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { VirtualQuery(a as _, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 {
            return None;
        }
        if mbi.State != MEM_COMMIT || mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0 || mbi.Protect == 0 {
            return None;
        }
        let end = mbi.BaseAddress as usize + mbi.RegionSize;
        if a + 8 > end {
            return None;
        }
        Some(unsafe { std::ptr::read_volatile(a as *const u64) })
    };
    // (the 2.7.2.0 map: tools/binary-mapper)
    let (pvmt, cvmt) = (0x2a81cb0usize, 0x2a330d8usize);
    let vt = |obj: usize| q(obj).map(|v| (v as usize).wrapping_sub(exe));
    let p = unsafe { *(mp as *const usize) };
    if p == 0 {
        return;
    }
    log!("layout: player {p:#x} vtable rva {:?} (player_ins_vmt {pvmt:#x}, chr_ins_vmt {cvmt:#x})", vt(p).map(|v| format!("{v:#x}")));
    // the vectors in the manager's tail: (allocator, begin, end, cap), elements' first pointer's vtable
    let size = std::mem::size_of::<WorldChrMan>() + 0x2000;
    let mut o = 0x1e000;
    while o + 32 <= size {
        if let (Some(b), Some(e), Some(c)) = (q(base + o + 8), q(base + o + 16), q(base + o + 24)) {
            let (b, e, c) = (b as usize, e as usize, c as usize);
            if b > 0x10000 && b <= e && e <= c && c - b < 0x100000 && c > b {
                for stride in [8usize, 16] {
                    if (e - b) % stride == 0 && e > b {
                        let first = q(b).map(|v| v as usize).unwrap_or(0);
                        let v0 = vt(first);
                        if let Some(v) = v0 {
                            if v < 0x6000000 {
                                log!("layout: vec at +{o:#x} n {} stride {stride}: first {first:#x} vtable rva {v:#x}", (e - b) / stride);
                            }
                        }
                    }
                }
            }
        }
        o += 8;
    }
    // the crate's idea of the player's fields: their offsets and the values there
    let pl = wcm.main_player.as_ref().unwrap();
    let ci = &pl.chr_ins as *const _ as usize;
    let off = |f: usize| f - ci;
    let mods_f = std::ptr::addr_of!(pl.chr_ins.modules) as usize;
    let mods = q(mods_f).unwrap_or(0) as usize;
    log!("layout: chr_ins.modules field +{:#x} -> {mods:#x}; team +{:#x} = {:?}; npc_param +{:#x}; char id +{:#x} = {:?}; entity +{:#x}",
        off(mods_f), off(std::ptr::addr_of!(pl.chr_ins.team_type) as usize), q(std::ptr::addr_of!(pl.chr_ins.team_type) as usize).map(|v| v & 0xff),
        off(std::ptr::addr_of!(pl.chr_ins.npc_param_id) as usize), off(std::ptr::addr_of!(pl.chr_ins.character_id) as usize),
        q(std::ptr::addr_of!(pl.chr_ins.character_id) as usize).map(|v| v & 0xffffffff), off(std::ptr::addr_of!(pl.chr_ins.event_entity_id) as usize));
    // the module container: each pointer in it and its vtable
    if mods != 0 {
        let mut line = String::new();
        for k in 0..40 {
            let ptr = q(mods + k * 8).unwrap_or(0) as usize;
            line += &format!(" {k}:{}", vt(ptr).map(|v| format!("{v:#x}")).unwrap_or("-".into()));
        }
        log!("layout: modules{line}");
    }
    // a scan of the player object's first 0x1000 bytes for the player's hp-like ints
    let mut line = String::new();
    for k in (0..0x1000).step_by(8) {
        if let Some(v) = q(p + k) {
            let v = v as usize;
            if let Some(t) = vt(v) {
                if t < 0x6000000 {
                    line += &format!(" +{k:#x}:{t:#x}");
                }
            }
        }
    }
    log!("layout: player pointers to game objects:{line}");
    // the manager's pointers labelled by their objects' vtables; any object whose +0x8c holds 10.0 (the summon manager's
    // disappear delay)
    let known: &[(usize, &str)] = &[(0x2a2ca18, "chr_cam"), (0x2aa2b08, "cscam"), (0x2a43348, "chr_set"), (0x2a4d620, "open_field_chr_set"),
        (0x2a81cb0, "player"), (0x2a49090, "enemy"), (0x2a51800, "grid_area_chr")];
    let mut line = String::new();
    for o in (0x1e400..0x1f3f8).step_by(8) {
        let Some(v) = q(base + o) else { continue };
        let v = v as usize;
        if let Some(t) = vt(v) {
            if let Some((_, nm)) = known.iter().find(|(r, _)| *r == t) {
                line += &format!(" +{o:#x}={nm}");
            }
        }
        if let Some(d) = q(v + 0x88) {
            if (d >> 32) as u32 == 10.0f32.to_bits() {
                line += &format!(" +{o:#x}=delay10@0x8c");
            }
        }
    }
    log!("layout: manager fields:{line}");
    log!("layout: crate offsets: summon_buddy_manager +{:#x} net_chr_sync +{:#x} debug_chr_creator +{:#x} chr_cam +{:#x}",
        std::ptr::addr_of!(wcm.summon_buddy_manager) as usize - base, std::ptr::addr_of!(wcm.net_chr_sync) as usize - base,
        std::ptr::addr_of!(wcm.debug_chr_creator) as usize - base, std::ptr::addr_of!(wcm.chr_cam) as usize - base);
}

/// Lab-only keys: F1 go to a hostile, F2 list nearby, F3 refill Ki, F4 fill Awakening, F5.. fighters.
#[cfg(feature = "lab")]
fn lab_keys(wcm: &mut WorldChrMan, oracle: &mut oracle::HitOracle, combat: &mut combat::Combat) {
    // (the keys only: the script below runs on)
    let keys_on = !lab::NO_LAB_KEYS.load(Ordering::Relaxed);
    let is_key_pressed = |k: i32| keys_on && eldenring::util::input::is_key_pressed(k);
    if is_key_pressed(0x70) || lab::GOTO.swap(false, std::sync::atomic::Ordering::Relaxed) {
        oracle.go_to_target(wcm);
    }
    if is_key_pressed(0x71) {
        oracle.dump(wcm);
    }
    if is_key_pressed(0xDB) || lab::BOSS.swap(false, std::sync::atomic::Ordering::Relaxed) {
        oracle.go_to_boss(wcm);
        bosses::probe_npc_rows();
    }
    if is_key_pressed(0xDD) {
        // ]: audition the next candidate boss spell
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        bosses::AUDITION.store(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed), std::sync::atomic::Ordering::Relaxed);
    }
    if is_key_pressed(0x69) || lab::WARP.swap(false, std::sync::atomic::Ordering::Relaxed) {
        // Numpad 9: warp to the first Site of Grace of the Academy of Raya Lucaria (area 14), for the
        // 0.7.0 "spells stop casting there" report
        use eldenring::cs::{BlockId, BonfireWarpParam, GameMan, SoloParamRepository};
        if let (Ok(repo), Ok(gm)) = (unsafe { SoloParamRepository::instance() }, unsafe { GameMan::instance_mut() }) {
            let grace = (0..2000).filter_map(|i| repo.get_row_by_index::<BonfireWarpParam>(i)).find(|r| r.area_no() == 14 && r.bonfire_entity_id() > 0);
            if let Some(g) = grace {
                gm.move_map_target = BlockId::from_parts(14, g.grid_x_no(), 0, 0);
                gm.initial_area_entity_id = g.bonfire_entity_id();
                gm.warp_requested = true;
                log!("lab: warp to Raya Lucaria grace {} (block 14 {} 0 0)", g.bonfire_entity_id(), g.grid_x_no());
            }
        }
    }
    if is_key_pressed(0x68) {
        // Numpad 8: fill the shared bullet pool (128) with bullets that never end (Holy Ground, owned by
        // the player, at their feet), as in a crowded fight; the next abilities must still work
        if let Some(p) = wcm.main_player.as_ref() {
            let pos = ability::position(p);
            let mut n = 0;
            for _ in 0..140 {
                if ability::fire_at(p, 2070, (pos.0, pos.1 + 0.2, pos.2), (0.0, -1.0, 0.0)).is_ok() {
                    n += 1;
                }
            }
            log!("lab: filled the bullet pool with {n} lasting bullets (pool {})", ability::pool_count());
        }
    }
    if is_key_pressed(0xDC) {
        // backslash: the next boss art now
        bosses::FORCE_ART.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let Some(player) = wcm.main_player.as_mut() else { return };
    lab::script_tick(player, combat);
    if is_key_pressed(0x72) {
        let d = &mut player.chr_ins.modules.data;
        d.fp = d.max_fp;
    }
    if is_key_pressed(0x73) {
        combat.awakening = combat::AWAKENING_FULL;
    }
    if is_key_pressed(0x23) {
        // End: flip keyboard/controller prompts
        binds::toggle_device();
    }
    if is_key_pressed(0x24) {
        // Home: respawn at the last grace (lab save only)
        player.chr_ins.modules.data.hp = 0;
    }
    const FIGHTER_KEYS: [(i32, &str); 9] = [
        (0x74, "goku"), (0x75, "naruto"), (0x76, "luffy"), (0x77, "deku"), (0x78, "all_might"),
        (0x79, "bakugo"), (0x7A, "todoroki"), (0x7B, "asta"), (0xBD, "custom"),
    ];
    for (key, id) in FIGHTER_KEYS {
        if is_key_pressed(key) {
            combat.set_fighter(id);
        }
    }
}
