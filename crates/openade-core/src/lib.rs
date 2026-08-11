//! Shared domain types for OpenADE.
//!
//! This crate is intentionally free of I/O beyond the local filesystem
//! (rules materialization). It defines the vocabulary shared by the
//! session daemon, the catalog MCP server, and the desktop app:
//!
//! - [`Harness`]: the supported coding agent CLIs.
//! - [`session`]: session states and metadata.
//! - [`context`]: the versioned context bundle injected into sessions.
//! - [`rules`]: one canonical rules source materialized per harness.

pub mod context;
pub mod harness;
pub mod rules;
pub mod session;

pub use context::ContextBundle;
pub use harness::Harness;
pub use session::{SessionMeta, SessionState};
