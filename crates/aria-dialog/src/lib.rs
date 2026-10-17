#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

//! Native file and message dialogs.
//!
//! `rfd` is a dependency of this crate and of no other crate in the workspace,
//! and every operation here is an `async fn`. `rfd`'s blocking calls return only
//! when a person answers the dialog, so one made from the thread that draws
//! stops the window redrawing until then, and one made from a test never returns
//! at all. Keeping the blocking API in a crate the editor does not depend on
//! removes it as something a call site can reach for.
//!
//! The answer is a path, a list of paths, or a yes. A cancelled dialog is `None`,
//! an empty list, or `false`: cancelling is an answer, not an error.

use std::path::PathBuf;

/// What a file dialog opens with: its title, the extension groups it offers, the
/// name it proposes for a new file, and the directory it starts in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSpec {
    title: String,
    filters: Vec<(String, Vec<String>)>,
    file_name: Option<String>,
    directory: Option<PathBuf>,
}

impl FileSpec {
    /// A dialog titled `title`, offering every file.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    /// Offer one named group of extensions, written without the dot. Groups
    /// accumulate in the order they are added, which is the order the dialog
    /// lists them in.
    pub fn filter<S: AsRef<str>>(mut self, name: impl Into<String>, extensions: &[S]) -> Self {
        let valid: Vec<String> = extensions
            .iter()
            .map(|ext| ext.as_ref().trim().trim_start_matches('.'))
            .filter(|ext| !ext.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if !valid.is_empty() {
            self.filters.push((name.into(), valid));
        }
        self
    }

    /// The name a save dialog proposes.
    pub fn file_name(mut self, name: impl Into<String>) -> Self {
        self.file_name = Some(name.into());
        self
    }

    /// The directory the dialog opens in. Ignored when it is not a directory,
    /// because a stale project path would otherwise open the dialog nowhere.
    pub fn directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directory = Some(directory.into());
        self
    }

    fn build(self) -> rfd::AsyncFileDialog {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(self.title);
        for (name, extensions) in &self.filters {
            if !extensions.is_empty() {
                dialog = dialog.add_filter(name, extensions);
            }
        }
        if let Some(name) = self.file_name {
            dialog = dialog.set_file_name(name);
        }
        if let Some(directory) = self.directory.filter(|dir| dir.is_dir()) {
            dialog = dialog.set_directory(directory);
        }
        dialog
    }
}

/// Ask for one existing file.
pub async fn open_file(spec: FileSpec) -> Option<PathBuf> {
    spec.build()
        .pick_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}

/// Ask for any number of existing files.
pub async fn open_files(spec: FileSpec) -> Vec<PathBuf> {
    spec.build()
        .pick_files()
        .await
        .unwrap_or_default()
        .iter()
        .map(|handle| handle.path().to_path_buf())
        .collect()
}

/// Ask where to write a file. The path may not exist yet.
pub async fn save_file(spec: FileSpec) -> Option<PathBuf> {
    spec.build()
        .save_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}

/// Ask for a directory.
pub async fn open_folder(spec: FileSpec) -> Option<PathBuf> {
    spec.build()
        .pick_folder()
        .await
        .map(|handle| handle.path().to_path_buf())
}

/// Ask a question with two answers. `false` is both "no" and "the dialog was
/// dismissed": neither is permission to go ahead.
pub async fn confirm(title: String, question: String) -> bool {
    rfd::AsyncMessageDialog::new()
        .set_title(title)
        .set_description(question)
        .set_buttons(rfd::MessageButtons::OkCancel)
        .show()
        .await
        == rfd::MessageDialogResult::Ok
}

#[cfg(test)]
mod tests {
    use super::FileSpec;

    /// Two filter groups are two groups. An overwriting builder would silently
    /// drop the first offer, which is the whole point of naming groups.
    #[test]
    fn filters_accumulate_in_the_order_they_are_added() {
        let spec = FileSpec::new("Import")
            .filter("Video", &["mp4", "mov"])
            .filter("Audio", &["wav"]);
        assert_eq!(
            spec.filters,
            vec![
                ("Video".to_owned(), vec!["mp4".to_owned(), "mov".to_owned()]),
                ("Audio".to_owned(), vec!["wav".to_owned()]),
            ]
        );
    }

    #[test]
    fn a_spec_carries_the_title_the_name_and_the_directory() {
        let spec = FileSpec::new("Save As")
            .file_name("cut.aria")
            .directory("/tmp");
        assert_eq!(spec.title, "Save As");
        assert_eq!(spec.file_name.as_deref(), Some("cut.aria"));
        assert_eq!(
            spec.directory.as_deref(),
            Some(std::path::Path::new("/tmp"))
        );
        assert!(spec.filters.is_empty(), "no filter was asked for");
    }

    /// An empty extension group or one with only empty strings is ignored rather
    /// than forwarded to the platform dialog where empty filters cause undefined/backend-dependent behavior.
    #[test]
    fn empty_filter_extensions_are_ignored() {
        let spec = FileSpec::new("Open")
            .filter("Empty", &[] as &[&str])
            .filter("Blank", &["", "  "])
            .filter("Valid", &[".mp4", "mov"]);
        assert_eq!(
            spec.filters,
            vec![("Valid".to_owned(), vec!["mp4".to_owned(), "mov".to_owned()])]
        );
    }
}
