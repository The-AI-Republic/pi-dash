//! api-v1 OpenAPI schema surface (D-23, stage 5).
//!
//! Ports the served `/api/schema/*` trio (`pi_dash/urls.py:31-44`, live only
//! when `ENABLE_DRF_SPECTACULAR=1`):
//!
//! * [`throttle`] — the global `AnonRateThrottle` (30/minute) decision plus
//!   the 429 denial rendering per renderer (PIDASHCONV-531).
//!
//! Wiring note: PIDASHCONV-535 (handlers) owns routes registration and the
//! view shell here; on rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod throttle;
