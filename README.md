# Elden Force: source code

The source of the compiled files in Elden Force (https://www.nexusmods.com/eldenring/mods/11429): `jumpring.dll` (the mod),
`elden_force_prepare.exe` (builds the fighters' outfits and animations from your game files before the first launch) and
`elden_force_start.exe` (the starter: opens Steam if needed, then starts Elden Ring through ModEngine2). Elden Force's own
code is MIT licensed (see LICENSE).

The other executables in the download are ModEngine2's own (https://github.com/soulsmods/ModEngine2, MIT) and Microsoft's
Visual C++ runtime DLLs.

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
`modengine2/elden_force_start.exe`. (Builds are not bit-for-bit identical to the released files: Rust embeds build paths.)
