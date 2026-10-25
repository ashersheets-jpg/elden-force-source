//! 1.3.0: the Credits page (Elden Force settings -> Credits; menu_pages.rs draws it). The user (2026-10-16): Carian Combo
//! Warriors' author asked for credit in game as well as in the README and the listings; every model author whose terms ask
//! for credit is named here with where the work comes from. One model author gave his whole catalogue on the condition
//! that he is NOT named or linked anywhere: his models are listed as used with permission, author unnamed.
//! Kept in step with THIRD_PARTY_NOTICES (tools_scripts/notices.py) and MODEL-SOURCES.md.

/// How many lines the page shows at once (the rest scroll)
pub const PAGE_LINES: usize = 22;

/// A line of the page: a heading (true) or an entry under it.
pub const LINES: &[(bool, &str)] = &[
    (true, "MOVESETS"),
    (false, "Carian Combo Warriors, by its author on Nexus Mods (nexusmods.com/eldenring/mods/4016): every fighter's basic"),
    (false, "   attacks and Your Tarnished's movesets, used with permission and credit"),
    (false, "Air boost: from ELDEN VINS by mahku (nexusmods.com/eldenring/mods/4709), used with permission"),
    (true, "FIGHTER MODELS"),
    (false, "Goku: Goku SSJ by Bluesands (Nexus Mods 7497)"),
    (false, "Naruto: Naruto Uzumaki (Shippuden) by Bluesands (Nexus Mods 5204)"),
    (false, "Gojo: Satoru Gojo by Bluesands (Nexus Mods 4573)"),
    (false, "Yuji: Yuji Itadori by Bluesands (Nexus Mods 4547)"),
    (false, "Ichigo's Tensa Zangetsu: AceMx242 (Nexus Mods 6662)"),
    (false, "Sasuke's Kusanagi and Yami's katana: made for Elden Force"),
    (false, "Deku, Todoroki, Ichigo, Sasuke, Zoro and Yami: used with their author's permission (the author asked not to be named)"),
    (false, "Luffy, All Might, Bakugo, Asta, Jotaro, DIO, Star Platinum, The World, Gon (and his fishing rod), Adult Gon and"),
    (false, "   Killua (and Godspeed): made for Elden Force"),
    (true, "VILLAIN MODELS"),
    (false, "Cell: Cell Perfect by Chechorams16 (Sketchfab, CC BY 4.0)"),
    (false, "Blackbeard: Marshall D. Teach/Blackbeard by Cyrone (Sketchfab, CC BY 4.0)"),
    (false, "Hisoka: Hisoka HxH by CriticalMammal (Sketchfab, CC BY 4.0)"),
    (false, "All For One: by junioadventure2 (DeviantArt)"),
    (false, "Aizen and Sukuna: by Bluesands (Nexus Mods 5897, 4547)"),
    (false, "Doflamingo: used with its author's permission (the author asked not to be named)"),
    (false, "Frieza, Madara (and his gunbai), Pain's Six Paths, Meruem, Shigaraki (both forms), Muscular (both forms), Kars,"),
    (false, "   Patry and Dante: made for Elden Force"),
    (true, "SOUND"),
    (false, "Battle shouts: Youfulca (Wingless Seraph) free battle voices, youfulca.com"),
    (false, "Soundtrack composed for Elden Force; played with Versilian Studios' VSCO 2 CE and VCSL (CC0), the pipe organ sampled"),
    (false, "   by Simon Dalzell (Ivy Audio), and FreePats' Upright Piano KW (CC0)"),
    (true, "CODE AND FONTS"),
    (false, "ModEngine2 (soulsmods), fromsoftware-rs, ER Mario, hudhook and Dear ImGui, symphonia, and the Rust crates listed in"),
    (false, "   THIRD_PARTY_NOTICES.md"),
    (false, "Fonts: Cinzel and Cormorant Garamond (SIL Open Font License), Roboto (Apache 2.0)"),
    (false, "QuestPath by imCioco: loaded only if you installed it yourself"),
    (true, "THE GAME"),
    (false, "Elden Ring by FromSoftware and Bandai Namco: Elden Force reads it from your own install. The anime characters belong"),
    (false, "   to their creators; Elden Force is a free fan mod."),
    (true, "FIND ELDEN FORCE"),
    (false, "Melty: melty.gg/m/elden-force    Nexus Mods: nexusmods.com/eldenring/mods/11429"),
    (false, "Trailer: youtu.be/v4707x-tXG0"),
];
