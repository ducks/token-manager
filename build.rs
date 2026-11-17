// Embed the sorted provider directory. Adding a TOML adapter needs no Rust edit.
use std::{env, fs, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-changed=providers");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut paths: Vec<_> = fs::read_dir(root.join("providers"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|suffix| suffix == "toml"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "No provider definitions");
    let mut source = String::from("const BUILTIN_PROVIDERS: &[&str] = &[\n");
    for path in paths {
        assert!(
            path.is_file() && !path.is_symlink(),
            "Provider definitions must be regular files"
        );
        source.push_str(&format!("include_str!({:?}),\n", path.to_str().unwrap()));
    }
    source.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("providers.rs"),
        source,
    )
    .unwrap();
}
