//! Importers, exporters, and media encoders: an import lands as one undo step
//! named for the file or leaves nothing, an export replaces the destination
//! only once the exporter returned and leaves no partial file behind, a media
//! encoder opens a [`VideoEncoder`] session guarded on every step, and a panic
//! in any of them fails the plugin that registered it, removing its
//! registrations.
//! Not caught here: an importer that never returns. An import runs on the
//! thread that asked for it, under the project host's group.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_engine::{Engine, EngineConfig};
use aria_models::{Project, TrackId, TrackKind};
use parking_lot::Mutex;

use super::{
    Encoder, EncoderSpec, Exporter, ExporterSpec, Importer, ImporterSpec, InterchangeError,
    InterchangeReport,
};
use crate::context::PluginContext;
use crate::error::PluginError;
use crate::plugin::{Plugin, PluginManifest};
use crate::project_host::{Origin, ProjectHost, TransactError, Transaction};
use aria_core::{
    AudioEncoderConfig, ColorSpace, CpuImage, EncodeError, EncoderConfig, FrameData, PixelFormat,
    Plane, Rational, RationalTime, Rect, Rotation, VideoEncoder, VideoFrame,
};

const PLUGIN: &str = "test.formats";

/// Reads one lane per line. `#text` is skipped and reported, `missing` locks
/// a lane that does not exist, and `panic` panics.
struct Lanes;

impl Importer for Lanes {
    fn import(
        &self,
        file: &Path,
        tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        let text =
            std::fs::read_to_string(file).map_err(|err| TransactError::Aborted(err.to_string()))?;
        let mut report = InterchangeReport::default();
        for line in text.lines() {
            if let Some(skipped) = line.strip_prefix('#') {
                report.skipped.push(skipped.to_owned());
                continue;
            }
            let command = match line {
                "panic" => panic!("the lane file is malformed"),
                "missing" => Command::Edit(EditCommand::SetTrackLocked {
                    track: TrackId::from_raw(999),
                    locked: true,
                }),
                name => add(name),
            };
            tx.apply(command)?;
        }
        Ok(report)
    }
}

fn add(name: &str) -> Command {
    Command::Edit(EditCommand::AddTrack {
        kind: TrackKind::Video,
        name: name.to_owned(),
        index: None,
        pinned: true,
    })
}

/// How [`LaneList`] ends, after it wrote every lane name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ends {
    Returns,
    Errs,
    Panics,
}

const REFUSAL: &str = "the format holds no lanes";

/// Writes one lane name per line.
struct LaneList(Ends);

impl Exporter for LaneList {
    fn export(&self, project: &Project, out: &mut dyn Write) -> std::io::Result<InterchangeReport> {
        for name in lane_names(project) {
            writeln!(out, "{name}")?;
        }
        match self.0 {
            Ends::Returns => Ok(InterchangeReport {
                skipped: vec!["effects".to_owned()],
            }),
            Ends::Errs => Err(std::io::Error::other(REFUSAL)),
            Ends::Panics => panic!("the exporter is broken"),
        }
    }
}

/// A plugin that registers [`Lanes`] and a [`LaneList`] while it mounts.
struct Formats {
    manifest: PluginManifest,
    ends: Ends,
}

impl Plugin for Formats {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register_importer(importer("lanes", &["lanes"]), Lanes)?;
        ctx.register_exporter(exporter("lanes", "lanes"), LaneList(self.ends))?;
        Ok(())
    }
}

fn formats(ends: Ends) -> Formats {
    Formats {
        manifest: PluginManifest::new(PLUGIN, "Formats", "0.1.0", &[], &[]),
        ends,
    }
}

fn importer(id: &str, extensions: &[&str]) -> ImporterSpec {
    ImporterSpec {
        id: id.to_owned(),
        name: format!("{id} files"),
        extensions: extensions.iter().map(|ext| (*ext).to_owned()).collect(),
    }
}

fn exporter(id: &str, extension: &str) -> ExporterSpec {
    ExporterSpec {
        id: id.to_owned(),
        name: format!("{id} file"),
        extension: extension.to_owned(),
    }
}

/// Lane names, sorted: stack order is not part of these contracts.
fn lane_names(project: &Project) -> Vec<String> {
    let mut names: Vec<String> = project
        .timeline()
        .tracks_ordered()
        .map(|track| track.name.clone())
        .collect();
    names.sort();
    names
}

struct Fixture {
    ctx: PluginContext,
    host: ProjectHost,
    dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let ctx = PluginContext::new();
    let engine = Engine::new(EngineConfig::default()).expect("engine");
    let host = ProjectHost::new(Arc::new(Mutex::new(engine)), &ctx);
    let dir = tempfile::tempdir().expect("temp dir");
    Fixture { ctx, host, dir }
}

impl Fixture {
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, text).expect("write the file");
        path
    }

    fn add_lane(&self, name: &str) {
        self.host
            .transact(Origin::Editor, "Add", vec![add(name)])
            .expect("adds a lane");
    }

    fn lanes(&self) -> Vec<String> {
        lane_names(&self.host.snapshot())
    }

    fn history(&self) -> Vec<(String, bool)> {
        self.host
            .engine()
            .lock()
            .history_entries()
            .iter()
            .map(|entry| (entry.label.to_string(), entry.applied))
            .collect()
    }

    /// Every file in the directory, sorted, hidden ones included.
    fn entries(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.dir.path())
            .expect("read the directory")
            .map(|entry| {
                entry
                    .expect("directory entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    fn mount(&self, ends: Ends) {
        self.ctx.mount(&formats(ends)).expect("mounts");
    }

    fn failure(&self) -> Option<String> {
        self.ctx
            .failures()
            .into_iter()
            .find(|failure| failure.id == PLUGIN)
            .map(|failure| failure.reason)
    }
}

fn with(mut lanes: Vec<String>, added: &[&str]) -> Vec<String> {
    lanes.extend(added.iter().map(|name| (*name).to_owned()));
    lanes.sort();
    lanes
}

#[test]
fn an_import_is_one_undo_step_named_for_the_file() {
    let fx = fixture();
    let _importer = fx
        .ctx
        .register_importer(importer("lanes", &["LANES"]), Lanes)
        .expect("registers");
    let before = fx.lanes();
    let steps = fx.history().len();
    let path = fx.file("Cut.Lanes", "A\n#a title card\nB\n#a speed ramp\n");

    let (report, receipt) = fx.ctx.import_file(&fx.host, &path).expect("imports");

    assert_eq!(report.skipped, ["a title card", "a speed ramp"]);
    assert_eq!(receipt.outcomes.len(), 2, "one outcome per command applied");
    assert_eq!(fx.lanes(), with(before.clone(), &["A", "B"]));
    let history = fx.history();
    assert_eq!(history.len(), steps + 1, "the import is one undo step");
    assert_eq!(history.last(), Some(&("Import Cut.Lanes".to_owned(), true)));
    assert!(fx.host.undo().expect("undo"), "the import undoes");
    assert_eq!(fx.lanes(), before, "one undo removes every imported lane");
}

#[test]
fn a_plugin_import_is_named_for_the_plugin() {
    let fx = fixture();
    fx.mount(Ends::Returns);
    let path = fx.file("reel.lanes", "A\n");

    fx.ctx.import_file(&fx.host, &path).expect("imports");

    assert_eq!(
        fx.history().last(),
        Some(&(format!("{PLUGIN}: Import reel.lanes"), true))
    );
}

/// A command the engine rejects and a panic in the importer both leave the
/// project and its history as they were, and only the panic fails the plugin.
#[test]
fn a_failed_import_leaves_nothing() {
    for (text, panics) in [("A\nmissing\nB\n", false), ("A\nB\npanic\n", true)] {
        let fx = fixture();
        fx.mount(Ends::Returns);
        let before = fx.lanes();
        let history = fx.history();
        let path = fx.file("reel.lanes", text);

        let err = fx
            .ctx
            .import_file(&fx.host, &path)
            .expect_err("the import fails");

        let InterchangeError::Import {
            importer,
            path: failed,
            source,
        } = &err
        else {
            panic!("{text:?}: expected an import error, got {err:?}");
        };
        assert_eq!((importer.as_str(), failed), ("lanes", &path), "{text:?}");
        match (source, panics) {
            (TransactError::Failed { .. }, false) => {}
            (TransactError::Panicked(message), true) => {
                assert!(message.contains("malformed"), "{message}");
            }
            (other, _) => panic!("{text:?}: unexpected cause {other:?}"),
        }
        assert_eq!(fx.lanes(), before, "{text:?}: no imported lane remains");
        assert_eq!(fx.history(), history, "{text:?}: no undo step remains");
        let failure = fx.failure();
        if panics {
            let reason = failure.expect("the panic fails the plugin");
            assert!(reason.contains("importer lanes panicked"), "{reason}");
            assert!(fx.ctx.importers().is_empty(), "its importer is removed");
            assert!(fx.ctx.exporters().is_empty(), "its exporter is removed");
        } else {
            assert_eq!(failure, None, "an engine refusal is not a plugin fault");
            assert_eq!(fx.ctx.importers().len(), 1);
        }
    }
}

#[test]
fn a_panicking_host_importer_fails_no_plugin() {
    let fx = fixture();
    fx.mount(Ends::Returns);
    let _importer = fx
        .ctx
        .register_importer(importer("host", &["host"]), Lanes)
        .expect("registers");
    let path = fx.file("reel.host", "panic\n");

    let err = fx.ctx.import_file(&fx.host, &path).expect_err("fails");

    assert!(
        matches!(
            &err,
            InterchangeError::Import {
                source: TransactError::Panicked(_),
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(fx.failure(), None, "the panic was not in the plugin's code");
    assert_eq!(fx.ctx.importers().len(), 2);
}

#[test]
fn a_file_no_importer_reads_is_named() {
    let fx = fixture();
    fx.mount(Ends::Returns);
    for name in ["reel.edl", "reel", "lanes", "reel.lanes.bak"] {
        let path = fx.file(name, "A\n");
        let err = fx.ctx.import_file(&fx.host, &path).expect_err(name);
        assert!(
            matches!(&err, InterchangeError::NoImporter(missing) if *missing == path),
            "{name}: {err:?}"
        );
    }
}

#[test]
fn extensions_match_without_case_and_the_first_registered_wins() {
    let ctx = PluginContext::new();
    let first = ctx
        .register_importer(importer("first", &["EDL"]), Lanes)
        .expect("registers");
    let _second = ctx
        .register_importer(importer("second", &["edl", "Xml"]), Lanes)
        .expect("registers");
    let read_by = |name: &str| {
        ctx.importer_for(Path::new(name))
            .map(|spec| spec.id.clone())
    };

    assert_eq!(ctx.importers()[0].extensions, ["edl"], "stored lowercase");
    assert_eq!(ctx.importers()[1].extensions, ["edl", "xml"]);
    for (name, expected) in [
        ("cut.edl", Some("first")),
        ("CUT.EdL", Some("first")),
        ("cut.XML", Some("second")),
        ("cut.otio", None),
        ("edl", None),
        ("cut", None),
    ] {
        assert_eq!(read_by(name).as_deref(), expected, "{name}");
    }

    drop(first);
    assert_eq!(read_by("cut.edl").as_deref(), Some("second"));
    assert_eq!(ctx.importers().len(), 1);
}

#[test]
fn unmounting_a_plugin_removes_its_formats() {
    let fx = fixture();
    fx.mount(Ends::Returns);
    assert_eq!(fx.ctx.importers().len(), 1, "held while mounted");
    assert_eq!(fx.ctx.exporters().len(), 1, "held while mounted");

    fx.ctx.unmount(PLUGIN).expect("unmounts");

    assert!(fx.ctx.importers().is_empty());
    assert!(fx.ctx.exporters().is_empty());
    let path = fx.file("reel.lanes", "A\n");
    assert!(matches!(
        fx.ctx.import_file(&fx.host, &path),
        Err(InterchangeError::NoImporter(_))
    ));
}

#[test]
fn a_registration_that_outlives_its_context_drops() {
    let (read, write, encode) = {
        let ctx = PluginContext::new();
        (
            ctx.register_importer(importer("lanes", &["lanes"]), Lanes)
                .expect("registers"),
            ctx.register_exporter(exporter("lanes", "lanes"), LaneList(Ends::Returns))
                .expect("registers"),
            ctx.register_encoder(
                encoder_spec("lanes.enc", "mov", true),
                TestEncoder {
                    step: EncoderStep::None,
                    ends: Ends::Returns,
                },
            )
            .expect("registers"),
        )
    };
    drop(read);
    drop(write);
    drop(encode);
}

/// Every malformed or taken spec is refused, and a refusal leaves the
/// registry as it was.
#[test]
fn malformed_or_taken_specs_are_refused() {
    let ctx = PluginContext::new();
    let _importer = ctx
        .register_importer(importer("lanes", &["lanes"]), Lanes)
        .expect("registers");
    let _exporter = ctx
        .register_exporter(exporter("lanes", "lanes"), LaneList(Ends::Returns))
        .expect("registers");

    let importers: [(&str, &[&str]); 7] = [
        ("", &["edl"]),
        ("lanes", &["edl"]),
        ("edl", &[]),
        ("edl", &[".edl"]),
        ("edl", &["edl", ""]),
        ("edl", &["e dl"]),
        ("edl", &["edlé"]),
    ];
    for (id, extensions) in importers {
        let refused = ctx.register_importer(importer(id, extensions), Lanes);
        assert!(
            matches!(refused, Err(PluginError::InterchangeRefused(_))),
            "importer {id:?} {extensions:?}"
        );
    }
    for (id, extension) in [
        ("", "edl"),
        ("lanes", "edl"),
        ("edl", ""),
        ("edl", ".edl"),
        ("edl", "tar.gz"),
        ("edl", "a/b"),
    ] {
        let refused = ctx.register_exporter(exporter(id, extension), LaneList(Ends::Returns));
        assert!(
            matches!(refused, Err(PluginError::InterchangeRefused(_))),
            "exporter {id:?} {extension:?}"
        );
    }
    let ids =
        |specs: Vec<Arc<ImporterSpec>>| specs.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(ctx.importers()), ["lanes"]);
    assert_eq!(ctx.exporters().len(), 1);
    let _encoder = ctx
        .register_encoder(
            encoder_spec("lanes.enc", "mov", true),
            TestEncoder {
                step: EncoderStep::None,
                ends: Ends::Returns,
            },
        )
        .expect("registers");
    for (id, extension) in [
        ("", "mov"),
        ("lanes.enc", "mov"),
        ("prores", ""),
        ("prores", ".mov"),
        ("prores", "tar.gz"),
        ("prores", "a/b"),
    ] {
        let refused = ctx.register_encoder(
            encoder_spec(id, extension, true),
            TestEncoder {
                step: EncoderStep::None,
                ends: Ends::Returns,
            },
        );
        assert!(
            matches!(refused, Err(PluginError::EncoderRefused(_))),
            "encoder {id:?} {extension:?}"
        );
    }
    assert_eq!(ctx.encoders().len(), 1);
}

#[test]
fn an_export_replaces_the_destination_and_leaves_no_partial_file() {
    let fx = fixture();
    let _exporter = fx
        .ctx
        .register_exporter(exporter("lanes", "LANES"), LaneList(Ends::Returns))
        .expect("registers");
    assert_eq!(fx.ctx.exporters()[0].extension, "lanes", "stored lowercase");
    fx.add_lane("A");
    fx.add_lane("B");
    let path = fx.file("out.lanes", "old\n");
    let project = fx.host.snapshot();

    let report = fx
        .ctx
        .export_file("lanes", &project, &path)
        .expect("exports");

    assert_eq!(report.skipped, ["effects"]);
    let expected: String = lane_names(&project)
        .iter()
        .map(|name| format!("{name}\n"))
        .collect();
    for lane in ["A", "B"] {
        assert!(expected.lines().any(|line| line == lane), "{expected}");
    }
    assert_eq!(std::fs::read_to_string(&path).expect("read"), expected);
    assert_eq!(fx.entries(), ["out.lanes"]);
}

/// An exporter that errs or panics after writing leaves an existing
/// destination as it was, creates no destination where none was, and leaves
/// no partial file. Only the panic fails the plugin.
#[test]
fn a_failed_export_leaves_the_destination_unchanged() {
    for ends in [Ends::Errs, Ends::Panics] {
        for existing in [Some("old\n"), None] {
            let fx = fixture();
            fx.add_lane("A");
            fx.mount(ends);
            let path = fx.dir.path().join("out.lanes");
            if let Some(text) = existing {
                std::fs::write(&path, text).expect("write");
            }
            let before = fx.entries();
            let case = format!("{ends:?} over {existing:?}");

            let err = fx
                .ctx
                .export_file("lanes", &fx.host.snapshot(), &path)
                .expect_err(&case);

            match (&err, ends) {
                (
                    InterchangeError::Export {
                        exporter, source, ..
                    },
                    Ends::Errs,
                ) => {
                    assert_eq!(exporter, "lanes", "{case}");
                    assert_eq!(source.to_string(), REFUSAL, "{case}");
                    assert_eq!(fx.failure(), None, "{case}: an error is not a fault");
                }
                (InterchangeError::ExportPanicked { message, .. }, Ends::Panics) => {
                    assert!(message.contains("broken"), "{case}: {message}");
                    let reason = fx.failure().expect("the panic fails the plugin");
                    assert!(reason.contains("exporter lanes panicked"), "{reason}");
                    assert!(fx.ctx.exporters().is_empty(), "{case}");
                }
                (other, _) => panic!("{case}: unexpected {other:?}"),
            }
            assert_eq!(
                std::fs::read_to_string(&path).ok().as_deref(),
                existing,
                "{case}"
            );
            assert_eq!(fx.entries(), before, "{case}: no partial file remains");
        }
    }
}

#[test]
fn an_export_names_a_missing_exporter_or_an_unwritable_path() {
    let fx = fixture();
    fx.mount(Ends::Returns);
    let project = fx.host.snapshot();

    let missing = fx
        .ctx
        .export_file("edl", &project, &fx.dir.path().join("out.edl"))
        .expect_err("no exporter");
    assert!(
        matches!(&missing, InterchangeError::NoExporter(id) if id == "edl"),
        "{missing:?}"
    );

    for (path, kind) in [
        (
            fx.dir.path().join("absent/out.lanes"),
            std::io::ErrorKind::NotFound,
        ),
        (fx.dir.path().join(".."), std::io::ErrorKind::InvalidInput),
    ] {
        let err = fx
            .ctx
            .export_file("lanes", &project, &path)
            .expect_err("unwritable");
        assert!(
            matches!(&err, InterchangeError::Export { source, .. } if source.kind() == kind),
            "{}: {err:?}",
            path.display()
        );
    }
    assert!(fx.entries().is_empty(), "nothing is written");
}

fn encoder_spec(id: &str, extension: &str, audio: bool) -> EncoderSpec {
    EncoderSpec {
        id: id.to_owned(),
        name: format!("Format {id}"),
        extension: extension.to_owned(),
        audio,
    }
}

fn test_encoder_config() -> EncoderConfig {
    EncoderConfig::new((4, 2), Rational::FPS_24, ColorSpace::BT709)
        .with_audio(AudioEncoderConfig::new(48_000, 2))
        .with_bitrate_kbps(2_000)
}

fn test_video_frame(width: u32, height: u32) -> VideoFrame {
    let stride = (width as usize) * 4;
    let plane = Plane::new(
        vec![0u8; stride * (height as usize)],
        stride,
        height as usize,
    );
    VideoFrame::new(
        RationalTime::zero(Rational::FPS_24),
        PixelFormat::Rgba8,
        ColorSpace::BT709,
        (width, height),
        Rect::from_size(width, height),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(vec![plane])),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EncoderStep {
    None,
    Open,
    Push,
    PushAudio,
    Finish,
}

struct TestEncoder {
    step: EncoderStep,
    ends: Ends,
}

struct TestEncoderSession {
    path: PathBuf,
    header: String,
    frames: usize,
    audio_samples: usize,
    step: EncoderStep,
    ends: Ends,
}

impl Encoder for TestEncoder {
    fn open(
        &self,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn VideoEncoder>, EncodeError> {
        if self.step == EncoderStep::Open {
            match self.ends {
                Ends::Returns => {}
                Ends::Errs => return Err(EncodeError::Start(REFUSAL.into())),
                Ends::Panics => panic!("open is broken"),
            }
        }
        Ok(Box::new(TestEncoderSession {
            path: path.to_owned(),
            header: format!("{}x{}", config.size.0, config.size.1),
            frames: 0,
            audio_samples: 0,
            step: self.step,
            ends: self.ends,
        }))
    }
}

impl VideoEncoder for TestEncoderSession {
    fn push(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if self.step == EncoderStep::Push {
            match self.ends {
                Ends::Returns => {}
                Ends::Errs => return Err(EncodeError::Encode(REFUSAL.into())),
                Ends::Panics => panic!("push is broken"),
            }
        }
        self.frames += usize::from(frame.coded_size.0 > 0);
        Ok(())
    }

    fn push_audio(&mut self, samples: &[f32], _pts: RationalTime) -> Result<(), EncodeError> {
        if self.step == EncoderStep::PushAudio {
            match self.ends {
                Ends::Returns => {}
                Ends::Errs => return Err(EncodeError::Encode(REFUSAL.into())),
                Ends::Panics => panic!("push_audio is broken"),
            }
        }
        self.audio_samples += samples.len();
        Ok(())
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        if self.step == EncoderStep::Finish {
            match self.ends {
                Ends::Returns => {}
                Ends::Errs => return Err(EncodeError::Encode(REFUSAL.into())),
                Ends::Panics => panic!("finish is broken"),
            }
        }
        std::fs::write(
            &self.path,
            format!("{}:{}:{}\n", self.header, self.frames, self.audio_samples),
        )
        .map_err(|err| EncodeError::Encode(err.to_string()))
    }
}

struct EncoderPlugin {
    manifest: PluginManifest,
    step: EncoderStep,
    ends: Ends,
}

impl Plugin for EncoderPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register_encoder(
            encoder_spec("lanes.enc", "MOV", true),
            TestEncoder {
                step: self.step,
                ends: self.ends,
            },
        )?;
        Ok(())
    }
}

#[test]
fn an_encoder_registers_opens_writes_frames_and_audio_and_unregisters() {
    let fx = fixture();
    fx.ctx
        .mount(&EncoderPlugin {
            manifest: PluginManifest::new(PLUGIN, "Formats", "1.0.0", &[], &[]),
            step: EncoderStep::None,
            ends: Ends::Returns,
        })
        .expect("mounts");
    let specs = fx.ctx.encoders();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].id, "lanes.enc");
    assert_eq!(specs[0].extension, "mov", "stored lowercase");
    assert!(specs[0].audio);
    assert_eq!(fx.ctx.encoder("lanes.enc"), Some(Arc::clone(&specs[0])));

    let out = fx.dir.path().join("clip.mov");
    let mut session = fx
        .ctx
        .open_encoder("lanes.enc", &out, &test_encoder_config())
        .expect("opens");
    let frame = test_video_frame(4, 2);
    session.push(&frame).expect("push 1");
    session.push(&frame).expect("push 2");
    session
        .push_audio(
            &[0.25, -0.25, 0.5, -0.5],
            RationalTime::zero(Rational::new(48_000, 1)),
        )
        .expect("push audio");
    session.finish().expect("finish");
    assert_eq!(std::fs::read_to_string(&out).expect("written"), "4x2:2:4\n");

    fx.ctx.unmount(PLUGIN).expect("unmounts");
    assert!(fx.ctx.encoders().is_empty());
    assert_eq!(fx.ctx.encoder("lanes.enc"), None);
    let err = fx
        .ctx
        .open_encoder("lanes.enc", &out, &test_encoder_config())
        .err()
        .expect("unmounted encoder cannot open");
    assert!(
        err.to_string().contains("lanes.enc"),
        "missing encoder error must name the id: {err}"
    );
}

#[test]
fn an_encoder_panic_in_any_step_fails_its_plugin_while_an_error_does_not() {
    for step in [
        EncoderStep::Open,
        EncoderStep::Push,
        EncoderStep::PushAudio,
        EncoderStep::Finish,
    ] {
        for ends in [Ends::Errs, Ends::Panics] {
            let fx = fixture();
            fx.ctx
                .mount(&EncoderPlugin {
                    manifest: PluginManifest::new(PLUGIN, "Formats", "1.0.0", &[], &[]),
                    step,
                    ends,
                })
                .expect("mounts");
            let out = fx.dir.path().join("clip.mov");
            let frame = test_video_frame(4, 2);
            let case = format!("{step:?} {ends:?}");

            let outcome: Result<(), EncodeError> = (|| {
                let mut session = fx
                    .ctx
                    .open_encoder("lanes.enc", &out, &test_encoder_config())?;
                session.push(&frame)?;
                session.push_audio(&[0.1, -0.1], RationalTime::zero(Rational::new(48_000, 1)))?;
                session.finish()
            })();
            let err = outcome.expect_err(&case);

            match ends {
                Ends::Returns => unreachable!(),
                Ends::Errs => {
                    assert!(
                        err.to_string().contains(REFUSAL),
                        "{case}: expected refusal in {err}"
                    );
                    assert_eq!(fx.failure(), None, "{case}: an error is not a fault");
                    assert_eq!(fx.ctx.encoders().len(), 1, "{case}: stays registered");
                }
                Ends::Panics => {
                    assert!(
                        err.to_string().contains("is broken"),
                        "{case}: expected panic message in {err}"
                    );
                    let reason = fx.failure().expect("panic fails the plugin");
                    assert!(
                        reason.contains("encoder lanes.enc panicked"),
                        "{case}: {reason}"
                    );
                    assert!(
                        fx.ctx.encoders().is_empty(),
                        "{case}: failed plugin unregisters its encoder"
                    );
                }
            }
        }
    }
}
