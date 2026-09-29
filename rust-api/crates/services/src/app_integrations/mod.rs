//! D-33 webhook serializers (stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/webhook.py` (`WebhookSerializer`
//! with `create`/`update` DNS + SSRF validation, and `WebhookLogSerializer`)
//! plus the model validators `validate_schema` / `validate_domain` from
//! `apps/api/pi_dash/db/models/webhook.py:21-31`.
//!
//! Write path, mirroring DRF `is_valid()` then `save()`:
//!
//! 1. field level (`validate_url_field`): trim, blank, `max_length=1024`,
//!    then every declared validator in order — `validate_schema`,
//!    `validate_domain`, Django `URLValidator` — collecting each failure
//!    into one `{"url": [...]}` list (the contract pins
//!    `{"url": ["Invalid schema. Only HTTP and HTTPS are allowed.",
//!    "Enter a valid URL."]}` for `"not-a-url"`).
//! 2. guards (`validate_create_url` / `validate_update_url`): hostname
//!    extraction, `getaddrinfo` resolution, the per-IP SSRF flag check,
//!    and the disallowed-domain check, each raising a single
//!    `{"url": "<message>"}` string error.
//!
//! Fixtures replayed by the unit tests beside this file:
//! `rust-api/fixtures/app_integrations/fx-web-04-webhook-serializer.json`
//! (golden I/O incl every `ValidationError` branch) and
//! `rust-api/fixtures/app_integrations/fx-web-05-ssrf-guard.json` (SSRF
//! matrix, schema/domain cases, disallowed domains).
//!
//! Ported bugs (translate, don't redesign):
//!
//! * B1 (`db/models/webhook.py:27-31`): [`validate_domain`] compares
//!   `urlparse(value).netloc`, which includes any `:port`, so
//!   `http://localhost:8000/hook` passes. Ported as-is.
//! * B2 (`app/views/webhook/base.py:84`): PATCH builds
//!   `context={request: request}` (the request object as the key), so
//!   `self.context.get("request")` is always `None` on update and the
//!   request host is never appended to the disallowed list. Ported as-is:
//!   [`validate_update_url`] takes the already-resolved context value, and
//!   the PATCH caller passes `None`.

pub mod serializers;
pub mod tasks;
