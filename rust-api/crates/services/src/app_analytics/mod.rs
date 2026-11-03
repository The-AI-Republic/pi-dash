//! App analytics + exporters services (D-35, stage 5).
//!
//! * [`shape`] — the four serializer units owned by PIDASHCONV-317:
//!   `AnalyticViewSerializer`, `ExporterHistorySerializer`,
//!   `ImporterSerializer` and the porter `IssueExportSerializer`.
//! * [`queries`] — the workspace base-analytics queries owned by PIDASHCONV-334:
//!   `AnalyticsEndpoint` validation + plot/detail SQL, the analytic-view
//!   queryset + lookups, and the export acknowledgement — plus the default /
//!   project / advance / exporter query builders owned by PIDASHCONV-349
//!   (FX-A-Q-03..Q-06).
//! * [`export_format`] — the porter + exporter format engines owned by
//!   PIDASHCONV-381: `DataExporter`, JSON/CSV/XLSX formatters, the
//!   `utils/exporters` plane and `IssueExportSchema` (FX-A-FMT-01).
pub mod export_format;
pub mod queries;
pub mod shape;
