//! What a package asks for, set against what a host granted it.

use super::manifest::{Access, FsScope, Permissions};

impl Permissions {
    /// Whether this asks for nothing beyond what every package has.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// What this asks for that `granted` does not allow. Empty when `granted`
    /// covers all of it. A path scope is covered by a granted path it lies
    /// under.
    pub fn beyond(&self, granted: &Self) -> Self {
        let wider = |asked: Access, held: Access| if asked > held { asked } else { Access::None };
        Self {
            project: wider(self.project, granted.project),
            media: wider(self.media, granted.media),
            gpu: self.gpu && !granted.gpu,
            net: missing(&self.net, &granted.net),
            fs: self
                .fs
                .iter()
                .filter(|scope| !granted.fs.iter().any(|held| covers(held, scope)))
                .cloned()
                .collect(),
            process: missing(&self.process, &granted.process),
            clipboard: self.clipboard && !granted.clipboard,
        }
    }

    /// One line per permission, as an install prompt lists them.
    pub fn describe(&self) -> Vec<String> {
        let mut lines = Vec::new();
        match self.project {
            Access::None => {}
            Access::Read => lines.push("Read the open project".to_owned()),
            Access::ReadWrite => lines.push("Read and edit the open project".to_owned()),
        }
        match self.media {
            Access::None => {}
            Access::Read => lines.push("Read and decode the project's media".to_owned()),
            Access::ReadWrite => {
                lines.push("Read, decode and write the project's media".to_owned())
            }
        }
        if self.gpu {
            lines.push("Run work on the GPU".to_owned());
        }
        lines.extend(self.net.iter().map(|host| format!("Connect to {host}")));
        lines.extend(self.fs.iter().map(|scope| match scope {
            FsScope::Project => "Read and write files in the project folder".to_owned(),
            FsScope::Pick => "Read and write files you pick".to_owned(),
            FsScope::Path(path) => format!("Read and write {}", path.display()),
        }));
        lines.extend(self.process.iter().map(|program| format!("Run {program}")));
        if self.clipboard {
            lines.push("Read the clipboard".to_owned());
        }
        lines
    }
}

fn missing(asked: &[String], held: &[String]) -> Vec<String> {
    asked
        .iter()
        .filter(|item| !held.contains(item))
        .cloned()
        .collect()
}

fn covers(held: &FsScope, asked: &FsScope) -> bool {
    match (held, asked) {
        (FsScope::Path(root), FsScope::Path(path)) => path.starts_with(root),
        (held, asked) => held == asked,
    }
}

#[cfg(test)]
#[path = "grants_tests.rs"]
mod tests;
