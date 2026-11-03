//! Git integrations pure layer (D-05, stage 5).
//!
//! Ports the DB-free closure of `apps/api/pi_dash/integrations/git/`:
//!
//! * [`dtos`] — `dtos.py:1-118` (all nine dataclasses).
//! * [`errors`] — `adapters/base.py:23-86` (`GitProviderError` hierarchy +
//!   the 14-method `GitProviderAdapter` contract).
//! * [`registry`] — `registry.py:18-53` (lookup rule, adapter order,
//!   provider triplets).
//!
//! Fixture ids replayed by the unit tests alongside each module:
//! `rust-api/fixtures/integrations/dtos/*.golden.json` and
//! `rust-api/fixtures/integrations/registry.golden.json`.

pub mod dtos;
pub mod errors;
pub mod registry;

pub use dtos::{
    GitProviderCapabilities, ParsedCodeReview, ParsedRepository, ProviderWebhookEvent,
    RemoteCodeReview, RemoteComment, RemoteIssue, RemoteRepository, RepositoryPage,
};
pub use errors::{GitProviderAdapter, GitProviderError};
pub use registry::{ProviderEntry, UnknownProvider};
