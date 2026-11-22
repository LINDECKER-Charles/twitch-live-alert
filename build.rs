use std::{env, fs, io, path::PathBuf};

// Embed the dashboard modules so the packaged sidecar runs independently of its working directory.
fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=web");
    let mut names = Vec::new();
    for entry in fs::read_dir("web")? {
        let path = entry?.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "js") {
            names.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    names.sort();
    let mut source =
        String::from("fn embedded_scripts(name:&str)->Option<&'static str>{match name {\n");
    for name in names {
        let relative = format!("/web/{name}");
        source.push_str(&format!(
            "{name:?}=>Some(include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"),{relative:?}))),\n"
        ));
    }
    source.push_str("_=>None}}");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("scripts.rs"),
        source,
    )
}
