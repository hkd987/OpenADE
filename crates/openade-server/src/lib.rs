//! The OpenADE multiplayer workspace server.
//!
//! A self-hostable hub (own binary, own deployment) for the team features
//! Xirp puts in Portal Workspaces: members upload chosen session
//! transcripts (harness-neutral records), teammates browse the shared
//! history, and it feeds future sessions' context — including picking a
//! shared session up in a different harness. Auth is org API tokens; the
//! schema is org-scoped so a hosted multi-tenant deployment runs the same
//! binary.

pub mod server;
pub mod store;
