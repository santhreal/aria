use std::path::PathBuf;

use aria_commands::TemplatePick;
use aria_models::{Pick, Template};

use crate::action::ApplyContext;
use crate::error::EngineError;
use crate::import::{import_media, spread};

/// Replace the session with a `.ariat` template filled by `picks` (CapCut
/// "use template").
///
/// Atomic by construction: the template load, every pick probe, and the fill
/// all happen against local values, and the session is only replaced once the
/// filled project exists — a failure anywhere leaves project, path, and
/// history untouched. Like `Open`/`Load` the history is cleared (a fresh
/// document has no undo past), but the result is a *new unsaved* project, so
/// the project path resets. Sample media whose files are missing is tolerated
/// (matches `Load`); the UI relinks or fills those slots. The picks are probed
/// together, so an apply waits for the slowest probe rather than the sum, and
/// a failure reports the first pick in slot order that failed.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    path: PathBuf,
    picks: Vec<TemplatePick>,
) -> Result<(), EngineError> {
    let template = Template::load_from_file(&path)?;

    // `dunce`, as import: a `\\?\`-verbatim path on Windows is one Media
    // Foundation cannot open on mapped drives.
    let paths = picks
        .iter()
        .map(|pick| {
            dunce::canonicalize(&pick.path)
                .map_err(|err| EngineError::Io(aria_models::path_error(&pick.path, &err)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let resolved = spread(&paths, |path| import_media(path))
        .into_iter()
        .zip(picks)
        .map(|(media, pick)| {
            Ok(Pick {
                media: media?,
                source_in: pick.source_in,
            })
        })
        .collect::<Result<Vec<_>, EngineError>>()?;

    let filled = template.apply(&resolved)?;
    super::adopt(ctx, filled, None);
    Ok(())
}
