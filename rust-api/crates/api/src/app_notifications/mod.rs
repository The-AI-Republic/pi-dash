//! D-34 app-notification HTTP layer (stage 5).
//!
//! [`gate`] ports the `@allow_permission` role matrix for the 7
//! notification routes (`apps/api/pi_dash/app/views/notification/base.py`
//! decorator lines `:47, :151, :163, :171, :179, :187, :199, :233`, plus
//! the undecorated preference endpoints `:291-308` and the undecorated
//! retrieve/destroy mixins). The handlers (PIDASHCONV-301…303) own
//! routing and the queryset/write bodies; sibling handler issues extend
//! this module, and merges keep both sides.

pub mod gate;

pub use gate::{
    decide_gate, gate_for, tenant_context, Gate, GateOutcome, RouteGate, GATES,
    UNAUTHENTICATED_BODY,
};
