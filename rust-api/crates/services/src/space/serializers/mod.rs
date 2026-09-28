//! Space public-API serializers (D-02, stage 4).
//!
//! * [`lite`] — leaf serializers: base classes, user/workspace/project lite
//!   shapes, full and lite state shapes.
//!
//! Later layer issues add `taxonomy`, `intake` and `issue` modules here;
//! they nest these leaves (e.g. `IssueFlatSerializer` embeds the lite user),
//! so the leaves land first.
pub mod lite;
