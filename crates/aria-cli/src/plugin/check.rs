//! `aria plugin check` — validate manifest, declared shaders, permissions, and
//! native ABI or WASM WIT world without starting Aria.

use std::fs;
use std::path::Path;

use aria_plugin::package::{
    Access, COMPONENT_FILE, FsScope, LIBRARY_DIR, MANIFEST_FILE, Package, PackageManifest,
    Permissions, Tier, is_archive, library_name, register_declared, unpack,
};
use aria_plugin::{PluginContext, PluginError};
use object::{Architecture, BinaryFormat, Object as _, ObjectSymbol as _};

use crate::args::Args;
use crate::fail::Failure;

pub(crate) const FLAGS: &[&str] = &[];
/// The sections [`check_dir`] prints, in order, each at the start of its line.
/// The command-line reference test compares `docs/command-line.md` against it.
#[cfg(test)]
pub(crate) const CHECK_SECTIONS: [&str; 7] = [
    "manifest",
    "shaders",
    "textures",
    "presets",
    "contributes",
    "permissions",
    "abi",
];

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let path = args.required_path(1, "a plugin directory or .ariaplugin archive")?;
    if let Some(extra) = args.positional.get(2) {
        return Err(Failure::Usage(format!("unexpected argument \"{extra}\"")));
    }
    check_path(&path).map(drop)
}

/// Validate a plugin directory or `.ariaplugin` archive, printing one line per
/// check to stdout.
pub(crate) fn check_path(path: &Path) -> Result<Package, Failure> {
    if is_archive(path) {
        let scratch = tempfile::tempdir()
            .map_err(|err| Failure::Failed(format!("{}: {err}", path.display())))?;
        let unpacked_dir = scratch.path().join("package");
        unpack(path, &unpacked_dir).map_err(|err| Failure::Failed(err.to_string()))?;
        check_dir(&unpacked_dir)
    } else {
        check_dir(path)
    }
}

/// Validate an unpacked plugin directory and return the opened [`Package`].
pub(crate) fn check_dir(dir: &Path) -> Result<Package, Failure> {
    // If this is a source package that has not been built yet, validate the
    // manifest syntax first and then tell the developer how to build it.
    if is_unbuilt_source_dir(dir) {
        let manifest_path = dir.join(MANIFEST_FILE);
        let text = fs::read_to_string(&manifest_path)
            .map_err(|err| Failure::Failed(format!("{}: {err}", manifest_path.display())))?;
        PackageManifest::parse(&text)
            .map_err(|reason| Failure::Failed(format!("{}: {reason}", manifest_path.display())))?;
        return Err(Failure::Failed(format!(
            "{}: source package has not been built (Cargo.toml present without {} or {}/); run `aria plugin pack {}` or `aria plugin dev {}`",
            dir.display(),
            COMPONENT_FILE,
            LIBRARY_DIR,
            dir.display(),
            dir.display()
        )));
    }

    let package = Package::open(dir).map_err(|err| Failure::Failed(err.to_string()))?;
    println!(
        "manifest     {} {} ({}, api {})",
        package.manifest.id, package.manifest.version, package.manifest.name, package.manifest.api
    );

    check_shaders(&package)?;
    print_presets(&package);
    print_contributions(&package);
    print_permissions(&package.manifest.permissions);
    check_code(&package)?;

    Ok(package)
}

fn is_unbuilt_source_dir(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file()
        && !dir.join(COMPONENT_FILE).is_file()
        && !dir.join(LIBRARY_DIR).is_dir()
}

fn check_shaders(package: &Package) -> Result<(), Failure> {
    let ctx = PluginContext::new();
    let _held = register_declared(&ctx, package)
        .map_err(|err| Failure::Failed(format_shader_error(package, err)))?;
    let effects = package.manifest.effects.len();
    let generators = package.manifest.generators.len();
    let transitions = package.manifest.transitions.len();
    let textures = package.manifest.textures.len();
    if effects == 0 && generators == 0 && transitions == 0 {
        println!("shaders      none");
    } else {
        println!(
            "shaders      {effects} effect(s), {generators} generator(s), {transitions} transition(s)"
        );
    }
    if textures > 0 {
        println!("textures     {textures} texture(s)");
    }
    Ok(())
}

pub(crate) fn format_presets_line(package: &Package) -> String {
    if package.presets.is_empty() {
        "presets      none".to_owned()
    } else {
        let names: Vec<&str> = package
            .presets
            .iter()
            .map(|preset| preset.name.as_str())
            .collect();
        format!(
            "presets      {} preset(s) ({})",
            package.presets.len(),
            names.join(", ")
        )
    }
}

fn print_presets(package: &Package) {
    println!("{}", format_presets_line(package));
}

fn print_contributions(package: &Package) {
    let manifest = &package.manifest;
    println!(
        "contributes  {} command(s), {} menu row(s), {} key(s), {} setting(s), {} preset(s), {} theme(s)",
        manifest.commands.len(),
        manifest.menus.len(),
        manifest.keys.len(),
        manifest.settings.len(),
        package.presets.len(),
        package.themes.len()
    );
}

fn format_shader_error(package: &Package, err: PluginError) -> String {
    match err {
        PluginError::RegisterRefused(refusal) => {
            let pass_index = refusal.pass.unwrap_or(0);
            let shader_file = find_shader_file(&package.manifest, &refusal.id, pass_index);
            match (shader_file, refusal.location) {
                (Some(file), Some((line, column))) => format!(
                    "{}: {}:{line}:{column}: {}",
                    refusal.id,
                    file.display(),
                    refusal.message
                ),
                (Some(file), None) => {
                    format!("{}: {}: {}", refusal.id, file.display(), refusal.message)
                }
                (None, Some((line, column))) => {
                    format!("{}:{line}:{column}: {}", refusal.id, refusal.message)
                }
                (None, None) => format!("{}: {}", refusal.id, refusal.message),
            }
        }
        PluginError::MountFailed(_, detail) => detail,
        other => other.to_string(),
    }
}

fn find_shader_file<'a>(
    manifest: &'a PackageManifest,
    id: &str,
    pass_index: usize,
) -> Option<&'a Path> {
    for effect in manifest.effects.iter().chain(manifest.generators.iter()) {
        if effect.id == id {
            return effect
                .passes
                .get(pass_index)
                .or_else(|| effect.passes.first())
                .map(|pass| pass.file.as_path());
        }
    }
    for transition in &manifest.transitions {
        if transition.id == id {
            return Some(transition.shader.as_path());
        }
    }
    None
}

fn print_permissions(p: &Permissions) {
    println!(
        "permissions  project={}, media={}, gpu={}, net={}, fs={}, process={}, clipboard={}",
        access_label(p.project),
        access_label(p.media),
        p.gpu,
        list_label(&p.net),
        fs_label(&p.fs),
        list_label(&p.process),
        p.clipboard,
    );
}

fn access_label(access: Access) -> &'static str {
    match access {
        Access::None => "none",
        Access::Read => "read",
        Access::ReadWrite => "read-write",
    }
}

fn list_label(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_owned()
    } else {
        format!("[{}]", items.join(", "))
    }
}

fn fs_label(scopes: &[FsScope]) -> String {
    if scopes.is_empty() {
        return "none".to_owned();
    }
    let parts: Vec<String> = scopes
        .iter()
        .map(|scope| match scope {
            FsScope::Project => "project".to_owned(),
            FsScope::Pick => "pick".to_owned(),
            FsScope::Path(path) => path.display().to_string(),
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

fn check_code(package: &Package) -> Result<(), Failure> {
    match &package.tier {
        Tier::Data => {
            println!("abi          data (no code)");
            Ok(())
        }
        Tier::Wasm { .. } => {
            let _plugin = aria_plugin_wasm::load(package, &aria_plugin_wasm::Limits::default())
                .map_err(|err| Failure::Failed(err.to_string()))?;
            println!("abi          wasm {}", aria_plugin_wasm::WORLD);
            Ok(())
        }
        Tier::Native { library } => {
            let lib_dir = package.dir.join(LIBRARY_DIR);
            let checked = inspect_lib_directory(&lib_dir)?;
            let _plugin = aria_plugin::external::load(package)
                .map_err(|reason| Failure::Failed(format!("{}: {reason}", library.display())))?;
            println!(
                "abi          native v{} (AriaPluginV1 {} bytes, {})",
                aria_plugin::external::ABI_VERSION,
                std::mem::size_of::<aria_plugin_abi::AriaPluginV1>(),
                checked.join(", ")
            );
            println!(
                "warning      native code runs unsandboxed and can reach the system beyond declared permissions"
            );
            Ok(())
        }
    }
}

/// Inspect every file in `<dir>/lib/` with `object`, checking that its binary
/// format and architecture match its `<os>-<arch>.<ext>` name and that it
/// exports `aria_plugin_entry`.
fn inspect_lib_directory(lib_dir: &Path) -> Result<Vec<String>, Failure> {
    let entries = fs::read_dir(lib_dir)
        .map_err(|err| Failure::Failed(format!("{}: {err}", lib_dir.display())))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|err| Failure::Failed(format!("{}: {err}", lib_dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        files.push((name, entry.path()));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut checked = Vec::with_capacity(files.len());
    for (file_name, path) in files {
        inspect_library_file(&path, &file_name)?;
        checked.push(format!("{LIBRARY_DIR}/{file_name}"));
    }
    if checked.is_empty() {
        return Err(Failure::Failed(format!(
            "{}: hold no libraries; this platform loads {}/{}",
            lib_dir.display(),
            LIBRARY_DIR,
            library_name()
        )));
    }
    Ok(checked)
}

fn inspect_library_file(path: &Path, file_name: &str) -> Result<(), Failure> {
    let meta = fs::symlink_metadata(path)
        .map_err(|err| Failure::Failed(format!("{}: {err}", path.display())))?;
    if !meta.is_file() {
        return Err(Failure::Failed(format!(
            "{}: expected a shared library file in {LIBRARY_DIR}/",
            path.display()
        )));
    }

    let (expected_os, expected_arch_name, expected_format, expected_arch) =
        parse_library_filename(path, file_name)?;

    let bytes =
        fs::read(path).map_err(|err| Failure::Failed(format!("{}: {err}", path.display())))?;
    let parsed = object::File::parse(&*bytes).map_err(|err| {
        Failure::Failed(format!(
            "{}: invalid {expected_os}-{expected_arch_name} shared library: {err}",
            path.display()
        ))
    })?;

    if parsed.format() != expected_format {
        return Err(Failure::Failed(format!(
            "{}: file name declares {expected_os} ({}), but binary format is {}",
            path.display(),
            format_label(expected_format),
            format_label(parsed.format())
        )));
    }

    if parsed.architecture() != expected_arch {
        return Err(Failure::Failed(format!(
            "{}: file name declares architecture {expected_arch_name} ({expected_arch:?}), but binary architecture is {:?}",
            path.display(),
            parsed.architecture()
        )));
    }

    if !exports_plugin_entry(&parsed) {
        return Err(Failure::Failed(format!(
            "{}: does not export aria_plugin_entry",
            path.display()
        )));
    }

    Ok(())
}

fn parse_library_filename<'a>(
    path: &Path,
    file_name: &'a str,
) -> Result<(&'a str, &'a str, BinaryFormat, Architecture), Failure> {
    let Some((stem, ext)) = file_name.rsplit_once('.') else {
        return Err(Failure::Failed(format!(
            "{}: library name must be <os>-<arch>.<ext>, such as {}",
            path.display(),
            library_name()
        )));
    };
    let Some((os, arch)) = stem.split_once('-') else {
        return Err(Failure::Failed(format!(
            "{}: library name must be <os>-<arch>.<ext>, such as {}",
            path.display(),
            library_name()
        )));
    };

    let (expected_format, expected_ext) = match os {
        "linux" => (BinaryFormat::Elf, "so"),
        "macos" => (BinaryFormat::MachO, "dylib"),
        "windows" => (BinaryFormat::Pe, "dll"),
        _ => {
            return Err(Failure::Failed(format!(
                "{}: unsupported platform OS \"{os}\" in library file name",
                path.display()
            )));
        }
    };
    if ext != expected_ext {
        return Err(Failure::Failed(format!(
            "{}: {os} libraries must use .{expected_ext}, got .{ext}",
            path.display()
        )));
    }

    let expected_arch = match arch {
        "x86_64" => Architecture::X86_64,
        "aarch64" => Architecture::Aarch64,
        "x86" => Architecture::I386,
        "arm" => Architecture::Arm,
        "riscv64" => Architecture::Riscv64,
        _ => {
            return Err(Failure::Failed(format!(
                "{}: unsupported architecture \"{arch}\" in library file name",
                path.display()
            )));
        }
    };

    Ok((os, arch, expected_format, expected_arch))
}

fn format_label(format: BinaryFormat) -> &'static str {
    match format {
        BinaryFormat::Elf => "ELF",
        BinaryFormat::MachO => "Mach-O",
        BinaryFormat::Pe => "PE",
        BinaryFormat::Coff => "COFF",
        BinaryFormat::Wasm => "Wasm",
        _ => "unknown",
    }
}

fn exports_plugin_entry(file: &object::File<'_>) -> bool {
    let is_entry = |name: &[u8]| {
        name == aria_plugin_abi::ARIA_PLUGIN_ENTRY_SYMBOL
            || (file.format() == BinaryFormat::MachO && name == b"_aria_plugin_entry")
    };
    if let Ok(mut exports) = file.exports()
        && exports.any(|export| {
            matches!(
                export,
                Ok(exp) if matches!(exp.name(), object::read::NameOrOrdinal::Name(name) if is_entry(name))
            )
        })
    {
        return true;
    }
    file.dynamic_symbols()
        .chain(file.symbols())
        .any(|sym| sym.is_definition() && sym.is_global() && sym.name_bytes().is_ok_and(is_entry))
}
