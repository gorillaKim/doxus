//! Agent sidecar - Node.js integration for doxus
//!
//! Manages CLI detection and agent lifecycle via JSONL protocol.

pub mod cli_detector;
pub mod manager;
pub mod protocol;
pub mod sidecar;
pub mod sync_sidecar;
pub mod prompt;

// Re-export commonly used items at crate root for convenience.
pub use cli_detector::{detect_cli, CliKind};
pub use manager::{AgentError, AgentManager};
pub use protocol::{AgentMessage, HostMessage};
pub use sidecar::{AgentError as SidecarError, SidecarManager, SidecarMessage};
pub use sync_sidecar::SyncSidecarManager;
pub use prompt::PromptLoader;
