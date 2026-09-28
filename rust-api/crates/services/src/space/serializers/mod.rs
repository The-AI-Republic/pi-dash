//! Space public-API serializers (D-02, stage 4).
//!
//! * [`lite`] — leaf serializers: base classes, user/workspace/project lite
//!   shapes, full and lite state shapes.
//! * [`taxonomy`] — cycle / module / label shapes (nest the lite leaves).
//! * [`intake`] — intake-issue and inbox shapes (nest the lite leaves, the
//!   taxonomy label lite, and the flat issue contract owned by PIDASHCONV-165).
pub mod intake;
pub mod lite;
pub mod taxonomy;
