//! Regression and end-to-end tests for `aria plugin`.
#![allow(clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use aria_plugin::package::{
    COMPONENT_FILE, LIBRARY_DIR, MANIFEST_FILE, Package, is_archive, library_name,
    register_declared, unpack,
};
use aria_plugin::{ContributionKind, PluginContext};
use aria_plugin_wasm::Limits;
use object::write::{Object as WriteObject, StandardSection, Symbol, SymbolSection};
use object::{Architecture, BinaryFormat, Endianness, SymbolFlags, SymbolKind, SymbolScope};

use super::dev::snapshot_sources;
use super::new::{NewTier, create_package, resolve_sdk_dependency_with};
use super::{Subcommand, run};
use crate::args::{Args, USAGE};
use crate::fail::Failure;

fn parse_and_run<I, S>(argv: I) -> Result<(), Failure>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args = Args::parse(argv)?;
    run(&args)
}

fn collect_relative_files(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let entries = fs::read_dir(dir).expect("read_dir");
        for entry in entries {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .expect("prefix")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
            }
        }
    }
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort();
    files
}

fn build_elf_or_macho(
    format: BinaryFormat,
    arch: Architecture,
    with_entry_symbol: bool,
) -> Vec<u8> {
    let mut obj = WriteObject::new(format, arch, Endianness::Little);
    let text = obj.section_id(StandardSection::Text);
    let offset = obj.append_section_data(text, &[0xc3], 1);
    if with_entry_symbol {
        let name = if format == BinaryFormat::MachO {
            b"_aria_plugin_entry".to_vec()
        } else {
            b"aria_plugin_entry".to_vec()
        };
        obj.add_symbol(Symbol {
            name,
            value: offset,
            size: 1,
            kind: SymbolKind::Text,
            scope: SymbolScope::Dynamic,
            weak: false,
            section: SymbolSection::Section(text),
            flags: SymbolFlags::None,
        });
    }
    obj.write().expect("serialize object")
}

/// Assemble a minimal valid 64-bit PE DLL header that `object::File::parse`
/// recognizes as `BinaryFormat::Pe`.
fn build_minimal_pe64(arch: Architecture) -> Vec<u8> {
    let machine: u16 = match arch {
        Architecture::X86_64 => 0x8664,
        Architecture::Aarch64 => 0xaa64,
        _ => panic!("unsupported test PE arch: {arch:?}"),
    };
    let mut bytes = vec![0u8; 64 + 4 + 20 + 240];
    // DOS header: "MZ" and e_lfanew = 64 at 0x3c.
    bytes[0] = b'M';
    bytes[1] = b'Z';
    bytes[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
    // NT signature: "PE\0\0" at 64.
    bytes[64..68].copy_from_slice(b"PE\0\0");
    // IMAGE_FILE_HEADER (20 bytes at 68..88):
    // machine at 68..70, size_of_optional_header = 240 at 84..86, characteristics = 0x2002 at 86..88.
    bytes[68..70].copy_from_slice(&machine.to_le_bytes());
    bytes[84..86].copy_from_slice(&240u16.to_le_bytes());
    bytes[86..88].copy_from_slice(&0x2002u16.to_le_bytes());
    // IMAGE_OPTIONAL_HEADER64 (240 bytes at 88..328):
    // magic = 0x020b (PE32+) at 88..90, number_of_rva_and_sizes = 16 at 196..200.
    bytes[88..90].copy_from_slice(&0x020bu16.to_le_bytes());
    bytes[196..200].copy_from_slice(&16u32.to_le_bytes());
    bytes
}

/// Closes the defect class where a missing or misspelled `aria plugin`
/// sub-command or flag is silently ignored or exits with code 1 instead of
/// reporting a usage error (exit 2) naming the valid sub-commands or flag.
#[test]
fn subcommands_and_unknown_flags_are_validated_with_exact_messages() {
    for sub in Subcommand::ALL {
        let key = sub.key();
        assert_eq!(Subcommand::parse(key), Some(sub));
        assert!(
            USAGE.contains(&format!("    plugin {key} ")),
            "USAGE must document `plugin {key}`"
        );
    }

    let missing = parse_and_run(["plugin"]).expect_err("missing subcommand must fail");
    assert_eq!(missing.code(), 2);
    assert_eq!(
        missing,
        Failure::Usage("a plugin sub-command is required (new, dev, check, pack)".into())
    );

    let unknown = parse_and_run(["plugin", "publish"]).expect_err("unknown subcommand must fail");
    assert_eq!(unknown.code(), 2);
    assert_eq!(
        unknown,
        Failure::Usage(
            "unknown plugin sub-command \"publish\" (expected new, dev, check, or pack)".into()
        )
    );

    for sub in ["new", "dev", "check", "pack"] {
        let err = parse_and_run(["plugin", sub, "target", "--bogus", "1"])
            .expect_err("unknown flag must fail");
        assert_eq!(err.code(), 2);
        assert_eq!(
            err,
            Failure::Usage("plugin does not take --bogus".into()),
            "subcommand {sub}"
        );
    }

    let bad_tier = parse_and_run(["plugin", "new", "com.example.demo", "--tier", "python"])
        .expect_err("invalid tier must fail");
    assert_eq!(bad_tier.code(), 2);
    assert_eq!(
        bad_tier,
        Failure::Usage("--tier wants wasm or native, got \"python\"".into())
    );

    let bad_no_build = parse_and_run(["plugin", "pack", "some-dir", "--no-build=yes"])
        .expect_err("--no-build with value must fail");
    assert_eq!(bad_no_build.code(), 2);
    assert_eq!(
        bad_no_build,
        Failure::Usage("--no-build takes no value, got \"yes\"".into())
    );

    let missing_id = parse_and_run(["plugin", "new"]).expect_err("missing id");
    assert_eq!(missing_id, Failure::Usage("a plugin id is required".into()));

    let missing_check = parse_and_run(["plugin", "check"]).expect_err("missing check path");
    assert_eq!(
        missing_check,
        Failure::Usage("a plugin directory or .ariaplugin archive is required".into())
    );

    let extra_arg =
        parse_and_run(["plugin", "check", "a", "b"]).expect_err("extra positional argument");
    assert_eq!(
        extra_arg,
        Failure::Usage("unexpected argument \"b\"".into())
    );
}

/// Closes the defect class where `aria plugin new` writes partial files before
/// validating the plugin id, overwrites an existing directory, or generates a
/// manifest or WGSL shader that the host refuses to open or compile.
#[test]
fn new_scaffolds_exact_files_for_both_tiers_and_refuses_bad_id_or_existing_dir() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let root = scratch.path();

    // 1. Default tier is wasm.
    parse_and_run([
        "plugin",
        "new",
        "com.example.color-tools",
        "--dir",
        &root.to_string_lossy(),
    ])
    .expect("new wasm plugin");

    let wasm_dir = root.join("com.example.color-tools");
    assert_eq!(
        collect_relative_files(&wasm_dir),
        vec![
            ".gitignore",
            "Cargo.toml",
            "plugin.toml",
            "shaders/color-tools.wgsl",
            "src/lib.rs",
            "wit/plugin.wit",
        ]
    );
    assert_eq!(
        fs::read_to_string(wasm_dir.join(".gitignore")).expect("gitignore"),
        "target/\n*.ariaplugin\nplugin.wasm\n"
    );
    assert_eq!(
        fs::read_to_string(wasm_dir.join("wit/plugin.wit")).expect("plugin.wit"),
        include_str!("../../../aria-plugin-wasm/wit/plugin.wit")
    );
    let wasm_cargo = fs::read_to_string(wasm_dir.join("Cargo.toml")).expect("Cargo.toml");
    assert!(wasm_cargo.contains("wit-bindgen = \"0.51\""));
    assert!(wasm_cargo.contains("crate-type = [\"cdylib\"]"));
    assert!(wasm_cargo.contains("[workspace]"));

    // Place a placeholder component file so `Package::open` selects `Tier::Wasm`
    // and verify the generated manifest and WGSL shader register cleanly.
    fs::write(wasm_dir.join(COMPONENT_FILE), b"\0asm").expect("placeholder wasm");
    let wasm_pkg = Package::open(&wasm_dir).expect("generated wasm plugin.toml opens");
    assert_eq!(wasm_pkg.manifest.id, "com.example.color-tools");
    assert_eq!(wasm_pkg.manifest.name, "Color Tools");
    assert_eq!(wasm_pkg.manifest.version, "0.1.0");
    assert_eq!(wasm_pkg.manifest.api, 1);
    assert_eq!(wasm_pkg.manifest.effects.len(), 1);
    assert_eq!(wasm_pkg.manifest.commands.len(), 1);
    assert_eq!(
        wasm_pkg.manifest.commands[0].id,
        "com.example.color-tools.run"
    );
    let ctx = PluginContext::new();
    let held = register_declared(&ctx, &wasm_pkg).expect("generated shader compiles");
    let kinds: Vec<ContributionKind> = held
        .iter()
        .filter_map(|registration| registration.contribution().map(|held| held.kind))
        .collect();
    assert_eq!(
        kinds,
        [ContributionKind::Effect, ContributionKind::Menu],
        "the declared effect and the run command's Sequence menu row"
    );
    let spec = aria_models::effect_spec("com.example.color-tools.color-tools")
        .expect("declared effect registered in catalog");
    assert_eq!(spec.label, "Color Tools");
    assert_eq!(spec.params.len(), 1);
    assert_eq!(spec.params[0].name, "gain");
    drop(held);

    // 2. Native tier.
    let sdk_dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../aria-sdk"));
    parse_and_run([
        "plugin",
        "new",
        "com.example.native-tools",
        "--tier",
        "native",
        "--dir",
        &root.to_string_lossy(),
        "--sdk",
        &sdk_dir.to_string_lossy(),
    ])
    .expect("new native plugin");

    let native_dir = root.join("com.example.native-tools");
    assert_eq!(
        collect_relative_files(&native_dir),
        vec![
            ".gitignore",
            "Cargo.toml",
            "plugin.toml",
            "shaders/native-tools.wgsl",
            "src/lib.rs",
        ]
    );
    assert_eq!(
        fs::read_to_string(native_dir.join(".gitignore")).expect("gitignore"),
        "target/\n*.ariaplugin\nlib/\n"
    );
    let native_cargo = fs::read_to_string(native_dir.join("Cargo.toml")).expect("Cargo.toml");
    let canonical_sdk = sdk_dir.canonicalize().expect("canonical sdk");
    assert!(
        native_cargo.contains(&format!("path = {:?}", canonical_sdk.to_string_lossy())),
        "native Cargo.toml must reference canonicalized --sdk path: {native_cargo}"
    );

    // Verify all three SDK resolution branches: explicit --sdk, built-tree SDK, and git fallback.
    let via_built = resolve_sdk_dependency_with(None, sdk_dir).expect("built-tree sdk resolves");
    assert!(via_built.contains("path ="));
    let via_git =
        resolve_sdk_dependency_with(None, &root.join("absent-sdk")).expect("git fallback resolves");
    assert_eq!(
        via_git,
        "{ git = \"https://github.com/santhreal/aria\", branch = \"main\" }"
    );

    // 3. Refuses existing directory with exit code 1 and leaves existing files intact.
    fs::write(native_dir.join("sentinel.txt"), "keep").expect("sentinel");
    let exists_err = parse_and_run([
        "plugin",
        "new",
        "com.example.native-tools",
        "--tier",
        "native",
        "--dir",
        &root.to_string_lossy(),
    ])
    .expect_err("existing directory must be refused");
    assert_eq!(exists_err.code(), 1);
    assert_eq!(
        exists_err.message(),
        format!("{}: already exists", native_dir.display())
    );
    assert_eq!(
        fs::read_to_string(native_dir.join("sentinel.txt")).expect("sentinel intact"),
        "keep"
    );

    // 4. Refuses invalid plugin IDs through PackageManifest::parse before writing anything.
    for bad_id in [
        "singlelabel",
        "aria.shipped.only",
        "com.Example.Upper",
        "com.example.-bad",
    ] {
        let err = super::new::create_package(bad_id, NewTier::Wasm, root, None)
            .expect_err("invalid id must be refused");
        assert_eq!(err.code(), 1);
        assert!(
            !root.join(bad_id).exists(),
            "nothing may be written when id {bad_id:?} is refused"
        );
    }
}

/// Closes the defect class where `aria plugin check` misses shader compile
/// locations, accepts a wrong-OS or wrong-arch library in `lib/`, accepts a
/// library without `aria_plugin_entry`, or misdiagnoses an unbuilt source crate.
#[test]
fn check_reports_unbuilt_source_broken_shader_and_bad_native_objects() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let root = scratch.path();

    // 1. Unbuilt source directory tells the developer to run `aria plugin pack` or `aria plugin dev`.
    let unbuilt = super::new::create_package("com.example.unbuilt", NewTier::Wasm, root, None)
        .expect("create unbuilt");
    let unbuilt_err =
        parse_and_run(["plugin", "check", &unbuilt.to_string_lossy()]).expect_err("unbuilt");
    assert_eq!(unbuilt_err.code(), 1);
    assert!(
        unbuilt_err
            .message()
            .contains("source package has not been built")
            && unbuilt_err.message().contains("aria plugin pack")
            && unbuilt_err.message().contains("aria plugin dev"),
        "unexpected message: {}",
        unbuilt_err.message()
    );

    // 2. Broken shader reports effect id, WGSL file path, line, and column.
    let broken_dir = root.join("broken-shader");
    fs::create_dir_all(broken_dir.join("shaders")).expect("shaders dir");
    fs::write(
        broken_dir.join(MANIFEST_FILE),
        "[plugin]\nid = \"com.example.broken\"\nname = \"Broken\"\nversion = \"0.1.0\"\napi = 1\n\n\
         [[effects]]\nid = \"tint\"\nname = \"Tint\"\nshader = \"shaders/broken.wgsl\"\n",
    )
    .expect("manifest");
    fs::write(
        broken_dir.join("shaders/broken.wgsl"),
        "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    return unknown_ident;\n}\n",
    )
    .expect("broken wgsl");

    let shader_err = parse_and_run(["plugin", "check", &broken_dir.to_string_lossy()])
        .expect_err("broken shader must fail check");
    assert_eq!(shader_err.code(), 1);
    assert!(
        shader_err
            .message()
            .starts_with("com.example.broken.tint: shaders/broken.wgsl:3:12:"),
        "expected effect id, file, line, and column, got: {}",
        shader_err.message()
    );

    // 3. Native library object checks: wrong binary format (PE and Mach-O in linux-x86_64.so),
    // wrong architecture (aarch64 in linux-x86_64.so), and missing `aria_plugin_entry`.
    let native_dir = root.join("bad-native");
    let lib_dir = native_dir.join(LIBRARY_DIR);
    fs::create_dir_all(&lib_dir).expect("lib dir");
    fs::write(
        native_dir.join(MANIFEST_FILE),
        "[plugin]\nid = \"com.example.native\"\nname = \"Native\"\nversion = \"0.1.0\"\napi = 1\n",
    )
    .expect("manifest");

    let host_lib = lib_dir.join(library_name());

    // 3a. PE bytes inside `lib/<host_lib>` (e.g. `lib/linux-x86_64.so`).
    fs::write(&host_lib, build_minimal_pe64(Architecture::X86_64)).expect("write PE");
    let pe_err = parse_and_run(["plugin", "check", &native_dir.to_string_lossy()])
        .expect_err("PE disguised as host library must fail");
    assert_eq!(pe_err.code(), 1);
    assert_eq!(
        pe_err.message(),
        format!(
            "{}: file name declares {} (ELF), but binary format is PE",
            host_lib.display(),
            std::env::consts::OS
        )
    );

    // 3b. Mach-O bytes inside `lib/<host_lib>`.
    let macho_bytes = build_elf_or_macho(BinaryFormat::MachO, Architecture::X86_64, true);
    fs::write(&host_lib, &macho_bytes).expect("write Mach-O");
    let macho_err = parse_and_run(["plugin", "check", &native_dir.to_string_lossy()])
        .expect_err("Mach-O disguised as host library must fail");
    assert_eq!(macho_err.code(), 1);
    assert_eq!(
        macho_err.message(),
        format!(
            "{}: file name declares {} (ELF), but binary format is Mach-O",
            host_lib.display(),
            std::env::consts::OS
        )
    );

    // 3c. ELF with wrong architecture (`Aarch64` in `linux-x86_64.so`).
    let wrong_arch_bytes = build_elf_or_macho(BinaryFormat::Elf, Architecture::Aarch64, true);
    fs::write(&host_lib, &wrong_arch_bytes).expect("write wrong arch ELF");
    let arch_err = parse_and_run(["plugin", "check", &native_dir.to_string_lossy()])
        .expect_err("wrong architecture ELF must fail");
    assert_eq!(arch_err.code(), 1);
    assert_eq!(
        arch_err.message(),
        format!(
            "{}: file name declares architecture {} (X86_64), but binary architecture is Aarch64",
            host_lib.display(),
            std::env::consts::ARCH
        )
    );

    // 3d. ELF with matching format and architecture, but lacking `aria_plugin_entry`.
    let no_sym_bytes = build_elf_or_macho(BinaryFormat::Elf, Architecture::X86_64, false);
    fs::write(&host_lib, &no_sym_bytes).expect("write symbol-less ELF");
    let sym_err = parse_and_run(["plugin", "check", &native_dir.to_string_lossy()])
        .expect_err("ELF without aria_plugin_entry must fail");
    assert_eq!(sym_err.code(), 1);
    assert_eq!(
        sym_err.message(),
        format!("{}: does not export aria_plugin_entry", host_lib.display())
    );

    // 3e. Cross-platform library in `lib/macos-aarch64.dylib` is also checked with `object`
    // even when `lib/<host_lib>` has valid format, architecture, and symbol.
    let valid_elf_sym = build_elf_or_macho(BinaryFormat::Elf, Architecture::X86_64, true);
    fs::write(&host_lib, &valid_elf_sym).expect("write host ELF with symbol");
    let cross_lib = lib_dir.join("macos-aarch64.dylib");
    let macho_no_sym = build_elf_or_macho(BinaryFormat::MachO, Architecture::Aarch64, false);
    fs::write(&cross_lib, &macho_no_sym).expect("write macos-aarch64 without symbol");
    let cross_err = parse_and_run(["plugin", "check", &native_dir.to_string_lossy()])
        .expect_err("cross-platform library without aria_plugin_entry must fail");
    assert_eq!(cross_err.code(), 1);
    assert_eq!(
        cross_err.message(),
        format!("{}: does not export aria_plugin_entry", cross_lib.display())
    );
}

/// Closes the defect class where `aria plugin pack` includes Rust crate build
/// sources (`Cargo.toml`, `Cargo.lock`, `src/`, `wit/`, `target/`) inside the
/// distributed `.ariaplugin` archive.
#[test]
fn pack_excludes_crate_sources_and_check_validates_archive() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let pkg_dir = scratch.path().join("com.example.data-pack");
    fs::create_dir_all(pkg_dir.join("shaders")).expect("shaders");
    fs::create_dir_all(pkg_dir.join("src")).expect("src");
    fs::create_dir_all(pkg_dir.join("wit")).expect("wit");
    fs::create_dir_all(pkg_dir.join("target/debug")).expect("target");

    fs::write(
        pkg_dir.join(MANIFEST_FILE),
        "[plugin]\nid = \"com.example.data-pack\"\nname = \"Data Pack\"\nversion = \"0.2.0\"\napi = 1\n\n\
         [[effects]]\nid = \"gain\"\nname = \"Gain\"\nshader = \"shaders/gain.wgsl\"\n\n\
         [[effects.params]]\nname = \"gain\"\nlabel = \"Gain\"\ndefault = 1.0\nmin = 0.0\nmax = 2.0\n",
    )
    .expect("manifest");
    fs::write(
        pkg_dir.join("shaders/gain.wgsl"),
        include_str!("templates/effect.wgsl"),
    )
    .expect("shader");
    fs::write(pkg_dir.join("Cargo.toml"), "[package]\nname = \"dummy\"\n").expect("Cargo.toml");
    fs::write(pkg_dir.join("Cargo.lock"), "# lock\n").expect("Cargo.lock");
    fs::write(pkg_dir.join("src/lib.rs"), "// source\n").expect("src/lib.rs");
    fs::write(pkg_dir.join("wit/plugin.wit"), "// wit\n").expect("wit/plugin.wit");
    fs::write(pkg_dir.join("target/debug/blob"), "bin").expect("target blob");
    fs::write(pkg_dir.join(".gitignore"), "target/\n").expect(".gitignore");

    // With `Cargo.toml` present and no built artifact staged, `pack --no-build`
    // must fail through `check` rather than shipping an unbuilt source crate as
    // a data-only package.
    let unbuilt_err = parse_and_run(["plugin", "pack", &pkg_dir.to_string_lossy(), "--no-build"])
        .expect_err("pack --no-build on unbuilt crate must fail");
    assert_eq!(unbuilt_err.code(), 1);
    assert!(
        unbuilt_err
            .message()
            .contains("source package has not been built")
    );

    // Data-only package (no `Cargo.toml`, `src`, `wit`, `target`, `Cargo.lock`).
    fs::remove_file(pkg_dir.join("Cargo.toml")).expect("remove Cargo.toml");
    fs::remove_file(pkg_dir.join("Cargo.lock")).expect("remove Cargo.lock");
    fs::remove_dir_all(pkg_dir.join("src")).expect("remove src");
    fs::remove_dir_all(pkg_dir.join("wit")).expect("remove wit");
    fs::remove_dir_all(pkg_dir.join("target")).expect("remove target");

    parse_and_run(["plugin", "pack", &pkg_dir.to_string_lossy(), "--no-build"])
        .expect("pack --no-build on data package succeeds");
    let archive_path = pkg_dir.join("com.example.data-pack-0.2.0.ariaplugin");
    assert!(archive_path.is_file(), "default archive path written");
    assert!(is_archive(&archive_path));

    let file = fs::File::open(&archive_path).expect("open zip");
    let mut zip = zip::ZipArchive::new(file).expect("read zip");
    let mut names = BTreeSet::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i).expect("zip entry");
        names.insert(entry.name().expect("zip entry name").into_owned());
    }
    assert_eq!(
        names,
        BTreeSet::from(["plugin.toml".to_owned(), "shaders/gain.wgsl".to_owned()])
    );

    // `aria plugin check <archive.ariaplugin>` unpacks to a scratch dir, validates, and cleans up.
    parse_and_run(["plugin", "check", &archive_path.to_string_lossy()])
        .expect("check archive succeeds");

    // Package with `presets/*.ariapreset` prints presets in `aria plugin check`
    // and refuses an invalid `.ariapreset` naming the file.
    fs::create_dir_all(pkg_dir.join("presets")).expect("presets dir");
    let preset = aria_models::Preset::new(
        "Warm Vignette",
        vec![aria_models::EffectInstance::new("vignette")],
        Vec::new(),
        Vec::new(),
    );
    let preset_path = pkg_dir.join("presets/warm-vignette.ariapreset");
    fs::write(&preset_path, preset.to_json().expect("json")).expect("write preset");
    let checked_pkg = super::check::check_dir(&pkg_dir).expect("check package with preset");
    assert_eq!(checked_pkg.presets, vec![preset]);
    assert_eq!(
        super::check::format_presets_line(&checked_pkg),
        "presets      1 preset (Warm Vignette)"
    );
    let bad_preset_path = pkg_dir.join("presets/broken.ariapreset");
    fs::write(&bad_preset_path, b"not valid json").expect("write bad preset");
    let bad_preset_err =
        parse_and_run(["plugin", "check", &pkg_dir.to_string_lossy()]).expect_err("bad preset");
    assert_eq!(bad_preset_err.code(), 1);
    assert!(
        bad_preset_err.message().contains("broken.ariapreset"),
        "expected file path in error, got: {}",
        bad_preset_err.message()
    );
}

/// Closes the defect class where `aria plugin dev` fails to resolve `--editor`
/// or fails to propagate the editor child's exit code.
#[test]
fn dev_resolves_editor_binary_and_propagates_exit_status() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let pkg_dir = scratch.path().join("com.example.dev-data");
    fs::create_dir_all(&pkg_dir).expect("pkg_dir");
    fs::write(
        pkg_dir.join(MANIFEST_FILE),
        "[plugin]\nid = \"com.example.dev-data\"\nname = \"Dev Data\"\nversion = \"0.1.0\"\napi = 1\n",
    )
    .expect("manifest");

    // Missing sibling `aria-desktop` when `--editor` is omitted fails naming `--editor`.
    let err = super::dev::resolve_editor_binary(None, Some(&scratch.path().join("aria")))
        .expect_err("missing sibling aria-desktop must fail");
    assert_eq!(err.code(), 1);
    assert!(
        err.message().contains("--editor"),
        "expected --editor in message, got: {}",
        err.message()
    );

    // Nonexistent `--editor` fails naming `--editor`.
    let bad_editor = scratch.path().join("no-such-editor");
    let err = parse_and_run([
        "plugin",
        "dev",
        &pkg_dir.to_string_lossy(),
        "--editor",
        &bad_editor.to_string_lossy(),
    ])
    .expect_err("missing explicit editor must fail");
    assert_eq!(err.code(), 1);
    assert!(err.message().contains("--editor"));

    #[cfg(unix)]
    {
        let true_bin = Path::new("/bin/true");
        if true_bin.is_file() {
            let data_dir = scratch.path().join("profile-data");
            parse_and_run([
                "plugin",
                "dev",
                &pkg_dir.to_string_lossy(),
                "--editor",
                "/bin/true",
                "--data-dir",
                &data_dir.to_string_lossy(),
            ])
            .expect("dev with /bin/true exits 0");
            assert!(data_dir.is_dir(), "--data-dir created");
        }
        let false_bin = Path::new("/bin/false");
        if false_bin.is_file() {
            let err = parse_and_run([
                "plugin",
                "dev",
                &pkg_dir.to_string_lossy(),
                "--editor",
                "/bin/false",
            ])
            .expect_err("dev with /bin/false exits non-zero");
            assert_eq!(err.code(), 1);
        }
    }
}

/// Defect class: `aria plugin dev` watch loop triggering self-rebuilds when
/// staging `plugin.wasm`, `lib/<os>-<arch>.<ext>`, or `target/`, or missing a
/// change in `Cargo.toml`, `src/`, or `wit/`.
#[test]
fn dev_snapshot_watches_sources_and_ignores_staged_artifacts() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let pkg_dir = create_package(
        "com.example.watch-scope",
        NewTier::Wasm,
        scratch.path(),
        None,
    )
    .expect("create wasm package");

    let base = snapshot_sources(&pkg_dir);
    assert_eq!(
        base.len(),
        3,
        "watches Cargo.toml, src/lib.rs, wit/plugin.wit"
    );

    // Staging outputs or build artifacts must not change the source snapshot.
    fs::write(pkg_dir.join(COMPONENT_FILE), b"\0asm\x01\0\0\0").expect("write plugin.wasm");
    fs::create_dir_all(pkg_dir.join(LIBRARY_DIR)).expect("create lib/");
    fs::write(pkg_dir.join(LIBRARY_DIR).join(library_name()), b"lib").expect("write lib");
    fs::create_dir_all(pkg_dir.join("target")).expect("create target/");
    fs::write(pkg_dir.join("target/stamp"), b"build").expect("write target");
    assert_eq!(
        snapshot_sources(&pkg_dir),
        base,
        "staging artifacts must not retrigger the source watcher"
    );

    // Modifying `src/lib.rs`, `Cargo.toml`, or `wit/plugin.wit` must each advance the snapshot.
    let src_path = pkg_dir.join("src/lib.rs");
    let orig_src = fs::read_to_string(&src_path).expect("read src/lib.rs");
    fs::write(&src_path, format!("{orig_src}\n// edit\n")).expect("modify src/lib.rs");
    let after_src = snapshot_sources(&pkg_dir);
    assert_ne!(after_src, base, "src/lib.rs edit detected");

    let cargo_path = pkg_dir.join("Cargo.toml");
    let orig_cargo = fs::read_to_string(&cargo_path).expect("read Cargo.toml");
    fs::write(&cargo_path, format!("{orig_cargo}\n# comment\n")).expect("modify Cargo.toml");
    let after_cargo = snapshot_sources(&pkg_dir);
    assert_ne!(after_cargo, after_src, "Cargo.toml edit detected");

    let wit_path = pkg_dir.join("wit/plugin.wit");
    let orig_wit = fs::read_to_string(&wit_path).expect("read wit/plugin.wit");
    fs::write(&wit_path, format!("{orig_wit}\n// wit edit\n")).expect("modify wit/plugin.wit");
    let after_wit = snapshot_sources(&pkg_dir);
    assert_ne!(after_wit, after_cargo, "wit/plugin.wit edit detected");
}

/// End-to-end proof: `aria plugin new` -> `aria plugin pack` -> `aria plugin check`
/// for both `--tier native` and `--tier wasm`, followed by loading the packed
/// package into `PluginContext`, verifying the mounted sample command and
/// declared effect, and running the sample command to verify storage writes.
#[test]
fn end_to_end_new_pack_check_and_mount_for_native_and_wasm() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let root = scratch.path();
    let sdk_dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../aria-sdk"));

    // --- 1. Native tier end-to-end ---
    let native_id = "com.example.e2e-native";
    parse_and_run([
        "plugin",
        "new",
        native_id,
        "--tier",
        "native",
        "--dir",
        &root.to_string_lossy(),
        "--sdk",
        &sdk_dir.to_string_lossy(),
    ])
    .expect("new native");

    let native_dir = root.join(native_id);
    parse_and_run(["plugin", "pack", &native_dir.to_string_lossy()]).expect("pack native");

    let native_archive = native_dir.join("com.example.e2e-native-0.1.0.ariaplugin");
    assert!(native_archive.is_file(), "native archive created");
    parse_and_run(["plugin", "check", &native_archive.to_string_lossy()])
        .expect("check native archive");

    let unpacked_native_dir = root.join("unpacked-native");
    let unpacked_native = unpack(&native_archive, &unpacked_native_dir).expect("unpack native");
    assert!(!unpacked_native_dir.join("Cargo.toml").exists());
    assert!(!unpacked_native_dir.join("src").exists());

    let native_data_root = root.join("native-profile");
    let native_plugin =
        aria_plugin::external::load_with_data_root(&unpacked_native, &native_data_root)
            .expect("load packed native plugin");
    let native_ctx = PluginContext::new();
    native_ctx
        .mount(native_plugin.as_ref())
        .expect("mount packed native plugin");
    let commands = native_ctx.commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].id, "com.example.e2e-native.run");
    assert_eq!(commands[0].title, "Run E2e Native");
    assert!(
        aria_models::effect_spec("com.example.e2e-native.e2e-native").is_some(),
        "declared effect mounted from native package"
    );
    native_ctx
        .run_command("com.example.e2e-native.run")
        .expect("run native sample command");
    assert_eq!(
        fs::read_to_string(unpacked_native.data_dir(&native_data_root).join("last_run"))
            .expect("native command wrote storage"),
        "com.example.e2e-native.run"
    );
    native_ctx
        .unmount(native_id)
        .expect("unmount native plugin");

    // --- 2. WASM tier end-to-end ---
    let wasm_id = "com.example.e2e-wasm";
    parse_and_run([
        "plugin",
        "new",
        wasm_id,
        "--tier",
        "wasm",
        "--dir",
        &root.to_string_lossy(),
    ])
    .expect("new wasm");

    let wasm_dir = root.join(wasm_id);
    parse_and_run(["plugin", "pack", &wasm_dir.to_string_lossy()]).expect("pack wasm");

    let wasm_archive = wasm_dir.join("com.example.e2e-wasm-0.1.0.ariaplugin");
    assert!(wasm_archive.is_file(), "wasm archive created");
    parse_and_run(["plugin", "check", &wasm_archive.to_string_lossy()])
        .expect("check wasm archive");

    // Verify the archive excludes `Cargo.toml`, `Cargo.lock`, `src`, `wit`, `target`, and `.gitignore`,
    // and that `--no-build --out` repacks the already-staged crate cleanly.
    let custom_out = root.join("custom-out/e2e-wasm.ariaplugin");
    parse_and_run([
        "plugin",
        "pack",
        &wasm_dir.to_string_lossy(),
        "--no-build",
        "--out",
        &custom_out.to_string_lossy(),
    ])
    .expect("pack --no-build --out on staged wasm package");
    let wasm_zip_file = fs::File::open(&custom_out).expect("open wasm zip");
    let mut wasm_zip = zip::ZipArchive::new(wasm_zip_file).expect("read wasm zip");
    let mut wasm_entries = BTreeSet::new();
    for i in 0..wasm_zip.len() {
        let entry = wasm_zip.by_index(i).expect("wasm zip entry");
        wasm_entries.insert(entry.name().expect("wasm zip entry name").into_owned());
    }
    assert_eq!(
        wasm_entries,
        BTreeSet::from([
            "plugin.toml".to_owned(),
            "plugin.wasm".to_owned(),
            "shaders/e2e-wasm.wgsl".to_owned(),
        ])
    );

    let unpacked_wasm_dir = root.join("unpacked-wasm");
    let unpacked_wasm = unpack(&wasm_archive, &unpacked_wasm_dir).expect("unpack wasm");
    assert!(!unpacked_wasm_dir.join("Cargo.toml").exists());
    assert!(!unpacked_wasm_dir.join("src").exists());
    assert!(!unpacked_wasm_dir.join("wit").exists());

    let wasm_data_root = root.join("wasm-profile");
    let wasm_plugin =
        aria_plugin_wasm::load_with_data_root(&unpacked_wasm, &Limits::default(), &wasm_data_root)
            .expect("load packed wasm plugin");
    let wasm_ctx = PluginContext::new();
    wasm_ctx
        .mount(wasm_plugin.as_ref())
        .expect("mount packed wasm plugin");
    let wasm_commands = wasm_ctx.commands();
    assert_eq!(wasm_commands.len(), 1);
    assert_eq!(wasm_commands[0].id, "com.example.e2e-wasm.run");
    assert_eq!(wasm_commands[0].title, "Run E2e Wasm");
    assert!(
        aria_models::effect_spec("com.example.e2e-wasm.e2e-wasm").is_some(),
        "declared effect mounted from wasm package"
    );
    wasm_ctx
        .run_command("com.example.e2e-wasm.run")
        .expect("run wasm sample command");
    assert_eq!(
        fs::read_to_string(unpacked_wasm.data_dir(&wasm_data_root).join("last_run"))
            .expect("wasm command wrote storage"),
        "com.example.e2e-wasm.run"
    );
    wasm_ctx.unmount(wasm_id).expect("unmount wasm plugin");
}

/// WHY: closes the defect class where `docs/plugins/worked-example.md` drifts from
/// `aria plugin new` scaffold output (`plugin.toml`, `shaders/<slug>.wgsl`, file tree),
/// `PackageManifest` schema rules, or `aria plugin` sub-commands (`new`, `dev`, `check`,
/// `pack`) and their accepted flags.
/// Does not catch runtime behavior of custom Rust snippets outside the scaffolded files.
#[test]
fn worked_example_doc_matches_scaffold_and_subcommands() {
    let doc_path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/plugins/worked-example.md"
    ));
    let doc = fs::read_to_string(doc_path).expect("read docs/plugins/worked-example.md");

    let scratch = tempfile::tempdir().expect("tempdir");
    let pkg_dir = create_package(
        "com.example.color-tools",
        NewTier::Wasm,
        scratch.path(),
        None,
    )
    .expect("scaffold com.example.color-tools");

    let created_files = collect_relative_files(&pkg_dir);
    assert_eq!(
        created_files,
        vec![
            ".gitignore",
            "Cargo.toml",
            "plugin.toml",
            "shaders/color-tools.wgsl",
            "src/lib.rs",
            "wit/plugin.wit",
        ],
        "scaffolded file list must match worked-example.md tree"
    );
    for rel in &created_files {
        let file_name = rel.rsplit('/').next().unwrap_or(rel);
        assert!(
            doc.contains(file_name),
            "docs/plugins/worked-example.md is missing scaffolded file `{rel}`"
        );
    }

    // Extract fenced blocks by language tag.
    let mut toml_blocks = Vec::new();
    let mut wgsl_blocks = Vec::new();
    let mut text_blocks = Vec::new();
    let mut current_lang: Option<&str> = None;
    let mut current_buf = String::new();
    for line in doc.lines() {
        if let Some(rest) = line.strip_prefix("```") {
            if let Some(lang) = current_lang.take() {
                match lang {
                    "toml" => toml_blocks.push(std::mem::take(&mut current_buf)),
                    "wgsl" => wgsl_blocks.push(std::mem::take(&mut current_buf)),
                    "text" => text_blocks.push(std::mem::take(&mut current_buf)),
                    _ => current_buf.clear(),
                }
            } else {
                current_lang = Some(rest.trim());
                current_buf.clear();
            }
        } else if current_lang.is_some() {
            current_buf.push_str(line);
            current_buf.push('\n');
        }
    }

    assert_eq!(
        toml_blocks.len(),
        2,
        "worked-example.md must contain 2 ```toml blocks (scaffolded and expanded)"
    );
    assert_eq!(
        wgsl_blocks.len(),
        1,
        "worked-example.md must contain 1 ```wgsl block"
    );

    let generated_toml = fs::read_to_string(pkg_dir.join("plugin.toml")).expect("read plugin.toml");
    assert_eq!(
        toml_blocks[0].trim(),
        generated_toml.trim(),
        "first ```toml block in worked-example.md must match `aria plugin new com.example.color-tools` output"
    );

    let generated_wgsl = fs::read_to_string(pkg_dir.join("shaders/color-tools.wgsl"))
        .expect("read shaders/color-tools.wgsl");
    assert_eq!(
        wgsl_blocks[0].trim(),
        generated_wgsl.trim(),
        "```wgsl block in worked-example.md must match `shaders/color-tools.wgsl`"
    );

    // Expanded plugin.toml in section 2 must parse cleanly and pass manifest/shader/permission checks.
    let expanded_manifest = aria_plugin::package::PackageManifest::parse(&toml_blocks[1])
        .expect("second ```toml block in worked-example.md must parse as PackageManifest");
    assert_eq!(expanded_manifest.id, "com.example.color-tools");
    assert_eq!(expanded_manifest.effects.len(), 1);
    assert_eq!(expanded_manifest.commands.len(), 1);
    assert_eq!(expanded_manifest.menus.len(), 2);
    assert_eq!(expanded_manifest.keys.len(), 2);
    assert_eq!(expanded_manifest.panels.len(), 1);
    assert_eq!(expanded_manifest.settings.len(), 1);

    // Verify every `aria plugin <subcommand>` invocation in the text blocks uses real subcommands and flags.
    let mut seen_subcommands = BTreeSet::new();
    for block in &text_blocks {
        for line in block.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("aria plugin ") {
                let tokens: Vec<&str> = rest.split_whitespace().collect();
                let sub_word = tokens.first().copied().expect("subcommand word");
                let sub = Subcommand::parse(sub_word).unwrap_or_else(|| {
                    panic!("worked-example.md invokes unknown `aria plugin {sub_word}`")
                });
                seen_subcommands.insert(sub.key());
                for tok in &tokens[1..] {
                    if let Some(flag_part) = tok.strip_prefix("--") {
                        let flag_name = flag_part.split('=').next().unwrap_or(flag_part);
                        assert!(
                            sub.flags().contains(&flag_name),
                            "worked-example.md passes unknown flag `--{flag_name}` to `aria plugin {}` (accepted: {:?})",
                            sub.key(),
                            sub.flags()
                        );
                    }
                }
            }
        }
    }
    for sub in Subcommand::ALL {
        assert!(
            seen_subcommands.contains(sub.key()),
            "worked-example.md must demonstrate `aria plugin {}`",
            sub.key()
        );
    }
}
