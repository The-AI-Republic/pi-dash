//! api-v1 assets / stickies / intake domain surface (D-21, stage 5).
//!
//! Ports `apps/api/pi_dash/api/serializers/` for the types layer:
//!
//! * [`intake`] — `intake.py:12-170` (PIDASHCONV-401; fixture `fx-ser-intake`).
//! * [`asset`], [`sticky`] — `asset.py:13-91`, `sticky.py:12-34`
//!   (PIDASHCONV-392; fixtures `fx-ser-asset`, `fx-ser-sticky`).
//!   [`asset`] covers the four asset serializers; [`sticky`] covers
//!   `StickySerializer.validate` plus the `validate_html_content` /
//!   `validate_binary_data` transcription from
//!   `apps/api/pi_dash/utils/content_validator.py`.
//!
//! The asset/sticky entries are pure input kernels: each entry point takes
//! already-parsed JSON (the Rust stack serves JSON bodies only) and returns
//! either the DRF `validated_data` object or the DRF error object, both as
//! `serde_json::Value` so the byte rendering stays with the handler layer.
//! Unknown input keys are ignored, exactly like DRF's `to_internal_value`.

pub mod asset;
pub mod intake;
pub mod sticky;
