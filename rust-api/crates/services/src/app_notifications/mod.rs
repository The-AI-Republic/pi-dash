#![forbid(unsafe_code)]

//! App notification serializers (D-34, stage 5).
//!
//! Ports the read shapes of `apps/api/pi_dash/app/serializers/notification.py`
//! (all 28 lines); see [`shape`] for the two serializers. The query layer
//! (PIDASHCONV-299) owns SQL and annotation evaluation; the handlers
//! (PIDASHCONV-301…303) own routing and writes. This module owns only the
//! wire shapes those layers render through.

pub mod shape;

pub use shape::{
    notification_to_representation, preference_to_representation, NotificationRow,
    NotificationView, PreferenceRow, PreferenceView, NOTIFICATION_ALL_FIELDS,
    NOTIFICATION_DECLARED_FIELDS, NOTIFICATION_WIRE_FIELDS, PREFERENCE_ALL_FIELDS,
};
