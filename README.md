# Elden Force: source code

The source of the compiled files in Elden Force (https://www.nexusmods.com/eldenring/mods/11429): `jumpring.dll` (the mod),
`elden_force_prepare.exe` (builds the fighters' outfits and animations from your game files before the first launch) and
`elden_force_start.exe` (the starter: opens Steam if needed, then starts Elden Ring through ModEngine2). Elden Force's own
code is MIT licensed (see LICENSE).

The other executables in the download are ModEngine2's own (https://github.com/soulsmods/ModEngine2, MIT) and Microsoft's
Visual C++ runtime DLLs.

## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).
(Applied for in October 2026; until the project is approved, releases are unsigned. Every release's programs are built
by GitHub Actions from this repository, see "Checking a download against this source" below.)

Team roles:
- Committers and reviewers: [Asher Sheets](https://github.com/ashersheets-jpg)
- Approvers: [Asher Sheets](https://github.com/ashersheets-jpg)

Privacy policy: this program will not transfer any information to other networked systems unless specifically requested
by the user or the person installing or operating it.

## What's here

- `mod/jumpring`: the mod (Rust). `src/bin/prepare.rs` is `elden_force_prepare.exe`.
- `mod/launcher`: `elden_force_start.exe` (Rust).
- `mod/hks`: Elden Force's own HKS/Lua scripts (`c0000.hks`, `elden_force.lua`). The download also has `c0000_ccw.hks` (Carian
  Combo Warriors' player script, by its author, https://www.nexusmods.com/eldenring/mods/4016, used with permission) and
  `jr_dispatch.lua` (generated from it); they are not Elden Force's own code and are not compiled, so they aren't here.
- `sheets`: the design sheets (fighters, abilities, animations...) that `mod/jumpring/build.rs` turns into Rust at build time.
- `tools/fromsoftware-rs`: the fromsoftware-rs crates (https://github.com/vswarte/fromsoftware-rs, MIT/Apache-2.0) with
  Elden Force's small changes (the Elden Ring 2.7.2 addresses and a few struct fields).
- `lab/judge2.txt`: a data list the mod's code includes.

## Build

Requirements: Windows 10/11 x64, Rust (stable, 2024 edition: https://rustup.rs) with the `x86_64-pc-windows-msvc` toolchain
and the Visual Studio Build Tools (C++).

```
cd mod/jumpring
cargo build --release
cd ../launcher
cargo build --release
```

Outputs:
- `mod/jumpring/target/release/jumpring.dll` and `mod/jumpring/target/release/elden_force_prepare.exe`
- `mod/launcher/target/release/elden_force_start.exe`

In the download they sit at `modengine2/jumpring/jumpring.dll`, `modengine2/jumpring/elden_force_prepare.exe` and
`modengine2/elden_force_start.exe`. (Your own builds are not bit-for-bit identical to the released files: Rust embeds
build paths.)

## Checking a download against this source

GitHub builds the three programs from this repository on its own Windows machines (`.github/workflows/build.yml`),
lists their SHA-256 hashes in each run's summary, and publishes a signed build-provenance attestation for each file.
From 2.0.6 on, the released jumpring.dll is the one GitHub built. (Since 2.0.5 the Melty download has only jumpring.dll: Melty starts the game through its own ModEngine2, and the outfits are built inside the game.) To check a file from a download, install the GitHub CLI
and run:

```
gh attestation verify jumpring.dll --repo ashersheets-jpg/elden-force-source
```

It confirms that the file was built by this repository's workflow, and from which commit.
