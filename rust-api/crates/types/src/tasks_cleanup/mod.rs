//! D-09 issue/analytics export + expiry DTOs (tasks layer).
//!
//! Ports the value shapes of `apps/api/pi_dash/bgtasks/export_task.py`,
//! `exporter_expired_task.py` and `analytic_plot_export.py` for the types
//! layer: Celery wire names, row-mapping / axis constants, exporter-row
//! statuses, the S3 key layout, the 8-day expiry rule and the mail payload
//! shape. No I/O: every constructor here is pure over caller-supplied
//! scalars so fixture goldens replay without a database, S3 or SMTP.

pub mod exports_dto;
