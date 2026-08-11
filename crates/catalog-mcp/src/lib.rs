//! `catalog-mcp` — the OpenADE context layer (PRD R5).
//!
//! An MCP server exposing read tools against an organization's software
//! catalog. The catalog backend is abstracted behind [`provider::CatalogProvider`];
//! the first implementation is [`backstage::BackstageProvider`] against the
//! Backstage REST APIs. Other IDP platforms (Port, Cortex, OpsLevel, a plain
//! Git-backed catalog) can implement the same trait later without touching
//! the tool surface.

pub mod backstage;
pub mod bundle;
pub mod github;
pub mod mcp;
pub mod provider;
pub mod router;
#[cfg(any(test, feature = "mock"))]
pub mod testutil;

pub use backstage::BackstageProvider;
pub use github::GithubProvider;
pub use provider::{CatalogProvider, Entity, EntityRef, ProviderError};
pub use router::MemoryRouter;
