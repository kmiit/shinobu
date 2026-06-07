use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{self, Command},
};

const EXAMPLE_PLUGINS: &[&str] = &[
    "snb_adapter_stdin",
    "snb_database_sqlite",
    "snb_plugin_example",
];

fn main() {
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        print_usage_and_exit();
    };

    let extra_args: Vec<OsString> = args.collect();
    let root = workspace_root();

    let result = match command.to_string_lossy().as_ref() {
        "build-example" => build_example(&root, &extra_args),
        "build-plugins" => build_plugins(&root, &extra_args),
        "build-all" => build_all(&root, &extra_args),
        "list-plugins" => list_plugins(&root),
        "build-plugin" => {
            let Some(plugin) = extra_args.first() else {
                eprintln!("usage: cargo xtask build-plugin <plugin-dir> [cargo build args...]");
                process::exit(2);
            };
            build_named_plugin(&root, plugin, &extra_args[1..])
        }
        _ => {
            print_usage_and_exit();
        }
    };

    if let Err(error) = result {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask should live in crates/xtask")
        .to_path_buf()
}

fn build_example(root: &Path, extra_args: &[OsString]) -> Result<(), String> {
    for plugin in EXAMPLE_PLUGINS {
        let manifest = root.join("plugins").join(plugin).join("Cargo.toml");
        if !manifest.is_file() {
            return Err(format!(
                "required example plugin manifest not found: {}",
                manifest.display()
            ));
        }
        cargo_build_lib(root, &manifest, extra_args)?;
    }
    Ok(())
}

fn build_plugins(root: &Path, extra_args: &[OsString]) -> Result<(), String> {
    let example_plugins: HashSet<&str> = EXAMPLE_PLUGINS.iter().copied().collect();
    let manifests = discover_plugin_manifests(root)?
        .into_iter()
        .filter(|manifest| {
            manifest
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .is_some_and(|name| !example_plugins.contains(name))
        })
        .collect::<Vec<_>>();

    if manifests.is_empty() {
        println!("no non-example plugins found");
        return Ok(());
    }

    for manifest in manifests {
        cargo_build_lib(root, &manifest, extra_args)?;
    }
    Ok(())
}

fn build_all(root: &Path, extra_args: &[OsString]) -> Result<(), String> {
    cargo_build_root(root, extra_args)?;

    for manifest in discover_plugin_manifests(root)? {
        cargo_build_lib(root, &manifest, extra_args)?;
    }

    Ok(())
}

fn build_named_plugin(
    root: &Path,
    plugin: &OsString,
    extra_args: &[OsString],
) -> Result<(), String> {
    let manifest = resolve_plugin_manifest(root, plugin)?;
    cargo_build_lib(root, &manifest, extra_args)
}

fn list_plugins(root: &Path) -> Result<(), String> {
    let example_plugins: HashSet<&str> = EXAMPLE_PLUGINS.iter().copied().collect();
    for manifest in discover_plugin_manifests(root)? {
        let name = manifest
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("<unknown>");
        let group = if example_plugins.contains(name) {
            "example"
        } else {
            "plugin"
        };
        println!("{name}\t{group}\t{}", manifest.display());
    }
    Ok(())
}

fn resolve_plugin_manifest(root: &Path, plugin: &OsString) -> Result<PathBuf, String> {
    let plugin_path = PathBuf::from(plugin);
    let manifest = if plugin_path.ends_with("Cargo.toml") {
        plugin_path
    } else if plugin_path.components().count() > 1 {
        plugin_path.join("Cargo.toml")
    } else {
        root.join("plugins").join(&plugin_path).join("Cargo.toml")
    };
    let manifest = if manifest.is_absolute() {
        manifest
    } else {
        root.join(manifest)
    };

    if manifest.is_file() {
        return Ok(manifest);
    }

    let Some(plugin_name) = plugin.to_str() else {
        return Err(format!("plugin manifest not found: {}", manifest.display()));
    };

    let candidates = discover_plugin_manifests(root)?;
    let matches = candidates
        .into_iter()
        .filter(|manifest| {
            manifest
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .is_some_and(|dir_name| plugin_name_matches(dir_name, plugin_name))
        })
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [manifest] => Ok(manifest.clone()),
        [] => Err(format!("plugin manifest not found for '{plugin_name}'")),
        _ => Err(format!("plugin name '{plugin_name}' is ambiguous")),
    }
}

fn plugin_name_matches(dir_name: &str, requested: &str) -> bool {
    dir_name == requested
        || dir_name.strip_prefix("snb_adapter_") == Some(requested)
        || dir_name.strip_prefix("snb_database_") == Some(requested)
        || dir_name.strip_prefix("snb_plugin_") == Some(requested)
}

fn discover_plugin_manifests(root: &Path) -> Result<Vec<PathBuf>, String> {
    let plugins_dir = root.join("plugins");
    let entries = fs::read_dir(&plugins_dir)
        .map_err(|error| format!("failed to read {}: {error}", plugins_dir.display()))?;

    let mut manifests = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read plugin entry: {error}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let manifest = path.join("Cargo.toml");
        if manifest.is_file() {
            manifests.push(manifest);
        }
    }

    manifests.sort();
    Ok(manifests)
}

fn cargo_build_lib(root: &Path, manifest: &Path, extra_args: &[OsString]) -> Result<(), String> {
    cargo_build_plugin(root, manifest, extra_args, true)
}

fn cargo_build_plugin(
    root: &Path,
    manifest: &Path,
    extra_args: &[OsString],
    lib_only: bool,
) -> Result<(), String> {
    let plugin_name = manifest
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("<unknown>");

    println!("building plugin {plugin_name}");

    let status = Command::new(cargo_bin())
        .current_dir(root)
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest)
        .args(lib_only_args(lib_only, extra_args))
        .args(default_target_dir_args(root, extra_args))
        .args(extra_args)
        .status()
        .map_err(|error| format!("failed to run cargo build for {plugin_name}: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build failed for {plugin_name}: {status}"))
    }
}

fn cargo_build_root(root: &Path, extra_args: &[OsString]) -> Result<(), String> {
    println!("building main workspace");

    let status = Command::new(cargo_bin())
        .current_dir(root)
        .arg("build")
        .args(extra_args)
        .status()
        .map_err(|error| format!("failed to run cargo build for main workspace: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build failed for main workspace: {status}"))
    }
}

fn cargo_bin() -> OsString {
    env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn lib_only_args(lib_only: bool, extra_args: &[OsString]) -> Vec<OsString> {
    if lib_only && !has_target_selection_arg(extra_args) {
        vec![OsString::from("--lib")]
    } else {
        Vec::new()
    }
}

fn has_target_selection_arg(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        matches!(
            arg.to_string_lossy().as_ref(),
            "--lib"
                | "--bins"
                | "--examples"
                | "--tests"
                | "--benches"
                | "--all-targets"
                | "--bin"
                | "--example"
                | "--test"
                | "--bench"
        )
    })
}

fn default_target_dir_args(root: &Path, extra_args: &[OsString]) -> Vec<OsString> {
    if has_target_dir_arg(extra_args) {
        return Vec::new();
    }

    vec![
        OsString::from("--target-dir"),
        root.join("target").into_os_string(),
    ]
}

fn has_target_dir_arg(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        arg == "--target-dir" || arg.starts_with("--target-dir=")
    })
}

fn print_usage_and_exit() -> ! {
    eprintln!(
        "usage:
  cargo xtask build-example [cargo build args...]
  cargo xtask build-plugins [cargo build args...]
  cargo xtask build-all [cargo build args...]
  cargo xtask build-plugin <plugin-dir> [cargo build args...]
  cargo xtask list-plugins"
    );
    process::exit(2);
}
