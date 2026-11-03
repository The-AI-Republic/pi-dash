//! License / instance-console domain surface (D-01, stage 3).
//!
//! Ports `apps/api/pi_dash/license/utils/` for the services layer:
//!
//! * [`encryption`] — `encryption.py:13-44` (`derive_key`, `encrypt_data`,
//!   `decrypt_data`).
//! * [`config`] — `instance_value.py:14-74` (`get_configuration_value`,
//!   `get_email_configuration`).
//!
//! The cryptography and the registry live in the F-03 kernel
//! (`pidash_db::config`); this module is the domain's calling convention over
//! that kernel, so downstream D-01 layers (`serializers`, `handlers`) import
//! from one domain path. No behavior is forked here: every semantic,
//! including the ported bugs, is inherited from the kernel and documented at
//! the function.
//!
//! Wiring note: the crate root declares `pub mod license;` (foundation
//! change, tracked separately); these files are new-files-only for this
//! issue.

pub mod config;
pub mod encryption;
