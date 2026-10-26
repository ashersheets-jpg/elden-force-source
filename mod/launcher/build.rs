//! The starter's Windows version info and manifest (../version_info.rs).

fn main() {
    embed_version_info(&[VersionTarget {
        link: "bin=elden_force_start",
        description: "Elden Force: starts Elden Ring with the mod through ModEngine2",
        file: "elden_force_start.exe",
        program: true,
    }]);
}

include!("../version_info.rs");
