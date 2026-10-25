//! Lab tool: ccw_survey <ccw folder> <out folder> <game folder> (see src/survey.rs), or
//! ccw_survey dump <archive path> <out file> <game folder>: one file from the game's archives, decompressed.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let r = if a[1] == "reg" { jumpring::ccw_reg(a[2].as_ref(), a[3].as_ref()) } else if a[1] == "dcx" { jumpring::dcx_file(a[2].as_ref(), a[3].as_ref(), a[4].clone().into()) } else if a[1] == "dumpmany" { jumpring::ccw_dump_many(a[2].as_ref(), a[3].as_ref(), a[4].clone().into()) } else if a[1] == "dump" { jumpring::ccw_dump(&a[2], a[3].as_ref(), a[4].clone().into()) } else { jumpring::ccw_survey(a[1].as_ref(), a[2].as_ref(), a[3].clone().into()) };
    if let Err(e) = r {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
