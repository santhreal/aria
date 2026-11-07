use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ids::MediaId;
use crate::media::MediaKind;

use super::Project;

impl Project {
    /// Correct the stored size of every video that a save from before the
    /// probe applied the container's display rotation recorded.
    ///
    /// Such an entry ([`MediaSource::upright`](crate::MediaSource::upright)
    /// false) holds the size as coded, so a video recorded upright on a phone
    /// states its sideways size and its upright picture draws squashed into
    /// that frame. `probe` returns a file's upright size, or `None` when the
    /// file does not open as a video. Each file is probed once, however many
    /// subclips share it, and the probes run in parallel. A probed size that is
    /// the stored one transposed replaces it, and an entry whose file probed at
    /// its stored size or the transposed one is marked upright, so a save
    /// records that it needs no probe again. An entry whose file is missing,
    /// does not probe, or probes at an unrelated size is left as it was:
    /// relinking corrects the last two.
    ///
    /// Returns how many entries changed size.
    pub fn orient_media<P>(&mut self, probe: P) -> usize
    where
        P: Fn(&Path) -> Option<(u32, u32)> + Sync,
    {
        let stale: Vec<(MediaId, PathBuf)> = self
            .media_iter()
            .filter(|media| !media.upright && media.kind() == MediaKind::Video)
            .map(|media| (media.id, media.path.clone()))
            .collect();
        if stale.is_empty() {
            return 0;
        }
        let mut paths: Vec<&Path> = stale.iter().map(|(_, path)| path.as_path()).collect();
        paths.sort_unstable();
        paths.dedup();
        let sizes = probe_all(&paths, &probe);

        let mut changed = 0;
        for (id, path) in &stale {
            let Some(&(width, height)) = sizes.get(path.as_path()) else {
                continue;
            };
            let Some(media) = self.media_mut(*id) else {
                continue;
            };
            if (width, height) == (media.height, media.width) && width != height {
                media.width = width;
                media.height = height;
                changed += 1;
            } else if (width, height) != (media.width, media.height) {
                continue;
            }
            media.upright = true;
        }
        changed
    }
}

/// The size `probe` reads for each of `paths` that probes, with at most one
/// probe per core running at a time.
fn probe_all<'a, P>(paths: &[&'a Path], probe: &P) -> HashMap<&'a Path, (u32, u32)>
where
    P: Fn(&Path) -> Option<(u32, u32)> + Sync,
{
    let lanes = std::thread::available_parallelism().map_or(1, usize::from);
    let per_lane = paths.len().div_ceil(lanes);
    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(per_lane)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|&path| probe(path).map(|size| (path, size)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    })
}
