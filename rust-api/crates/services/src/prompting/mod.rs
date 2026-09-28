//! Prompting domain surface (D-04, stage 4).
//!
//! Ports `apps/api/pi_dash/prompting/` for the services layer, bottom-up:
//!
//! * [`shape`] — serializer output shapes (`serializers.py`).
//! * [`registry`] — section catalog + reader (`registry.py`).
//! * [`renderer`] — sandboxed Jinja rendering (`renderer.py`).
//! * [`recipes`] — ordered section lists per kind (`recipes.py`).
//! * [`composer`] — override resolution into assembled prompts
//!   (`composer.py`, up to `compile_template` + `_user_for_run`; turn
//!   builders own to PIDASHCONV-142).
//! * [`validation`] — save-time override validation (`validation.py`).
//! * [`context`] — template context variables + first/scheduler/direct
//!   turn builders (`context.py`, `composer.py` turn builders).
//! * [`seed`] — default/review/test template seeding, reseed commands,
//!   override revalidation (`seed.py`, `management/commands/`).
//!
//! Wiring note: the crate root declares `pub mod prompting;` (seam for
//! this issue's new files); every file under this module is new.
pub mod composer;
pub mod context;
pub mod recipes;
pub mod registry;
pub mod renderer;
pub mod seed;
pub mod shape;
pub mod validation;
