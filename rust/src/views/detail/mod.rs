//! Job and node detail screens.
//!
//! This module provides read-only detail viewers for jobs and nodes,
//! including batch scripts, log viewers, array task expansion, dependency
//! graphs, and attach prompts.

pub mod array_tasks;
pub mod attach_prompt;
pub mod batch_script;
pub mod dependency;
pub mod job_detail;
pub mod job_info;
pub mod log_viewer;
pub mod node_detail;

// Re-exports will be used when modal integration is added
#[allow(unused_imports)]
pub use array_tasks::ArrayTaskScreen;
#[allow(unused_imports)]
pub use attach_prompt::AttachPromptScreen;
#[allow(unused_imports)]
pub use batch_script::BatchScriptScreen;
#[allow(unused_imports)]
pub use dependency::DependencyScreen;
#[allow(unused_imports)]
pub use job_detail::JobDetailScreen;
#[allow(unused_imports)]
pub use job_info::JobInfoScreen;
#[allow(unused_imports)]
pub use log_viewer::LogViewerScreen;
#[allow(unused_imports)]
pub use node_detail::NodeDetailScreen;

/// Action outcome from handle_key methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No action needed.
    None,
    /// Close the screen.
    Close,
    /// Return a value (for modals that return values).
    Value(String),
}
