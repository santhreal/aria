//! Plugin errors.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin {0} failed to mount: {1}")]
    MountFailed(&'static str, String),
    #[error("missing dependency: {0} requires {1}")]
    MissingDependency(&'static str, &'static str),
    #[error("service conflict: {0}")]
    ServiceConflict(String),
    #[error("plugin {0} failed to unmount: {1}")]
    UnmountFailed(&'static str, String),
    /// The plugins in the cycle, in the order they were declared, so the message
    /// names the loop rather than a single arbitrary member of it.
    #[error("dependency cycle: {0}")]
    DependencyCycle(String),
}
