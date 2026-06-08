use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
    process::{self, Command},
};

const EXAMPLE_PLUGINS: &[&str] = &[
    "snb_adapter_stdin",
    "snb_database_sqlite",
    "snb_plugin_example",
];
const SNB_MANIFEST: &str = "snb.toml";

#[derive(Clone)]
struct PluginBuild {
    name: String,
    manifest: PathBuf,
    kind: PluginBuildKind,
}

#[derive(Clone)]
enum PluginBuildKind {
    Cargo,
    SnbSource(SnbSourceBuild),
}

#[derive(Clone)]
struct SnbSourceBuild {
    plugin_dir: PathBuf,
    source: PathBuf,
    package_name: String,
    package_version: String,
    dependency_name: String,
    dependencies: toml::Table,
}

struct PackageInfo {
    name: String,
    version: String,
}

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
        let plugin = PluginBuild {
            name: (*plugin).to_string(),
            manifest,
            kind: PluginBuildKind::Cargo,
        };
        cargo_build_lib(root, &plugin, extra_args)?;
    }
    Ok(())
}

fn build_plugins(root: &Path, extra_args: &[OsString]) -> Result<(), String> {
    let example_plugins: HashSet<&str> = EXAMPLE_PLUGINS.iter().copied().collect();
    let plugins = discover_plugins(root)?
        .into_iter()
        .filter(|plugin| !example_plugins.contains(plugin.name.as_str()))
        .collect::<Vec<_>>();

    if plugins.is_empty() {
        println!("no non-example plugins found");
        return Ok(());
    }

    for plugin in plugins {
        cargo_build_lib(root, &plugin, extra_args)?;
    }
    Ok(())
}

fn build_all(root: &Path, extra_args: &[OsString]) -> Result<(), String> {
    cargo_build_root(root, extra_args)?;

    for plugin in discover_plugins(root)? {
        cargo_build_lib(root, &plugin, extra_args)?;
    }

    Ok(())
}

fn build_named_plugin(
    root: &Path,
    plugin: &OsString,
    extra_args: &[OsString],
) -> Result<(), String> {
    let plugin = resolve_plugin(root, plugin)?;
    cargo_build_lib(root, &plugin, extra_args)
}

fn list_plugins(root: &Path) -> Result<(), String> {
    let example_plugins: HashSet<&str> = EXAMPLE_PLUGINS.iter().copied().collect();
    for plugin in discover_plugins(root)? {
        let group = if example_plugins.contains(plugin.name.as_str()) {
            "example"
        } else {
            "plugin"
        };
        println!("{}\t{group}\t{}", plugin.name, plugin.manifest.display());
    }
    Ok(())
}

fn absolutize(root: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn plugin_in_dir(root: &Path, dir: &Path) -> Result<Option<PluginBuild>, String> {
    let name = plugin_dir_name(dir);
    let snb_manifest = dir.join(SNB_MANIFEST);
    if snb_manifest.is_file() {
        return parse_snb_plugin(root, dir, name, &snb_manifest).map(Some);
    }

    let manifest = dir.join("Cargo.toml");
    Ok(manifest.is_file().then_some(PluginBuild {
        name,
        manifest,
        kind: PluginBuildKind::Cargo,
    }))
}

fn parse_snb_plugin(
    root: &Path,
    plugin_dir: &Path,
    plugin_name: String,
    snb_manifest: &Path,
) -> Result<PluginBuild, String> {
    let content = fs::read_to_string(snb_manifest)
        .map_err(|error| format!("failed to read {}: {error}", snb_manifest.display()))?;
    let value = content
        .parse::<toml::Table>()
        .map_err(|error| format!("failed to parse {}: {error}", snb_manifest.display()))?;
    let build = value
        .get("build")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{} must contain [build]", snb_manifest.display()))?;

    if let Some(manifest) = build.get("manifest").and_then(toml::Value::as_str) {
        let manifest = plugin_relative_path(plugin_dir, snb_manifest, "build.manifest", manifest)?;
        if !manifest.is_file() {
            return Err(format!(
                "{} points to missing manifest: {}",
                snb_manifest.display(),
                manifest.display()
            ));
        }
        return Ok(PluginBuild {
            name: plugin_name,
            manifest,
            kind: PluginBuildKind::Cargo,
        });
    }

    let source = build
        .get("source")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            format!(
                "{} [build] must contain either manifest or source",
                snb_manifest.display()
            )
        })?;
    let source = plugin_relative_path(plugin_dir, snb_manifest, "build.source", source)?;
    if !source.is_file() {
        return Err(format!(
            "{} points to missing source: {}",
            snb_manifest.display(),
            source.display()
        ));
    }

    let package_info = cargo_package_info(&plugin_dir.join("Cargo.toml"))?;
    let package_name = build
        .get("package")
        .or_else(|| build.get("name"))
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("snb_{}", package_info.name.replace('-', "_")));
    let package_version = build
        .get("version")
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or(package_info.version);
    let dependencies = value
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .cloned()
        .unwrap_or_default();

    Ok(PluginBuild {
        manifest: generated_snb_manifest(root, &plugin_name),
        name: plugin_name,
        kind: PluginBuildKind::SnbSource(SnbSourceBuild {
            plugin_dir: plugin_dir.to_path_buf(),
            source,
            package_name,
            package_version,
            dependency_name: package_info.name,
            dependencies,
        }),
    })
}

fn plugin_relative_path(
    plugin_dir: &Path,
    snb_manifest: &Path,
    field: &str,
    value: &str,
) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!(
            "{} [{field}] must be relative to the plugin directory",
            snb_manifest.display()
        ));
    }
    Ok(plugin_dir.join(path))
}

fn cargo_package_info(manifest: &Path) -> Result<PackageInfo, String> {
    let content = fs::read_to_string(manifest)
        .map_err(|error| format!("failed to read {}: {error}", manifest.display()))?;
    let value = content
        .parse::<toml::Table>()
        .map_err(|error| format!("failed to parse {}: {error}", manifest.display()))?;
    let package = value
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{} must contain [package]", manifest.display()))?;
    let name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("{} [package].name is required", manifest.display()))?
        .to_string();
    let version = package
        .get("version")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("{} [package].version is required", manifest.display()))?
        .to_string();
    Ok(PackageInfo { name, version })
}

fn plugin_dir_name(dir: &Path) -> String {
    dir.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unknown>")
        .to_string()
}

fn generated_snb_manifest(root: &Path, plugin_name: &str) -> PathBuf {
    root.join("target")
        .join("xtask-snb")
        .join(plugin_name)
        .join("Cargo.toml")
}

fn manifest_plugin_name(root: &Path, manifest: &Path) -> String {
    let manifest = absolutize(root, manifest.to_path_buf());
    if let Ok(relative) = manifest.strip_prefix(root.join("plugins"))
        && let Some(Component::Normal(name)) = relative.components().next()
        && let Some(name) = name.to_str()
    {
        return name.to_string();
    }

    manifest
        .parent()
        .unwrap_or(manifest.as_path())
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unknown>")
        .to_string()
}

fn resolve_plugin(root: &Path, plugin: &OsString) -> Result<PluginBuild, String> {
    let plugin_path = PathBuf::from(plugin);
    if plugin_path.ends_with("Cargo.toml") {
        let manifest = absolutize(root, plugin_path);
        if manifest.is_file() {
            return Ok(PluginBuild {
                name: manifest_plugin_name(root, &manifest),
                manifest,
                kind: PluginBuildKind::Cargo,
            });
        }
        return Err(format!("plugin manifest not found: {}", manifest.display()));
    }

    let plugin_dir = if plugin_path.components().count() > 1 {
        absolutize(root, plugin_path)
    } else {
        root.join("plugins").join(&plugin_path)
    };

    if let Some(plugin) = plugin_in_dir(root, &plugin_dir)? {
        return Ok(plugin);
    }

    let Some(plugin_name) = plugin.to_str() else {
        return Err(format!(
            "plugin manifest not found: {}",
            plugin_dir.join("Cargo.toml").display()
        ));
    };

    let matches = discover_plugins(root)?
        .into_iter()
        .filter(|candidate| plugin_name_matches(&candidate.name, plugin_name))
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [plugin] => Ok(plugin.clone()),
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

fn discover_plugins(root: &Path) -> Result<Vec<PluginBuild>, String> {
    let plugins_dir = root.join("plugins");
    let entries = fs::read_dir(&plugins_dir)
        .map_err(|error| format!("failed to read {}: {error}", plugins_dir.display()))?;

    let mut plugins = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read plugin entry: {error}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        if let Some(plugin) = plugin_in_dir(root, &path)? {
            plugins.push(plugin);
        }
    }

    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(plugins)
}

fn cargo_build_lib(
    root: &Path,
    plugin: &PluginBuild,
    extra_args: &[OsString],
) -> Result<(), String> {
    cargo_build_plugin(root, plugin, extra_args, true)
}

fn cargo_build_plugin(
    root: &Path,
    plugin: &PluginBuild,
    extra_args: &[OsString],
    lib_only: bool,
) -> Result<(), String> {
    let manifest = prepare_plugin_manifest(root, plugin)?;

    println!("building plugin {}", plugin.name);

    let status = Command::new(cargo_bin())
        .current_dir(root)
        .arg("build")
        .arg("--manifest-path")
        .arg(&manifest)
        .args(lib_only_args(lib_only, extra_args))
        .args(default_target_dir_args(root, extra_args))
        .args(extra_args)
        .status()
        .map_err(|error| format!("failed to run cargo build for {}: {error}", plugin.name))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build failed for {}: {status}", plugin.name))
    }
}

fn prepare_plugin_manifest(root: &Path, plugin: &PluginBuild) -> Result<PathBuf, String> {
    match &plugin.kind {
        PluginBuildKind::Cargo => Ok(plugin.manifest.clone()),
        PluginBuildKind::SnbSource(build) => write_generated_snb_manifest(root, plugin, build),
    }
}

fn write_generated_snb_manifest(
    root: &Path,
    plugin: &PluginBuild,
    build: &SnbSourceBuild,
) -> Result<PathBuf, String> {
    let parent = plugin.manifest.parent().ok_or_else(|| {
        format!(
            "invalid generated manifest path: {}",
            plugin.manifest.display()
        )
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;

    let base_dependencies = [build.dependency_name.as_str(), "snb_core", "snb_macros"];
    for dep in base_dependencies {
        if build.dependencies.contains_key(dep) {
            return Err(format!(
                "{} snb.toml [dependencies] must not override generated dependency `{dep}`",
                build.plugin_dir.display()
            ));
        }
    }

    let mut content = String::new();
    content.push_str("[package]\n");
    content.push_str(&format!("name = {}\n", toml_string(&build.package_name)));
    content.push_str(&format!(
        "version = {}\n",
        toml_string(&build.package_version)
    ));
    content.push_str("edition = \"2024\"\n\n");
    content.push_str("[workspace]\n\n");
    content.push_str("[lib]\n");
    content.push_str(&format!("path = {}\n", toml_path(&build.source)));
    content.push_str("crate-type = [\"cdylib\"]\n\n");
    content.push_str("[dependencies]\n");
    content.push_str(&format!(
        "{} = {{ path = {} }}\n",
        toml_key(&build.dependency_name),
        toml_path(&build.plugin_dir)
    ));
    content.push_str(&format!(
        "snb_core = {{ path = {} }}\n",
        toml_path(&root.join("crates").join("snb_core"))
    ));
    content.push_str(&format!(
        "snb_macros = {{ path = {} }}\n",
        toml_path(&root.join("crates").join("snb_macros"))
    ));
    for (name, value) in &build.dependencies {
        let value = normalize_dependency_value(&build.plugin_dir, value.clone());
        content.push_str(&format!(
            "{} = {}\n",
            toml_key(name),
            toml_inline_value(&value)
        ));
    }

    fs::write(&plugin.manifest, content)
        .map_err(|error| format!("failed to write {}: {error}", plugin.manifest.display()))?;
    Ok(plugin.manifest.clone())
}

fn normalize_dependency_value(plugin_dir: &Path, mut value: toml::Value) -> toml::Value {
    if let toml::Value::Table(table) = &mut value
        && let Some(toml::Value::String(path)) = table.get_mut("path")
    {
        let path_value = PathBuf::from(path.as_str());
        if !path_value.is_absolute() {
            *path = plugin_dir.join(path_value).to_string_lossy().into_owned();
        }
    }
    value
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

fn toml_path(path: &Path) -> String {
    toml_string(path.to_string_lossy().as_ref())
}

fn toml_key(key: &str) -> String {
    if key
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        key.to_string()
    } else {
        toml_string(key)
    }
}

fn toml_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn toml_inline_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(value) => toml_string(value),
        toml::Value::Integer(value) => value.to_string(),
        toml::Value::Float(value) => value.to_string(),
        toml::Value::Boolean(value) => value.to_string(),
        toml::Value::Datetime(value) => value.to_string(),
        toml::Value::Array(values) => {
            let values = values
                .iter()
                .map(toml_inline_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{values}]")
        }
        toml::Value::Table(values) => {
            let values = values
                .iter()
                .map(|(key, value)| format!("{} = {}", toml_key(key), toml_inline_value(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {values} }}")
        }
    }
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
