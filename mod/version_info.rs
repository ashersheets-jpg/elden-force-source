// Windows version info (publisher, product, description, version, source link) and, for programs, an asInvoker manifest,
// compiled with the Windows SDK's rc.exe and linked into each binary. Bare unsigned binaries with none of this score as
// suspicious with machine-learning antivirus scanners (2026-10-25: Nexus quarantined 2.0.3 over three such false positives).
// Shared by the crates' build.rs files through include!(). Skipped (with a warning) where no rc.exe is found.

/// One binary: the cargo link-arg target ("cdylib" or "bin=<name>"), its description, its file name, and whether it is a
/// program (which also gets the manifest).
pub struct VersionTarget<'a> {
    pub link: &'a str,
    pub description: &'a str,
    pub file: &'a str,
    pub program: bool,
}

fn find_rc() -> Option<std::path::PathBuf> {
    if let Ok(rc) = std::env::var("RC_EXE") {
        return Some(rc.into());
    }
    let kits = std::path::Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<_> = std::fs::read_dir(kits).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    versions.sort();
    versions.into_iter().rev().map(|v| v.join(r"x64\rc.exe")).find(|rc| rc.exists())
}

pub fn embed_version_info(targets: &[VersionTarget]) {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Some(rc) = find_rc() else {
        println!("cargo:warning=no rc.exe (Windows SDK) found: building without version info");
        return;
    };
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let v = |k: &str| std::env::var(k).unwrap();
    let (major, minor, patch) = (v("CARGO_PKG_VERSION_MAJOR"), v("CARGO_PKG_VERSION_MINOR"), v("CARGO_PKG_VERSION_PATCH"));
    let dotted = format!("{major}.{minor}.{patch}.0");
    let commas = format!("{major},{minor},{patch},0");
    for t in targets {
        let stem = t.file.replace('.', "_");
        let mut rcs = String::new();
        if t.program {
            let manifest = out.join(format!("{stem}.manifest"));
            std::fs::write(&manifest, MANIFEST).unwrap();
            rcs += &format!("1 24 \"{}\"\n", manifest.display().to_string().replace('\\', "\\\\"));
        }
        rcs += &format!(
            r#"1 VERSIONINFO
FILEVERSION {commas}
PRODUCTVERSION {commas}
FILEOS 0x40004
FILETYPE {filetype}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Elden Force"
      VALUE "FileDescription", "{desc}"
      VALUE "FileVersion", "{dotted}"
      VALUE "InternalName", "{file}"
      VALUE "OriginalFilename", "{file}"
      VALUE "ProductName", "Elden Force"
      VALUE "ProductVersion", "{dotted}"
      VALUE "LegalCopyright", "MIT License"
      VALUE "Comments", "Open source: https://github.com/ashersheets-jpg/elden-force-source"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
            filetype = if t.program { "0x1" } else { "0x2" },
            desc = t.description,
            file = t.file,
        );
        let rc_path = out.join(format!("{stem}.rc"));
        let res_path = out.join(format!("{stem}.res"));
        std::fs::write(&rc_path, rcs).unwrap();
        let ok = std::process::Command::new(&rc)
            .args(["/nologo", "/fo"])
            .arg(&res_path)
            .arg(&rc_path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            println!("cargo:warning=rc.exe failed for {}: building it without version info", t.file);
            continue;
        }
        println!("cargo:rustc-link-arg-{}={}", t.link, res_path.display());
    }
    println!("cargo:rerun-if-changed=../version_info.rs");
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>
"#;
