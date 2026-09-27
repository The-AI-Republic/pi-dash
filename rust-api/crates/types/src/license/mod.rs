//! License / instance-console domain surface (D-01, stage 3).
//!
//! Ports `apps/api/pi_dash/license/api/serializers/` for the types layer:
//!
//! * [`serializers_workspace`] — `user.py` (`UserLiteSerializer`) and
//!   `workspace.py` (`WorkspaceSerializer`) output shapes plus the pure
//!   `validate_slug` rule (PIDASHCONV-115).

pub mod serializers_workspace;
