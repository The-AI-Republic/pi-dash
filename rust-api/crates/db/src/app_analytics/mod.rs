//! Analytics + exporters app domain surface (D-35, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/analytic.py`,
//! `db/models/exporter.py` and `db/models/importer.py` for the db layer:
//!
//! * [`models`] — `AnalyticView`, `ExporterHistory`, `Importer`
//!   (PIDASHCONV-318, struct + column/constraint mapping plus the
//!   `generate_token` default as a function). Reads, serializers,
//!   guards, tasks and handlers belong to the sibling D-35 issues;
//!   the domain gate is PIDASHCONV-440.

pub mod models;

pub use models::{
    analytic_view::AnalyticView, exporter_history::ExporterHistory, importer::Importer, OnDelete,
};
