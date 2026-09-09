use {
    grafdag_core::Document,
    schemask_core::Maskoidy,
    std::{
        env,
        fs,
        path::PathBuf,
    },
};

/// Write the spec for the document json format (`schemask.json` at the repo root),
/// which the readme points at, so it can't drift from the types. Only done in a git
/// checkout - elsewhere (an unpacked source archive) there's nothing to keep up to
/// date.
fn write_schemask() {
    println!("cargo:rerun-if-changed=../grafdag_core/src");
    let root =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("../../..")
            .canonicalize()
            .unwrap();
    if !root.join(".git").exists() {
        return;
    }
    let mut out = serde_json::to_string_pretty(&Document::schemask()).unwrap();
    out.push('\n');
    let dest = root.join("schemask.json");
    if fs::read_to_string(&dest).ok().as_deref() == Some(&out) {
        return;
    }
    fs::write(&dest, out).unwrap();
}

fn main() {
    write_schemask();
    tauri_build::build();
}
