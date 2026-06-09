use std::fs;
use std::path::{Path, PathBuf};

fn find_workspace_root(manifest_dir: &Path) -> PathBuf {
    let mut dir = manifest_dir.to_path_buf();
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            let contents = fs::read_to_string(&cargo_toml).expect("failed to read Cargo.toml");
            let contents = contents.replace("\r\n", "\n");
            let doc: toml::Value = toml::from_str(&contents).expect("failed to parse Cargo.toml");
            if doc.get("workspace").is_some() {
                return dir;
            }
        }
        assert!(dir.pop(), "workspace root not found")
    }
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_dir = find_workspace_root(&manifest_dir);
    let cargo_toml = workspace_dir.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", cargo_toml.display());
    let contents = fs::read_to_string(&cargo_toml).expect("failed to read workspace Cargo.toml");
    let contents = contents.replace("\r\n", "\n");
    let doc: toml::Value = toml::from_str(&contents).expect("failed to parse workspace Cargo.toml");

    let abi_version = doc
        .get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get("snb"))
        .and_then(|s| s.get("abi_version"))
        .and_then(|v| v.as_str())
        .expect("[workspace.metadata.snb].abi_version is missing or invalid");

    println!("cargo:rustc-env=SNB_ABI_VERSION={abi_version}");
}
