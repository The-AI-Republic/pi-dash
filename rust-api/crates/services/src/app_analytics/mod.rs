//! App analytics + exporters services (D-35, stage 5).
//!
//! * [`shape`] — the four serializer units owned by PIDASHCONV-317:
//!   `AnalyticViewSerializer`, `ExporterHistorySerializer`,
//!   `ImporterSerializer` and the porter `IssueExportSerializer`.
//! * [`queries`] — the workspace base-analytics queries owned by PIDASHCONV-334:
//!   `AnalyticsEndpoint` validation + plot/detail SQL, the analytic-view
//!   queryset + lookups, and the export acknowledgement.
pub mod queries;
pub mod shape;
