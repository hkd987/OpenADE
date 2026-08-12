//! The OpenADE session daemon.
//!
//! The daemon is the long-lived process that owns everything a session needs
//! to survive the UI closing (PRD R1): PTY sessions with scrollback, Git
//! worktree isolation (R2), harness adapters (R4), transcript recording (R6
//! groundwork), and a localhost HTTP API the desktop app attaches to.

pub mod adapters;
pub mod artifact;
pub mod config;
pub mod daemon;
pub mod memory_repo;
pub mod pty;
pub mod server;
pub mod transcript;
pub mod workspace;
pub mod worktree;

pub use daemon::Daemon;
