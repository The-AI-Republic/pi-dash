//! Project-core handlers (PIDASHCONV-571).
//!
//! Ports `ProjectViewSet` (`apps/api/pi_dash/app/views/project/base.py:46-429`,
//! drift baseline `01a93e17`) with identical URL paths, status codes and JSON
//! bytes. Routes live in `apps/api/pi_dash/app/urls/project.py:24-45` — all
//! three paths share the route name `"project"`, which is what gates the `pk`
//! rewrite below:
//!
//! - `GET projects/` — compact `.values()` list (`base.py:145-223`)
//! - `POST projects/` — create (`base.py:258-312`)
//! - `GET projects/details/` — full-serializer list (`base.py:101-142`)
//! - `GET projects/<pk>/` — retrieve (`base.py:226-255`)
//! - `PUT projects/<pk>/` — no override: DRF default full update through
//!   `ProjectListSerializer` with the four required keys (`deleted_at`,
//!   `name`, `identifier`, `workspace`) and the four gated unique
//!   validators (Q-PUT-400); valid PUTs 200, including soft-delete via
//!   `deleted_at`
//! - `PATCH projects/<pk>/` — partial update (`base.py:314-380`)
//! - `DELETE projects/<pk>/` — destroy (`base.py:382-429`)
//! - `POST`/`DELETE projects/<project_id>/archive/` —
//!   `ProjectArchiveUnarchiveEndpoint` (`base.py:432-446`)
//! - `GET`/`DELETE project-identifiers/` — `ProjectIdentifierEndpoint`
//!   (`base.py:449-476`)
//! - `POST projects/<project_id>/project-views/` — `ProjectUserViewsEndpoint`
//!   (`base.py:479-500`)
//!
//! Fixture ids: FX-APROJ-09 (`rust-api/fixtures/app_project/`,
//! PIDASHCONV-562) for the project-core slice (routes, goldens, error
//! bodies, side-effect rows); FX-APROJ-08 for the task kwargs (L8,
//! PIDASHCONV-570) this module publishes.
//!
//! Handler notes (all verified against the Python source):
//! - Auth is Django-session + `IsAuthenticated` on every row: anonymous
//!   answers the 401 [`ANON_BODY`] before anything else runs (the
//!   `app_views_search` precedent).
//! - `_rewrite_project_kwarg` (`app/views/base.py:49-81`) runs before the
//!   gates: UUID-looking `pk`/`project_id` passes through unverified (the
//!   body 404s it as before); anything else is an identifier lookup and a
//!   miss answers the resolve 404. `pk` rewrites only under the `"project"`
//!   route name; `project_id` always. Unauthenticated requests skip the
//!   rewrite — unreachable here since [`actor`] 401s first.
//! - Gates come from [`super::gates`]: the list/retrieve/identifier rows are
//!   workspace-level, archive is project-level with the workspace-admin
//!   override, and PUT/PATCH/DELETE/project-views are auth-only at the gate
//!   with inline checks in the bodies.
//! - Datetimes render in the request user's zone (`TimezoneMixin` +
//!   DRF `enforce_timezone` over `get_current_timezone`), `isoformat` with
//!   `+00:00` rewritten to `Z`, microseconds only when nonzero.
//! - `?fields=` on `list_detail` is dead (B-fields-dead,
//!   `serializers/base.py:14-18`): full objects always render.
//! - Task publishes (`recent_visited_task`, `model_activity`,
//!   `webhook_activity`, `soft_delete_related_objects`) are best-effort
//!   `rust_job_queue` rows; without the queue the response still stands
//!   (the space intake precedent).
//!
//! Ported quirks (translate, don't redesign):
//! - Q-PUT-400: PUT runs the `ProjectListSerializer` full-update path:
//!   `deleted_at`/`name`/`identifier`/`workspace` are all required, and
//!   the four `unique_together` validators skip unchanged/None values
//!   (`enforce_uniqueness` gating) with the triples probing the live
//!   base — PUTs missing keys 400, valid ones 200.
//! - Q-identifier-case: `validate_identifier` matches the raw stripped value
//!   case-sensitively while `save()` uppercases, so a lowercase dup passes
//!   validation and dies at the unique index (`IntegrityError` → 400
//!   `{"error":"The payload is not valid"}`).
//! - Q-activity-raw-data: `model_activity.requested_data` is the raw
//!   `request.data` — the PATCH `intake_view` alias key never reaches it.
//! - Q-member-property: `ProjectMember.save` unconditionally inserts a
//!   `ProjectUserProperty` row (MIN − 10000 or 65535) *before* the member
//!   row itself.
//! - Q-first-default: the first live project of a workspace becomes default
//!   even when created with `is_default = False`.
//! - Q-identifier-delete-unvalidated: the identifier DELETE `.strip()`s the
//!   raw value, so a non-string `name` 500s (`AttributeError`).
//! - Q-null-member-500: an active membership row with a NULL `member_id`
//!   500s (`None.is_bot` inside `get_members`).
//!
//! Out of scope (sibling handler issues): favorites + deploy boards
//! (`base.py:503-581`), members/invites/join (`member.py`, `invite.py`),
//! states + estimates (FX-APROJ-10). The serializers live in
//! `pidash_services::app_project`, the gates in [`super::gates`], the task
//! shapes in `pidash_services::app_project::tasks`.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Utc, Weekday};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::Row;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_types::WorkspaceId;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Canonical gate-table paths for the owned method+path rows (mirrors
/// [`super::gates::GATES`] so the table stays the single source of truth;
/// the unit test pins them together).
pub const PATH_PROJECTS: &str = "workspaces/<slug>/projects/";
pub const PATH_PROJECTS_DETAILS: &str = "workspaces/<slug>/projects/details/";
pub const PATH_PROJECT_DETAIL: &str = "workspaces/<slug>/projects/<pk>/";
pub const PATH_IDENTIFIERS: &str = "workspaces/<slug>/project-identifiers/";
pub const PATH_PROJECT_VIEWS: &str = "workspaces/<slug>/projects/<project_id>/project-views/";
pub const PATH_ARCHIVE: &str = "workspaces/<slug>/projects/<project_id>/archive/";

/// Register the six project-core paths. Owned methods serve from Rust;
/// everything else proxies to Django (its 401-anon-before-405, DRF
/// metadata and sibling actions live there). `pk`/`project_id` stay
/// `<str:>` like the Django converters: non-UUID tails reach the handlers
/// and run the identifier rewrite (a miss is the resolve 404), exactly as
/// `_rewrite_project_kwarg` does.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/",
            owned(
                axum::routing::get(project_list).post(project_create),
                &["GET", "POST"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/details/",
            owned(axum::routing::get(project_list_detail), &["GET"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{pk}/",
            owned(
                axum::routing::get(project_retrieve)
                    .put(project_put)
                    .patch(project_patch)
                    .delete(project_destroy),
                &["GET", "PUT", "PATCH", "DELETE"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/project-identifiers/",
            owned(
                axum::routing::get(identifiers_get).delete(identifiers_delete),
                &["GET", "DELETE"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/project-views/",
            owned(axum::routing::post(user_views_post), &["POST"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/archive/",
            owned(
                axum::routing::post(archive_post).delete(unarchive_delete),
                &["POST", "DELETE"],
            ),
        )
}

/// An owned path: listed methods serve from Rust, everything else proxies
/// to Django. OPTIONS proxies too: DRF answers metadata (401 anon / 200
/// authed) where axum would 405. HEAD rides axum's `get` handling like
/// Django's `GET`-backed `HEAD` (the `app_views_search` precedent).
fn owned(
    router: axum::routing::MethodRouter<AppState>,
    methods: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = router;
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        if methods.contains(&method) {
            continue;
        }
        router = match method {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            _ => router.options(crate::edge::proxy),
        };
    }
    router
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// `Project.resolve` miss (`db/models/project.py:214-218`): Django `Http404`
/// propagates through DRF's `exception_handler` as `NotFound(*args)`,
/// rendering lowercase compact `{"detail": ...}` (verified against the
/// pinned DRF 3.15.2 source; the cycles/v1 ports carry the same bytes).
/// NOTE: `queries::RESOLVE_NOT_FOUND_BODY` still renders the pre-#969
/// capital-`D` spaced form and is not used here.
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `get_object` miss on PUT (`UpdateModelMixin`, no override):
/// `get_object_or_404` raises `Http404("No Project matches the given
/// query.")`, which DRF renders with the message (verified live).
pub const OBJECT_LIST_NOT_FOUND_BODY: &str = r#"{"detail":"No Project matches the given query."}"#;
/// `ProjectViewSet.retrieve` miss, including archived rows
/// (`base.py:227-230`): the view's own 404, not `handle_exception`'s.
pub const RETRIEVE_NOT_FOUND_BODY: &str = r#"{"error":"Project does not exist"}"#;
/// `ProjectViewSet.retrieve` on a public project the caller is not a member
/// of (`base.py:240-244`).
pub const RETRIEVE_NONMEMBER_BODY: &str = r#"{"error":"You are not a member of this project"}"#;
/// `ProjectViewSet.retrieve` on a secret project the caller is not a member
/// of (`base.py:235-239`).
pub const RETRIEVE_SECRET_BODY: &str = r#"{"error":"You do not have permission"}"#;
/// `ProjectViewSet.partial_update` / `destroy` inline admin denial
/// (`base.py:332-336`, `:426-429`): same bytes as the decorator body.
pub const ADMIN_REQUIRED_BODY: &str = r#"{"error":"You don't have the required permissions."}"#;
/// `ProjectViewSet.partial_update` on an archived project (`base.py:343-347`).
pub const ARCHIVED_UPDATE_BODY: &str = r#"{"error":"Archived projects cannot be updated"}"#;
/// `ProjectViewSet.destroy` on the default project (`base.py:399-403`).
pub const DEFAULT_DELETE_BODY: &str = r#"{"error":"Default project cannot be deleted"}"#;
/// `ProjectIdentifierEndpoint` missing-name 400 (`base.py:454-455`, `:465-466`).
pub const IDENTIFIER_NAME_REQUIRED_BODY: &str = r#"{"error":"Name is required"}"#;
/// `ProjectIdentifierEndpoint.delete` on a live project's identifier
/// (`base.py:468-472`).
pub const IDENTIFIER_LIVE_BODY: &str =
    r#"{"error":"Cannot delete an identifier of an existing project"}"#;
/// `ProjectUserViewsEndpoint.post` for a non-member (`base.py:485-486`).
pub const USER_VIEWS_FORBIDDEN_BODY: &str = r#"{"error":"Forbidden"}"#;
/// DRF `DateTimeField` invalid-input message (verified against DRF 3.15.2
/// `fields.py:1129`): answers unparseable `archived_at` / `deleted_at`.
pub const INVALID_DATETIME_MESSAGE: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";
/// DRF `DateTimeField` `overflow` message (`fields.py:1132`): an aware
/// input whose `astimezone` crosses the `0001..=9999` years (probed:
/// `9999-12-31T23:00:00-14:00` and `0001-01-01T00:00:00+14:00`).
pub const OVERFLOW_DATETIME_MESSAGE: &str = "Datetime value out of range.";
/// `BasePaginator.get_per_page` ceiling (`default_per_page=1000`,
/// `max_per_page=1000`, `paginator.py:643-654`).
pub const MAX_PER_PAGE: i64 = 1000;

/// `TIMEZONE_CHOICES` (`db/models/workspace.py:120`):
/// `pytz.common_timezones`, generated from the pinned environment.
pub const TIMEZONE_CHOICES: &[&str] = &[
    "Africa/Abidjan",
    "Africa/Accra",
    "Africa/Addis_Ababa",
    "Africa/Algiers",
    "Africa/Asmara",
    "Africa/Bamako",
    "Africa/Bangui",
    "Africa/Banjul",
    "Africa/Bissau",
    "Africa/Blantyre",
    "Africa/Brazzaville",
    "Africa/Bujumbura",
    "Africa/Cairo",
    "Africa/Casablanca",
    "Africa/Ceuta",
    "Africa/Conakry",
    "Africa/Dakar",
    "Africa/Dar_es_Salaam",
    "Africa/Djibouti",
    "Africa/Douala",
    "Africa/El_Aaiun",
    "Africa/Freetown",
    "Africa/Gaborone",
    "Africa/Harare",
    "Africa/Johannesburg",
    "Africa/Juba",
    "Africa/Kampala",
    "Africa/Khartoum",
    "Africa/Kigali",
    "Africa/Kinshasa",
    "Africa/Lagos",
    "Africa/Libreville",
    "Africa/Lome",
    "Africa/Luanda",
    "Africa/Lubumbashi",
    "Africa/Lusaka",
    "Africa/Malabo",
    "Africa/Maputo",
    "Africa/Maseru",
    "Africa/Mbabane",
    "Africa/Mogadishu",
    "Africa/Monrovia",
    "Africa/Nairobi",
    "Africa/Ndjamena",
    "Africa/Niamey",
    "Africa/Nouakchott",
    "Africa/Ouagadougou",
    "Africa/Porto-Novo",
    "Africa/Sao_Tome",
    "Africa/Tripoli",
    "Africa/Tunis",
    "Africa/Windhoek",
    "America/Adak",
    "America/Anchorage",
    "America/Anguilla",
    "America/Antigua",
    "America/Araguaina",
    "America/Argentina/Buenos_Aires",
    "America/Argentina/Catamarca",
    "America/Argentina/Cordoba",
    "America/Argentina/Jujuy",
    "America/Argentina/La_Rioja",
    "America/Argentina/Mendoza",
    "America/Argentina/Rio_Gallegos",
    "America/Argentina/Salta",
    "America/Argentina/San_Juan",
    "America/Argentina/San_Luis",
    "America/Argentina/Tucuman",
    "America/Argentina/Ushuaia",
    "America/Aruba",
    "America/Asuncion",
    "America/Atikokan",
    "America/Bahia",
    "America/Bahia_Banderas",
    "America/Barbados",
    "America/Belem",
    "America/Belize",
    "America/Blanc-Sablon",
    "America/Boa_Vista",
    "America/Bogota",
    "America/Boise",
    "America/Cambridge_Bay",
    "America/Campo_Grande",
    "America/Cancun",
    "America/Caracas",
    "America/Cayenne",
    "America/Cayman",
    "America/Chicago",
    "America/Chihuahua",
    "America/Ciudad_Juarez",
    "America/Costa_Rica",
    "America/Creston",
    "America/Cuiaba",
    "America/Curacao",
    "America/Danmarkshavn",
    "America/Dawson",
    "America/Dawson_Creek",
    "America/Denver",
    "America/Detroit",
    "America/Dominica",
    "America/Edmonton",
    "America/Eirunepe",
    "America/El_Salvador",
    "America/Fort_Nelson",
    "America/Fortaleza",
    "America/Glace_Bay",
    "America/Goose_Bay",
    "America/Grand_Turk",
    "America/Grenada",
    "America/Guadeloupe",
    "America/Guatemala",
    "America/Guayaquil",
    "America/Guyana",
    "America/Halifax",
    "America/Havana",
    "America/Hermosillo",
    "America/Indiana/Indianapolis",
    "America/Indiana/Knox",
    "America/Indiana/Marengo",
    "America/Indiana/Petersburg",
    "America/Indiana/Tell_City",
    "America/Indiana/Vevay",
    "America/Indiana/Vincennes",
    "America/Indiana/Winamac",
    "America/Inuvik",
    "America/Iqaluit",
    "America/Jamaica",
    "America/Juneau",
    "America/Kentucky/Louisville",
    "America/Kentucky/Monticello",
    "America/Kralendijk",
    "America/La_Paz",
    "America/Lima",
    "America/Los_Angeles",
    "America/Lower_Princes",
    "America/Maceio",
    "America/Managua",
    "America/Manaus",
    "America/Marigot",
    "America/Martinique",
    "America/Matamoros",
    "America/Mazatlan",
    "America/Menominee",
    "America/Merida",
    "America/Metlakatla",
    "America/Mexico_City",
    "America/Miquelon",
    "America/Moncton",
    "America/Monterrey",
    "America/Montevideo",
    "America/Montserrat",
    "America/Nassau",
    "America/New_York",
    "America/Nome",
    "America/Noronha",
    "America/North_Dakota/Beulah",
    "America/North_Dakota/Center",
    "America/North_Dakota/New_Salem",
    "America/Nuuk",
    "America/Ojinaga",
    "America/Panama",
    "America/Paramaribo",
    "America/Phoenix",
    "America/Port-au-Prince",
    "America/Port_of_Spain",
    "America/Porto_Velho",
    "America/Puerto_Rico",
    "America/Punta_Arenas",
    "America/Rankin_Inlet",
    "America/Recife",
    "America/Regina",
    "America/Resolute",
    "America/Rio_Branco",
    "America/Santarem",
    "America/Santiago",
    "America/Santo_Domingo",
    "America/Sao_Paulo",
    "America/Scoresbysund",
    "America/Sitka",
    "America/St_Barthelemy",
    "America/St_Johns",
    "America/St_Kitts",
    "America/St_Lucia",
    "America/St_Thomas",
    "America/St_Vincent",
    "America/Swift_Current",
    "America/Tegucigalpa",
    "America/Thule",
    "America/Tijuana",
    "America/Toronto",
    "America/Tortola",
    "America/Vancouver",
    "America/Whitehorse",
    "America/Winnipeg",
    "America/Yakutat",
    "Antarctica/Casey",
    "Antarctica/Davis",
    "Antarctica/DumontDUrville",
    "Antarctica/Macquarie",
    "Antarctica/Mawson",
    "Antarctica/McMurdo",
    "Antarctica/Palmer",
    "Antarctica/Rothera",
    "Antarctica/Syowa",
    "Antarctica/Troll",
    "Antarctica/Vostok",
    "Arctic/Longyearbyen",
    "Asia/Aden",
    "Asia/Almaty",
    "Asia/Amman",
    "Asia/Anadyr",
    "Asia/Aqtau",
    "Asia/Aqtobe",
    "Asia/Ashgabat",
    "Asia/Atyrau",
    "Asia/Baghdad",
    "Asia/Bahrain",
    "Asia/Baku",
    "Asia/Bangkok",
    "Asia/Barnaul",
    "Asia/Beirut",
    "Asia/Bishkek",
    "Asia/Brunei",
    "Asia/Chita",
    "Asia/Choibalsan",
    "Asia/Colombo",
    "Asia/Damascus",
    "Asia/Dhaka",
    "Asia/Dili",
    "Asia/Dubai",
    "Asia/Dushanbe",
    "Asia/Famagusta",
    "Asia/Gaza",
    "Asia/Hebron",
    "Asia/Ho_Chi_Minh",
    "Asia/Hong_Kong",
    "Asia/Hovd",
    "Asia/Irkutsk",
    "Asia/Jakarta",
    "Asia/Jayapura",
    "Asia/Jerusalem",
    "Asia/Kabul",
    "Asia/Kamchatka",
    "Asia/Karachi",
    "Asia/Kathmandu",
    "Asia/Khandyga",
    "Asia/Kolkata",
    "Asia/Krasnoyarsk",
    "Asia/Kuala_Lumpur",
    "Asia/Kuching",
    "Asia/Kuwait",
    "Asia/Macau",
    "Asia/Magadan",
    "Asia/Makassar",
    "Asia/Manila",
    "Asia/Muscat",
    "Asia/Nicosia",
    "Asia/Novokuznetsk",
    "Asia/Novosibirsk",
    "Asia/Omsk",
    "Asia/Oral",
    "Asia/Phnom_Penh",
    "Asia/Pontianak",
    "Asia/Pyongyang",
    "Asia/Qatar",
    "Asia/Qostanay",
    "Asia/Qyzylorda",
    "Asia/Riyadh",
    "Asia/Sakhalin",
    "Asia/Samarkand",
    "Asia/Seoul",
    "Asia/Shanghai",
    "Asia/Singapore",
    "Asia/Srednekolymsk",
    "Asia/Taipei",
    "Asia/Tashkent",
    "Asia/Tbilisi",
    "Asia/Tehran",
    "Asia/Thimphu",
    "Asia/Tokyo",
    "Asia/Tomsk",
    "Asia/Ulaanbaatar",
    "Asia/Urumqi",
    "Asia/Ust-Nera",
    "Asia/Vientiane",
    "Asia/Vladivostok",
    "Asia/Yakutsk",
    "Asia/Yangon",
    "Asia/Yekaterinburg",
    "Asia/Yerevan",
    "Atlantic/Azores",
    "Atlantic/Bermuda",
    "Atlantic/Canary",
    "Atlantic/Cape_Verde",
    "Atlantic/Faroe",
    "Atlantic/Madeira",
    "Atlantic/Reykjavik",
    "Atlantic/South_Georgia",
    "Atlantic/St_Helena",
    "Atlantic/Stanley",
    "Australia/Adelaide",
    "Australia/Brisbane",
    "Australia/Broken_Hill",
    "Australia/Darwin",
    "Australia/Eucla",
    "Australia/Hobart",
    "Australia/Lindeman",
    "Australia/Lord_Howe",
    "Australia/Melbourne",
    "Australia/Perth",
    "Australia/Sydney",
    "Canada/Atlantic",
    "Canada/Central",
    "Canada/Eastern",
    "Canada/Mountain",
    "Canada/Newfoundland",
    "Canada/Pacific",
    "Europe/Amsterdam",
    "Europe/Andorra",
    "Europe/Astrakhan",
    "Europe/Athens",
    "Europe/Belgrade",
    "Europe/Berlin",
    "Europe/Bratislava",
    "Europe/Brussels",
    "Europe/Bucharest",
    "Europe/Budapest",
    "Europe/Busingen",
    "Europe/Chisinau",
    "Europe/Copenhagen",
    "Europe/Dublin",
    "Europe/Gibraltar",
    "Europe/Guernsey",
    "Europe/Helsinki",
    "Europe/Isle_of_Man",
    "Europe/Istanbul",
    "Europe/Jersey",
    "Europe/Kaliningrad",
    "Europe/Kirov",
    "Europe/Kyiv",
    "Europe/Lisbon",
    "Europe/Ljubljana",
    "Europe/London",
    "Europe/Luxembourg",
    "Europe/Madrid",
    "Europe/Malta",
    "Europe/Mariehamn",
    "Europe/Minsk",
    "Europe/Monaco",
    "Europe/Moscow",
    "Europe/Oslo",
    "Europe/Paris",
    "Europe/Podgorica",
    "Europe/Prague",
    "Europe/Riga",
    "Europe/Rome",
    "Europe/Samara",
    "Europe/San_Marino",
    "Europe/Sarajevo",
    "Europe/Saratov",
    "Europe/Simferopol",
    "Europe/Skopje",
    "Europe/Sofia",
    "Europe/Stockholm",
    "Europe/Tallinn",
    "Europe/Tirane",
    "Europe/Ulyanovsk",
    "Europe/Vaduz",
    "Europe/Vatican",
    "Europe/Vienna",
    "Europe/Vilnius",
    "Europe/Volgograd",
    "Europe/Warsaw",
    "Europe/Zagreb",
    "Europe/Zurich",
    "GMT",
    "Indian/Antananarivo",
    "Indian/Chagos",
    "Indian/Christmas",
    "Indian/Cocos",
    "Indian/Comoro",
    "Indian/Kerguelen",
    "Indian/Mahe",
    "Indian/Maldives",
    "Indian/Mauritius",
    "Indian/Mayotte",
    "Indian/Reunion",
    "Pacific/Apia",
    "Pacific/Auckland",
    "Pacific/Bougainville",
    "Pacific/Chatham",
    "Pacific/Chuuk",
    "Pacific/Easter",
    "Pacific/Efate",
    "Pacific/Fakaofo",
    "Pacific/Fiji",
    "Pacific/Funafuti",
    "Pacific/Galapagos",
    "Pacific/Gambier",
    "Pacific/Guadalcanal",
    "Pacific/Guam",
    "Pacific/Honolulu",
    "Pacific/Kanton",
    "Pacific/Kiritimati",
    "Pacific/Kosrae",
    "Pacific/Kwajalein",
    "Pacific/Majuro",
    "Pacific/Marquesas",
    "Pacific/Midway",
    "Pacific/Nauru",
    "Pacific/Niue",
    "Pacific/Norfolk",
    "Pacific/Noumea",
    "Pacific/Pago_Pago",
    "Pacific/Palau",
    "Pacific/Pitcairn",
    "Pacific/Pohnpei",
    "Pacific/Port_Moresby",
    "Pacific/Rarotonga",
    "Pacific/Saipan",
    "Pacific/Tahiti",
    "Pacific/Tarawa",
    "Pacific/Tongatapu",
    "Pacific/Wake",
    "Pacific/Wallis",
    "US/Alaska",
    "US/Arizona",
    "US/Central",
    "US/Eastern",
    "US/Hawaii",
    "US/Mountain",
    "US/Pacific",
    "UTC",
];

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` / inline-admin body.
    Forbidden,
    /// 404, `Project.resolve` miss.
    ProjectNotFound,
    /// 404, `ObjectDoesNotExist` branch (bare `.get()` miss).
    ObjectNotFound,
    /// 400, Django `ValidationError` branch (the `save()` unset-default
    /// guard).
    ValidationFailed,
    /// 400, `IntegrityError` branch (unique/NN/FK violations, including
    /// Q-identifier-case).
    IntegrityFailed,
    /// 500, generic branch.
    ServerError,
    /// A pre-rendered exact body with its status.
    Raw(StatusCode, String),
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        use pidash_services::app_project::tasks as t;
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, super::gates::ANON_BODY.to_owned()),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                super::gates::FORBIDDEN_BODY.to_owned(),
            ),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::ObjectNotFound => (StatusCode::NOT_FOUND, t::OBJECT_NOT_FOUND_BODY.to_owned()),
            Denial::ValidationFailed => {
                (StatusCode::BAD_REQUEST, t::VALIDATION_ERROR_BODY.to_owned())
            }
            Denial::IntegrityFailed => {
                (StatusCode::BAD_REQUEST, t::INTEGRITY_ERROR_BODY.to_owned())
            }
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                t::SERVER_ERROR_BODY.to_owned(),
            ),
            Denial::Raw(status, body) => (*status, body.clone()),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

/// Map a `sqlx` failure the way `handle_exception` does: integrity-class
/// database errors (`23xxx`: unique, not-null, FK, check violations)
/// answer the `IntegrityError` 400; everything else is the generic 500.
fn db_denial(error: sqlx::Error) -> Denial {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().is_some_and(|code| code.starts_with("23")) {
            return Denial::IntegrityFailed;
        }
    }
    Denial::ServerError
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// DRF `JSONRenderer.render`: after dumping, `\\u2028`/`\\u2029` are
/// always escaped (lowercase hex) so the output is a strict JavaScript
/// subset — `serde_json` emits them raw, so every rendered HTTP body
/// passes through here.
fn escape_u2028(body: String) -> String {
    body.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("project response")
}

fn json_ok(body: String) -> Response {
    json_response(StatusCode::OK, escape_u2028(body))
}

fn json_created(body: String) -> Response {
    json_response(StatusCode::CREATED, escape_u2028(body))
}

fn no_content() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty 204")
}

// ---------------------------------------------------------------------------
// Query map (Django QueryDict: repeats legal, .get returns the last)
// ---------------------------------------------------------------------------

/// One query value, repeated or not.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map every list handler extracts.
pub type QueryMap = HashMap<String, OneOrMany>;

/// Django `QueryDict.get`: the last value, or `None` when absent.
pub fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => one.clone(),
        OneOrMany::Many(many) => many.last().cloned().unwrap_or_default(),
    })
}

/// Django truthiness of `request.GET.get(key, False)`: absent is falsy;
/// a present value is truthy unless it is the empty string.
pub fn query_truthy(query: &QueryMap, key: &str) -> bool {
    query_last(query, key).is_some_and(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// Request context: auth + rewrite + tenant + membership
// ---------------------------------------------------------------------------

/// Authenticated actor plus time zone (`TimezoneMixin.initial` activates
/// the user's zone; datetimes render in it).
pub struct Actor {
    pub id: uuid::Uuid,
    pub timezone: Tz,
}

/// Session auth (`BaseSessionAuthentication` + `IsAuthenticated` on the
/// views): anonymous answers the DRF `NotAuthenticated` body before
/// anything else runs (the `app_views_search` precedent).
async fn actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Actor, Denial> {
    let pool = pool_of(state)?;
    let resolved =
        crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
            .await
            .map_err(|_| Denial::ServerError)?
            .ok_or(Denial::Unauthorized)?;
    Ok(Actor {
        id: resolved.id,
        timezone: resolved.timezone,
    })
}

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// `_rewrite_project_kwarg` (`app/views/base.py:49-81`): UUID-looking input
/// passes through unverified (the handlers scope it — the L6
/// `is_uuid_like` rule, same spellings as `uuid.UUID()`); anything else
/// matches `UPPER(identifier)` in the workspace; a miss answers the
/// resolve 404. The doubled `deleted_at` guard (`Project.objects` manager
/// plus the explicit filter) is one predicate here.
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    use pidash_db::app_project::models::project::{classify_lookup, ProjectLookup};
    match classify_lookup(raw) {
        ProjectLookup::Pk(id) => Ok(id),
        ProjectLookup::Identifier(name) => {
            let row: Option<(uuid::Uuid,)> = sqlx::query_as(
                r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
                   WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
            )
            .bind(slug)
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(db_denial)?;
            row.map(|row| row.0).ok_or(Denial::ProjectNotFound)
        }
    }
}

/// Membership roles for the gates, resolved with the same row filters
/// Python uses: active, non-deleted rows scoped to the workspace slug (and
/// project id for the project row) — the `app_views_search` precedent.
/// The joined `workspaces` rows carry no soft-delete guard (forward-FK
/// traversal applies only the base model's manager — L6 ported bug 9).
struct Membership {
    workspace_role: Option<i16>,
    project_role: Option<i16>,
}

async fn membership(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Membership, Denial> {
    let workspace_role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    let row: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.project_id = $2 AND pm.member_id = $3
             AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    Ok(Membership {
        workspace_role: workspace_role.map(|row| row.0),
        project_role: row.map(|row| row.0),
    })
}

/// Active workspace role for `(user, slug)`, or `None` (no row), for the
/// workspace-level gates. Mirrors the `allow_permission` workspace lookup
/// (`is_active=True`, soft-deleted rows excluded, `app/permissions/base.py`).
async fn workspace_role(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
    slug: &str,
) -> Result<Option<i16>, Denial> {
    let row: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    Ok(row.map(|row| row.0))
}

/// Build `AllowFacts` for one gate row: the workspace half from the
/// workspace role, the project half from the project role (absent on the
/// collection paths, which carry no project id).
fn allow_facts(
    slug: &str,
    workspace_role: Option<i16>,
    project_role: Option<i16>,
    allowed: &[i32],
) -> AllowFacts {
    let ws = workspace_role.map(i32::from);
    let proj = project_role.map(i32::from);
    AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: ws.is_some(),
        has_allowed_workspace_role: ws.is_some_and(|role| allowed.contains(&role)),
        is_creator: false,
        has_allowed_project_role: proj.is_some_and(|role| allowed.contains(&role)),
        is_project_member: proj.is_some(),
        is_workspace_admin: ws == Some(ROLE_ADMIN),
    }
}

/// Enforce the gate-table row for one method+path: anonymous never
/// reaches here ([`actor`] denied first); a deny answers the decorator
/// 403. `project_role` is `None` on paths without a project kwarg.
fn check_gate(
    method: &str,
    path: &str,
    slug: &str,
    workspace_role: Option<i16>,
    project_role: Option<i16>,
) -> Result<(), Denial> {
    use super::gates::{decide_gate, deny_body, gate_for, tenant_context, GateOutcome};
    let row = gate_for(method, path).ok_or(Denial::ServerError)?;
    let scope = tenant_context(slug);
    let roles: &[i32] = match &row.gate {
        super::gates::Gate::Workspace { roles } | super::gates::Gate::Project { roles } => roles,
        _ => &[],
    };
    match decide_gate(
        &row.gate,
        &scope,
        &allow_facts(slug, workspace_role, project_role, roles),
    ) {
        GateOutcome::Allow => Ok(()),
        GateOutcome::Deny => {
            debug_assert!(deny_body(&row.gate).is_some());
            Err(Denial::Forbidden)
        }
        GateOutcome::Unauthenticated => Err(Denial::Unauthorized),
    }
}

/// `base_host(request, is_app=True)` (`utils/host.py:17-66`):
/// `APP_BASE_URL` when set, else `WEB_URL`, else `ImproperlyConfigured`
/// → 500. (The value feeds task kwargs only, never HTTP bytes.)
fn request_origin(state: &AppState) -> Result<String, Denial> {
    // `base_host` uses `or`: an empty string means unset (each level
    // falls through independently).
    let urls = &state.settings().urls;
    urls.app_base_url
        .clone()
        .filter(|url| !url.is_empty())
        .or_else(|| urls.web_url.clone().filter(|url| !url.is_empty()))
        .ok_or(Denial::ServerError)
}

/// `timezone.now()` truncated to microseconds: Postgres `timestamptz`
/// stores micros, and Python datetimes carry micros at most — the archive
/// response renders the assigned value, so nanos would leak a 9-digit
/// fraction Python never emits.
fn utc_now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    let micros = now.timestamp() * 1_000_000 + i64::from(now.timestamp_subsec_micros());
    DateTime::from_timestamp(
        micros.div_euclid(1_000_000),
        (micros.rem_euclid(1_000_000) as u32) * 1000,
    )
    .expect("now")
}

// ---------------------------------------------------------------------------
// Rendering: datetimes, rows, serializer shapes
// ---------------------------------------------------------------------------

/// DRF `DateTimeField.to_representation` for one instant: shifted into the
/// request user's zone (already enforced by `TimezoneMixin`), `isoformat`
/// with `+00:00` rewritten to `Z`, microseconds only when nonzero —
/// `crate::serializer::render_datetime_in` ports exactly this.
fn render_dt(value: &DateTime<Utc>, timezone: Tz) -> String {
    crate::serializer::render_datetime_in(value, &timezone)
}

fn render_dt_opt(value: Option<DateTime<Utc>>, timezone: Tz) -> Option<String> {
    value.map(|value| render_dt(&value, timezone))
}

/// `str(archived_at)` (`base.py:441`): space separator, `+00:00` suffix,
/// microseconds only when nonzero.
fn render_python_datetime(value: &DateTime<Utc>) -> String {
    let mut out = value.format("%Y-%m-%d %H:%M:%S").to_string();
    let nanos = value.timestamp_subsec_nanos();
    if nanos != 0 {
        out.push_str(&format!(".{:06}", nanos / 1000));
    }
    out.push_str("+00:00");
    out
}

/// `str(value)` for a truthy JSON body value, as `validate()` calls it on
/// `description_html` before sanitizing (`serializers/project.py:47`):
/// strings pass through; numbers/bools render like Python; objects/arrays
/// render like CPython `repr` (single quotes, `True`/`False`/`None`).
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", python_repr_string(key), python_repr(item)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// CPython `repr` for one JSON value (nested position: strings quoted).
fn python_repr(value: &Value) -> String {
    match value {
        Value::String(text) => python_repr_string(text),
        Value::Array(_) | Value::Object(_) => python_str(value),
        _ => python_str(value),
    }
}

/// CPython `repr` for one string: single quotes unless the text contains
/// one (but no double quote), with `\`/`\n`/`\r`/`\t` escaped.
fn python_repr_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    out.push(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            ch => out.push(ch),
        }
    }
    out.push(quote);
    out
}

/// `json.dumps(value)` with CPython defaults (`", "`/`": "` separators,
/// `ensure_ascii=True`), as `partial_update` dumps `current_instance`
/// (`base.py:341`). ASCII escapes render `\uXXXX` lowercase, matching
/// CPython; lone surrogates cannot occur (Rust strings are valid UTF-8).
/// Control characters escape as CPython does (`\n` short, others `\u00XX`).
fn cpython_dumps(value: &Value) -> String {
    let mut out = String::new();
    cpython_write(&mut out, value);
    out
}

fn cpython_write(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => cpython_write_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                cpython_write(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                cpython_write_string(out, key);
                out.push_str(": ");
                cpython_write(out, item);
            }
            out.push('}');
        }
    }
}

fn cpython_write_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch if (ch as u32) < 0x7F => out.push(ch),
            ch => {
                let code = ch as u32;
                if code > 0xFFFF {
                    let code = code - 0x1_0000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xD800 + (code >> 10),
                        0xDC00 + (code & 0x3FF)
                    ));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
        }
    }
    out.push('"');
}

/// One `get_queryset` row: every live `projects` column plus the four
/// serializer annotations (`queries::ANNOTATION_SELECT`, L6). The
/// `select_related` joins change no rendered byte, so they are not
/// re-fetched; `members_list` is a separate prefetch like Django's.
struct ProjectRow {
    id: uuid::Uuid,
    name: String,
    description: String,
    description_text: Option<Value>,
    description_html: Option<Value>,
    network: i16,
    identifier: String,
    workspace_id: uuid::Uuid,
    default_assignee_id: Option<uuid::Uuid>,
    project_lead_id: Option<uuid::Uuid>,
    emoji: Option<String>,
    icon_prop: Option<Value>,
    module_view: bool,
    cycle_view: bool,
    issue_views_view: bool,
    page_view: bool,
    intake_view: bool,
    is_time_tracking_enabled: bool,
    is_issue_type_enabled: bool,
    guest_view_all_features: bool,
    cover_image: Option<String>,
    cover_image_asset_id: Option<uuid::Uuid>,
    estimate_id: Option<uuid::Uuid>,
    archive_in: i32,
    close_in: i32,
    logo_props: Value,
    default_state_id: Option<uuid::Uuid>,
    archived_at: Option<DateTime<Utc>>,
    is_default: bool,
    timezone: String,
    external_source: Option<String>,
    external_id: Option<String>,
    repo_url: String,
    base_branch: String,
    agent_default_interval_seconds: i32,
    agent_default_max_ticks: i32,
    agent_review_default_interval_seconds: i32,
    agent_test_default_interval_seconds: i32,
    agent_ticking_enabled: bool,
    default_agent_executor: String,
    members_can_edit_states: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    is_favorite: bool,
    sort_order: Option<f64>,
    member_role: Option<i16>,
    anchor: Option<String>,
}

impl ProjectRow {
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            description_text: row.try_get("description_text")?,
            description_html: row.try_get("description_html")?,
            network: row.try_get("network")?,
            identifier: row.try_get("identifier")?,
            workspace_id: row.try_get("workspace_id")?,
            default_assignee_id: row.try_get("default_assignee_id")?,
            project_lead_id: row.try_get("project_lead_id")?,
            emoji: row.try_get("emoji")?,
            icon_prop: row.try_get("icon_prop")?,
            module_view: row.try_get("module_view")?,
            cycle_view: row.try_get("cycle_view")?,
            issue_views_view: row.try_get("issue_views_view")?,
            page_view: row.try_get("page_view")?,
            intake_view: row.try_get("intake_view")?,
            is_time_tracking_enabled: row.try_get("is_time_tracking_enabled")?,
            is_issue_type_enabled: row.try_get("is_issue_type_enabled")?,
            guest_view_all_features: row.try_get("guest_view_all_features")?,
            cover_image: row.try_get("cover_image")?,
            cover_image_asset_id: row.try_get("cover_image_asset_id")?,
            estimate_id: row.try_get("estimate_id")?,
            archive_in: row.try_get("archive_in")?,
            close_in: row.try_get("close_in")?,
            logo_props: row.try_get("logo_props")?,
            default_state_id: row.try_get("default_state_id")?,
            archived_at: row.try_get("archived_at")?,
            is_default: row.try_get("is_default")?,
            timezone: row.try_get("timezone")?,
            external_source: row.try_get("external_source")?,
            external_id: row.try_get("external_id")?,
            repo_url: row.try_get("repo_url")?,
            base_branch: row.try_get("base_branch")?,
            agent_default_interval_seconds: row.try_get("agent_default_interval_seconds")?,
            agent_default_max_ticks: row.try_get("agent_default_max_ticks")?,
            agent_review_default_interval_seconds: row
                .try_get("agent_review_default_interval_seconds")?,
            agent_test_default_interval_seconds: row
                .try_get("agent_test_default_interval_seconds")?,
            agent_ticking_enabled: row.try_get("agent_ticking_enabled")?,
            default_agent_executor: row.try_get("default_agent_executor")?,
            members_can_edit_states: row.try_get("members_can_edit_states")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            created_by_id: row.try_get("created_by_id")?,
            updated_by_id: row.try_get("updated_by_id")?,
            deleted_at: row.try_get("deleted_at")?,
            is_favorite: row.try_get("is_favorite")?,
            sort_order: row.try_get("sort_order")?,
            member_role: row.try_get("member_role")?,
            anchor: row.try_get("anchor")?,
        })
    }
}

/// `ProjectViewSet.get_queryset` (`base.py:52-99`): live projects of the
/// workspace with the four annotations, scoped for guests/members, minus
/// the no-op filter backends (`filterset_fields=[]`, `search_fields=[]`).
/// The scoping probes run before the fetch in the handlers, exactly as
/// Python evaluates them before appending the filters.
///
/// Two shapes: [`fetch_project_row_scoped`] (retrieve/PUT/destroy/PATCH
/// re-fetch — the scoped queryset or `.get(pk, workspace__slug=slug)`)
/// and the bare [`fetch_project_row`] (PATCH main fetch `.get(pk)`,
/// PUT re-render, create re-fetch, `current_instance`).
async fn fetch_project_row(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    slug: &str,
) -> Result<Option<ProjectRow>, Denial> {
    fetch_project_row_opts(pool, project_id, user_id, slug, true, false).await
}

/// `fetch_project_row` with the soft-delete filter optional: PUT
/// re-renders the in-memory instance after `save()`, so a PUT that sets
/// `deleted_at` answers 200 with the deleted row — no re-fetch filter.
async fn fetch_project_row_opts(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    slug: &str,
    live_only: bool,
    scoped: bool,
) -> Result<Option<ProjectRow>, Denial> {
    let deleted = if live_only {
        "AND p.deleted_at IS NULL"
    } else {
        ""
    };
    let scope = if scoped {
        "JOIN workspaces w ON w.id = p.workspace_id AND w.slug = $3"
    } else {
        ""
    };
    let sql = format!(
        r#"SELECT DISTINCT p.id, p.name, p.description, p.description_text, p.description_html,
              p.network, p.identifier, p.workspace_id,
              p.default_assignee_id, p.project_lead_id, p.emoji, p.icon_prop,
              p.module_view, p.cycle_view, p.issue_views_view, p.page_view, p.intake_view,
              p.is_time_tracking_enabled, p.is_issue_type_enabled, p.guest_view_all_features,
              p.cover_image, p.cover_image_asset_id, p.estimate_id,
              p.archive_in, p.close_in, p.logo_props, p.default_state_id, p.archived_at,
              p.is_default, p.timezone, p.external_source, p.external_id,
              p.repo_url, p.base_branch,
              p.agent_default_interval_seconds, p.agent_default_max_ticks,
              p.agent_review_default_interval_seconds, p.agent_test_default_interval_seconds,
              p.agent_ticking_enabled,
              p.default_agent_executor, p.members_can_edit_states,
              p.created_at, p.updated_at, p.created_by_id, p.updated_by_id, p.deleted_at,
              EXISTS (SELECT 1 FROM user_favorites uf
                      WHERE uf.deleted_at IS NULL AND uf.user_id = $2
                        AND uf.project_id = p.id AND uf.entity_type = 'project'
                        AND uf.entity_identifier = p.id) AS is_favorite,
              (SELECT pup.sort_order FROM project_user_properties pup
               WHERE pup.deleted_at IS NULL AND pup.project_id = p.id AND pup.user_id = $2
                 AND pup.workspace_id = p.workspace_id ORDER BY pup.created_at DESC) AS sort_order,
              (SELECT pm.role FROM project_members pm
               WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $2
                 AND pm.project_id = p.id ORDER BY pm.created_at DESC) AS member_role,
              {anchor} AS anchor
           FROM projects p {scope}
           WHERE p.id = $1 {deleted}"#,
        deleted = deleted,
        scope = scope,
        anchor = anchor_sql("$3"),
    );
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(project_id)
        .bind(user_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(db_denial)?;
    row.map(|row| ProjectRow::from_row(&row).map_err(|_| Denial::ServerError))
        .transpose()
}

/// [`fetch_project_row`] scoped to the workspace slug (live rows): the
/// retrieve/PUT `get_queryset` fetches, the destroy
/// `.get(pk, workspace__slug=slug)`, and the PATCH re-fetch
/// (`base.py:367` uses the scoped queryset — a miss there is the 500
/// `AttributeError` branch).
async fn fetch_project_row_scoped(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    slug: &str,
) -> Result<Option<ProjectRow>, Denial> {
    fetch_project_row_opts(pool, project_id, user_id, slug, true, true).await
}

/// One `members_list` prefetch row (`base.py:71-81`): active memberships
/// of the project in the workspace, newest first, with the member's bot
/// bit for `get_members` (`serializers/project.py:127-128`).
struct MemberRow {
    member_id: Option<uuid::Uuid>,
    is_bot: Option<bool>,
}

async fn fetch_members(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    slug: &str,
) -> Result<Vec<MemberRow>, Denial> {
    let rows = sqlx::query(
        r#"SELECT pm.member_id AS member_id, u.is_bot AS is_bot
           FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           LEFT JOIN users u ON u.id = pm.member_id
           WHERE pm.project_id = $1 AND w.slug = $2
             AND pm.deleted_at IS NULL AND pm.is_active
           ORDER BY pm.created_at DESC"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_all(pool)
    .await
    .map_err(db_denial)?;
    rows.iter()
        .map(|row| {
            Ok(MemberRow {
                member_id: row.try_get("member_id").map_err(|_| Denial::ServerError)?,
                is_bot: row.try_get("is_bot").map_err(|_| Denial::ServerError)?,
            })
        })
        .collect()
}

/// `ProjectListSerializer.get_members`: active, non-bot member ids newest
/// first (L1 `project_list_members`). An active row with a NULL member 500s
/// (`None.is_bot` — Q-null-member-500).
fn render_members(members: &[MemberRow]) -> Result<Vec<String>, Denial> {
    let mut out = Vec::with_capacity(members.len());
    for member in members {
        let Some(id) = member.member_id else {
            return Err(Denial::ServerError);
        };
        if member.is_bot != Some(true) {
            out.push(id.to_string());
        }
    }
    Ok(out)
}

/// `ProjectListSerializer.get_next_work_item_sequence`
/// (`serializers/project.py:131-132`, L1 `next_work_item_sequence`):
/// `MAX(sequence)+1`, or 1 when the table holds no row (or a 0 max —
/// falsy). The aggregate runs on the soft manager (`deleted_at` guard);
/// the `deleted` boolean column is not filtered, as written.
async fn next_sequence(pool: &sqlx::PgPool, project_id: &uuid::Uuid) -> Result<i64, Denial> {
    let row: (Option<i64>,) = sqlx::query_as(
        r#"SELECT MAX(sequence) FROM issue_sequences
           WHERE project_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .map_err(db_denial)?;
    Ok(pidash_services::app_project::ser_project::next_work_item_sequence(row.0))
}

/// `FileAsset.asset_url` (`db/models/asset.py:80-103`) for one attached
/// asset id. Forward-FK reads use the plain `_base_manager`
/// (`related_descriptors.py:153`), so the read is unfiltered even
/// though `FileAsset.objects` is the filtering `SoftDeletionManager`:
/// a soft-deleted asset still answers its URL (live-probed 200), and
/// a dangling id raises `DoesNotExist` → unhandled 500 (unreachable —
/// the `projects_cover_image_asset_id` FK forbids dangling ids).
/// Static kinds answer the static path; `ISSUE_ATTACHMENT` and the four
/// `*_DESCRIPTION` kinds answer workspace-scoped URLs (the workspace
/// slug traverses the same unfiltered manager — a NULL workspace is
/// the `AttributeError` 500); anything else (including a NULL or
/// unrecognized entity type) is `None`.
/// One `file_assets` cover row: entity type plus the nullable owner
/// columns the entity-URL table reads.
type CoverAssetRow = (
    Option<String>,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
);

async fn file_asset_url(
    pool: &sqlx::PgPool,
    asset_id: &uuid::Uuid,
) -> Result<Option<String>, Denial> {
    let row: Option<CoverAssetRow> = sqlx::query_as(
        r#"SELECT entity_type, workspace_id, project_id, issue_id
               FROM file_assets WHERE id = $1"#,
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    let Some((entity_type, workspace_id, project_id, issue_id)) = row else {
        return Err(Denial::ServerError);
    };
    match entity_type.as_deref() {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Ok(Some(format!("/api/assets/v2/static/{asset_id}/")))
        }
        Some("ISSUE_ATTACHMENT") => {
            let slug = asset_workspace_slug(pool, workspace_id).await?;
            Ok(Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{}/issues/{}/attachments/{asset_id}/",
                opt_uuid_py(project_id),
                opt_uuid_py(issue_id),
            )))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            let slug = asset_workspace_slug(pool, workspace_id).await?;
            Ok(Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{}/{asset_id}/",
                opt_uuid_py(project_id),
            )))
        }
        _ => Ok(None),
    }
}

/// `asset.workspace.slug`: a NULL workspace is the `AttributeError` 500;
/// the forward-FK read uses the plain `_base_manager`, so the lookup
/// is unfiltered (miss → `DoesNotExist` 500 — unreachable via the FK).
async fn asset_workspace_slug(
    pool: &sqlx::PgPool,
    workspace_id: Option<uuid::Uuid>,
) -> Result<String, Denial> {
    let Some(id) = workspace_id else {
        return Err(Denial::ServerError);
    };
    let row: Option<(String,)> = sqlx::query_as(r#"SELECT slug FROM workspaces WHERE id = $1"#)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_denial)?;
    row.map(|row| row.0).ok_or(Denial::ServerError)
}

/// f-string rendering of a nullable UUID FK (`None` → `"None"`).
fn opt_uuid_py(id: Option<uuid::Uuid>) -> String {
    id.map(|id| id.to_string())
        .unwrap_or_else(|| "None".to_owned())
}

/// `Project.cover_image_url` (`db/models/project.py:175-185`, L5
/// `cover_image_url`): an attached asset answers its `asset_url` as-is —
/// even `None` for an unrecognized entity, with NO fallback to the legacy
/// text; otherwise the legacy `cover_image` text; else `None`.
async fn cover_image_url(
    pool: &sqlx::PgPool,
    asset_id: Option<uuid::Uuid>,
    cover_image: Option<&str>,
) -> Result<Option<String>, Denial> {
    use pidash_db::app_project::models::project::cover_image_url as pick;
    let has_asset = asset_id.is_some();
    let asset_url: Option<String> = match asset_id {
        None => None,
        Some(id) => file_asset_url(pool, &id).await?,
    };
    Ok(pick(has_asset, asset_url.as_deref(), cover_image).map(str::to_owned))
}

/// Per-request user inputs to `agent_executor_options`
/// (`core/agent_execution.py:69-80`): the bot bit plus the BYOK key
/// presence bit. Python re-reads the key config per project row; the
/// verdict cannot change mid-request, so it is read once.
struct ExecutorUser {
    flags: pidash_services::dispatch::policy::UserFlags,
    llm_has_key: bool,
}

async fn executor_user(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<ExecutorUser, Denial> {
    use pidash_services::dispatch::policy::UserFlags;
    // `resolve_actor` guarantees an authenticated, active row; only the
    // bot bit is still unknown.
    let row: Option<(bool,)> = sqlx::query_as(r#"SELECT is_bot FROM users WHERE id = $1"#)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(db_denial)?;
    let is_bot = row.map(|row| row.0).ok_or(Denial::ServerError)?;
    let key: Option<(Option<Vec<u8>>,)> = sqlx::query_as(
        r#"SELECT api_key_encrypted FROM assistant_user_llm_config WHERE user_id = $1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    let llm_has_key = key.is_some_and(|row| pidash_db::assistant::models::has_secret(&row.0));
    Ok(ExecutorUser {
        flags: UserFlags {
            is_active: true,
            is_bot,
        },
        llm_has_key,
    })
}

/// `get_agent_executor_options(project)` (`core/agent_execution.py:83-128`,
/// L1 `project_executor_options`): the cloud leg consults the
/// `has_usable_llm_config` EE seam only on configured instances; the local
/// leg is the runners EXISTS; the managed leg is L4's
/// `managed_runner_availability` with the EE profile seam.
async fn executor_options(
    pool: &sqlx::PgPool,
    state: &AppState,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    user: &ExecutorUser,
) -> Result<[pidash_services::dispatch::policy::ExecutorOption; 3], Denial> {
    use pidash_services::assistant::seams;
    use pidash_services::dispatch::admission::{
        managed_runner_availability, ENROLLED_MANAGED_RUNNERS_EXISTS_SQL, HEARTBEAT_GRACE_SECS,
        ONLINE_MANAGED_RUNNER_SQL,
    };
    use pidash_services::dispatch::policy::LOCAL_RUNNER_EXISTS_SQL;
    let local: Option<(i32,)> = sqlx::query_as(LOCAL_RUNNER_EXISTS_SQL)
        .bind(project_id)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(db_denial)?;
    let settings = state.settings();
    let llm_has_key = user.llm_has_key;
    let managed =
        if pidash_services::dispatch::policy::managed_runner_is_enabled(&settings.managed_runner) {
            let enrolled: Option<(i32,)> = sqlx::query_as(ENROLLED_MANAGED_RUNNERS_EXISTS_SQL)
                .bind(user_id)
                .bind(project_id)
                .bind(workspace_id)
                .fetch_optional(pool)
                .await
                .map_err(db_denial)?;
            // `HEARTBEAT_GRACE` (`runner/services/matcher.py:44`): online
            // while the last heartbeat is within 90 seconds.
            let cutoff = Utc::now() - chrono::Duration::seconds(HEARTBEAT_GRACE_SECS);
            let online: Option<(uuid::Uuid,)> = sqlx::query_as(ONLINE_MANAGED_RUNNER_SQL)
                .bind(user_id)
                .bind(project_id)
                .bind(workspace_id)
                .bind(cutoff)
                .fetch_optional(pool)
                .await
                .map_err(db_denial)?;
            managed_runner_availability(
                &settings.managed_runner,
                Some(&user.flags),
                || {
                    let profile = seams::agent_model_profile_for_user(llm_has_key);
                    pidash_services::dispatch::admission::LlmProfile {
                        available: profile.available,
                        reason_code: profile.reason_code,
                    }
                },
                enrolled.is_some(),
                online.is_some(),
            )
        } else {
            managed_runner_availability(
                &settings.managed_runner,
                Some(&user.flags),
                || {
                    let profile = seams::agent_model_profile_for_user(llm_has_key);
                    pidash_services::dispatch::admission::LlmProfile {
                        available: profile.available,
                        reason_code: profile.reason_code,
                    }
                },
                false,
                false,
            )
        };
    Ok(
        pidash_services::app_project::ser_project::project_executor_options(
            &settings.cloud_agent,
            Some(user.flags),
            true,
            || seams::has_usable_llm_config(llm_has_key),
            local.is_some(),
            managed,
        ),
    )
}

/// Render one `ProjectListSerializer` row (`serializers/project.py:189-270`)
/// from its `get_queryset` row: members prefetch, next sequence, executor
/// options and cover URL resolve per row exactly as the serializer method
/// fields do.
async fn render_list_row(
    pool: &sqlx::PgPool,
    state: &AppState,
    row: &ProjectRow,
    slug: &str,
    actor: &Actor,
    user: &ExecutorUser,
) -> Result<String, Denial> {
    use pidash_services::app_project::ser_project::ProjectListRead;
    if row.sort_order.is_some_and(|float| !float.is_finite()) {
        // DRF `STRICT_JSON`: non-finite floats 500 at render, never
        // `null` (see the compact-list site).
        return Err(Denial::ServerError);
    }
    let members = fetch_members(pool, &row.id, slug).await?;
    let rendered_members = render_members(&members)?;
    let sequence = next_sequence(pool, &row.id).await?;
    let options =
        executor_options(pool, state, &row.id, &row.workspace_id, &actor.id, user).await?;
    let cover_url =
        cover_image_url(pool, row.cover_image_asset_id, row.cover_image.as_deref()).await?;
    let timezone = actor.timezone;
    let read = ProjectListRead {
        id: row.id.to_string(),
        is_favorite: Some(row.is_favorite),
        sort_order: Some(row.sort_order),
        member_role: Some(row.member_role.map(i64::from)),
        anchor: Some(row.anchor.clone()),
        name: row.name.clone(),
        inbox_view: row.intake_view,
        members: rendered_members,
        cover_image_url: cover_url,
        deleted_at: render_dt_opt(row.deleted_at, timezone),
        timezone: row.timezone.clone(),
        created_at: render_dt(&row.created_at, timezone),
        updated_at: render_dt(&row.updated_at, timezone),
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        network: i64::from(row.network),
        identifier: row.identifier.clone(),
        default_assignee: row.default_assignee_id.map(|id| id.to_string()),
        project_lead: row.project_lead_id.map(|id| id.to_string()),
        emoji: row.emoji.clone(),
        icon_prop: row.icon_prop.clone(),
        module_view: row.module_view,
        cycle_view: row.cycle_view,
        issue_views_view: row.issue_views_view,
        page_view: row.page_view,
        intake_view: row.intake_view,
        is_time_tracking_enabled: row.is_time_tracking_enabled,
        is_issue_type_enabled: row.is_issue_type_enabled,
        description: row.description.clone(),
        description_text: row.description_text.clone(),
        description_html: row.description_html.clone(),
        workspace: row.workspace_id.to_string(),
        guest_view_all_features: row.guest_view_all_features,
        logo_props: row.logo_props.clone(),
        cover_image_asset: row.cover_image_asset_id.map(|id| id.to_string()),
        cover_image: row.cover_image.clone(),
        estimate: row.estimate_id.map(|id| id.to_string()),
        archive_in: i64::from(row.archive_in),
        close_in: i64::from(row.close_in),
        default_state: row.default_state_id.map(|id| id.to_string()),
        archived_at: render_dt_opt(row.archived_at, timezone),
        is_default: row.is_default,
        external_source: row.external_source.clone(),
        external_id: row.external_id.clone(),
        repo_url: row.repo_url.clone(),
        base_branch: row.base_branch.clone(),
        agent_default_interval_seconds: i64::from(row.agent_default_interval_seconds),
        agent_default_max_ticks: i64::from(row.agent_default_max_ticks),
        agent_review_default_interval_seconds: i64::from(row.agent_review_default_interval_seconds),
        agent_test_default_interval_seconds: i64::from(row.agent_test_default_interval_seconds),
        agent_ticking_enabled: row.agent_ticking_enabled,
        default_agent_executor: row.default_agent_executor.clone(),
        agent_executor_options: options.to_vec(),
        next_work_item_sequence: sequence,
        members_can_edit_states: row.members_can_edit_states,
    };
    serde_json::to_string(&read).map_err(|_| Denial::ServerError)
}

/// `Workspace.logo_url` (`db/models/workspace.py:261-269`): the attached
/// asset's URL as-is (even `None`), else the legacy `logo` text, else
/// `None` — the same pick rule as the project cover.
async fn workspace_detail(pool: &sqlx::PgPool, workspace_id: &uuid::Uuid) -> Result<Value, Denial> {
    use pidash_services::app_workspace::ser_workspace::{
        workspace_lite_to_representation, WorkspaceLiteRow,
    };
    let row: Option<(String, String, Option<String>, Option<uuid::Uuid>)> =
        sqlx::query_as(r#"SELECT name, slug, logo, logo_asset_id FROM workspaces WHERE id = $1"#)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(db_denial)?;
    let Some((name, slug, logo, logo_asset_id)) = row else {
        return Err(Denial::ServerError);
    };
    let logo_url: Option<String> = match logo_asset_id {
        Some(id) => file_asset_url(pool, &id).await?,
        None => logo.filter(|text| !text.is_empty()),
    };
    let id = workspace_id.to_string();
    let lite = WorkspaceLiteRow {
        name: &name,
        slug: &slug,
        id: &id,
        logo_url: logo_url.as_deref(),
    };
    let view = workspace_lite_to_representation(&lite);
    serde_json::to_value(&view).map_err(|_| Denial::ServerError)
}

/// `current_instance` (`base.py:340-341`): `json.dumps` (CPython defaults)
/// of the pre-save `ProjectSerializer` read shape. The project is fetched
/// bare (`Project.objects.get`, no annotations); `workspace_detail` nests
/// the D-24 lite shape; datetimes render in the caller's zone.
async fn render_current_instance(
    pool: &sqlx::PgPool,
    state: &AppState,
    project_id: &uuid::Uuid,
    actor: &Actor,
    user: &ExecutorUser,
    slug: &str,
) -> Result<String, Denial> {
    use pidash_services::app_project::ser_project::ProjectRead;
    let row = fetch_project_row(pool, project_id, &actor.id, slug)
        .await?
        .ok_or(Denial::ServerError)?;
    let detail = workspace_detail(pool, &row.workspace_id).await?;
    let options =
        executor_options(pool, state, &row.id, &row.workspace_id, &actor.id, user).await?;
    let timezone = actor.timezone;
    let read = ProjectRead {
        id: row.id.to_string(),
        workspace_detail: detail,
        inbox_view: row.intake_view,
        agent_executor_options: options.to_vec(),
        created_at: render_dt(&row.created_at, timezone),
        updated_at: render_dt(&row.updated_at, timezone),
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        workspace: row.workspace_id.to_string(),
        deleted_at: None,
        name: row.name.clone(),
        description: row.description.clone(),
        description_text: row.description_text.clone(),
        description_html: row.description_html.clone(),
        network: i64::from(row.network),
        identifier: row.identifier.clone(),
        default_assignee: row.default_assignee_id.map(|id| id.to_string()),
        project_lead: row.project_lead_id.map(|id| id.to_string()),
        emoji: row.emoji.clone(),
        icon_prop: row.icon_prop.clone(),
        module_view: row.module_view,
        cycle_view: row.cycle_view,
        issue_views_view: row.issue_views_view,
        page_view: row.page_view,
        intake_view: row.intake_view,
        is_time_tracking_enabled: row.is_time_tracking_enabled,
        is_issue_type_enabled: row.is_issue_type_enabled,
        guest_view_all_features: row.guest_view_all_features,
        cover_image: row.cover_image.clone(),
        cover_image_asset: row.cover_image_asset_id.map(|id| id.to_string()),
        estimate: row.estimate_id.map(|id| id.to_string()),
        archive_in: i64::from(row.archive_in),
        close_in: i64::from(row.close_in),
        logo_props: row.logo_props.clone(),
        default_state: row.default_state_id.map(|id| id.to_string()),
        archived_at: render_dt_opt(row.archived_at, timezone),
        is_default: row.is_default,
        timezone: row.timezone.clone(),
        external_source: row.external_source.clone(),
        external_id: row.external_id.clone(),
        repo_url: row.repo_url.clone(),
        base_branch: row.base_branch.clone(),
        agent_default_interval_seconds: i64::from(row.agent_default_interval_seconds),
        agent_default_max_ticks: i64::from(row.agent_default_max_ticks),
        agent_review_default_interval_seconds: i64::from(row.agent_review_default_interval_seconds),
        agent_test_default_interval_seconds: i64::from(row.agent_test_default_interval_seconds),
        agent_ticking_enabled: row.agent_ticking_enabled,
        default_agent_executor: row.default_agent_executor.clone(),
        members_can_edit_states: row.members_can_edit_states,
    };
    let value = serde_json::to_value(&read).map_err(|_| Denial::ServerError)?;
    Ok(cpython_dumps(&value))
}

// ---------------------------------------------------------------------------
// Body parsing + DRF field validation
// ---------------------------------------------------------------------------

/// Parse a PUT serializer body: empty → `{}`, object → its map, `null`
/// → the `No data provided` 400 (DRF `serializers.py:580`, probed), any
/// other JSON → the DRF non-dictionary 400, malformed → the `ParseError`
/// 400 (the v1 `parse_body` precedent; serde's message stands in for
/// CPython's, as in every merged port — no fixture pins parser-error
/// bytes). PUT-only: the mixin passes `data` straight to the
/// serializer, while create/PATCH consume `request.data` as a mapping
/// first (`{**...}` / `.get` → 500 on non-dicts — [`parse_get_body`]).
/// DRF `ParseError` reason for JSON-whitespace-only input (CPython
/// `json.load` skips ` \t\n\r`, then fails `Expecting value` at the
/// true line/column — live-probed, including multi-line blanks).
/// Returns `None` when the input is not all JSON whitespace.
fn blank_json_reason(raw: &[u8]) -> Option<String> {
    if raw.is_empty()
        || !raw
            .iter()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
    {
        return None;
    }
    let text = std::str::from_utf8(raw).ok()?;
    let line = text.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = text.rsplit('\n').next().map_or(1, str::len) + 1;
    Some(format!(
        "JSON parse error - Expecting value: line {line} column {column} (char {})",
        text.len()
    ))
}

/// Map a body-parse failure to DRF's `ParseError` shape: blank input
/// is byte-exact (see [`blank_json_reason`]); anything else carries
/// the parser reason after the same prefix (the analytics
/// `json_parse_denial` precedent).
fn json_parse_denial(raw: &[u8], error: &serde_json::Error) -> Denial {
    let reason = blank_json_reason(raw).unwrap_or_else(|| format!("JSON parse error - {error}"));
    Denial::Raw(
        StatusCode::BAD_REQUEST,
        format!("{{\"detail\":{}}}", json_string(&reason)),
    )
}

pub fn parse_body(raw: &[u8]) -> Result<Map<String, Value>, Denial> {
    if raw.is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(Value::Null) => Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            "{\"non_field_errors\":[\"No data provided\"]}".to_owned(),
        )),
        Ok(other) => {
            let kind = match &other {
                Value::Array(_) => "list",
                Value::String(_) => "str",
                Value::Number(_) => {
                    if other.as_i64().is_some() {
                        "int"
                    } else {
                        "float"
                    }
                }
                Value::Bool(_) => "bool",
                Value::Null | Value::Object(_) => unreachable!("matched above"),
            };
            Err(Denial::Raw(
                StatusCode::BAD_REQUEST,
                format!(
                    "{{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got {kind}.\"]}}"
                ),
            ))
        }
        Err(error) => Err(json_parse_denial(raw, &error)),
    }
}

/// Parse a body the view consumes as a mapping (`identifiers_delete`,
/// `user_views_post`, create, PATCH): empty → `{}`, object → its map,
/// malformed → `ParseError` 400, any other JSON → 500. The `.get()` call
/// sites (`identifiers_delete`, `user_views_post`, PATCH's
/// `request.data.get` at `base.py:341`) raise `AttributeError`; the
/// `{**request.data, ...}` spreads (create at `base.py:261`, PATCH at
/// `base.py:349-354`) raise `TypeError` — both map to the generic 500
/// via `handle_exception` (probed for all five non-dict shapes — the
/// notifications `parse_body` precedent).
fn parse_get_body(raw: &[u8]) -> Result<Map<String, Value>, Denial> {
    if raw.is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(Denial::ServerError),
        Err(error) => Err(json_parse_denial(raw, &error)),
    }
}

/// Python `str.strip()` membership: Rust `White_Space` plus U+001C-U+001F
/// (verified by exhaustively diffing `str.strip` against
/// `char::is_whitespace` over all code points — those four are the only
/// differences). Used by `CharField` (strip + blank check) and the
/// identifier `name.strip()` lookups.
fn is_py_strip_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// Python `str.strip()` (see [`is_py_strip_ws`]).
fn strip_py(text: &str) -> String {
    text.trim_matches(is_py_strip_ws).to_owned()
}

/// Python `re` `\s` membership (str patterns): same set as
/// [`is_py_strip_ws`] (verified by the same exhaustive diff). Used by
/// DRF's `re_decimal` (`IntegerField`) and the `parse_datetime` regex
/// fallback — NOT by `int()`/`float()`, which accept exactly Rust
/// `White_Space` (U+001C-U+001F raise there; probed).
fn is_re_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// Unicode decimal-digit (`Nd`) runs as `(start, end)` code points,
/// generated from `unicodedata.decimal` (64 runs; every run is one or
/// more `0-9` decades, so the value is `(cp - start) % 10`). Python
/// `int()`/`float()` and the `parse_datetime` regex fallback accept all
/// of these; Rust `char::to_digit` is ASCII-only.
const ND_RANGES: &[(u32, u32)] = &[
    (0x0030, 0x0039),
    (0x0660, 0x0669),
    (0x06F0, 0x06F9),
    (0x07C0, 0x07C9),
    (0x0966, 0x096F),
    (0x09E6, 0x09EF),
    (0x0A66, 0x0A6F),
    (0x0AE6, 0x0AEF),
    (0x0B66, 0x0B6F),
    (0x0BE6, 0x0BEF),
    (0x0C66, 0x0C6F),
    (0x0CE6, 0x0CEF),
    (0x0D66, 0x0D6F),
    (0x0DE6, 0x0DEF),
    (0x0E50, 0x0E59),
    (0x0ED0, 0x0ED9),
    (0x0F20, 0x0F29),
    (0x1040, 0x1049),
    (0x1090, 0x1099),
    (0x17E0, 0x17E9),
    (0x1810, 0x1819),
    (0x1946, 0x194F),
    (0x19D0, 0x19D9),
    (0x1A80, 0x1A89),
    (0x1A90, 0x1A99),
    (0x1B50, 0x1B59),
    (0x1BB0, 0x1BB9),
    (0x1C40, 0x1C49),
    (0x1C50, 0x1C59),
    (0xA620, 0xA629),
    (0xA8D0, 0xA8D9),
    (0xA900, 0xA909),
    (0xA9D0, 0xA9D9),
    (0xA9F0, 0xA9F9),
    (0xAA50, 0xAA59),
    (0xABF0, 0xABF9),
    (0xFF10, 0xFF19),
    (0x104A0, 0x104A9),
    (0x10D30, 0x10D39),
    (0x11066, 0x1106F),
    (0x110F0, 0x110F9),
    (0x11136, 0x1113F),
    (0x111D0, 0x111D9),
    (0x112F0, 0x112F9),
    (0x11450, 0x11459),
    (0x114D0, 0x114D9),
    (0x11650, 0x11659),
    (0x116C0, 0x116C9),
    (0x11730, 0x11739),
    (0x118E0, 0x118E9),
    (0x11950, 0x11959),
    (0x11C50, 0x11C59),
    (0x11D50, 0x11D59),
    (0x11DA0, 0x11DA9),
    (0x11F50, 0x11F59),
    (0x16A60, 0x16A69),
    (0x16AC0, 0x16AC9),
    (0x16B50, 0x16B59),
    (0x1D7CE, 0x1D7FF),
    (0x1E140, 0x1E149),
    (0x1E2F0, 0x1E2F9),
    (0x1E4F0, 0x1E4F9),
    (0x1E950, 0x1E959),
    (0x1FBF0, 0x1FBF9),
];

/// Python decimal-digit value (`unicodedata.decimal`), or `None`.
fn nd_value(ch: char) -> Option<u32> {
    let cp = ch as u32;
    // ASCII fast path (also what `char::to_digit` covers).
    if ch.is_ascii_digit() {
        return Some(cp - 0x30);
    }
    for &(start, end) in ND_RANGES {
        if cp >= start && cp <= end {
            return Some((cp - start) % 10);
        }
    }
    None
}

/// Django `ProhibitNullCharactersValidator` message (appended to every
/// `CharField` after the length validators).
const NULL_CHARACTERS_MESSAGE: &str = "Null characters are not allowed.";

/// DRF `CharField` (`fields.py`): null → the null error (nullable fields
/// check null *before* calling, via [`opt_char`]); blank (`""`, or
/// whitespace-only — `trim_whitespace` only affects this check) → the blank
/// error unless allowed, else `""`; bool/dict/list → invalid; numbers
/// stringify. The value is STRIPPED (`to_internal_value` returns
/// `value.strip()`), and the validators run on the stripped value,
/// collecting every failure in order: `max_length`, then the
/// `ProhibitNullCharactersValidator` 400. (The third validator,
/// `ProhibitSurrogateCharactersValidator`, cannot fire: `serde_json`
/// rejects lone surrogates at parse, so they surface as the `ParseError`
/// 400 instead — same status, accepted parser-bytes gap.)
fn validate_char(
    value: &Value,
    max_length: Option<usize>,
    allow_blank: bool,
) -> Result<String, Vec<String>> {
    let stripped = char_to_string(value, allow_blank)?;
    let mut errors = char_length_errors(&stripped, max_length);
    if stripped.contains('\0') {
        errors.push(NULL_CHARACTERS_MESSAGE.to_owned());
    }
    if errors.is_empty() {
        Ok(stripped)
    } else {
        Err(errors)
    }
}

/// `CharField` coercion without the length/content validators: null →
/// the null error, blank → the blank error (or `""` when allowed),
/// bool/dict/list → invalid, else the stripped string.
fn char_to_string(value: &Value, allow_blank: bool) -> Result<String, Vec<String>> {
    if value.is_null() {
        return Err(vec!["This field may not be null.".to_owned()]);
    }
    match value {
        Value::String(text) => {
            if text.is_empty() || strip_py(text).is_empty() {
                if allow_blank {
                    return Ok(String::new());
                }
                return Err(vec!["This field may not be blank.".to_owned()]);
            }
            Ok(strip_py(text))
        }
        Value::Number(number) => Ok(number.to_string()),
        _ => Err(vec!["Not a valid string.".to_owned()]),
    }
}

/// `MaxLengthValidator` on the stripped value (code points), collecting.
fn char_length_errors(stripped: &str, max_length: Option<usize>) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(max) = max_length {
        if stripped.chars().count() > max {
            errors.push(format!(
                "Ensure this field has no more than {max} characters."
            ));
        }
    }
    errors
}

/// Nullable `CharField`: null → `None`, else `validate_char`.
fn opt_char(
    value: &Value,
    max_length: Option<usize>,
    allow_blank: bool,
) -> Result<Option<String>, Vec<String>> {
    if value.is_null() {
        return Ok(None);
    }
    validate_char(value, max_length, allow_blank).map(Some)
}

/// DRF `BooleanField`: membership in the true/false sets — `1.0 == 1`
/// is a member of `TRUE_VALUES` (probed), `-0.0 == 0` of `FALSE_VALUES`;
/// anything else (including `2`, `0.5`, dicts/lists) → invalid.
fn validate_bool(value: &Value) -> Result<bool, String> {
    const INVALID: &str = "Must be a valid boolean.";
    match value {
        Value::Null => Err("This field may not be null.".to_owned()),
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if number.as_i64() == Some(1) || number.as_f64() == Some(1.0) {
                Ok(true)
            } else if number.as_i64() == Some(0) || number.as_f64() == Some(0.0) {
                Ok(false)
            } else {
                Err(INVALID.to_owned())
            }
        }
        Value::String(text) => match text.to_ascii_lowercase().as_str() {
            "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
            "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
            _ => Err(INVALID.to_owned()),
        },
        _ => Err(INVALID.to_owned()),
    }
}

/// DRF `IntegerField` failure: a 400 message, or a valid spelling
/// outside `i64` (Python `int()` is unbounded, so it still runs the
/// min/max validators — `TooLarge` always fails the bound on its
/// sign's side, live-probed as a 400, never the column 500).
#[derive(Debug, PartialEq, Eq)]
enum IntError {
    Invalid(String),
    TooLarge { negative: bool },
}

/// Narrow a validated `IntegerField` (`i64`) to its `int4` column.
/// Unreachable in practice — every arm range-checks to `i32` first
/// (live-probed 400s) — but never wrap: `try_from`, not `as`.
fn i32_or_500(value: i64) -> Result<i32, Denial> {
    i32::try_from(value).map_err(|_| Denial::ServerError)
}

/// DRF `MinValueValidator`/`MaxValueValidator` on an `IntegerField`
/// (the model field's tightest limits — live-probed: a plain model
/// `IntegerField` enforces ±2³¹, `archive_in`/`close_in` enforce
/// 0..12, one message each; `TooLarge` spellings fail the bound on
/// their sign's side). Returns the value or the 400 message.
fn check_int_range(result: Result<i64, IntError>, min: i64, max: i64) -> Result<i64, String> {
    match result {
        Ok(number) => {
            if number < min {
                Err(format!(
                    "Ensure this value is greater than or equal to {min}."
                ))
            } else if number > max {
                Err(format!("Ensure this value is less than or equal to {max}."))
            } else {
                Ok(number)
            }
        }
        Err(IntError::Invalid(message)) => Err(message),
        Err(IntError::TooLarge { negative }) => Err(if negative {
            format!("Ensure this value is greater than or equal to {min}.")
        } else {
            format!("Ensure this value is less than or equal to {max}.")
        }),
    }
}

/// DRF `IntegerField` (`fields.py`): `int(re_decimal.sub('', str(data)))`
/// with `re_decimal = re.compile(r'\.0*\s*$')` — the `\s` is Python-`re`
/// whitespace (see [`is_re_ws`]). Strings over 1000 *characters* → the
/// size error. `str(data)`: JSON strings verbatim; JSON ints render
/// exactly; JSON floats render like CPython `str(float)` (no exponent
/// below `1e16`, so integral floats there validate — `1e3` → `1000` —
/// while fractional or huge ones fail); bools/dicts/lists → invalid.
/// `int()` accepts an optional sign, single underscores between digits,
/// surrounding Rust-`White_Space` (exactly — U+001C-U+001F raise), and
/// Unicode decimal digits ([`nd_value`]); a valid spelling outside `i64`
/// is [`IntError::TooLarge`] (fails the min/max bound on its side).
fn validate_int(value: &Value) -> Result<i64, IntError> {
    const INVALID: &str = "A valid integer is required.";
    let invalid = || IntError::Invalid(INVALID.to_owned());
    if value.is_null() {
        return Err(IntError::Invalid("This field may not be null.".to_owned()));
    }
    if let Value::String(text) = value {
        if text.chars().count() > 1000 {
            return Err(IntError::Invalid("String value too large.".to_owned()));
        }
    }
    match value {
        Value::String(text) => {
            let stripped = strip_re_decimal(text);
            match parse_python_int(&stripped) {
                IntParse::Value(int) => Ok(int),
                IntParse::TooLarge { negative } => Err(IntError::TooLarge { negative }),
                IntParse::Invalid => Err(invalid()),
            }
        }
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Ok(int);
            }
            if let Some(uint) = number.as_u64() {
                // Valid spelling (`str` is pure digits, non-negative).
                return i64::try_from(uint).map_err(|_| IntError::TooLarge { negative: false });
            }
            let Some(float) = number.as_f64() else {
                return Err(invalid());
            };
            // CPython `str(float)`: integral floats below `1e16` render
            // `"<digits>.0"` (never an exponent — probed); anything else
            // renders a fraction or an exponent, which `int()` rejects.
            if float.fract() != 0.0 || !float.is_finite() || float.abs() >= 1e16 {
                return Err(invalid());
            }
            let rendered = format!("{float:.1}");
            let stripped = strip_re_decimal(&rendered);
            // Well-formed by construction (`<digits>.0` below `1e16`);
            // `None` is unreachable, mapped invalid defensively.
            match parse_python_int(&stripped) {
                IntParse::Value(int) => Ok(int),
                _ => Err(invalid()),
            }
        }
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => Err(invalid()),
        Value::Null => unreachable!("checked above"),
    }
}

/// DRF `re_decimal.sub('', text)`: remove one trailing `\.0*` plus
/// trailing `re`-whitespace. When no dot-pattern matches, the ORIGINAL
/// text is returned (so trailing U+001C-U+001F still fail `int()`).
fn strip_re_decimal(text: &str) -> String {
    let trimmed = text.trim_end_matches(is_re_ws);
    if let Some(prefix) = trimmed.strip_suffix('0') {
        let zeros = prefix.trim_end_matches('0');
        if let Some(stripped) = zeros.strip_suffix('.') {
            return stripped.to_owned();
        }
        return text.to_owned();
    }
    trimmed.strip_suffix('.').unwrap_or(text).to_owned()
}

/// Python `int(text)` outcome: a value, an invalid spelling (400),
/// or a valid spelling outside `i64` (runs the min/max validators —
/// the sign decides which bound fails).
enum IntParse {
    Value(i64),
    Invalid,
    TooLarge { negative: bool },
}

/// Python `int(text)` (base 10): surrounding Rust-`White_Space`, one
/// optional ASCII sign, then decimal digits ([`nd_value`]) with single
/// underscores allowed only between two digits. Any 20+-digit magnitude
/// exceeds `i64` ([`IntParse::TooLarge`]).
fn parse_python_int(text: &str) -> IntParse {
    let text = text.trim_matches(|ch: char| ch.is_whitespace());
    let (negative, digits) = match text.strip_prefix(['+', '-']) {
        Some(rest) => (text.starts_with('-'), rest),
        None => (false, text),
    };
    if digits.is_empty() {
        return IntParse::Invalid;
    }
    // Collect digit values; `_` must sit between two digits.
    let chars: Vec<char> = digits.chars().collect();
    let mut values: Vec<u32> = Vec::with_capacity(chars.len());
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '_' {
            let prev_ok =
                index > 0 && chars[index - 1] != '_' && nd_value(chars[index - 1]).is_some();
            let next_ok = index + 1 < chars.len() && nd_value(chars[index + 1]).is_some();
            if !(prev_ok && next_ok) {
                return IntParse::Invalid;
            }
            continue;
        }
        let Some(digit) = nd_value(*ch) else {
            return IntParse::Invalid;
        };
        values.push(digit);
    }
    if values.is_empty() {
        return IntParse::Invalid;
    }
    // Magnitude check before narrowing: skip leading zeros; more than
    // 19 significant digits always exceeds `i64`.
    let significant = values.iter().skip_while(|digit| **digit == 0).count();
    if significant > 19 {
        return IntParse::TooLarge { negative };
    }
    let mut magnitude: i128 = 0;
    for digit in &values {
        magnitude = magnitude * 10 + i128::from(*digit);
    }
    if negative {
        if magnitude > i128::from(i64::MAX) + 1 {
            return IntParse::TooLarge { negative: true };
        }
        IntParse::Value(-(magnitude as i64))
    } else {
        if magnitude > i128::from(i64::MAX) {
            return IntParse::TooLarge { negative: false };
        }
        IntParse::Value(magnitude as i64)
    }
}

/// DRF `ChoiceField` over string choices: the input is `str()`-coerced
/// before lookup; a miss renders the raw input via `str()` in the message.
fn validate_str_choice(value: &Value, choices: &[&str]) -> Result<String, String> {
    if value.is_null() {
        return Err("This field may not be null.".to_owned());
    }
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Array(_) | Value::Object(_) => python_str(value),
        Value::Null => unreachable!("checked above"),
    };
    if choices.contains(&text.as_str()) {
        Ok(text)
    } else {
        Err(format!("\"{text}\" is not a valid choice."))
    }
}

/// DRF `ChoiceField` over `NETWORK_CHOICES` (`0`/`2`): same `str()`
/// coercion — `0`, `2`, `"0"`, `"2"` pass; `True`/`2.0`/dicts fail with
/// the `str()` rendering.
fn validate_network(value: &Value) -> Result<i32, String> {
    if value.is_null() {
        return Err("This field may not be null.".to_owned());
    }
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Array(_) | Value::Object(_) => python_str(value),
        Value::Null => unreachable!("checked above"),
    };
    match text.as_str() {
        "0" => Ok(0),
        "2" => Ok(2),
        _ => Err(format!("\"{text}\" is not a valid choice.")),
    }
}

/// DRF `DateTimeField` (default `iso-8601` input formats): null → `None`
/// when allowed; only strings parse (numbers/bools → invalid — DRF
/// passes non-`date`/`datetime` objects to `parse_datetime`, which
/// needs `str`); `parse_datetime` ([`parse_iso_core`] then
/// [`parse_iso_regex`]) decides acceptance; naive inputs resolve
/// `fold=0` in the request user's zone; anything else → the exact 400
/// message.
fn validate_datetime(
    value: &Value,
    timezone: Tz,
    allow_null: bool,
) -> Result<Option<DateTime<Utc>>, String> {
    if value.is_null() {
        if allow_null {
            return Ok(None);
        }
        return Err("This field may not be null.".to_owned());
    }
    let Value::String(text) = value else {
        return Err(INVALID_DATETIME_MESSAGE.to_owned());
    };
    match parse_drf_datetime(text, timezone) {
        DttmParse::Ok(when) => Ok(Some(when)),
        DttmParse::Invalid => Err(INVALID_DATETIME_MESSAGE.to_owned()),
        DttmParse::Overflow => Err(OVERFLOW_DATETIME_MESSAGE.to_owned()),
    }
}

/// One `parse_drf_datetime` outcome: DRF distinguishes the unparseable
/// `invalid` 400 from the `astimezone` `overflow` 400.
enum DttmParse {
    Ok(DateTime<Utc>),
    Invalid,
    Overflow,
}

impl DttmParse {
    #[cfg(test)]
    fn ok(self) -> Option<DateTime<Utc>> {
        match self {
            DttmParse::Ok(when) => Some(when),
            DttmParse::Invalid | DttmParse::Overflow => None,
        }
    }
}

fn parse_drf_datetime(text: &str, timezone: Tz) -> DttmParse {
    let Some(parsed) = parse_iso_core(text)
        .or_else(|| parse_iso_regex(text))
        .or_else(|| {
            // DRF's `strptime(value, 'iso-8601')` fallthrough
            // (`to_internal_value` runs it when `parse_datetime` returns
            // `None` — PIDASHCONV-773): the literal matches
            // case-insensitively and yields naive 1900-01-01 through the
            // naive arm below. Exact match: padding fails on both sides
            // (probed); ASCII-only (765 unicode-gap family).
            text.eq_ignore_ascii_case("iso-8601").then(|| {
                let naive = NaiveDate::from_ymd_opt(1900, 1, 1)
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
                    .expect("1900-01-01 valid");
                ParsedDateTime {
                    naive,
                    offset_micros: None,
                }
            })
        })
    else {
        return DttmParse::Invalid;
    };
    match parsed.offset_micros {
        // DRF `enforce_timezone`: aware values convert via
        // `value.astimezone(request_zone)`, which raises `OverflowError`
        // (the `overflow` 400) iff EITHER the UTC intermediate or the
        // zone-local wall time leaves `0001..=9999` (CPython checks
        // both conversions; fuzz-found:
        // `9999-12-31T10:00:00-14:00` overflows in New York although
        // the local wall time is in range). `chrono` represents wider
        // years, so both bounds are checked explicitly.
        Some(offset) => match parsed
            .naive
            .checked_sub_signed(Duration::microseconds(offset))
        {
            Some(naive) => {
                if !(1..=9999).contains(&naive.date().year()) {
                    return DttmParse::Overflow;
                }
                let utc = naive.and_utc();
                if (1..=9999).contains(&utc.with_timezone(&timezone).year()) {
                    DttmParse::Ok(utc)
                } else {
                    DttmParse::Overflow
                }
            }
            None => DttmParse::Overflow,
        },
        None => match resolve_naive_fold_zero(&parsed.naive, timezone) {
            Some(when) => DttmParse::Ok(when),
            None => DttmParse::Invalid,
        },
    }
}

/// DRF `enforce_timezone` over the request user's `ZoneInfo`
/// (`TimezoneMixin` activates `zoneinfo`, never `pytz`): naive wall
/// times attach with `fold=0` and are ALWAYS accepted (probed:
/// `valid_datetime` is true for ambiguous and nonexistent times alike,
/// so the `make_aware` 400 is dead with `ZoneInfo`). Ambiguous resolves
/// to the first occurrence; a nonexistent (gap) wall time resolves with
/// the pre-transition offset, found by scanning back in 15-minute steps
/// (real-zone gaps are under 125h — dateline skips included).
fn resolve_naive_fold_zero(naive: &NaiveDateTime, timezone: Tz) -> Option<DateTime<Utc>> {
    use chrono::{LocalResult, TimeZone};
    match timezone.from_local_datetime(naive) {
        LocalResult::Single(local) => Some(local.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, _) => Some(first.with_timezone(&Utc)),
        LocalResult::None => {
            let mut probe = *naive;
            for _ in 0..500 {
                probe = probe.checked_sub_signed(Duration::minutes(15))?;
                if let LocalResult::Single(local) = timezone.from_local_datetime(&probe) {
                    let offset = probe.signed_duration_since(local.naive_utc());
                    return naive.checked_sub_signed(offset).map(|utc| utc.and_utc());
                }
            }
            None
        }
    }
}

/// One `parse_datetime` result: the wall time (fractions truncated to
/// micros) plus a fixed UTC offset in micros (`None` = naive, read in
/// the request user's zone).
struct ParsedDateTime {
    naive: NaiveDateTime,
    offset_micros: Option<i64>,
}

/// ASCII digit pair, or `None`.
fn ascii2(bytes: &[u8]) -> Option<u32> {
    if bytes.len() == 2 && bytes[0].is_ascii_digit() && bytes[1].is_ascii_digit() {
        Some(u32::from(bytes[0] - b'0') * 10 + u32::from(bytes[1] - b'0'))
    } else {
        None
    }
}

/// Exactly two ASCII digits, or `false` (short slices included).
fn two_ascii_digits(bytes: Option<&[u8]>) -> bool {
    matches!(bytes, Some(pair) if pair.len() == 2 && pair[0].is_ascii_digit() && pair[1].is_ascii_digit())
}

/// Post-offset end: empty, or exactly one trailing NUL (probed:
/// `+00:00\x00` reads, `+00:00\x00\x00` fails).
fn is_end_or_nul(rest: &str) -> bool {
    rest.is_empty() || rest.as_bytes() == b"\0"
}

/// ASCII 4-digit group, or `None`.
fn ascii4(bytes: &[u8]) -> Option<i32> {
    if bytes.len() == 4 && bytes.iter().all(|byte| byte.is_ascii_digit()) {
        Some(
            bytes
                .iter()
                .fold(0i32, |acc, byte| acc * 10 + i32::from(byte - b'0')),
        )
    } else {
        None
    }
}

/// `datetime.fromisoformat` (Python 3.12) as Django's `parse_datetime`
/// calls it: strict-ASCII extended/basic/week dates, any single char
/// separator, extended/basic times with fraction-after-any-component
/// (always a fraction of a second, truncated to micros), `Z` or numeric
/// offsets (offset seconds and sub-second fractions allowed). Every rule
/// below was verified by probing; anything else falls through to
/// [`parse_iso_regex`]. (`ValueError` and `None` both become the 400
/// `invalid` message in DRF, so both are `None` here.)
fn parse_iso_core(text: &str) -> Option<ParsedDateTime> {
    let bytes = text.as_bytes();
    let (first, second) = parse_iso_core_date_opts(bytes)?;
    parse_iso_core_rest(text, first)
        .or_else(|| second.and_then(|opt| parse_iso_core_rest(text, opt)))
}

/// One week-date reading: taken vs untaken weekday (calendar dates
/// have a single option). `ban_digit_sep` forbids an ASCII-digit
/// separator after a taken extended-week day (probed:
/// `2024-W01-1200:00` is invalid while `2024W011200` reads).
#[derive(Clone, Copy)]
struct DateOption {
    date: NaiveDate,
    pos: usize,
    ban_digit_sep: bool,
}

fn parse_iso_core_rest(text: &str, opt: DateOption) -> Option<ParsedDateTime> {
    let DateOption {
        date,
        mut pos,
        ban_digit_sep,
    } = opt;
    if pos == text.len() {
        // Date-only: midnight, naive (3.11+ `fromisoformat`).
        return Some(ParsedDateTime {
            naive: date.and_hms_opt(0, 0, 0)?,
            offset_micros: None,
        });
    }
    // Separator: any single char (probed: `T`, `t`, space, tab,
    // newline, `x`, `_`, digits, `+`/`-`, `Z`, non-ASCII — so
    // `2024-01-01+05:30` is 05:30 naive, and `2024-01-01Z` is an
    // empty time → invalid).
    let sep = text[pos..].chars().next()?;
    if ban_digit_sep && sep.is_ascii_digit() {
        return None;
    }
    pos += sep.len_utf8();
    if pos >= text.len() {
        return None;
    }
    let (time, frac_len, rest) = parse_iso_core_time(&text[pos..])?;
    let offset_micros = parse_iso_core_tz(rest, frac_len)?;
    Some(ParsedDateTime {
        naive: NaiveDateTime::new(date, time),
        offset_micros,
    })
}

/// ISO weekday from a `1..=7` day digit.
fn isoweekday(day: u32) -> Weekday {
    match day {
        1 => Weekday::Mon,
        2 => Weekday::Tue,
        3 => Weekday::Wed,
        4 => Weekday::Thu,
        5 => Weekday::Fri,
        6 => Weekday::Sat,
        _ => Weekday::Sun,
    }
}

/// ISO week date bounded to the `0001..=9999` years: week dates can
/// resolve outside the week-year (probed: `2020-W53-7` is
/// 2021-01-03); past `9999` Python raises, which Django cannot
/// regex-rescue (probed: `9999-W52-7` is invalid). `chrono`
/// represents wider years, so the bound is checked explicitly.
fn isoywd_bounded(year: i32, week: u32, weekday: Weekday) -> Option<NaiveDate> {
    let date = NaiveDate::from_isoywd_opt(year, week, weekday)?;
    if (1..=9999).contains(&date.year()) {
        Some(date)
    } else {
        None
    }
}

/// Core date readings: extended `YYYY-MM-DD`, basic `YYYYMMDD`, week
/// `YYYY-Www[-d]`, or basic week `YYYYWww[d]`. Calendar dates yield
/// one reading; week dates yield two — weekday taken first, untaken
/// (Monday) as the backtrack when the taken reading fails anywhere
/// downstream (probed: `2024W011200` reads Monday 00:00 via the
/// taken day, `2024W013000000` falls back to Monday 00:00:00,
/// `2024-W01-7x00:00` falls back to Sunday 00:00). After a taken
/// extended-week day an ASCII-digit separator fails the reading
/// (probed: `2024-W01-1200:00` is invalid, while `2024W011200`
/// reads and a non-ASCII digit separator reads).
fn parse_iso_core_date_opts(bytes: &[u8]) -> Option<(DateOption, Option<DateOption>)> {
    if bytes.len() >= 8 && bytes[4] == b'-' {
        if bytes[5] == b'W' {
            let year = ascii4(bytes.get(0..4)?)?;
            if year < 1 {
                return None;
            }
            let week = ascii2(bytes.get(6..8)?)?;
            if !(1..=53).contains(&week) {
                return None;
            }
            let monday = isoywd_bounded(year, week, Weekday::Mon)?;
            let backtrack = DateOption {
                date: monday,
                pos: 8,
                ban_digit_sep: false,
            };
            if bytes.get(8) == Some(&b'-') {
                if let Some(day) = bytes
                    .get(9)
                    .and_then(|byte| (b'1'..=b'7').contains(byte).then(|| u32::from(byte - b'0')))
                {
                    if let Some(date) = isoywd_bounded(year, week, isoweekday(day)) {
                        let taken = DateOption {
                            date,
                            pos: 10,
                            ban_digit_sep: true,
                        };
                        return Some((taken, Some(backtrack)));
                    }
                }
            }
            return Some((backtrack, None));
        }
        let year = ascii4(bytes.get(0..4)?)?;
        if year < 1 || bytes[7] != b'-' {
            return None;
        }
        let month = ascii2(bytes.get(5..7)?)?;
        let day = ascii2(bytes.get(8..10)?)?;
        let date = NaiveDate::from_ymd_opt(year, month, day)?;
        return Some((
            DateOption {
                date,
                pos: 10,
                ban_digit_sep: false,
            },
            None,
        ));
    }
    if bytes.len() >= 7 && bytes[4] == b'W' {
        let year = ascii4(bytes.get(0..4)?)?;
        if year < 1 {
            return None;
        }
        let week = ascii2(bytes.get(5..7)?)?;
        if !(1..=53).contains(&week) {
            return None;
        }
        let monday = isoywd_bounded(year, week, Weekday::Mon)?;
        let backtrack = DateOption {
            date: monday,
            pos: 7,
            ban_digit_sep: false,
        };
        if let Some(day) = bytes
            .get(7)
            .and_then(|byte| (b'1'..=b'7').contains(byte).then(|| u32::from(byte - b'0')))
        {
            if let Some(date) = isoywd_bounded(year, week, isoweekday(day)) {
                let taken = DateOption {
                    date,
                    pos: 8,
                    ban_digit_sep: false,
                };
                return Some((taken, Some(backtrack)));
            }
        }
        return Some((backtrack, None));
    }
    if bytes.len() < 8 {
        return None;
    }
    let year = ascii4(bytes.get(0..4)?)?;
    if year < 1 {
        return None;
    }
    let month = ascii2(bytes.get(4..6)?)?;
    let day = ascii2(bytes.get(6..8)?)?;
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    Some((
        DateOption {
            date,
            pos: 8,
            ban_digit_sep: false,
        },
        None,
    ))
}

/// Core time: extended `HH[:MM[:SS]]` (strict 2-digit components) or
/// basic `HH[MM[SS]]` (2/4/6 digits), plus an optional fraction after
/// any component — always a fraction of a second. Basic `HHMMSS` also
/// takes a dot-less fraction (2+ digits, like the offset grammar).
/// Returns the time, the fraction digit count (0 when none — the tail
/// rule depends on it), and the unparsed rest.
fn parse_iso_core_time(text: &str) -> Option<(NaiveTime, usize, &str)> {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes.get(2) == Some(&b':') {
        let hour = ascii2(bytes.get(0..2)?)?;
        if hour > 23 {
            return None;
        }
        // A colon NOT followed by two ASCII digits ends the time at
        // the shorter component (the colon re-enters the tail rule —
        // probed: `T00:+00:00` and `T00:00:+00:00` read midnight UTC).
        if !two_ascii_digits(bytes.get(3..5)) {
            let time = NaiveTime::from_hms_opt(hour, 0, 0)?;
            return Some((time, 0, &text[2..]));
        }
        let minute = ascii2(bytes.get(3..5)?)?;
        if minute > 59 {
            return None;
        }
        let mut pos = 5;
        let mut second = 0;
        if bytes.get(5) == Some(&b':') {
            if !two_ascii_digits(bytes.get(6..8)) {
                let time = NaiveTime::from_hms_micro_opt(hour, minute, 0, 0)?;
                return Some((time, 0, &text[5..]));
            }
            second = ascii2(bytes.get(6..8)?)?;
            if second > 59 {
                return None;
            }
            pos = 8;
        }
        let (micros, frac_len, rest) = split_core_fraction(&text[pos..]);
        let time = NaiveTime::from_hms_micro_opt(hour, minute, second, micros)?;
        return Some((time, frac_len, rest));
    }
    let run = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    // Odd runs leave one digit for the pre-tz skip (`T000+00:00`
    // reads midnight); a lone digit is no time at all.
    let (hour, minute, second, mut pos) = match run {
        0 | 1 => return None,
        2 | 3 => (ascii2(bytes.get(0..2)?)?, 0, 0, 2),
        4 | 5 => (ascii2(bytes.get(0..2)?)?, ascii2(bytes.get(2..4)?)?, 0, 4),
        _ => (
            ascii2(bytes.get(0..2)?)?,
            ascii2(bytes.get(2..4)?)?,
            ascii2(bytes.get(4..6)?)?,
            6,
        ),
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    // Dot-less fraction after `HHMMSS` only (2+ digits, truncated to
    // micros); a single trailing digit is left for the pre-tz skip.
    let mut micros = 0;
    let mut frac_len = 0;
    if pos == 6 && run - pos >= 2 {
        frac_len = run - pos;
        micros = frac_micros_of(&text[pos..pos + frac_len]);
        pos = run;
    }
    if frac_len == 0 {
        let (dot_micros, dot_len, rest) = split_core_fraction(&text[pos..]);
        micros = dot_micros;
        frac_len = dot_len;
        let time = NaiveTime::from_hms_micro_opt(hour, minute, second, micros)?;
        return Some((time, frac_len, rest));
    }
    let time = NaiveTime::from_hms_micro_opt(hour, minute, second, micros)?;
    Some((time, frac_len, &text[pos..]))
}

/// Core fraction: `[.,]` plus 1+ ASCII digits (any count), truncated to
/// micros (first 6, padded right). Returns the micros, the digit count
/// (the tail rule depends on it: 6+ digits scan for the tz), and the
/// rest. No fraction → `(0, 0, text)`.
fn split_core_fraction(text: &str) -> (u32, usize, &str) {
    let mut chars = text.chars();
    match chars.next() {
        Some('.') | Some(',') => {}
        _ => return (0, 0, text),
    }
    let digits: String = chars.take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return (0, 0, text);
    }
    let rest = &text[1 + digits.len()..];
    (frac_micros_of(&digits), digits.len(), rest)
}

/// Fraction digits to micros: truncate to 6, pad right with zeros.
fn frac_micros_of(digits: &str) -> u32 {
    let mut micros = digits.to_owned();
    micros.truncate(6);
    while micros.len() < 6 {
        micros.push('0');
    }
    micros.parse::<u32>().unwrap_or(0)
}

/// Core timezone: end (naive), `Z`, or a numeric offset; the whole rest
/// must be consumed. Returns the offset in micros (`None` = naive).
/// The pre-tz gap depends on the fraction length (all probed): without
/// a fraction, exactly one ASCII byte (any except the tz starts
/// `Z`/`+`/`-` — digits, letters, dots, spaces, controls alike;
/// multi-byte chars fail) may precede an immediate tz
/// (`00:00:005+00:00`, `00:00.+00:00`, `000000 +00:00` all read); after
/// a short (1-5 digit) fraction only an immediate tz or the end
/// follows; after a long (6+ digit) fraction any text (any length, any
/// charset — digits, NUL, non-ASCII, newlines) is skipped until the
/// first tz start (`Z`/`+`/`-` — lowercase `z` is skipped, not a
/// start), which must then parse strictly with nothing after
/// (`.123456xyz+00:00` reads; `.123456++00:00` and `.123456x` fail).
/// NUL quirks (all probed): a lone trailing NUL ends the time in the
/// no-fraction tail (`T00:00:00\x00` reads naive); the long-fraction
/// scan with no tz start reads naive iff the rest starts with NUL
/// (`.123456\x00x` reads, `.123456x\x00` fails — the tz search itself
/// passes NULs); after `Z` anything starting with NUL is ignored
/// (`Z\x00x` reads UTC); after a numeric offset exactly one trailing
/// NUL is allowed (`+00:00\x00` reads, `+00:00\x00\x00` fails).
/// Short fractions and bare dates take no NUL (`.5\x00` fails).
fn parse_iso_core_tz(rest: &str, frac_len: usize) -> Option<Option<i64>> {
    if rest.is_empty() {
        return Some(None);
    }
    let mut rest = rest;
    if frac_len >= 6 {
        // Scan to the first tz start (passing NULs and all); with no
        // tz start the rest reads naive iff it starts with NUL.
        let mut offset = None;
        for (index, ch) in rest.char_indices() {
            if matches!(ch, 'Z' | '+' | '-') {
                offset = Some(index);
                break;
            }
        }
        match offset {
            Some(index) => rest = &rest[index..],
            None => {
                if rest.as_bytes().first() == Some(&b'\0') {
                    return Some(None);
                }
                return None;
            }
        }
    } else if frac_len == 0 {
        if rest.as_bytes() == b"\0" {
            return Some(None);
        }
        match rest.as_bytes().first() {
            // Tz starts never skip (a malformed offset fails outright).
            Some(b'Z' | b'+' | b'-') => {}
            Some(byte) if byte.is_ascii() => {
                rest = &rest[1..];
                if rest.is_empty() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    if rest.as_bytes() == b"Z"
        || rest.len() > 1 && rest.as_bytes()[0] == b'Z' && rest.as_bytes()[1] == b'\0'
    {
        return Some(Some(0));
    }
    let bytes = rest.as_bytes();
    let (negative, digits) = match bytes.first() {
        Some(b'+') => (false, &rest[1..]),
        Some(b'-') => (true, &rest[1..]),
        _ => return None,
    };
    let dbytes = digits.as_bytes();
    // Sequential: `HH`, optional `:MM`, optional `:SS`, optional
    // fraction — or an all-digit basic run (`HH[MM[SS[frac]]]`) when no
    // colon or fraction separator follows `HH`.
    let hour = ascii2(dbytes.get(0..2)?)? as i64;
    match dbytes.get(2) {
        None => offset_micros(negative, hour, 0, 0, 0),
        Some(b':') => {
            let minute = ascii2(dbytes.get(3..5)?)? as i64;
            let mut pos = 5;
            let mut second = 0i64;
            if dbytes.get(5) == Some(&b':') {
                second = ascii2(dbytes.get(6..8)?)? as i64;
                pos = 8;
            }
            let (frac_micros, frac_len, rest) = split_core_fraction(&digits[pos..]);
            // A trailing NUL is allowed only without a fraction
            // (probed: `+00:00\x00` reads, `+00:00:00.5\x00` fails).
            let end_ok = if frac_len == 0 {
                is_end_or_nul(rest)
            } else {
                rest.is_empty()
            };
            if !end_ok {
                return None;
            }
            offset_micros(negative, hour, minute, second, frac_micros)
        }
        Some(b'.') | Some(b',') => {
            let (frac_micros, frac_len, rest) = split_core_fraction(&digits[2..]);
            let end_ok = if frac_len == 0 {
                is_end_or_nul(rest)
            } else {
                rest.is_empty()
            };
            if !end_ok {
                return None;
            }
            offset_micros(negative, hour, 0, 0, frac_micros)
        }
        _ => {
            // Basic run: all remaining bytes must be digits (plus one
            // optional trailing NUL on the fraction-less lengths —
            // probed: `+0000\x00` reads, `+00000001\x00` fails);
            // lengths 2/4/6, or 8+ (a 1-digit fraction is invalid —
            // probed). `HHMM`/`HHMMSS` also take a dotted fraction
            // (probed: `+0000.0` reads, `+0100.5` is 3600.5s).
            let run = dbytes.iter().take_while(|b| b.is_ascii_digit()).count();
            if (run == 4 || run == 6) && matches!(dbytes.get(run), Some(b'.') | Some(b',')) {
                let minute = ascii2(dbytes.get(2..4)?)? as i64;
                let second = if run == 6 {
                    ascii2(dbytes.get(4..6)?)? as i64
                } else {
                    0
                };
                let (frac_micros, frac_len, rest) = split_core_fraction(&digits[run..]);
                if frac_len == 0 || !rest.is_empty() {
                    return None;
                }
                return offset_micros(negative, hour, minute, second, frac_micros);
            }
            let nul_end = run <= 6 && run + 1 == dbytes.len() && dbytes[run] == b'\0';
            if run != dbytes.len() && !nul_end {
                return None;
            }
            match run {
                2 => offset_micros(negative, hour, 0, 0, 0),
                4 => {
                    let minute = ascii2(dbytes.get(2..4)?)? as i64;
                    offset_micros(negative, hour, minute, 0, 0)
                }
                6 => {
                    let minute = ascii2(dbytes.get(2..4)?)? as i64;
                    let second = ascii2(dbytes.get(4..6)?)? as i64;
                    offset_micros(negative, hour, minute, second, 0)
                }
                _ if run >= 8 => {
                    let minute = ascii2(dbytes.get(2..4)?)? as i64;
                    let second = ascii2(dbytes.get(4..6)?)? as i64;
                    let frac = core::str::from_utf8(&dbytes[6..]).ok()?;
                    offset_micros(negative, hour, minute, second, frac_micros_of(frac))
                }
                _ => None,
            }
        }
    }
}

/// Fixed-offset total in micros: no per-field range checks (probed:
/// `+00:60` is valid); the total must be strictly under 24h. Quirk
/// (probed): a zero `HH:MM:SS` triple drops the fraction
/// (`+00:00:00.5` is UTC).
fn offset_micros(
    negative: bool,
    hour: i64,
    minute: i64,
    second: i64,
    frac_micros: u32,
) -> Option<Option<i64>> {
    let whole = hour * 3_600 + minute * 60 + second;
    let micros = if whole == 0 {
        0
    } else {
        whole * 1_000_000 + i64::from(frac_micros)
    };
    if micros.abs() >= 86_400_000_000 {
        return None;
    }
    Some(Some(if negative { -micros } else { micros }))
}

/// Django `parse_datetime`'s regex fallback (`dateparse.py`), used when
/// `fromisoformat` fails: extended date with 1-2-digit fields, `[T ]`
/// separator, `HH:MM` with optional `:SS` and fraction, optional
/// whitespace, optional short offset. `\d` is Unicode-aware here
/// (probed: fullwidth digits accepted on this path only).
fn parse_iso_regex(text: &str) -> Option<ParsedDateTime> {
    let chars: Vec<char> = text.chars().collect();
    let mut pos = 0;
    // `\d{4}-\d{1,2}-\d{1,2}`.
    let year = take_nd(&chars, &mut pos, 4, 4)? as i32;
    if year < 1 || take_char(&chars, &mut pos) != Some('-') {
        return None;
    }
    let month = take_nd(&chars, &mut pos, 1, 2)?;
    if take_char(&chars, &mut pos) != Some('-') {
        return None;
    }
    let day = take_nd(&chars, &mut pos, 1, 2)?;
    // `[T ]`.
    if !matches!(take_char(&chars, &mut pos), Some('T' | ' ')) {
        return None;
    }
    // `\d{1,2}:\d{1,2}`.
    let hour = take_nd(&chars, &mut pos, 1, 2)?;
    if hour > 23 || take_char(&chars, &mut pos) != Some(':') {
        return None;
    }
    let minute = take_nd(&chars, &mut pos, 1, 2)?;
    if minute > 59 {
        return None;
    }
    // Optional `:SS` plus optional fraction (1-6 captured, up to 6
    // ignored — 13+ fraction digits fail the whole match).
    let mut second = 0;
    let mut micros = 0;
    if chars.get(pos) == Some(&':') {
        pos += 1;
        second = take_nd(&chars, &mut pos, 1, 2)?;
        if second > 59 {
            return None;
        }
        if matches!(chars.get(pos), Some('.') | Some(',')) {
            pos += 1;
            let start = pos;
            let mut digits = 0u32;
            let mut count = 0usize;
            while count < 12 {
                match chars.get(pos).and_then(|ch| nd_value(*ch)) {
                    Some(digit) => {
                        if count < 6 {
                            digits = digits * 10 + digit;
                        }
                        count += 1;
                        pos += 1;
                    }
                    None => break,
                }
            }
            if pos == start {
                return None;
            }
            for _ in count.min(6)..6 {
                digits *= 10;
            }
            micros = digits;
        }
    }
    // `\s*` then optional `Z|[+-]\d{2}(?::?\d{2})?`, then end.
    while chars.get(pos).is_some_and(|ch| is_re_ws(*ch)) {
        pos += 1;
    }
    let mut offset_micros = None;
    if chars.get(pos) == Some(&'Z') {
        pos += 1;
        offset_micros = Some(0);
    } else if matches!(chars.get(pos), Some('+') | Some('-')) {
        let negative = chars[pos] == '-';
        pos += 1;
        let hour = take_nd(&chars, &mut pos, 2, 2)? as i64;
        let mut minute = 0i64;
        if chars.get(pos) == Some(&':') {
            pos += 1;
            minute = take_nd(&chars, &mut pos, 2, 2)? as i64;
        } else if chars.get(pos).is_some_and(|ch| nd_value(*ch).is_some()) {
            minute = take_nd(&chars, &mut pos, 2, 2)? as i64;
        }
        let total_minutes = hour * 60 + minute;
        if total_minutes * 60 >= 86_400 {
            return None;
        }
        let total = total_minutes * 60_000_000;
        offset_micros = Some(if negative { -total } else { total });
    }
    // `re` `$` matches before one trailing newline (`dateparse.py`
    // `datetime_re`), so `…Z\n` / `…+00:00\n` are valid on this regex
    // path only (never the `fromisoformat` path).
    if chars.get(pos) == Some(&'\n') {
        pos += 1;
    }
    if pos != chars.len() {
        return None;
    }
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let time = NaiveTime::from_hms_micro_opt(hour, minute, second, micros)?;
    Some(ParsedDateTime {
        naive: NaiveDateTime::new(date, time),
        offset_micros,
    })
}

/// Take `min..=max` [`nd_value`] digits, accumulating their value.
fn take_nd(chars: &[char], pos: &mut usize, min: usize, max: usize) -> Option<u32> {
    let mut value = 0u32;
    let mut count = 0usize;
    while count < max {
        match chars.get(*pos).and_then(|ch| nd_value(*ch)) {
            Some(digit) => {
                value = value * 10 + digit;
                count += 1;
                *pos += 1;
            }
            None => break,
        }
    }
    if count < min {
        return None;
    }
    Some(value)
}

/// Take one char, advancing.
fn take_char(chars: &[char], pos: &mut usize) -> Option<char> {
    let ch = *chars.get(*pos)?;
    *pos += 1;
    Some(ch)
}

/// `base_branch` `RegexValidator("^[A-Za-z0-9._/-]*$")`: the pattern is
/// anchored so a char-table check matches `search` exactly.
fn validate_base_branch(value: &str) -> Result<String, String> {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '/' | '-'))
    {
        Ok(value.to_owned())
    } else {
        Err("Branch name may contain only letters, numbers, and . _ / -".to_owned())
    }
}

/// FK input classification: bools → the `incorrect_type` message;
/// strings/ints → UUID parse (a miss is Django's `ValidationError`, caught
/// per-field by `to_internal_value` with the curly-quote message —
/// verified against DRF 3.15.2 `serializers.py`); anything else → the same
/// UUID message with the `str()` rendering.
fn classify_fk_input(value: &Value) -> Result<uuid::Uuid, String> {
    match value {
        Value::Null => Err("This field may not be null.".to_owned()),
        Value::Bool(_) => Err("Incorrect type. Expected pk value, received bool.".to_owned()),
        Value::String(text) => {
            uuid::Uuid::parse_str(text).map_err(|_| format!("“{text}” is not a valid UUID."))
        }
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                // `uuid.UUID(int=…)` accepts any non-negative int (`u128`
                // holds every `i64`); negatives raise → the UUID message.
                if int >= 0 {
                    return Ok(uuid::Uuid::from_u128(int as u128));
                }
                Err(format!("“{int}” is not a valid UUID."))
            } else {
                Err(format!("“{number}” is not a valid UUID."))
            }
        }
        Value::Array(_) | Value::Object(_) => {
            Err(format!("“{}” is not a valid UUID.", python_str(value)))
        }
    }
}

/// One FK existence probe: `PrimaryKeyRelatedField` looks the value up in
/// the target's default manager; a miss answers `Invalid pk …`. `sql`
/// carries the manager's filters (soft-deleted rows excluded, except the
/// stock-manager `users` table; triage excluded for `default_state`).
async fn validate_fk(
    pool: &sqlx::PgPool,
    value: &Value,
    allow_null: bool,
    sql: &str,
) -> Result<Option<uuid::Uuid>, Result<String, Denial>> {
    // `RelatedField.run_validation` forces empty strings to `None`
    // (`relations.py:151-155`) before the null check.
    if value.is_null() || value == &Value::String(String::new()) {
        if allow_null {
            return Ok(None);
        }
        return Err(Ok("This field may not be null.".to_owned()));
    }
    let id = classify_fk_input(value).map_err(Ok)?;
    let row: Option<(i32,)> = sqlx::query_as(sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| Err(db_denial(error)))?;
    if row.is_none() {
        let rendered = match value {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            _ => python_str(value),
        };
        return Err(Ok(format!(
            "Invalid pk \"{rendered}\" - object does not exist."
        )));
    }
    Ok(Some(id))
}

/// FK target tables with their default-manager filters. DRF builds auto
/// `PrimaryKeyRelatedField` querysets from `_default_manager`
/// (`field_mapping.py:256`): `AuditModel` inherits `SoftDeleteModel`
/// (`db/mixins.py:85`), so `FileAsset`/`Estimate`/`Workspace` reject
/// soft-deleted ids with 400 (`State` additionally excludes triage via
/// `StateManager`); only `User` (Django's plain `UserManager`) is
/// unfiltered. (Forward-FK *reads* use the plain `_base_manager`
/// instead — see [`file_asset_url`] — so the read path stays
/// unfiltered. Both halves live-probed.)
const FK_USERS_SQL: &str = r#"SELECT 1 FROM users WHERE id = $1"#;
const FK_FILE_ASSETS_SQL: &str =
    r#"SELECT 1 FROM file_assets WHERE id = $1 AND deleted_at IS NULL"#;
const FK_ESTIMATES_SQL: &str = r#"SELECT 1 FROM estimates WHERE id = $1 AND deleted_at IS NULL"#;
/// `StateManager` excludes triage (`db/models/state.py:79-83`) as well as
/// soft-deleted rows.
const FK_STATES_SQL: &str =
    r#"SELECT 1 FROM states WHERE id = $1 AND deleted_at IS NULL AND "group" <> 'triage'"#;
const FK_WORKSPACES_SQL: &str = r#"SELECT 1 FROM workspaces WHERE id = $1 AND deleted_at IS NULL"#;

/// Which serializer validates the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValidateMode {
    /// `ProjectSerializer`, full write (create): required
    /// `{name, identifier}`, `workspace`/`deleted_at` read-only (ignored),
    /// `validate_name`/`validate_identifier` active, no unique validators
    /// (their sets contain the read-only fields, so DRF skips them).
    Create,
    /// `ProjectSerializer`, `partial=True` (PATCH): nothing required, same
    /// read-only set and `validate_*` methods, plus the `is_default` guard.
    Partial,
    /// `ProjectListSerializer`, full write (PUT): required `{deleted_at,
    /// name, identifier, workspace}` (the `unique_together` validators
    /// force `deleted_at` required — verified live), no `read_only_fields`,
    /// no `validate_*`, four unique validators active.
    Full,
}

/// Validated write data: one `Option` per writable model field (`None` =
/// absent from the body). Read-only and unknown keys never land here —
/// DRF ignores them in input.
#[derive(Debug, Default)]
struct Validated {
    name: Option<String>,
    description: Option<String>,
    description_text: Option<Option<Value>>,
    description_html: Option<Option<Value>>,
    network: Option<i32>,
    identifier: Option<String>,
    default_assignee: Option<Option<uuid::Uuid>>,
    project_lead: Option<Option<uuid::Uuid>>,
    emoji: Option<Option<String>>,
    icon_prop: Option<Option<Value>>,
    module_view: Option<bool>,
    cycle_view: Option<bool>,
    issue_views_view: Option<bool>,
    page_view: Option<bool>,
    intake_view: Option<bool>,
    is_time_tracking_enabled: Option<bool>,
    is_issue_type_enabled: Option<bool>,
    guest_view_all_features: Option<bool>,
    cover_image: Option<Option<String>>,
    cover_image_asset: Option<Option<uuid::Uuid>>,
    estimate: Option<Option<uuid::Uuid>>,
    archive_in: Option<i64>,
    close_in: Option<i64>,
    logo_props: Option<Value>,
    default_state: Option<Option<uuid::Uuid>>,
    archived_at: Option<Option<DateTime<Utc>>>,
    is_default: Option<bool>,
    timezone: Option<String>,
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
    repo_url: Option<String>,
    base_branch: Option<String>,
    agent_default_interval_seconds: Option<i64>,
    agent_default_max_ticks: Option<i64>,
    agent_review_default_interval_seconds: Option<i64>,
    agent_test_default_interval_seconds: Option<i64>,
    agent_ticking_enabled: Option<bool>,
    default_agent_executor: Option<String>,
    members_can_edit_states: Option<bool>,
    /// Writable on update (`update()` persists it; `save()` overwrites
    /// only `updated_by` — `models/base.py`). On create the crum
    /// `save()` overwrites it with the actor, like Python.
    created_by: Option<Option<uuid::Uuid>>,
    updated_by: Option<Option<uuid::Uuid>>,
    /// PUT-only writable (`ProjectSerializer.read_only_fields` otherwise).
    deleted_at: Option<Option<DateTime<Utc>>>,
    /// PUT-only writable.
    workspace: Option<uuid::Uuid>,
}

/// Field errors in serializer field order (DRF collects per field in
/// iteration order, then `validate()` / validators append after).
type FieldErrors = Map<String, Value>;

fn push_error(errors: &mut FieldErrors, field: &str, message: String) {
    push_errors(errors, field, vec![message]);
}

/// A field error list: DRF's `run_validators` collects every validator
/// failure (e.g. `base_branch` reports `[regex, max_length]`).
fn push_errors(errors: &mut FieldErrors, field: &str, messages: Vec<String>) {
    let list: Vec<Value> = messages.into_iter().map(Value::String).collect();
    errors.insert(field.to_owned(), Value::Array(list));
}

fn render_field_errors(errors: &FieldErrors) -> String {
    let body = serde_json::to_string(&Value::Object(errors.clone())).expect("field errors");
    escape_u2028(body)
}

/// The writable model fields in DRF serializer order (the error-dict
/// order): `[pk] + declared + concrete + forward_relations`
/// (`serializers.py::get_default_field_names`, verified by a live
/// `ProjectListSerializer` field probe — `PUT {}` reports
/// `[deleted_at, name, identifier, workspace]`). `created_at` /
/// `updated_at` are auto (read-only), `id` is read-only.
const WRITE_FIELDS: &[&str] = &[
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "network",
    "identifier",
    "emoji",
    "icon_prop",
    "module_view",
    "cycle_view",
    "issue_views_view",
    "page_view",
    "intake_view",
    "is_time_tracking_enabled",
    "is_issue_type_enabled",
    "is_default",
    "guest_view_all_features",
    "members_can_edit_states",
    "cover_image",
    "archive_in",
    "close_in",
    "logo_props",
    "archived_at",
    "timezone",
    "external_source",
    "external_id",
    "repo_url",
    "base_branch",
    "agent_default_interval_seconds",
    "agent_default_max_ticks",
    "agent_review_default_interval_seconds",
    "agent_test_default_interval_seconds",
    "agent_ticking_enabled",
    "default_agent_executor",
    "created_by",
    "updated_by",
    "workspace",
    "default_assignee",
    "project_lead",
    "cover_image_asset",
    "estimate",
    "default_state",
];

/// Validate one body field-by-field. `instance` is the row being updated
/// (`None` on create) for the dup-exclusion; `workspace_id` scopes the dup
/// probes. Returns the validated data, or the 400 field-errors body.
/// DB failures inside the probes answer `Denial` directly.
#[allow(clippy::too_many_arguments)]
async fn validate_project_input(
    pool: &sqlx::PgPool,
    data: &Map<String, Value>,
    mode: ValidateMode,
    timezone: Tz,
    workspace_id: &uuid::Uuid,
    instance: Option<&ProjectRow>,
) -> Result<Validated, Result<String, Denial>> {
    let mut errors = FieldErrors::new();
    let mut out = Validated::default();
    // Required per mode: create needs `{name, identifier}`; PUT (full, no
    // read-only fields) needs `{deleted_at, name, identifier, workspace}`;
    // PATCH needs nothing.
    let required = |field: &str| match mode {
        ValidateMode::Create => matches!(field, "name" | "identifier"),
        ValidateMode::Partial => false,
        ValidateMode::Full => matches!(field, "deleted_at" | "name" | "identifier" | "workspace"),
    };
    // Read-only on input: `ProjectSerializer` ignores `workspace` /
    // `deleted_at` (`read_only_fields`); PUT ignores neither.
    let read_only =
        |field: &str| mode != ValidateMode::Full && matches!(field, "deleted_at" | "workspace");
    for field in WRITE_FIELDS {
        if read_only(field) {
            continue;
        }
        let Some(value) = data.get(*field) else {
            if required(field) {
                push_error(&mut errors, field, "This field is required.".to_owned());
            }
            continue;
        };
        if let Err(denial) = validate_one_field(
            pool,
            timezone,
            mode,
            workspace_id,
            instance,
            field,
            value,
            &mut out,
        )
        .await
        {
            match denial {
                Ok(messages) => push_errors(&mut errors, field, messages),
                Err(denial) => return Err(Err(denial)),
            }
        }
    }
    if !errors.is_empty() {
        return Err(Ok(render_field_errors(&errors)));
    }
    Ok(out)
}

/// Validate one provided value into `out`. `Ok(messages)` is a field
/// error list (DRF collects per-field validator failures);
/// `Err(denial)` a DB failure.
#[allow(clippy::too_many_arguments)]
async fn validate_one_field(
    pool: &sqlx::PgPool,
    timezone: Tz,
    mode: ValidateMode,
    workspace_id: &uuid::Uuid,
    instance: Option<&ProjectRow>,
    field: &str,
    value: &Value,
    out: &mut Validated,
) -> Result<(), Result<Vec<String>, Denial>> {
    let fail = |message: String| Err(Ok(vec![message]));
    let fail_many = |messages: Vec<String>| Err(Ok(messages));
    let fail_fk = |error: Result<String, Denial>| Err(error.map(|message| vec![message]));
    match field {
        // `name`/`identifier` parse (L1) then run the `validate_*`
        // dup probes inline, like DRF's per-field `validate_<name>`
        // methods — in field order, independent of other fields'
        // errors. PUT (`ProjectListSerializer`) has no such methods.
        "name" => {
            let name =
                match pidash_services::app_project::ser_project::parse_name_input(Some(value)) {
                    Ok(name) => name,
                    Err(errors) => {
                        return fail_many(
                            errors
                                .iter()
                                .map(|error| error.name_detail().to_owned())
                                .collect(),
                        )
                    }
                };
            if mode != ValidateMode::Full {
                if let Err(error) = validate_name_unique(pool, workspace_id, instance, &name).await
                {
                    match error {
                        Ok(message) => return fail(message),
                        Err(denial) => return Err(Err(denial)),
                    }
                }
            }
            out.name = Some(name);
        }
        "identifier" => {
            let identifier = match pidash_services::app_project::ser_project::parse_identifier_input(
                Some(value),
            ) {
                Ok(identifier) => identifier,
                Err(errors) => {
                    return fail_many(
                        errors
                            .iter()
                            .map(|error| error.identifier_detail().to_owned())
                            .collect(),
                    )
                }
            };
            if mode != ValidateMode::Full {
                if let Err(error) =
                    validate_identifier_unique(pool, workspace_id, instance, &identifier).await
                {
                    match error {
                        Ok(message) => return fail(message),
                        Err(denial) => return Err(Err(denial)),
                    }
                }
            }
            out.identifier = Some(identifier);
        }
        "description" => match validate_char(value, None, true) {
            Ok(text) => out.description = Some(text),
            Err(messages) => return fail_many(messages),
        },
        "description_text" => {
            if value.is_null() {
                out.description_text = Some(None);
            } else {
                out.description_text = Some(Some(value.clone()));
            }
        }
        "description_html" => {
            if value.is_null() {
                out.description_html = Some(None);
            } else {
                out.description_html = Some(Some(value.clone()));
            }
        }
        "network" => match validate_network(value) {
            Ok(network) => out.network = Some(network),
            Err(message) => return fail(message),
        },
        "default_assignee" => match validate_fk(pool, value, true, FK_USERS_SQL).await {
            Ok(id) => out.default_assignee = Some(id),
            Err(error) => return fail_fk(error),
        },
        "project_lead" => match validate_fk(pool, value, true, FK_USERS_SQL).await {
            Ok(id) => out.project_lead = Some(id),
            Err(error) => return fail_fk(error),
        },
        "emoji" => match opt_char(value, Some(255), true) {
            Ok(text) => out.emoji = Some(text),
            Err(messages) => return fail_many(messages),
        },
        "icon_prop" => {
            if value.is_null() {
                out.icon_prop = Some(None);
            } else {
                out.icon_prop = Some(Some(value.clone()));
            }
        }
        "module_view" => match validate_bool(value) {
            Ok(flag) => out.module_view = Some(flag),
            Err(message) => return fail(message),
        },
        "cycle_view" => match validate_bool(value) {
            Ok(flag) => out.cycle_view = Some(flag),
            Err(message) => return fail(message),
        },
        "issue_views_view" => match validate_bool(value) {
            Ok(flag) => out.issue_views_view = Some(flag),
            Err(message) => return fail(message),
        },
        "page_view" => match validate_bool(value) {
            Ok(flag) => out.page_view = Some(flag),
            Err(message) => return fail(message),
        },
        "intake_view" => match validate_bool(value) {
            Ok(flag) => out.intake_view = Some(flag),
            Err(message) => return fail(message),
        },
        "is_time_tracking_enabled" => match validate_bool(value) {
            Ok(flag) => out.is_time_tracking_enabled = Some(flag),
            Err(message) => return fail(message),
        },
        "is_issue_type_enabled" => match validate_bool(value) {
            Ok(flag) => out.is_issue_type_enabled = Some(flag),
            Err(message) => return fail(message),
        },
        "guest_view_all_features" => match validate_bool(value) {
            Ok(flag) => out.guest_view_all_features = Some(flag),
            Err(message) => return fail(message),
        },
        "cover_image" => match opt_char(value, None, true) {
            Ok(text) => out.cover_image = Some(text),
            Err(messages) => return fail_many(messages),
        },
        "cover_image_asset" => match validate_fk(pool, value, true, FK_FILE_ASSETS_SQL).await {
            Ok(id) => out.cover_image_asset = Some(id),
            Err(error) => return fail_fk(error),
        },
        "estimate" => match validate_fk(pool, value, true, FK_ESTIMATES_SQL).await {
            Ok(id) => out.estimate = Some(id),
            Err(error) => return fail_fk(error),
        },
        "archive_in" => match check_int_range(validate_int(value), 0, 12) {
            Ok(number) => out.archive_in = Some(number),
            Err(message) => return fail(message),
        },
        "close_in" => match check_int_range(validate_int(value), 0, 12) {
            Ok(number) => out.close_in = Some(number),
            Err(message) => return fail(message),
        },
        "logo_props" => {
            if value.is_null() {
                return fail("This field may not be null.".to_owned());
            }
            out.logo_props = Some(value.clone());
        }
        "default_state" => match validate_fk(pool, value, true, FK_STATES_SQL).await {
            Ok(id) => out.default_state = Some(id),
            Err(error) => return fail_fk(error),
        },
        "archived_at" => match validate_datetime(value, timezone, true) {
            Ok(when) => out.archived_at = Some(when),
            Err(message) => return fail(message),
        },
        "is_default" => match validate_bool(value) {
            Ok(flag) => out.is_default = Some(flag),
            Err(message) => return fail(message),
        },
        "timezone" => match validate_str_choice(value, TIMEZONE_CHOICES) {
            Ok(zone) => out.timezone = Some(zone),
            Err(message) => return fail(message),
        },
        "external_source" => match opt_char(value, Some(255), true) {
            Ok(text) => out.external_source = Some(text),
            Err(messages) => return fail_many(messages),
        },
        "external_id" => match opt_char(value, Some(255), true) {
            Ok(text) => out.external_id = Some(text),
            Err(messages) => return fail_many(messages),
        },
        "repo_url" => match validate_char(value, Some(512), true) {
            Ok(text) => out.repo_url = Some(text),
            Err(messages) => return fail_many(messages),
        },
        // Model `RegexValidator` runs before the `CharField`
        // validators (`field.validators` comes first in
        // `ModelSerializer` assembly — probed order `[regex,
        // max_length]`), collecting every failure.
        "base_branch" => match char_to_string(value, true) {
            Ok(text) => {
                let mut errors = Vec::new();
                if let Err(message) = validate_base_branch(&text) {
                    errors.push(message);
                }
                errors.extend(char_length_errors(&text, Some(128)));
                if text.contains('\0') {
                    errors.push(NULL_CHARACTERS_MESSAGE.to_owned());
                }
                if errors.is_empty() {
                    out.base_branch = Some(text);
                } else {
                    return fail_many(errors);
                }
            }
            Err(messages) => return fail_many(messages),
        },
        "agent_default_interval_seconds" => {
            match check_int_range(
                validate_int(value),
                i64::from(i32::MIN),
                i64::from(i32::MAX),
            ) {
                Ok(number) => out.agent_default_interval_seconds = Some(number),
                Err(message) => return fail(message),
            }
        }
        "agent_default_max_ticks" => {
            match check_int_range(
                validate_int(value),
                i64::from(i32::MIN),
                i64::from(i32::MAX),
            ) {
                Ok(number) => out.agent_default_max_ticks = Some(number),
                Err(message) => return fail(message),
            }
        }
        "agent_review_default_interval_seconds" => {
            match check_int_range(
                validate_int(value),
                i64::from(i32::MIN),
                i64::from(i32::MAX),
            ) {
                Ok(number) => out.agent_review_default_interval_seconds = Some(number),
                Err(message) => return fail(message),
            }
        }
        "agent_test_default_interval_seconds" => {
            match check_int_range(
                validate_int(value),
                i64::from(i32::MIN),
                i64::from(i32::MAX),
            ) {
                Ok(number) => out.agent_test_default_interval_seconds = Some(number),
                Err(message) => return fail(message),
            }
        }
        "agent_ticking_enabled" => match validate_bool(value) {
            Ok(flag) => out.agent_ticking_enabled = Some(flag),
            Err(message) => return fail(message),
        },
        "default_agent_executor" => {
            match validate_str_choice(value, &["local_runner", "cloud_agent", "managed_runner"]) {
                Ok(executor) => out.default_agent_executor = Some(executor),
                Err(message) => return fail(message),
            }
        }
        "members_can_edit_states" => match validate_bool(value) {
            Ok(flag) => out.members_can_edit_states = Some(flag),
            Err(message) => return fail(message),
        },
        "created_by" => match validate_fk(pool, value, true, FK_USERS_SQL).await {
            Ok(id) => out.created_by = Some(id),
            Err(error) => return fail_fk(error),
        },
        "updated_by" => match validate_fk(pool, value, true, FK_USERS_SQL).await {
            Ok(id) => out.updated_by = Some(id),
            Err(error) => return fail_fk(error),
        },
        "deleted_at" => match validate_datetime(value, timezone, true) {
            Ok(when) => out.deleted_at = Some(when),
            Err(message) => return fail(message),
        },
        "workspace" => match validate_fk(pool, value, false, FK_WORKSPACES_SQL).await {
            Ok(id) => out.workspace = id,
            Err(error) => return fail_fk(error),
        },
        _ => unreachable!("WRITE_FIELDS is exhaustive"),
    }
    Ok(())
}

/// `ProjectSerializer.validate_name` (`serializers/project.py:48-63`, L1
/// `check_name_value`): the forbidden-name table, then a live-row dup probe
/// on the stripped value (case-sensitive), excluding the instance.
async fn validate_name_unique(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    instance: Option<&ProjectRow>,
    name: &str,
) -> Result<(), Result<String, Denial>> {
    use pidash_services::app_project::ser_project::check_name_value;
    let row: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM projects
           WHERE workspace_id = $1 AND name = $2 AND deleted_at IS NULL
             AND ($3::uuid IS NULL OR id <> $3)"#,
    )
    .bind(workspace_id)
    .bind(name)
    .bind(instance.map(|row| row.id))
    .fetch_optional(pool)
    .await
    .map_err(|error| Err(db_denial(error)))?;
    check_name_value(name, row.is_some()).map_err(|error| Ok(error.detail().to_owned()))
}

/// `ProjectSerializer.validate_identifier`
/// (`serializers/project.py:65-82`, L1 `check_identifier_value`):
/// forbidden characters, then a live-row dup probe on the stripped *raw*
/// value (case-sensitive — the uppercasing happens later in `save()`,
/// Q-identifier-case), excluding the instance.
async fn validate_identifier_unique(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    instance: Option<&ProjectRow>,
    identifier: &str,
) -> Result<(), Result<String, Denial>> {
    use pidash_services::app_project::ser_project::check_identifier_value;
    let row: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM projects
           WHERE workspace_id = $1 AND identifier = $2 AND deleted_at IS NULL
             AND ($3::uuid IS NULL OR id <> $3)"#,
    )
    .bind(workspace_id)
    .bind(identifier)
    .bind(instance.map(|row| row.id))
    .fetch_optional(pool)
    .await
    .map_err(|error| Err(db_denial(error)))?;
    check_identifier_value(identifier, row.is_some()).map_err(|error| Ok(error.detail().to_owned()))
}

/// `ProjectSerializer.validate` (`serializers/project.py:83-106`):
/// runs only when every field validated, over the *validated* attrs —
/// first the executor gate (a `CloudAgentUnavailableAPI`, rendered as
/// the D-11 409, *not* a field error), then the `is_default` guard
/// (`instance.is_default and validated is False`, raised as a plain
/// string → `non_field_errors`), then the `description_html` sanitize
/// branch (truthy → `str()` → clean → writeback, or the invalid 400).
/// Returns the status + body, or a DB `Denial`.
fn run_serializer_validate(
    state: &AppState,
    validated: &mut Validated,
    instance: Option<&ProjectRow>,
) -> Result<(), Result<(StatusCode, String), Denial>> {
    use pidash_services::app_project::ser_project::{
        check_description_html, check_executor_gate, check_is_default_unset,
        cloud_unavailable_body, ExecutorGate, HtmlVerdict,
    };
    // Executor gate over the validated executor (`data.get(...)` misses
    // when absent, and `None != "cloud_agent"` passes).
    let configured =
        pidash_services::dispatch::policy::cloud_agent_is_configured(&state.settings().cloud_agent);
    if matches!(
        check_executor_gate(validated.default_agent_executor.as_deref(), configured),
        ExecutorGate::CloudUnavailable
    ) {
        let body = cloud_unavailable_body();
        let rendered = match serde_json::to_string(&body) {
            Ok(rendered) => rendered,
            Err(_) => return Err(Err(Denial::ServerError)),
        };
        return Err(Ok((StatusCode::CONFLICT, rendered)));
    }
    // `is_default` guard (`instance.is_default and validated is False`).
    if let Err(error) = check_is_default_unset(
        instance.is_some_and(|row| row.is_default),
        validated.is_default,
    ) {
        return Err(Ok((StatusCode::BAD_REQUEST, error.body().to_owned())));
    }
    // `description_html` branch (L1 runs the truthiness gate; the
    // closure runs the shared cleaner only for truthy values).
    let html: Option<&Value> = validated
        .description_html
        .as_ref()
        .and_then(|inner| inner.as_ref());
    match check_description_html(html, || {
        // The value is truthy here (L1 checked); `str()` it like Python.
        let text = python_str(html.expect("truthy html"));
        match crate::space::sanitize::sanitize_html(&text) {
            crate::space::sanitize::Sanitize::Clean(clean) => HtmlVerdict {
                is_valid: true,
                sanitized: Some(clean),
            },
            crate::space::sanitize::Sanitize::Invalid => HtmlVerdict {
                is_valid: false,
                sanitized: None,
            },
        }
    }) {
        Ok(cleaned) => {
            if let Some(cleaned) = cleaned {
                validated.description_html = Some(Some(Value::String(cleaned)));
            }
        }
        Err(error) => return Err(Ok((StatusCode::BAD_REQUEST, error.body().to_owned()))),
    }
    Ok(())
}

/// The four PUT unique validators (`ProjectListSerializer`, DRF
/// `UniqueTogetherValidator` × 4 — the two `unique_together` sets plus the
/// two multi-field `UniqueConstraint`s, each on the live-only default
/// manager with the instance excluded). On update a set fires only when
/// every one of its fields is present in the validated attrs AND at
/// least one field changed AND no changed value is `None`
/// (`validators.py::UniqueTogetherValidator.__call__` — the
/// `checked_values` gating; the identifier compares raw-vs-instance, as
/// DRF does, since `save()` uppercases later). A set whose filter
/// matches appends, in validator order, to one `non_field_errors` list.
/// The triple probes carry the live-manager `deleted_at IS NULL` base,
/// so a triple with a null `deleted_at` coincides with its pair probe
/// (both messages kept, exactly as in DRF) while a non-null one can
/// never match. Runs only after all field validation passed
/// (`to_internal_value` raising skips validators entirely).
async fn run_put_unique_validators(
    pool: &sqlx::PgPool,
    validated: &Validated,
    instance: &ProjectRow,
) -> Result<(), Result<String, Denial>> {
    let identifier = validated.identifier.clone();
    let name = validated.name.clone();
    let workspace = validated.workspace;
    let deleted_at = validated.deleted_at;
    // Changed-vs-instance per field (raw validated values, as DRF
    // compares `attrs[field] != getattr(instance, field)`).
    let changed = |field: &str| -> bool {
        match field {
            "identifier" => identifier.as_deref() != Some(instance.identifier.as_str()),
            "name" => name.as_deref() != Some(instance.name.as_str()),
            "workspace" => workspace != Some(instance.workspace_id),
            "deleted_at" => deleted_at != Some(instance.deleted_at),
            _ => false,
        }
    };
    // A changed value that is `None` vetoes its set.
    let changed_to_none = |field: &str| -> bool {
        if !changed(field) {
            return false;
        }
        match field {
            "identifier" => identifier.is_none(),
            "name" => name.is_none(),
            "workspace" => workspace.is_none(),
            "deleted_at" => deleted_at == Some(None),
            _ => false,
        }
    };
    let mut messages: Vec<String> = Vec::new();
    // (fields, probe column): presence- then changed/None-gated like DRF.
    for (fields, column) in [
        (
            ["identifier", "workspace", "deleted_at"].as_slice(),
            "identifier",
        ),
        (["name", "workspace", "deleted_at"].as_slice(), "name"),
        (["identifier", "workspace"].as_slice(), "identifier"),
        (["name", "workspace"].as_slice(), "name"),
    ] {
        let have = |field: &str| match field {
            "identifier" => identifier.is_some(),
            "name" => name.is_some(),
            "workspace" => workspace.is_some(),
            "deleted_at" => deleted_at.is_some(),
            _ => false,
        };
        if !fields.iter().all(|field| have(field)) {
            continue;
        }
        if !fields.iter().any(|field| changed(field)) {
            continue;
        }
        if fields.iter().any(|field| changed_to_none(field)) {
            continue;
        }
        let value = if column == "identifier" {
            identifier.clone().expect("present")
        } else {
            name.clone().expect("present")
        };
        let conflict: Option<(i32,)> = if fields.len() == 3 {
            sqlx::query_as(&format!(
                "SELECT 1 FROM projects WHERE workspace_id = $1 AND {column} = $2 \
                 AND deleted_at IS NULL AND deleted_at IS NOT DISTINCT FROM $3 AND id <> $4"
            ))
            .bind(workspace.expect("present"))
            .bind(value)
            .bind(deleted_at.expect("present"))
            .bind(instance.id)
            .fetch_optional(pool)
            .await
            .map_err(|error| Err(db_denial(error)))?
        } else {
            sqlx::query_as(&format!(
                "SELECT 1 FROM projects WHERE workspace_id = $1 AND {column} = $2 \
                 AND deleted_at IS NULL AND id <> $3"
            ))
            .bind(workspace.expect("present"))
            .bind(value)
            .bind(instance.id)
            .fetch_optional(pool)
            .await
            .map_err(|error| Err(db_denial(error)))?
        };
        if conflict.is_some() {
            messages.push(format!(
                "The fields {} must make a unique set.",
                fields.join(", ")
            ));
        }
    }
    if messages.is_empty() {
        Ok(())
    } else {
        let list = serde_json::to_string(&messages).map_err(|_| Err(Denial::ServerError))?;
        Err(Ok(format!("{{\"non_field_errors\":{list}}}")))
    }
}

// ---------------------------------------------------------------------------
// Task publishes (deferred, best-effort)
// ---------------------------------------------------------------------------

/// Best-effort deferred publish (the space intake precedent): without
/// the queue the response still stands.
async fn enqueue_message(pool: &sqlx::PgPool, message: pidash_jobs::celery::CeleryTaskMessage) {
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// One `recent_visited_task.delay(...)` enqueue (`base.py:246-253`, L8
/// `RecentVisitedEmit`): the rewritten project id twice (as `project_id`
/// and `entity_identifier`), stringified (QUIRK-uuid-string-args).
fn recent_visited_message(
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    slug: &str,
) -> pidash_jobs::celery::CeleryTaskMessage {
    use pidash_services::app_project::tasks::RecentVisitedEmit;
    let emit = RecentVisitedEmit {
        slug: slug.to_owned(),
        project_id: project_id.to_string(),
        user_id: user_id.to_string(),
    };
    let task = emit.task_name().to_owned();
    pidash_jobs::celery::CeleryTaskMessage::new(task, vec![], emit.kwargs())
}

/// One `model_activity.delay(...)` enqueue on create (`base.py:299-307`,
/// L8 `model_activity_on_create`): the raw body as `requested_data`
/// (Q-activity-raw-data), no `current_instance`.
fn model_activity_create_message(
    project_id: &uuid::Uuid,
    requested_data: Map<String, Value>,
    actor_id: &uuid::Uuid,
    slug: &str,
    origin: &str,
) -> pidash_jobs::celery::CeleryTaskMessage {
    use pidash_services::app_project::tasks::model_activity_on_create;
    let emit = model_activity_on_create(
        project_id.to_string(),
        requested_data,
        actor_id.to_string(),
        slug.to_owned(),
        origin.to_owned(),
    );
    let task = emit.task_name().to_owned();
    pidash_jobs::celery::CeleryTaskMessage::new(task, vec![], emit.kwargs())
}

/// One `model_activity.delay(...)` enqueue on partial update
/// (`base.py:366-376`, L8 `model_activity_on_partial_update`): the raw
/// body plus the pre-save `current_instance` dump.
#[allow(clippy::too_many_arguments)]
fn model_activity_update_message(
    project_id: &uuid::Uuid,
    requested_data: Map<String, Value>,
    current_instance: String,
    actor_id: &uuid::Uuid,
    slug: &str,
    origin: &str,
) -> pidash_jobs::celery::CeleryTaskMessage {
    use pidash_services::app_project::tasks::model_activity_on_partial_update;
    let emit = model_activity_on_partial_update(
        project_id.to_string(),
        requested_data,
        current_instance,
        actor_id.to_string(),
        slug.to_owned(),
        origin.to_owned(),
    );
    let task = emit.task_name().to_owned();
    pidash_jobs::celery::CeleryTaskMessage::new(task, vec![], emit.kwargs())
}

/// One `webhook_activity.delay(...)` enqueue on destroy (`base.py:407-415`,
/// L8 `WebhookActivityEmit`): the stringified UUID args plus `event_type`
/// `deleted`.
fn webhook_destroy_message(
    project_id: &uuid::Uuid,
    actor_id: &uuid::Uuid,
    slug: &str,
    origin: &str,
) -> pidash_jobs::celery::CeleryTaskMessage {
    use pidash_services::app_project::tasks::WebhookActivityEmit;
    let emit = WebhookActivityEmit {
        actor_id: actor_id.to_string(),
        slug: slug.to_owned(),
        current_site: origin.to_owned(),
        event_id: project_id.to_string(),
    };
    let task = emit.task_name().to_owned();
    pidash_jobs::celery::CeleryTaskMessage::new(task, vec![], emit.kwargs())
}

/// The `SoftDeleteModel.delete` emit (`db/mixins.py:72-76`):
/// `.delay(app_label, model_name, pk, using=None)` — three string args,
/// `{"using": null}` kwargs.
fn soft_delete_message(pk: &uuid::Uuid) -> pidash_jobs::celery::CeleryTaskMessage {
    let mut kwargs = Map::with_capacity(1);
    kwargs.insert("using".to_owned(), Value::Null);
    pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String("project".to_owned()),
            Value::String(pk.to_string()),
        ],
        kwargs,
    )
}

// ---------------------------------------------------------------------------
// Writes: project insert/update, members, states, identifiers
// ---------------------------------------------------------------------------

/// `Project.objects.create(**validated_data, workspace_id=...)`
/// (`serializers/project.py:110-117` + `Project.save`,
/// `db/models/project.py:241-303`): identifier stripped+upper-cased
/// (name kept verbatim), timezone kept when provided else the workspace
/// zone, `is_default` resolved (first live project wins regardless),
/// crum stamps `created_by`. `workspace_tz` is the workspace row's zone.
#[allow(clippy::too_many_arguments)]
async fn insert_project(
    pool: &sqlx::PgPool,
    state: &AppState,
    validated: &Validated,
    workspace_id: &uuid::Uuid,
    workspace_tz: &str,
    actor_id: &uuid::Uuid,
    now: DateTime<Utc>,
) -> Result<uuid::Uuid, Denial> {
    use pidash_db::app_project::models::project as m;
    let has_default: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM projects
           WHERE workspace_id = $1 AND is_default AND deleted_at IS NULL LIMIT 1"#,
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    let is_default =
        m::auto_default_on_create(validated.is_default.unwrap_or(false), has_default.is_some());
    let timezone = match validated.timezone.as_deref() {
        Some(requested) => m::timezone_on_create(true, requested, workspace_tz),
        None => m::timezone_on_create(false, "", workspace_tz),
    };
    let identifier =
        m::normalize_identifier(validated.identifier.as_deref().expect("required on create"));
    let default_executor = match validated.default_agent_executor.as_deref() {
        Some(executor) => executor.to_owned(),
        None => {
            let candidate = state.settings().default_agent_executor.clone();
            if ["local_runner", "cloud_agent", "managed_runner"].contains(&candidate.as_str()) {
                candidate
            } else {
                "local_runner".to_owned()
            }
        }
    };
    let id = uuid::Uuid::new_v4();
    let name = validated.name.as_deref().expect("required on create");
    // `int4` columns: out-of-range `IntegerField` values 500 at the
    // column (`archive_in`/`close_in` are already fenced by 0..12).
    let archive_in = i32_or_500(validated.archive_in.unwrap_or(0))?;
    let close_in = i32_or_500(validated.close_in.unwrap_or(0))?;
    let agent_interval = i32_or_500(validated.agent_default_interval_seconds.unwrap_or(10800))?;
    let agent_max_ticks = i32_or_500(validated.agent_default_max_ticks.unwrap_or(10))?;
    let agent_review = i32_or_500(
        validated
            .agent_review_default_interval_seconds
            .unwrap_or(10800),
    )?;
    let agent_test = i32_or_500(
        validated
            .agent_test_default_interval_seconds
            .unwrap_or(10800),
    )?;
    // `save()` runs inside `transaction.atomic()`, clearing the previous
    // default *before* the insert — the partial unique index forbids two
    // live defaults even momentarily.
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    if is_default {
        sqlx::query(
            r#"UPDATE projects SET is_default = FALSE
               WHERE workspace_id = $1 AND is_default AND deleted_at IS NULL AND id <> $2"#,
        )
        .bind(workspace_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_denial)?;
    }
    let result = sqlx::query(
        r#"INSERT INTO projects (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
              name, description, description_text, description_html, network, workspace_id, identifier,
              default_assignee_id, project_lead_id, emoji, icon_prop,
              module_view, cycle_view, issue_views_view, page_view, intake_view,
              is_time_tracking_enabled, is_issue_type_enabled, guest_view_all_features,
              cover_image, cover_image_asset_id, estimate_id, archive_in, close_in, logo_props,
              default_state_id, archived_at, is_default, timezone, external_source, external_id,
              repo_url, base_branch,
              agent_default_interval_seconds, agent_default_max_ticks,
              agent_review_default_interval_seconds, agent_test_default_interval_seconds,
              agent_ticking_enabled, default_agent_executor, members_can_edit_states)
           VALUES ($1, $2, $2, $3, NULL, NULL,
              $4, $5, $6, $7, $8, $9, $10,
              $11, $12, $13, $14,
              $15, $16, $17, $18, $19,
              $20, $21, $22,
              $23, $24, $25, $26, $27, $28,
              $29, $30, $31, $32, $33, $34,
              $35, $36,
              $37, $38, $39, $40,
              $41, $42, $43)"#,
    )
    .bind(id)
    .bind(now)
    .bind(actor_id)
    .bind(name)
    .bind(validated.description.as_deref().unwrap_or(""))
    .bind(validated.description_text.clone().unwrap_or(None))
    .bind(validated.description_html.clone().unwrap_or(None))
    .bind(validated.network.unwrap_or(2))
    .bind(workspace_id)
    .bind(identifier)
    .bind(validated.default_assignee.unwrap_or(None))
    .bind(validated.project_lead.unwrap_or(None))
    .bind(validated.emoji.clone().unwrap_or(None))
    .bind(validated.icon_prop.clone().unwrap_or(None))
    .bind(validated.module_view.unwrap_or(false))
    .bind(validated.cycle_view.unwrap_or(false))
    .bind(validated.issue_views_view.unwrap_or(false))
    .bind(validated.page_view.unwrap_or(true))
    .bind(validated.intake_view.unwrap_or(false))
    .bind(validated.is_time_tracking_enabled.unwrap_or(false))
    .bind(validated.is_issue_type_enabled.unwrap_or(false))
    .bind(validated.guest_view_all_features.unwrap_or(false))
    .bind(validated.cover_image.clone().unwrap_or(None))
    .bind(validated.cover_image_asset.unwrap_or(None))
    .bind(validated.estimate.unwrap_or(None))
    .bind(archive_in)
    .bind(close_in)
    .bind(validated.logo_props.clone().unwrap_or(Value::Object(Map::new())))
    .bind(validated.default_state.unwrap_or(None))
    .bind(validated.archived_at.unwrap_or(None))
    .bind(is_default)
    .bind(timezone)
    .bind(validated.external_source.clone().unwrap_or(None))
    .bind(validated.external_id.clone().unwrap_or(None))
    .bind(validated.repo_url.as_deref().unwrap_or(""))
    .bind(validated.base_branch.as_deref().unwrap_or("main"))
    .bind(agent_interval)
    .bind(agent_max_ticks)
    .bind(agent_review)
    .bind(agent_test)
    .bind(validated.agent_ticking_enabled.unwrap_or(true))
    .bind(default_executor)
    .bind(validated.members_can_edit_states.unwrap_or(true))
    .execute(&mut *tx)
    .await
    .map_err(db_denial)?;
    debug_assert_eq!(result.rows_affected(), 1);
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    Ok(id)
}

/// `ProjectIdentifier.objects.create(name=project.identifier, ...)`
/// (`serializers/project.py:114`): the *saved* (upper-cased) identifier.
/// `AuditModel` has no crum `save`, so both audit columns stay NULL; `id`
/// is the stock auto-int.
async fn insert_project_identifier(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    identifier: &str,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    sqlx::query(
        r#"INSERT INTO project_identifiers
             (created_at, updated_at, created_by_id, updated_by_id, deleted_at,
              workspace_id, project_id, name)
           VALUES ($1, $1, NULL, NULL, NULL, $2, $3, $4)"#,
    )
    .bind(now)
    .bind(workspace_id)
    .bind(project_id)
    .bind(identifier)
    .execute(pool)
    .await
    .map_err(db_denial)?;
    Ok(())
}

/// `ProjectMember.objects.create(project_id=..., member=..., role=20)`
/// (`base.py:282-296`): workspace backfilled from the project
/// (`ProjectBaseModel.save`), Q-member-property row inserted *first*
/// (`MIN(sort_order) - 10000`, else 65535, L5
/// `sort_order_on_create`), crum stamps `created_by`.
async fn insert_member(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    member_id: &uuid::Uuid,
    actor_id: &uuid::Uuid,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    use pidash_db::app_issues::models_core::project_user_property as props;
    // `ProjectMember.save` (`project.py:348-362`): the new property
    // takes the member's live minimum in the workspace minus 10000 —
    // per (workspace, user), NOT per project.
    let min: (Option<f64>,) = sqlx::query_as(
        r#"SELECT MIN(sort_order) FROM project_user_properties
           WHERE workspace_id = $1 AND user_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(workspace_id)
    .bind(member_id)
    .fetch_one(pool)
    .await
    .map_err(db_denial)?;
    let sort_order = pidash_db::app_project::models::project_member::sort_order_on_create(min.0);
    sqlx::query(
        r#"INSERT INTO project_user_properties
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
              project_id, workspace_id, user_id, filters, display_filters, display_properties,
              rich_filters, preferences, sort_order)
           VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, $9, $10, $11, $12)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(actor_id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(member_id)
    .bind(props::default_filters())
    .bind(props::default_display_filters())
    .bind(props::default_display_properties())
    .bind(pidash_db::app_project::models::project_user_property::default_rich_filters())
    .bind(pidash_db::app_project::models::project_member::default_preferences())
    .bind(sort_order)
    .execute(pool)
    .await
    .map_err(db_denial)?;
    sqlx::query(
        r#"INSERT INTO project_members
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
              project_id, workspace_id, member_id, comment, role,
              view_props, default_props, preferences, sort_order, is_active)
           VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, NULL, $7, $8, $8, $9, 65535, TRUE)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(actor_id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(member_id)
    .bind(ROLE_ADMIN)
    .bind(pidash_db::app_project::models::project_member::default_props())
    .bind(pidash_db::app_project::models::project_member::default_preferences())
    .execute(pool)
    .await
    .map_err(db_denial)?;
    Ok(())
}

/// `State.objects.bulk_create([... DEFAULT_STATES ...])` (`base.py:283-293`):
/// explicit name/color/sequence/group/default plus project/workspace and
/// `created_by`; everything else is Django field defaults (`description`
/// `slug` `""`, `is_triage` false, nullables NULL, `updated_by` NULL —
/// `bulk_create` calls no `save()`), stamps at `now`.
async fn insert_default_states(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    actor_id: &uuid::Uuid,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    use pidash_db::app_project::models::state::DEFAULT_STATES;
    for &(name, color, sequence, group, default) in DEFAULT_STATES.iter() {
        sqlx::query(
            r#"INSERT INTO states
                 (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                  project_id, workspace_id, name, description, color, slug, sequence,
                  "group", is_triage, "default", external_source, external_id)
               VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, '', $7, '', $8, $9, FALSE, $10, NULL, NULL)"#,
        )
        .bind(uuid::Uuid::new_v4())
        .bind(now)
        .bind(actor_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(name)
        .bind(color)
        .bind(sequence)
        .bind(group)
        .bind(default)
        .execute(pool)
        .await
        .map_err(db_denial)?;
    }
    Ok(())
}

/// Apply one validated update (`serializer.save()` → `Project.save`,
/// `db/models/project.py:241-303`): identifier re-normalized, `is_default`
/// guarded (unsetting the live default without a replacement refuses
/// with the `ValidationError` 400) and flipped (clear predecessors),
/// `updated_at`/`updated_by` stamped by `auto_now`/crum. Django's
/// `save()` writes every column; provided values win, the rest keep the
/// instance values.
async fn apply_update(
    pool: &sqlx::PgPool,
    validated: &Validated,
    instance: &ProjectRow,
    actor_id: &uuid::Uuid,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    use pidash_db::app_project::models::project as m;
    let new_default = validated.is_default.unwrap_or(instance.is_default);
    // `save()` is atomic (`project.py:292-301`): guard, clear, write.
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    if !new_default && instance.is_default {
        let replacement: Option<(i32,)> = sqlx::query_as(
            r#"SELECT 1 FROM projects
               WHERE workspace_id = $1 AND is_default AND deleted_at IS NULL AND id <> $2"#,
        )
        .bind(instance.workspace_id)
        .bind(instance.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_denial)?;
        m::unset_default_check(new_default, instance.is_default, replacement.is_some())
            .map_err(|_| Denial::ValidationFailed)?;
    }
    if new_default {
        sqlx::query(
            r#"UPDATE projects SET is_default = FALSE
               WHERE workspace_id = $1 AND is_default AND deleted_at IS NULL AND id <> $2"#,
        )
        .bind(instance.workspace_id)
        .bind(instance.id)
        .execute(&mut *tx)
        .await
        .map_err(db_denial)?;
    }
    let identifier = match validated.identifier.as_deref() {
        Some(raw) => m::normalize_identifier(raw),
        None => instance.identifier.clone(),
    };
    let archive_in = i32_or_500(validated.archive_in.unwrap_or(instance.archive_in as i64))?;
    let close_in = i32_or_500(validated.close_in.unwrap_or(instance.close_in as i64))?;
    let agent_interval = i32_or_500(
        validated
            .agent_default_interval_seconds
            .unwrap_or(instance.agent_default_interval_seconds as i64),
    )?;
    let agent_max_ticks = i32_or_500(
        validated
            .agent_default_max_ticks
            .unwrap_or(instance.agent_default_max_ticks as i64),
    )?;
    let agent_review = i32_or_500(
        validated
            .agent_review_default_interval_seconds
            .unwrap_or(instance.agent_review_default_interval_seconds as i64),
    )?;
    let agent_test = i32_or_500(
        validated
            .agent_test_default_interval_seconds
            .unwrap_or(instance.agent_test_default_interval_seconds as i64),
    )?;
    sqlx::query(
        r#"UPDATE projects SET
             name = $1, description = $2, description_text = $3, description_html = $4,
             network = $5, identifier = $6,
             default_assignee_id = $7, project_lead_id = $8, emoji = $9, icon_prop = $10,
             module_view = $11, cycle_view = $12, issue_views_view = $13, page_view = $14,
             intake_view = $15, is_time_tracking_enabled = $16, is_issue_type_enabled = $17,
             guest_view_all_features = $18,
             cover_image = $19, cover_image_asset_id = $20, estimate_id = $21,
             archive_in = $22, close_in = $23, logo_props = $24,
             default_state_id = $25, archived_at = $26, is_default = $27, timezone = $28,
             external_source = $29, external_id = $30, repo_url = $31, base_branch = $32,
             agent_default_interval_seconds = $33, agent_default_max_ticks = $34,
             agent_review_default_interval_seconds = $35, agent_test_default_interval_seconds = $36,
             agent_ticking_enabled = $37, default_agent_executor = $38,
             members_can_edit_states = $39,
             workspace_id = $40, deleted_at = $41,
             updated_at = $42, updated_by_id = $43, created_by_id = $44
           WHERE id = $45"#,
    )
    .bind(validated.name.as_deref().unwrap_or(&instance.name))
    .bind(
        validated
            .description
            .as_deref()
            .unwrap_or(&instance.description),
    )
    .bind(
        validated
            .description_text
            .clone()
            .unwrap_or(instance.description_text.clone()),
    )
    .bind(
        validated
            .description_html
            .clone()
            .unwrap_or(instance.description_html.clone()),
    )
    .bind(validated.network.unwrap_or(instance.network as i32))
    .bind(identifier)
    .bind(
        validated
            .default_assignee
            .unwrap_or(instance.default_assignee_id),
    )
    .bind(validated.project_lead.unwrap_or(instance.project_lead_id))
    .bind(validated.emoji.clone().unwrap_or(instance.emoji.clone()))
    .bind(
        validated
            .icon_prop
            .clone()
            .unwrap_or(instance.icon_prop.clone()),
    )
    .bind(validated.module_view.unwrap_or(instance.module_view))
    .bind(validated.cycle_view.unwrap_or(instance.cycle_view))
    .bind(
        validated
            .issue_views_view
            .unwrap_or(instance.issue_views_view),
    )
    .bind(validated.page_view.unwrap_or(instance.page_view))
    .bind(validated.intake_view.unwrap_or(instance.intake_view))
    .bind(
        validated
            .is_time_tracking_enabled
            .unwrap_or(instance.is_time_tracking_enabled),
    )
    .bind(
        validated
            .is_issue_type_enabled
            .unwrap_or(instance.is_issue_type_enabled),
    )
    .bind(
        validated
            .guest_view_all_features
            .unwrap_or(instance.guest_view_all_features),
    )
    .bind(
        validated
            .cover_image
            .clone()
            .unwrap_or(instance.cover_image.clone()),
    )
    .bind(
        validated
            .cover_image_asset
            .unwrap_or(instance.cover_image_asset_id),
    )
    .bind(validated.estimate.unwrap_or(instance.estimate_id))
    .bind(archive_in)
    .bind(close_in)
    .bind(
        validated
            .logo_props
            .clone()
            .unwrap_or_else(|| instance.logo_props.clone()),
    )
    .bind(validated.default_state.unwrap_or(instance.default_state_id))
    .bind(validated.archived_at.unwrap_or(instance.archived_at))
    .bind(new_default)
    .bind(validated.timezone.as_deref().unwrap_or(&instance.timezone))
    .bind(
        validated
            .external_source
            .clone()
            .unwrap_or(instance.external_source.clone()),
    )
    .bind(
        validated
            .external_id
            .clone()
            .unwrap_or(instance.external_id.clone()),
    )
    .bind(validated.repo_url.as_deref().unwrap_or(&instance.repo_url))
    .bind(
        validated
            .base_branch
            .as_deref()
            .unwrap_or(&instance.base_branch),
    )
    .bind(agent_interval)
    .bind(agent_max_ticks)
    .bind(agent_review)
    .bind(agent_test)
    .bind(
        validated
            .agent_ticking_enabled
            .unwrap_or(instance.agent_ticking_enabled),
    )
    .bind(
        validated
            .default_agent_executor
            .as_deref()
            .unwrap_or(&instance.default_agent_executor),
    )
    .bind(
        validated
            .members_can_edit_states
            .unwrap_or(instance.members_can_edit_states),
    )
    .bind(validated.workspace.unwrap_or(instance.workspace_id))
    .bind(validated.deleted_at.unwrap_or(None))
    .bind(now)
    .bind(actor_id)
    // `created_by` is writable on both serializers (`update()` persists
    // it; `save()` overwrites only `updated_by` — `models/base.py`).
    .bind(validated.created_by.unwrap_or(instance.created_by_id))
    .bind(instance.id)
    .execute(&mut *tx)
    .await
    .map_err(db_denial)?;
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `partial_update` default-intake ensure (`base.py:355-364`): when the
/// effective `intake_view` is truthy and no default intake exists for the
/// project, create `"<updated name> Intake"` (workspace backfilled from
/// the project, crum stamps `created_by`).
async fn ensure_default_intake(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    project_name: &str,
    actor_id: &uuid::Uuid,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    let existing: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM intakes
           WHERE project_id = $1 AND is_default AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    if existing.is_none() {
        sqlx::query(
            r#"INSERT INTO intakes
                 (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                  project_id, workspace_id, name, description, is_default, view_props, logo_props)
               VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, '', TRUE, '{}', '{}')"#,
        )
        .bind(uuid::Uuid::new_v4())
        .bind(now)
        .bind(actor_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(format!("{project_name} Intake"))
        .execute(pool)
        .await
        .map_err(db_denial)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Scoping (guests see member projects, members add public ones)
// ---------------------------------------------------------------------------

/// The two role probes (`base.py:158-177`), evaluated in order before the
/// main query, exactly as Python evaluates them before appending filters.
async fn scope_flags(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<(bool, bool), Denial> {
    let guest: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2 AND wm.is_active
             AND wm.deleted_at IS NULL AND wm.role = $3"#,
    )
    .bind(user_id)
    .bind(slug)
    .bind(ROLE_GUEST)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    let member: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2 AND wm.is_active
             AND wm.deleted_at IS NULL AND wm.role = $3"#,
    )
    .bind(user_id)
    .bind(slug)
    .bind(ROLE_MEMBER)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    Ok((guest.is_some(), member.is_some()))
}

/// `Workspace.objects.get(slug=slug)` for the bodies that fetch it after
/// the gate (the soft manager excludes deleted rows; a miss answers the
/// `ObjectDoesNotExist` branch).
async fn workspace_or_404(pool: &sqlx::PgPool, slug: &str) -> Result<(uuid::Uuid, String), Denial> {
    let row: Option<(uuid::Uuid, String)> = sqlx::query_as(
        r#"SELECT id, timezone FROM workspaces WHERE slug = $1 AND deleted_at IS NULL"#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    row.ok_or(Denial::ObjectNotFound)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET projects/` (`base.py:145-223`): the compact `.values()` list —
/// 17 concrete columns plus the `member_role` / `intake_count` /
/// `inbox_view` / `sort_order` annotations — rendered by DRF's
/// `JSONEncoder` straight off the UTC-aware DB values (no `localtime` on
/// this path, unlike the serializer fields): datetimes stay UTC with
/// `+00:00` → `Z`.
async fn project_list(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let role = workspace_role(&pool, &actor.id, &slug).await?;
    check_gate("GET", PATH_PROJECTS, &slug, role, None)?;
    // No `Workspace` fetch here (`base.py:145-223` never fetches it —
    // the queryset filters `workspace__slug`).
    let (is_guest, is_member) = scope_flags(&pool, &slug, &actor.id).await?;
    // The `COUNT` annotation turns the queryset into `GROUP BY
    // projects.id`, which drops the `-created_at` Meta ordering: Django
    // emits NO `ORDER BY` here (verified from `str(qs.query)`), so the
    // row order is the aggregate's output order. Reproduce the SQL shape
    // exactly — same joins, same subqueries, no ordering — or multi-row
    // lists diverge whenever the hash order is not newest-first.
    let query = format!(
        r#"SELECT DISTINCT p.id, p.name, p.identifier, p.logo_props, p.archived_at,
              p.workspace_id, p.cycle_view, p.issue_views_view, p.module_view, p.page_view,
              p.is_default, p.guest_view_all_features, p.project_lead_id, p.network,
              p.created_at, p.updated_at, p.created_by_id, p.updated_by_id,
              (SELECT pm.role FROM project_members pm
               WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $2
                 AND pm.project_id = p.id ORDER BY pm.created_at DESC) AS member_role,
              COUNT(ii.id) FILTER (WHERE ii.deleted_at IS NULL AND ii.status = -2) AS intake_count,
              p.intake_view AS inbox_view,
              (SELECT pup.sort_order FROM project_user_properties pup
               WHERE pup.deleted_at IS NULL AND pup.project_id = p.id AND pup.user_id = $2
                 AND pup.workspace_id = p.workspace_id ORDER BY pup.created_at DESC) AS sort_order
           FROM projects p
           INNER JOIN workspaces w ON (p.workspace_id = w.id)
           LEFT OUTER JOIN intake_issues ii ON (p.id = ii.project_id)
           {scope_join}
           WHERE p.deleted_at IS NULL AND w.slug = $1 AND ({scope_where})
           GROUP BY p.id"#,
        scope_join = scope_join(is_guest, is_member),
        scope_where = scope_where(is_guest, is_member),
    );
    let rows = sqlx::query(&query)
        .bind(&slug)
        .bind(actor.id)
        .fetch_all(&pool)
        .await
        .map_err(db_denial)?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(list_row_value(row, actor.timezone)?);
    }
    let body = serde_json::to_string(&Value::Array(items)).map_err(|_| Denial::ServerError)?;
    Ok(json_ok(body))
}

/// The guest/member scoping joins. Django filters on the reverse-FK
/// accessor (`project_projectmember__member=...`), which joins WITHOUT
/// the soft-delete filter — a soft-DELETED membership still grants list
/// visibility. Port the bug: no `deleted_at` check here (verified from
/// `str(qs.query)`; guest `INNER JOIN`, member `LEFT OUTER JOIN`).
fn scope_join(is_guest: bool, is_member: bool) -> &'static str {
    if is_guest {
        "INNER JOIN project_members pm_scope ON (p.id = pm_scope.project_id)"
    } else if is_member {
        "LEFT OUTER JOIN project_members pm_scope ON (p.id = pm_scope.project_id)"
    } else {
        ""
    }
}

/// The scoping `WHERE` fragment matching [`scope_join`]: guests see
/// member projects; members add public (`network = 2`) ones; admins skip
/// both (`base.py:199-222`).
fn scope_where(is_guest: bool, is_member: bool) -> &'static str {
    if is_guest {
        "(pm_scope.is_active AND pm_scope.member_id = $2)"
    } else if is_member {
        "((pm_scope.is_active AND pm_scope.member_id = $2) OR p.network = 2)"
    } else {
        "TRUE"
    }
}

/// One `.values()` row in `LIST_VALUES_COLUMNS` order, rendered like
/// DRF's `JSONEncoder` (`encoders.py`: datetimes `isoformat` + `Z`,
/// UUIDs str, floats shortest, JSON inline).
fn list_row_value(row: &sqlx::postgres::PgRow, timezone: Tz) -> Result<Value, Denial> {
    let get_uuid = |key: &str| -> Result<Option<String>, Denial> {
        let id: Option<uuid::Uuid> = row.try_get(key).map_err(|_| Denial::ServerError)?;
        Ok(id.map(|id| id.to_string()))
    };
    // UTC, not the caller's zone: `JSONEncoder.isoformat()` on the raw
    // `.values()` datetimes (see `project_list` docs). The `timezone`
    // parameter stays for the shared call shape only.
    let _ = timezone;
    let get_dt = |key: &str| -> Result<Option<String>, Denial> {
        let dt: Option<DateTime<Utc>> = row.try_get(key).map_err(|_| Denial::ServerError)?;
        Ok(dt.map(|dt| render_dt(&dt, chrono_tz::UTC)))
    };
    let mut map = Map::with_capacity(22);
    let id: uuid::Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
    map.insert("id".to_owned(), Value::String(id.to_string()));
    let name: String = row.try_get("name").map_err(|_| Denial::ServerError)?;
    map.insert("name".to_owned(), Value::String(name));
    let identifier: String = row.try_get("identifier").map_err(|_| Denial::ServerError)?;
    map.insert("identifier".to_owned(), Value::String(identifier));
    let logo_props: Value = row.try_get("logo_props").map_err(|_| Denial::ServerError)?;
    map.insert("logo_props".to_owned(), logo_props);
    map.insert(
        "archived_at".to_owned(),
        get_dt("archived_at")?
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    let workspace_id: uuid::Uuid = row
        .try_get("workspace_id")
        .map_err(|_| Denial::ServerError)?;
    map.insert(
        "workspace".to_owned(),
        Value::String(workspace_id.to_string()),
    );
    for key in [
        "cycle_view",
        "issue_views_view",
        "module_view",
        "page_view",
        "is_default",
        "guest_view_all_features",
    ] {
        let flag: bool = row.try_get(key).map_err(|_| Denial::ServerError)?;
        map.insert(key.to_owned(), Value::Bool(flag));
    }
    map.insert(
        "project_lead".to_owned(),
        get_uuid("project_lead_id")?
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    let network: i16 = row.try_get("network").map_err(|_| Denial::ServerError)?;
    map.insert(
        "network".to_owned(),
        Value::Number((i64::from(network)).into()),
    );
    map.insert(
        "created_at".to_owned(),
        Value::String(get_dt("created_at")?.ok_or(Denial::ServerError)?),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(get_dt("updated_at")?.ok_or(Denial::ServerError)?),
    );
    map.insert(
        "created_by".to_owned(),
        get_uuid("created_by_id")?
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "updated_by".to_owned(),
        get_uuid("updated_by_id")?
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    let member_role: Option<i16> = row
        .try_get("member_role")
        .map_err(|_| Denial::ServerError)?;
    map.insert(
        "member_role".to_owned(),
        member_role
            .map(|role| Value::Number((i64::from(role)).into()))
            .unwrap_or(Value::Null),
    );
    let intake_count: i64 = row
        .try_get("intake_count")
        .map_err(|_| Denial::ServerError)?;
    map.insert(
        "intake_count".to_owned(),
        Value::Number(intake_count.into()),
    );
    let inbox_view: bool = row.try_get("inbox_view").map_err(|_| Denial::ServerError)?;
    map.insert("inbox_view".to_owned(), Value::Bool(inbox_view));
    let sort_order: Option<f64> = row.try_get("sort_order").map_err(|_| Denial::ServerError)?;
    if sort_order.is_some_and(|float| !float.is_finite()) {
        // DRF `STRICT_JSON` (`allow_nan=False`): a non-finite float
        // 500s at render (`ValueError`) — never `null`. (Reachable via
        // user-views `"nan"` then GET.)
        return Err(Denial::ServerError);
    }
    map.insert(
        "sort_order".to_owned(),
        sort_order
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
    );
    Ok(Value::Object(map))
}

/// `GET projects/details/` (`base.py:101-142`): full-serializer rows in
/// `(sort_order, name)` order — or the `BasePaginator.paginate` envelope
/// when *both* `per_page` and `cursor` are truthy (`:129`). `?fields=`
/// is dead (B-fields-dead): full objects always render.
async fn project_list_detail(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let role = workspace_role(&pool, &actor.id, &slug).await?;
    check_gate("GET", PATH_PROJECTS_DETAILS, &slug, role, None)?;
    // No `Workspace` fetch here (`base.py:101-142` never fetches it —
    // the queryset filters `workspace__slug`, so a soft-deleted
    // workspace with a live membership still answers 200).
    let (is_guest, is_member) = scope_flags(&pool, &slug, &actor.id).await?;
    if query_truthy(&query, "per_page") && query_truthy(&query, "cursor") {
        return paginate_list_detail(&state, &pool, &actor, &slug, &query, is_guest, is_member)
            .await;
    }
    let query_sql = format!(
        "SELECT DISTINCT {cols}, {fav} AS is_favorite, {sort_} AS sort_order, \
         {role} AS member_role, {anchor} AS anchor \
         FROM projects p INNER JOIN workspaces w ON (p.workspace_id = w.id) {scope_join} \
         WHERE p.deleted_at IS NULL AND w.slug = $1 AND ({scope_where}) \
         ORDER BY sort_order, p.name",
        cols = project_columns(),
        fav = favorite_sql(),
        sort_ = sort_order_sql(),
        role = member_role_sql(),
        anchor = anchor_sql("$1"),
        scope_join = scope_join(is_guest, is_member),
        scope_where = scope_where(is_guest, is_member),
    );
    let rows = sqlx::query(&query_sql)
        .bind(&slug)
        .bind(actor.id)
        .fetch_all(&pool)
        .await
        .map_err(db_denial)?;
    let user = executor_user(&pool, &actor.id).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        let project = ProjectRow::from_row(row).map_err(|_| Denial::ServerError)?;
        items.push(render_list_row(&pool, &state, &project, &slug, &actor, &user).await?);
    }
    Ok(json_ok(format!("[{}]", items.join(","))))
}

/// The shared `projects` column list for the multi-row fetches.
fn project_columns() -> &'static str {
    "p.id, p.name, p.description, p.description_text, p.description_html, \
     p.network, p.identifier, p.workspace_id, \
     p.default_assignee_id, p.project_lead_id, p.emoji, p.icon_prop, \
     p.module_view, p.cycle_view, p.issue_views_view, p.page_view, p.intake_view, \
     p.is_time_tracking_enabled, p.is_issue_type_enabled, p.guest_view_all_features, \
     p.cover_image, p.cover_image_asset_id, p.estimate_id, \
     p.archive_in, p.close_in, p.logo_props, p.default_state_id, p.archived_at, \
     p.is_default, p.timezone, p.external_source, p.external_id, \
     p.repo_url, p.base_branch, \
     p.agent_default_interval_seconds, p.agent_default_max_ticks, \
     p.agent_review_default_interval_seconds, p.agent_test_default_interval_seconds, \
     p.agent_ticking_enabled, p.default_agent_executor, p.members_can_edit_states, \
     p.created_at, p.updated_at, p.created_by_id, p.updated_by_id, p.deleted_at"
}

fn favorite_sql() -> &'static str {
    "EXISTS (SELECT 1 FROM user_favorites uf WHERE uf.deleted_at IS NULL \
     AND uf.user_id = $2 AND uf.project_id = p.id AND uf.entity_type = 'project' \
     AND uf.entity_identifier = p.id)"
}

fn sort_order_sql() -> &'static str {
    "(SELECT pup.sort_order FROM project_user_properties pup \
     WHERE pup.deleted_at IS NULL AND pup.project_id = p.id AND pup.user_id = $2 \
     AND pup.workspace_id = p.workspace_id ORDER BY pup.created_at DESC)"
}

fn member_role_sql() -> &'static str {
    "(SELECT pm.role FROM project_members pm WHERE pm.deleted_at IS NULL \
     AND pm.is_active AND pm.member_id = $2 AND pm.project_id = p.id \
     ORDER BY pm.created_at DESC)"
}

/// The `anchor` annotation (`base.py:80-86`): `DeployBoard` rows for
/// `entity_name="project"`, this project id, in the request workspace
/// (slug) — the exact predicate, not the `(project_id, workspace_id)`
/// shorthand. `slug_bind` is the `$N` placeholder carrying the slug.
fn anchor_sql(slug_bind: &str) -> String {
    format!(
        "(SELECT board.anchor FROM deploy_boards board \
         JOIN workspaces bw ON bw.id = board.workspace_id \
         WHERE board.deleted_at IS NULL AND board.entity_name = 'project' \
         AND board.entity_identifier = p.id AND bw.slug = {slug_bind})"
    )
}

/// Map the `paginate` `order_by` key onto a project column or annotation
/// expression (`OffsetPaginator.get_result` re-orders by `(key dir,
/// -created_at)`). Unknown keys raise Django's `FieldError` → 500.
/// `pk` is Django's primary-key alias (`F("pk")` resolves — no key
/// allowlist in `paginator.py`); `workspace__slug` maps to
/// `p.workspace_id`, NOT `w.slug` — the fetch is `SELECT DISTINCT`
/// and `w.slug` is not in the select list, so Postgres rejects
/// `ORDER BY w.slug` (`for SELECT DISTINCT, ORDER BY expressions
/// must appear in select list`). Both are constant under the
/// `w.slug = $1` filter, so ties fall to `-created_at` exactly as in
/// Python (`F("workspace__slug")` 200s — general dunder spans stay
/// 500 here).
fn order_column(key: &str) -> Result<&'static str, Denial> {
    match key {
        "pk" => Ok("p.id"),
        "workspace__slug" => Ok("p.workspace_id"),
        "created_at" => Ok("p.created_at"),
        "updated_at" => Ok("p.updated_at"),
        "archived_at" => Ok("p.archived_at"),
        "deleted_at" => Ok("p.deleted_at"),
        "id" => Ok("p.id"),
        "name" => Ok("p.name"),
        "description" => Ok("p.description"),
        "identifier" => Ok("p.identifier"),
        "network" => Ok("p.network"),
        "workspace" => Ok("p.workspace_id"),
        "default_assignee" => Ok("p.default_assignee_id"),
        "project_lead" => Ok("p.project_lead_id"),
        "emoji" => Ok("p.emoji"),
        "module_view" => Ok("p.module_view"),
        "cycle_view" => Ok("p.cycle_view"),
        "issue_views_view" => Ok("p.issue_views_view"),
        "page_view" => Ok("p.page_view"),
        "intake_view" => Ok("p.intake_view"),
        "is_time_tracking_enabled" => Ok("p.is_time_tracking_enabled"),
        "is_issue_type_enabled" => Ok("p.is_issue_type_enabled"),
        "is_default" => Ok("p.is_default"),
        "guest_view_all_features" => Ok("p.guest_view_all_features"),
        "members_can_edit_states" => Ok("p.members_can_edit_states"),
        "cover_image" => Ok("p.cover_image"),
        "cover_image_asset" => Ok("p.cover_image_asset_id"),
        "estimate" => Ok("p.estimate_id"),
        "archive_in" => Ok("p.archive_in"),
        "close_in" => Ok("p.close_in"),
        "default_state" => Ok("p.default_state_id"),
        "timezone" => Ok("p.timezone"),
        "external_source" => Ok("p.external_source"),
        "external_id" => Ok("p.external_id"),
        "repo_url" => Ok("p.repo_url"),
        "base_branch" => Ok("p.base_branch"),
        "agent_default_interval_seconds" => Ok("p.agent_default_interval_seconds"),
        "agent_default_max_ticks" => Ok("p.agent_default_max_ticks"),
        "agent_review_default_interval_seconds" => Ok("p.agent_review_default_interval_seconds"),
        "agent_test_default_interval_seconds" => Ok("p.agent_test_default_interval_seconds"),
        "agent_ticking_enabled" => Ok("p.agent_ticking_enabled"),
        "default_agent_executor" => Ok("p.default_agent_executor"),
        "created_by" => Ok("p.created_by_id"),
        "updated_by" => Ok("p.updated_by_id"),
        "is_favorite" => Ok("is_favorite"),
        "sort_order" => Ok("sort_order"),
        "member_role" => Ok("member_role"),
        "anchor" => Ok("anchor"),
        _ => Err(Denial::ServerError),
    }
}

/// Pagination branch (`:114-140`): `paginate(order_by=…,
/// request=…, queryset=…, on_results=list-serializer)` through the F-07
/// kernel (the notifications `paginate_list` precedent).
#[allow(clippy::too_many_arguments)]
async fn paginate_list_detail(
    state: &AppState,
    pool: &sqlx::PgPool,
    actor: &Actor,
    slug: &str,
    query: &QueryMap,
    is_guest: bool,
    is_member: bool,
) -> Result<Response, Denial> {
    use crate::paginator::{self, Cursor, PageError, PageResponse};
    let page_denial = |error: PageError| match error {
        PageError::InvalidPerPage
        | PageError::PerPageTooLarge(_)
        | PageError::InvalidCursor
        | PageError::OffsetTooLarge
        | PageError::NegativeOffset => Denial::Raw(
            StatusCode::BAD_REQUEST,
            format!("{{\"detail\":{}}}", json_string(&error.detail())),
        ),
        _ => Denial::ServerError,
    };
    let per_page = paginator::parse_per_page(
        query_last(query, "per_page").as_deref(),
        MAX_PER_PAGE,
        MAX_PER_PAGE,
    )
    .map_err(page_denial)?;
    let cursor = match query_last(query, "cursor") {
        None => Cursor::default_for(per_page),
        Some(raw) => Cursor::from_string(&raw).map_err(page_denial)?,
    };
    let order_raw = query_last(query, "order_by").unwrap_or_else(|| "-created_at".to_owned());
    let (key, descending) = match order_raw.strip_prefix('-') {
        Some(key) => (key, true),
        None => (order_raw.as_str(), false),
    };
    let direction = if descending { "DESC" } else { "ASC" };
    let scope_join = scope_join(is_guest, is_member);
    let scope_where = scope_where(is_guest, is_member);
    let count: (i64,) = sqlx::query_as(&format!(
        "SELECT COUNT(DISTINCT p.id) FROM projects p \
         INNER JOIN workspaces w ON (p.workspace_id = w.id) {scope_join} \
         WHERE p.deleted_at IS NULL AND w.slug = $1 AND ({scope_where})"
    ))
    .bind(slug)
    .bind(actor.id)
    .fetch_one(pool)
    .await
    .map_err(db_denial)?;
    // `limit = min(limit, max_limit)`; the window math follows
    // `OffsetPaginator.get_result` exactly (see the kernel docs).
    let limit = paginator::clamp_limit(per_page, crate::paginator::MAX_LIMIT);
    let window = paginator::offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;
    // `order_by` is lazy in Python: offset errors raise before the
    // `FieldError`, so the key validates only after the window.
    let column = order_column(key)?;
    let fetch = format!(
        "SELECT DISTINCT {cols}, {fav} AS is_favorite, {sort_} AS sort_order, \
         {role} AS member_role, {anchor} AS anchor \
         FROM projects p INNER JOIN workspaces w ON (p.workspace_id = w.id) {scope_join} \
         WHERE p.deleted_at IS NULL AND w.slug = $1 AND ({scope_where}) \
         ORDER BY {column} {direction} NULLS LAST, p.created_at DESC LIMIT $3 OFFSET $4",
        cols = project_columns(),
        fav = favorite_sql(),
        sort_ = sort_order_sql(),
        role = member_role_sql(),
        anchor = anchor_sql("$1"),
        scope_join = scope_join,
        scope_where = scope_where,
        column = column,
        direction = direction,
    );
    let fetched = sqlx::query(&fetch)
        .bind(slug)
        .bind(actor.id)
        .bind(window.stop - window.offset)
        .bind(window.offset)
        .fetch_all(pool)
        .await
        .map_err(db_denial)?;
    let has_more = fetched.len() as i64 > limit;
    let kept: Vec<usize> =
        paginator::apply_offset_window(&(0..fetched.len()).collect::<Vec<_>>(), limit)
            .map_err(page_denial)?;
    let user = executor_user(pool, &actor.id).await?;
    let mut bodies = Vec::with_capacity(kept.len());
    for index in kept {
        let row = &fetched[index];
        let project = ProjectRow::from_row(row).map_err(|_| Denial::ServerError)?;
        let rendered = render_list_row(pool, state, &project, slug, actor, &user).await?;
        bodies.push(serde_json::from_str::<Value>(&rendered).map_err(|_| Denial::ServerError)?);
    }
    let next = paginator::next_cursor(limit, window.page, has_more);
    let prev = paginator::prev_cursor(limit, window.page);
    let page = PageResponse {
        grouped_by: None,
        sub_grouped_by: None,
        total_count: count.0,
        next_cursor: next.to_string(),
        prev_cursor: prev.to_string(),
        next_page_results: next.has_results_or_false(),
        prev_page_results: prev.has_results_or_false(),
        count: bodies.len(),
        total_pages: paginator::max_hits(count.0, limit).map_err(page_denial)?,
        total_results: count.0,
        extra_stats: None,
        results: bodies,
    };
    let body = serde_json::to_string(&page.to_json_value()).map_err(|_| Denial::ServerError)?;
    Ok(json_ok(body))
}

/// `GET projects/<pk>/` (`base.py:226-255`): the `pk` rewrite runs first
/// (non-UUID → identifier lookup, miss → resolve 404); archived or
/// missing rows answer the view 404; non-members of secret projects 403,
/// of public ones 409; then the `recent_visited` enqueue and the
/// full-serializer 200.
async fn project_retrieve(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, pk)): Path<(String, String)>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &pk).await?;
    let role = workspace_role(&pool, &actor.id, &slug).await?;
    check_gate("GET", PATH_PROJECT_DETAIL, &slug, role, None)?;
    // Scoped by the slug (`get_queryset`): a UUID from another
    // workspace is the view 404, never 403/409.
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&format!(
        "SELECT DISTINCT {cols}, {fav} AS is_favorite, {sort_} AS sort_order, \
         {role} AS member_role, {anchor} AS anchor \
         FROM projects p JOIN workspaces w ON w.id = p.workspace_id \
         WHERE p.id = $1 AND p.deleted_at IS NULL AND p.archived_at IS NULL AND w.slug = $3",
        cols = project_columns(),
        fav = favorite_sql(),
        sort_ = sort_order_sql(),
        role = member_role_sql(),
        anchor = anchor_sql("$3"),
    ))
    .bind(project_id)
    .bind(actor.id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    let Some(row) = row else {
        return Err(Denial::Raw(
            StatusCode::NOT_FOUND,
            RETRIEVE_NOT_FOUND_BODY.to_owned(),
        ));
    };
    let project = ProjectRow::from_row(&row).map_err(|_| Denial::ServerError)?;
    let members = fetch_members(&pool, &project_id, &slug).await?;
    // The membership test runs over the raw prefetch (bots included —
    // `str(m.member_id)`), unlike the rendered `members` list.
    let is_member = members
        .iter()
        .any(|member| member.member_id == Some(actor.id));
    if !is_member {
        if project.network == 0 {
            return Err(Denial::Raw(
                StatusCode::FORBIDDEN,
                RETRIEVE_SECRET_BODY.to_owned(),
            ));
        }
        return Err(Denial::Raw(
            StatusCode::CONFLICT,
            RETRIEVE_NONMEMBER_BODY.to_owned(),
        ));
    }
    enqueue_message(&pool, recent_visited_message(&project_id, &actor.id, &slug)).await;
    let user = executor_user(&pool, &actor.id).await?;
    let body = render_list_row(&pool, &state, &project, &slug, &actor, &user).await?;
    Ok(json_ok(body))
}

/// `POST projects/` (`base.py:258-312`): validated create — project row
/// (+ identifier row inside `create()`), creator membership (+ lead
/// membership when set and different), the 8 default states, then the
/// re-fetch, `model_activity` enqueue and 201.
async fn project_create(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let role = workspace_role(&pool, &actor.id, &slug).await?;
    check_gate("POST", PATH_PROJECTS, &slug, role, None)?;
    let (workspace_id, workspace_tz) = workspace_or_404(&pool, &slug).await?;
    let data = parse_get_body(&body)?;
    let mut validated = match validate_project_input(
        &pool,
        &data,
        ValidateMode::Create,
        actor.timezone,
        &workspace_id,
        None,
    )
    .await
    {
        Ok(validated) => validated,
        Err(Ok(errors)) => return Err(Denial::Raw(StatusCode::BAD_REQUEST, errors)),
        Err(Err(denial)) => return Err(denial),
    };
    match run_serializer_validate(&state, &mut validated, None) {
        Ok(()) => {}
        Err(Ok((status, body))) => return Err(Denial::Raw(status, body)),
        Err(Err(denial)) => return Err(denial),
    }
    let now = utc_now_micros();
    let project_id = insert_project(
        &pool,
        &state,
        &validated,
        &workspace_id,
        &workspace_tz,
        &actor.id,
        now,
    )
    .await?;
    let identifier = pidash_db::app_project::models::project::normalize_identifier(
        validated.identifier.as_deref().expect("required on create"),
    );
    insert_project_identifier(&pool, &project_id, &workspace_id, &identifier, now).await?;
    insert_member(&pool, &project_id, &workspace_id, &actor.id, &actor.id, now).await?;
    if let Some(Some(lead)) = validated.project_lead {
        if lead != actor.id {
            insert_member(&pool, &project_id, &workspace_id, &lead, &actor.id, now).await?;
        }
    }
    insert_default_states(&pool, &project_id, &workspace_id, &actor.id, now).await?;
    let project = fetch_project_row(&pool, &project_id, &actor.id, &slug)
        .await?
        .ok_or(Denial::ServerError)?;
    let origin = request_origin(&state)?;
    enqueue_message(
        &pool,
        model_activity_create_message(&project_id, data, &actor.id, &slug, &origin),
    )
    .await;
    let user = executor_user(&pool, &actor.id).await?;
    let body = render_list_row(&pool, &state, &project, &slug, &actor, &user).await?;
    Ok(json_created(body))
}

/// `PUT projects/<pk>/` (`UpdateModelMixin`, no override): DRF default
/// full update through `ProjectListSerializer` — `get_object` 404s
/// missing rows with the default message, and the four required keys
/// (`deleted_at`, `name`, `identifier`, `workspace`) plus the four
/// unique validators decide the outcome (Q-PUT-400).
async fn project_put(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, pk)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &pk).await?;
    check_gate("PUT", PATH_PROJECT_DETAIL, &slug, None, None)?;
    let instance = fetch_project_row_scoped(&pool, &project_id, &actor.id, &slug)
        .await?
        .ok_or(Denial::Raw(
            StatusCode::NOT_FOUND,
            OBJECT_LIST_NOT_FOUND_BODY.to_owned(),
        ))?;
    let data = parse_body(&body)?;
    let validated = match validate_project_input(
        &pool,
        &data,
        ValidateMode::Full,
        actor.timezone,
        &instance.workspace_id,
        Some(&instance),
    )
    .await
    {
        Ok(validated) => validated,
        Err(Ok(errors)) => return Err(Denial::Raw(StatusCode::BAD_REQUEST, errors)),
        Err(Err(denial)) => return Err(denial),
    };
    match run_put_unique_validators(&pool, &validated, &instance).await {
        Ok(()) => {}
        Err(Ok(errors)) => return Err(Denial::Raw(StatusCode::BAD_REQUEST, errors)),
        Err(Err(denial)) => return Err(denial),
    }
    let now = utc_now_micros();
    apply_update(&pool, &validated, &instance, &actor.id, now).await?;
    let project = fetch_project_row_opts(&pool, &project_id, &actor.id, &slug, false, false)
        .await?
        .ok_or(Denial::ServerError)?;
    let user = executor_user(&pool, &actor.id).await?;
    let body = render_list_row(&pool, &state, &project, &slug, &actor, &user).await?;
    Ok(json_ok(body))
}

/// Python truthiness for a JSON body value (`if intake_view:`):
/// null/false/0/`""`/`[]`/`{}` are falsy, everything else truthy.
/// (`"false"` coerces to `False` yet is truthy — the gate runs on the
/// raw value, not the validated bool.)
fn python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `PATCH projects/<pk>/` (`base.py:314-380`): the admin-guard matrix
/// (workspace OR project admin, else the decorator-body 403), the bare
/// gets, the archived 400, the `inbox_view` → `intake_view` alias forced
/// into the data, partial validation, the default-intake ensure, and the
/// re-fetch + `model_activity` 200.
async fn project_patch(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, pk)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &pk).await?;
    check_gate("PATCH", PATH_PROJECT_DETAIL, &slug, None, None)?;
    let membership = membership(&pool, &slug, &project_id, &actor.id).await?;
    let is_admin = membership.workspace_role == Some(ROLE_ADMIN as i16)
        || membership.project_role == Some(ROLE_ADMIN as i16);
    if !is_admin {
        return Err(Denial::Raw(
            StatusCode::FORBIDDEN,
            ADMIN_REQUIRED_BODY.to_owned(),
        ));
    }
    let (workspace_id, _) = workspace_or_404(&pool, &slug).await?;
    // Bare `.get(pk)` — no workspace, no archived filter (a miss is the
    // `ObjectDoesNotExist` branch, not the view 404).
    let instance = fetch_project_row(&pool, &project_id, &actor.id, &slug)
        .await?
        .ok_or(Denial::ObjectNotFound)?;
    // `request.data` is touched (`:341`) before the archived check
    // (`:343`): archived + malformed JSON answers the `ParseError` 400,
    // archived + non-dict JSON answers the `.get` 500.
    let data = parse_get_body(&body)?;
    let user = executor_user(&pool, &actor.id).await?;
    let current_instance =
        render_current_instance(&pool, &state, &project_id, &actor, &user, &slug).await?;
    if instance.archived_at.is_some() {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            ARCHIVED_UPDATE_BODY.to_owned(),
        ));
    }
    // `intake_view = request.data.get("inbox_view", project.intake_view)`,
    // forced over any explicit `intake_view` key (`:348-350`); the
    // intake gate below tests this RAW value's truthiness (`:358`).
    let intake_raw = data
        .get("inbox_view")
        .cloned()
        .unwrap_or(Value::Bool(instance.intake_view));
    let mut aliased = data.clone();
    aliased.insert("intake_view".to_owned(), intake_raw.clone());
    let mut validated = match validate_project_input(
        &pool,
        &aliased,
        ValidateMode::Partial,
        actor.timezone,
        &workspace_id,
        Some(&instance),
    )
    .await
    {
        Ok(validated) => validated,
        Err(Ok(errors)) => return Err(Denial::Raw(StatusCode::BAD_REQUEST, errors)),
        Err(Err(denial)) => return Err(denial),
    };
    match run_serializer_validate(&state, &mut validated, Some(&instance)) {
        Ok(()) => {}
        Err(Ok((status, body))) => return Err(Denial::Raw(status, body)),
        Err(Err(denial)) => return Err(denial),
    }
    let now = utc_now_micros();
    apply_update(&pool, &validated, &instance, &actor.id, now).await?;
    // Python order (`base.py:356-367`): the intake ensure runs BEFORE the
    // scoped re-fetch, using the post-save in-memory name (DRF `update()`
    // setattr's the instance before `save()`, so that is the validated
    // name when provided, else the stored one — `workspace` is read-only
    // so the workspace id is unchanged too). On the cross-workspace 500
    // path Python still creates the intake row before `str(None.id)`.
    if python_truthy(&intake_raw) {
        let saved_name = validated.name.as_deref().unwrap_or(&instance.name);
        ensure_default_intake(
            &pool,
            &project_id,
            &instance.workspace_id,
            saved_name,
            &actor.id,
            now,
        )
        .await?;
    }
    // The re-fetch uses the scoped queryset, so a miss is the 500
    // `AttributeError` branch (`str(project.id)` on `None`).
    let updated = fetch_project_row_scoped(&pool, &project_id, &actor.id, &slug)
        .await?
        .ok_or(Denial::ServerError)?;
    let origin = request_origin(&state)?;
    enqueue_message(
        &pool,
        model_activity_update_message(
            &project_id,
            data,
            current_instance,
            &actor.id,
            &slug,
            &origin,
        ),
    )
    .await;
    let body = render_list_row(&pool, &state, &updated, &slug, &actor, &user).await?;
    Ok(json_ok(body))
}

/// `DELETE projects/<pk>/` (`base.py:382-429`): the admin matrix, the
/// bare get, the default 400, then soft delete (+ its emit),
/// `webhook_activity`, and the deploy-board/favorite cascades — 204.
async fn project_destroy(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, pk)): Path<(String, String)>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &pk).await?;
    check_gate("DELETE", PATH_PROJECT_DETAIL, &slug, None, None)?;
    let membership = membership(&pool, &slug, &project_id, &actor.id).await?;
    let is_admin = membership.workspace_role == Some(ROLE_ADMIN as i16)
        || membership.project_role == Some(ROLE_ADMIN as i16);
    if !is_admin {
        return Err(Denial::Raw(
            StatusCode::FORBIDDEN,
            ADMIN_REQUIRED_BODY.to_owned(),
        ));
    }
    let instance = fetch_project_row_scoped(&pool, &project_id, &actor.id, &slug)
        .await?
        .ok_or(Denial::ObjectNotFound)?;
    if instance.is_default {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            DEFAULT_DELETE_BODY.to_owned(),
        ));
    }
    let now = utc_now_micros();
    // `project.delete()`: stamps + its `soft_delete_related_objects` emit.
    sqlx::query(
        r#"UPDATE projects SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3"#,
    )
    .bind(now)
    .bind(actor.id)
    .bind(project_id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    enqueue_message(&pool, soft_delete_message(&project_id)).await;
    let origin = request_origin(&state)?;
    enqueue_message(
        &pool,
        webhook_destroy_message(&project_id, &actor.id, &slug, &origin),
    )
    .await;
    // The cascades scope by `(project_id, workspace-slug)`; the joined
    // workspace rows carry no soft-delete guard (bug-9 pattern).
    sqlx::query(
        r#"UPDATE deploy_boards SET deleted_at = $1
           WHERE project_id = $2 AND deleted_at IS NULL
             AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $3)"#,
    )
    .bind(now)
    .bind(project_id)
    .bind(&slug)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    sqlx::query(
        r#"UPDATE user_favorites SET deleted_at = $1
           WHERE project_id = $2 AND deleted_at IS NULL
             AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $3)"#,
    )
    .bind(now)
    .bind(project_id)
    .bind(&slug)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    Ok(no_content())
}

/// `POST projects/<project_id>/archive/`
/// (`ProjectArchiveUnarchiveEndpoint.post`, `base.py:435-442`): stamp
/// `archived_at`, soft-delete the workspace's favorites for the project
/// (any entity type — the filter has none), and answer the Python-`str`
/// timestamp.
async fn archive_post(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, project_id)): Path<(String, String)>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_id).await?;
    let membership = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_gate(
        "POST",
        PATH_ARCHIVE,
        &slug,
        membership.workspace_role,
        membership.project_role,
    )?;
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    row.ok_or(Denial::ObjectNotFound)?;
    let now = utc_now_micros();
    sqlx::query(
        r#"UPDATE projects SET archived_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3"#,
    )
    .bind(now)
    .bind(actor.id)
    .bind(project_id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    sqlx::query(
        r#"UPDATE user_favorites SET deleted_at = $1
           WHERE project_id = $2 AND deleted_at IS NULL
             AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $3)"#,
    )
    .bind(now)
    .bind(project_id)
    .bind(&slug)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    Ok(json_ok(format!(
        "{{\"archived_at\":{}}}",
        json_string(&render_python_datetime(&now))
    )))
}

/// `DELETE projects/<project_id>/archive/`
/// (`ProjectArchiveUnarchiveEndpoint.delete`, `base.py:444-446`): clear
/// `archived_at` — 204.
async fn unarchive_delete(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, project_id)): Path<(String, String)>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_id).await?;
    let membership = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_gate(
        "DELETE",
        PATH_ARCHIVE,
        &slug,
        membership.workspace_role,
        membership.project_role,
    )?;
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    row.ok_or(Denial::ObjectNotFound)?;
    let now = utc_now_micros();
    sqlx::query(
        r#"UPDATE projects SET archived_at = NULL, updated_at = $1, updated_by_id = $2 WHERE id = $3"#,
    )
    .bind(now)
    .bind(actor.id)
    .bind(project_id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    Ok(no_content())
}

/// `GET project-identifiers/` (`ProjectIdentifierEndpoint.get`,
/// `base.py:450-460`): the upper-cased `?name=` (blank → 400) looks up
/// live identifier rows in the workspace; answers `exists` + the
/// `(id, name, project)` triples in newest-first order.
async fn identifiers_get(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let role = workspace_role(&pool, &actor.id, &slug).await?;
    check_gate("GET", PATH_IDENTIFIERS, &slug, role, None)?;
    let name = query_last(&query, "name").unwrap_or_default();
    let name = strip_py(&name).to_uppercase();
    if name.is_empty() {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            IDENTIFIER_NAME_REQUIRED_BODY.to_owned(),
        ));
    }
    let rows: Vec<(i64, String, uuid::Uuid)> = sqlx::query_as(
        r#"SELECT pi.id, pi.name, pi.project_id FROM project_identifiers pi
           JOIN workspaces w ON w.id = pi.workspace_id
           WHERE pi.name = $1 AND w.slug = $2 AND pi.deleted_at IS NULL
           ORDER BY pi.created_at DESC"#,
    )
    .bind(&name)
    .bind(&slug)
    .fetch_all(&pool)
    .await
    .map_err(db_denial)?;
    let mut identifiers = Vec::with_capacity(rows.len());
    for (id, name, project) in &rows {
        let mut item = Map::with_capacity(3);
        item.insert("id".to_owned(), Value::Number((*id).into()));
        item.insert("name".to_owned(), Value::String(name.clone()));
        item.insert("project".to_owned(), Value::String(project.to_string()));
        identifiers.push(Value::Object(item));
    }
    let mut body = Map::with_capacity(2);
    body.insert(
        "exists".to_owned(),
        Value::Number((rows.len() as i64).into()),
    );
    body.insert("identifiers".to_owned(), Value::Array(identifiers));
    let rendered = serde_json::to_string(&Value::Object(body)).map_err(|_| Denial::ServerError)?;
    Ok(json_ok(rendered))
}

/// `DELETE project-identifiers/` (`ProjectIdentifierEndpoint.delete`,
/// `base.py:462-476`): the upper-cased body `name` (blank → 400,
/// non-string → 500 — Q-identifier-delete-unvalidated) must not belong
/// to a live project (400), then its rows soft-delete — 204.
async fn identifiers_delete(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let role = workspace_role(&pool, &actor.id, &slug).await?;
    check_gate("DELETE", PATH_IDENTIFIERS, &slug, role, None)?;
    let data = parse_get_body(&body)?;
    let name = match data.get("name") {
        None => String::new(),
        Some(Value::String(text)) => strip_py(text).to_uppercase(),
        // `.strip()` on a non-string → `AttributeError` → 500.
        Some(_) => return Err(Denial::ServerError),
    };
    if name.is_empty() {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            IDENTIFIER_NAME_REQUIRED_BODY.to_owned(),
        ));
    }
    let live: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.identifier = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(&name)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    if live.is_some() {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            IDENTIFIER_LIVE_BODY.to_owned(),
        ));
    }
    let now = utc_now_micros();
    sqlx::query(
        r#"UPDATE project_identifiers SET deleted_at = $1
           WHERE name = $2 AND deleted_at IS NULL
             AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $3)"#,
    )
    .bind(now)
    .bind(&name)
    .bind(&slug)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    Ok(no_content())
}

/// `POST projects/<project_id>/project-views/`
/// (`ProjectUserViewsEndpoint.post`, `base.py:479-500`): the caller's own
/// membership row (non-members 403) takes the four view keys (`sort_order`
/// through `float()`, so garbage 500s) — 204.
async fn user_views_post(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, project_id)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Response, Denial> {
    let actor = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let project_id = resolve_project_id(&pool, &slug, &project_id).await?;
    check_gate("POST", PATH_PROJECT_VIEWS, &slug, None, None)?;
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    row.ok_or(Denial::ObjectNotFound)?;
    let member: Option<(uuid::Uuid, Value, Value, Value, f64)> = sqlx::query_as(
        r#"SELECT id, view_props, default_props, preferences, sort_order FROM project_members
           WHERE member_id = $1 AND project_id = $2 AND is_active AND deleted_at IS NULL"#,
    )
    .bind(actor.id)
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    let Some((member_id, view_props, default_props, preferences, sort_order)) = member else {
        return Err(Denial::Raw(
            StatusCode::FORBIDDEN,
            USER_VIEWS_FORBIDDEN_BODY.to_owned(),
        ));
    };
    let data = parse_get_body(&body)?;
    // Explicit null writes NULL into the `NOT NULL` JSON columns → the
    // `IntegrityError` 400 (`save()` assigns before any validation).
    for key in ["view_props", "default_props", "preferences"] {
        if data.get(key).is_some_and(Value::is_null) {
            return Err(Denial::IntegrityFailed);
        }
    }
    let view_props = data.get("view_props").cloned().unwrap_or(view_props);
    let default_props = data.get("default_props").cloned().unwrap_or(default_props);
    let preferences = data.get("preferences").cloned().unwrap_or(preferences);
    let sort_order = match data.get("sort_order") {
        None => sort_order,
        // Explicit null → NULL into the `NOT NULL` float column → the
        // `IntegrityError` 400 (`get_prep_value(None)` is `None`).
        Some(Value::Null) => return Err(Denial::IntegrityFailed),
        // `float(value)` on save: bools/numbers pass, strings parse
        // (Python `float()` spellings), anything else raises → 500.
        Some(value) => python_float(value).ok_or(Denial::ServerError)?,
    };
    let now = utc_now_micros();
    sqlx::query(
        r#"UPDATE project_members SET view_props = $1, default_props = $2, preferences = $3,
             sort_order = $4, updated_at = $5, updated_by_id = $6 WHERE id = $7"#,
    )
    .bind(view_props)
    .bind(default_props)
    .bind(preferences)
    .bind(sort_order)
    .bind(now)
    .bind(actor.id)
    .bind(member_id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    Ok(no_content())
}

/// Python `float(value)` for the `sort_order` write: bools → 1.0/0.0,
/// numbers verbatim, strings stripped (exactly Rust `White_Space` —
/// U+001C-U+001F raise) and parsed, everything else `None` (→ 500,
/// like the `TypeError`/`ValueError` from `get_prep_value`). Digits
/// are Unicode decimal ([`nd_value`]); the sign, `.`, and exponent
/// marker are ASCII-only; `_` is allowed only between two digits
/// (`"1_"`, `"_1"`, `"1__0"`, `"1_.0"`, `"1e_1"` all raise — probed).
/// `inf`/`infinity`/`nan` spellings are ASCII case-insensitive with an
/// optional ASCII sign.
fn python_float(value: &Value) -> Option<f64> {
    match value {
        Value::Null | Value::Array(_) | Value::Object(_) => None,
        Value::Bool(true) => Some(1.0),
        Value::Bool(false) => Some(0.0),
        Value::Number(number) => number.as_f64(),
        Value::String(text) => {
            let text = text.trim_matches(|ch: char| ch.is_whitespace());
            if text.is_empty() {
                return None;
            }
            // Every `_` must join two digits (verified before removal,
            // so `inf_inity` fails instead of parsing as `infinity`).
            if text.contains('_') {
                let chars: Vec<char> = text.chars().collect();
                for (index, ch) in chars.iter().enumerate() {
                    if *ch == '_' {
                        let prev_ok = index > 0 && nd_value(chars[index - 1]).is_some();
                        let next_ok =
                            index + 1 < chars.len() && nd_value(chars[index + 1]).is_some();
                        if !(prev_ok && next_ok) {
                            return None;
                        }
                    }
                }
                let clean: String = text.chars().filter(|ch| *ch != '_').collect();
                return parse_float_spelling(&clean);
            }
            parse_float_spelling(text)
        }
    }
}

/// One stripped, underscore-free `float()` spelling: the `inf`/`nan`
/// words, or the float grammar over Unicode decimal digits (mapped to
/// ASCII before parsing — `f64::from_str` is ASCII-only).
fn parse_float_spelling(text: &str) -> Option<f64> {
    let body = text.strip_prefix(['+', '-']).unwrap_or(text);
    if matches!(
        body.to_ascii_lowercase().as_str(),
        "inf" | "infinity" | "nan"
    ) {
        return text.parse::<f64>().ok();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    if matches!(chars.peek(), Some('+') | Some('-')) {
        out.push(chars.next().expect("peeked sign"));
    }
    let mut int_digits = 0;
    while let Some(&ch) = chars.peek() {
        if let Some(digit) = nd_value(ch) {
            out.push(char::from_digit(digit, 10).expect("decimal digit"));
            chars.next();
            int_digits += 1;
        } else {
            break;
        }
    }
    let mut frac_digits = 0;
    if chars.peek() == Some(&'.') {
        out.push('.');
        chars.next();
        while let Some(&ch) = chars.peek() {
            if let Some(digit) = nd_value(ch) {
                out.push(char::from_digit(digit, 10).expect("decimal digit"));
                chars.next();
                frac_digits += 1;
            } else {
                break;
            }
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return None;
    }
    if matches!(chars.peek(), Some('e') | Some('E')) {
        out.push(chars.next().expect("peeked exponent"));
        if matches!(chars.peek(), Some('+') | Some('-')) {
            out.push(chars.next().expect("peeked exponent sign"));
        }
        let mut exp_digits = 0;
        while let Some(&ch) = chars.peek() {
            if let Some(digit) = nd_value(ch) {
                out.push(char::from_digit(digit, 10).expect("decimal digit"));
                chars.next();
                exp_digits += 1;
            } else {
                break;
            }
        }
        if exp_digits == 0 {
            return None;
        }
    }
    if chars.next().is_some() {
        return None;
    }
    out.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Blank bodies reproduce CPython's `Expecting value` line/column
    /// (live-probed against Django, incl. multi-line blanks).
    #[test]
    fn blank_body_reason_matches_cpython() {
        assert_eq!(blank_json_reason(b""), None);
        assert_eq!(
            blank_json_reason(b"  ").as_deref(),
            Some("JSON parse error - Expecting value: line 1 column 3 (char 2)")
        );
        assert_eq!(
            blank_json_reason(b"\n\t ").as_deref(),
            Some("JSON parse error - Expecting value: line 2 column 3 (char 3)")
        );
        // `\x0b` is not JSON whitespace: not a blank.
        assert_eq!(blank_json_reason(b"\x0b"), None);
        assert_eq!(blank_json_reason(b"{oops"), None);
    }

    /// Every owned method+path row resolves in the gate table, and the
    /// `PATH_*` consts stay in sync with it.
    #[test]
    fn gate_rows_resolve() {
        use super::super::gates::{gate_for, Gate, GATES};
        for path in [
            PATH_PROJECTS,
            PATH_PROJECTS_DETAILS,
            PATH_PROJECT_DETAIL,
            PATH_IDENTIFIERS,
            PATH_PROJECT_VIEWS,
            PATH_ARCHIVE,
        ] {
            assert!(
                GATES.iter().any(|row| row.path == path),
                "gate table covers {path}"
            );
        }
        for (method, path) in [
            ("GET", PATH_PROJECTS),
            ("POST", PATH_PROJECTS),
            ("GET", PATH_PROJECTS_DETAILS),
            ("GET", PATH_PROJECT_DETAIL),
            ("PUT", PATH_PROJECT_DETAIL),
            ("PATCH", PATH_PROJECT_DETAIL),
            ("DELETE", PATH_PROJECT_DETAIL),
            ("GET", PATH_IDENTIFIERS),
            ("DELETE", PATH_IDENTIFIERS),
            ("POST", PATH_PROJECT_VIEWS),
            ("POST", PATH_ARCHIVE),
            ("DELETE", PATH_ARCHIVE),
        ] {
            assert!(gate_for(method, path).is_some(), "{method} {path}");
        }
        // Spot-check the gate kinds: collection rows are workspace-level,
        // archive is project-level, PUT/PATCH/DELETE/user-views are
        // auth-only at the gate.
        assert!(matches!(
            gate_for("GET", PATH_PROJECTS).unwrap().gate,
            Gate::Workspace { .. }
        ));
        assert!(matches!(
            gate_for("POST", PATH_ARCHIVE).unwrap().gate,
            Gate::Project { .. }
        ));
        assert!(matches!(
            gate_for("PUT", PATH_PROJECT_DETAIL).unwrap().gate,
            Gate::Authenticated
        ));
        assert!(matches!(
            gate_for("POST", PATH_PROJECT_VIEWS).unwrap().gate,
            Gate::Authenticated
        ));
    }

    /// The denial bytes: lowercase compact `detail` (post-#969/#997),
    /// exact inline `error` bodies.
    #[test]
    fn denial_bodies_are_exact() {
        // Byte-verify the resolve 404 (this const exists because the L6
        // sibling still renders the stale capital-`D` spaced form).
        assert_eq!(PROJECT_NOT_FOUND_BODY, "{\"detail\":\"Project not found\"}");
        assert_eq!(PROJECT_NOT_FOUND_BODY.as_bytes()[2], b'd');
        assert_eq!(
            OBJECT_LIST_NOT_FOUND_BODY,
            "{\"detail\":\"No Project matches the given query.\"}"
        );
        assert_eq!(
            RETRIEVE_NOT_FOUND_BODY,
            "{\"error\":\"Project does not exist\"}"
        );
        assert_eq!(
            RETRIEVE_NONMEMBER_BODY,
            "{\"error\":\"You are not a member of this project\"}"
        );
        assert_eq!(
            RETRIEVE_SECRET_BODY,
            "{\"error\":\"You do not have permission\"}"
        );
        assert_eq!(
            ADMIN_REQUIRED_BODY,
            "{\"error\":\"You don't have the required permissions.\"}"
        );
        assert_eq!(
            ARCHIVED_UPDATE_BODY,
            "{\"error\":\"Archived projects cannot be updated\"}"
        );
        assert_eq!(
            DEFAULT_DELETE_BODY,
            "{\"error\":\"Default project cannot be deleted\"}"
        );
        assert_eq!(
            IDENTIFIER_NAME_REQUIRED_BODY,
            "{\"error\":\"Name is required\"}"
        );
        assert_eq!(
            IDENTIFIER_LIVE_BODY,
            "{\"error\":\"Cannot delete an identifier of an existing project\"}"
        );
        assert_eq!(USER_VIEWS_FORBIDDEN_BODY, "{\"error\":\"Forbidden\"}");
        let (status, _) = Denial::ProjectNotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, body) = Denial::ObjectNotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, "{\"error\":\"The required object does not exist.\"}");
        let (status, body) = Denial::ValidationFailed.status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "{\"error\":\"Please provide valid detail\"}");
        let (status, body) = Denial::IntegrityFailed.status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "{\"error\":\"The payload is not valid\"}");
    }

    #[test]
    fn query_helpers_match_querydict() {
        let mut query = QueryMap::new();
        assert_eq!(query_last(&query, "per_page"), None);
        assert!(!query_truthy(&query, "per_page"));
        query.insert("per_page".to_owned(), OneOrMany::One(String::new()));
        assert!(!query_truthy(&query, "per_page"));
        query.insert(
            "per_page".to_owned(),
            OneOrMany::Many(vec!["10".to_owned(), "25".to_owned()]),
        );
        assert_eq!(query_last(&query, "per_page").as_deref(), Some("25"));
        assert!(query_truthy(&query, "per_page"));
    }

    #[test]
    fn python_str_matches_cpython() {
        assert_eq!(python_str(&json!("abc")), "abc");
        assert_eq!(python_str(&Value::Null), "None");
        assert_eq!(python_str(&json!(true)), "True");
        assert_eq!(python_str(&json!(12)), "12");
        assert_eq!(python_str(&json!({"a": 1})), "{'a': 1}");
        assert_eq!(python_str(&json!([1, "x", Value::Null])), "[1, 'x', None]");
        assert_eq!(python_repr_string("it's"), "\"it's\"");
        assert_eq!(python_repr_string("say \"hi\""), "'say \"hi\"'");
    }

    #[test]
    fn cpython_dumps_matches_defaults() {
        let value = json!({"b": 1, "a": [true, Value::Null, "x"]});
        assert_eq!(
            cpython_dumps(&value),
            "{\"b\": 1, \"a\": [true, null, \"x\"]}"
        );
        // `ensure_ascii`: lowercase `\uXXXX`, surrogate pairs above BMP.
        assert_eq!(cpython_dumps(&json!("caf\u{e9}")), "\"caf\\u00e9\"");
        assert_eq!(cpython_dumps(&json!("\u{1f600}")), "\"\\ud83d\\ude00\"");
        assert_eq!(
            cpython_dumps(&json!("a\nb\t\"q\"")),
            "\"a\\nb\\t\\\"q\\\"\""
        );
        assert_eq!(cpython_dumps(&json!("\u{1}")), "\"\\u0001\"");
    }

    #[test]
    fn char_validation_matches_drf() {
        let null = || vec!["This field may not be null.".to_owned()];
        let blank = || vec!["This field may not be blank.".to_owned()];
        assert_eq!(validate_char(&Value::Null, None, true), Err(null()));
        assert_eq!(validate_char(&json!(""), None, false), Err(blank()));
        assert_eq!(validate_char(&json!(""), None, true), Ok(String::new()));
        assert_eq!(validate_char(&json!("   "), None, false), Err(blank()));
        assert_eq!(
            validate_char(&json!(true), None, true),
            Err(vec!["Not a valid string.".to_owned()])
        );
        assert_eq!(validate_char(&json!(12), None, true), Ok("12".to_owned()));
        assert_eq!(
            validate_char(&json!("abcdef"), Some(5), true),
            Err(vec![
                "Ensure this field has no more than 5 characters.".to_owned()
            ])
        );
        assert_eq!(opt_char(&Value::Null, Some(255), true), Ok(None));
        // The stored value is STRIPPED, and `max_length` runs on the
        // stripped value.
        assert_eq!(
            validate_char(&json!("  padded  "), None, true),
            Ok("padded".to_owned())
        );
        assert_eq!(
            validate_char(&json!("  ab  "), Some(3), true),
            Ok("ab".to_owned())
        );
        // U+001C strips (Python `str.strip`, not Rust `trim`).
        assert_eq!(
            validate_char(&json!("\u{1c}ab\u{1c}"), None, true),
            Ok("ab".to_owned())
        );
        // NUL content 400s, collecting after `max_length`.
        assert_eq!(
            validate_char(&json!("a\0b"), None, true),
            Err(vec![NULL_CHARACTERS_MESSAGE.to_owned()])
        );
        assert_eq!(
            validate_char(&json!("abcdef\0"), Some(5), true),
            Err(vec![
                "Ensure this field has no more than 5 characters.".to_owned(),
                NULL_CHARACTERS_MESSAGE.to_owned(),
            ])
        );
    }

    #[test]
    fn bool_validation_matches_drf_sets() {
        assert_eq!(validate_bool(&json!(true)), Ok(true));
        assert_eq!(validate_bool(&json!("YES")), Ok(true));
        assert_eq!(validate_bool(&json!(1)), Ok(true));
        assert_eq!(validate_bool(&json!(1.0)), Ok(true));
        assert_eq!(validate_bool(&json!(0)), Ok(false));
        assert_eq!(validate_bool(&json!(0.0)), Ok(false));
        assert_eq!(validate_bool(&json!(-0.0)), Ok(false));
        for bad in [json!(2), json!(2.0), json!(0.5), json!(" true")] {
            assert_eq!(
                validate_bool(&bad),
                Err("Must be a valid boolean.".to_owned())
            );
        }
        assert_eq!(
            validate_bool(&json!("yes please")),
            Err("Must be a valid boolean.".to_owned())
        );
        assert_eq!(
            validate_bool(&json!({"a": 1})),
            Err("Must be a valid boolean.".to_owned())
        );
    }

    #[test]
    fn int_validation_matches_drf() {
        let invalid = || IntError::Invalid("A valid integer is required.".to_owned());
        assert_eq!(validate_int(&json!(12)), Ok(12));
        assert_eq!(validate_int(&json!("12")), Ok(12));
        assert_eq!(validate_int(&json!(1.0)), Ok(1));
        assert_eq!(validate_int(&json!(1.5)), Err(invalid()));
        assert_eq!(validate_int(&json!(true)), Err(invalid()));
        // `str(float)` spellings: `1e3` → `1000.0` → `1000`.
        assert_eq!(validate_int(&json!(1000.0)), Ok(1000));
        assert_eq!(validate_int(&json!(1e16)), Err(invalid()));
        // `int()` spellings: padding, trailing dot, underscores,
        // Unicode decimal digits.
        assert_eq!(validate_int(&json!(" 5")), Ok(5));
        assert_eq!(validate_int(&json!("5.")), Ok(5));
        assert_eq!(validate_int(&json!("1_0")), Ok(10));
        assert_eq!(validate_int(&json!("５")), Ok(5));
        assert_eq!(validate_int(&json!("１２３")), Ok(123));
        for bad in ["1_", "_1", "1__0", "0x1", "1e3", "１_２_"] {
            assert_eq!(validate_int(&json!(bad)), Err(invalid()), "{bad:?}");
        }
        // The 1000-char limit counts characters, not bytes.
        assert_eq!(
            validate_int(&json!("é".repeat(1001))),
            Err(IntError::Invalid("String value too large.".to_owned()))
        );
        assert_eq!(validate_int(&json!("é".repeat(1000))), Err(invalid()));
        // Valid spellings outside `i64` carry their sign (the
        // min/max range check turns them into the bound 400).
        assert_eq!(
            validate_int(&json!("9".repeat(50))),
            Err(IntError::TooLarge { negative: false })
        );
        assert_eq!(
            validate_int(&json!(format!("-{}", "9".repeat(50)))),
            Err(IntError::TooLarge { negative: true })
        );
        assert_eq!(
            validate_int(&json!(u64::MAX)),
            Err(IntError::TooLarge { negative: false })
        );
        // Range checks: the live-probed min/max 400s.
        assert_eq!(
            check_int_range(validate_int(&json!(2i64.pow(40))), -2147483648, 2147483647),
            Err("Ensure this value is less than or equal to 2147483647.".to_owned())
        );
        assert_eq!(
            check_int_range(validate_int(&json!(-2i64.pow(40))), -2147483648, 2147483647),
            Err("Ensure this value is greater than or equal to -2147483648.".to_owned())
        );
        assert_eq!(
            check_int_range(validate_int(&json!("9".repeat(50))), 0, 12),
            Err("Ensure this value is less than or equal to 12.".to_owned())
        );
        assert_eq!(check_int_range(validate_int(&json!(12)), 0, 12), Ok(12));
        assert_eq!(validate_int(&json!(1e19)), Err(invalid()));
        assert_eq!(validate_int(&json!(-1e19)), Err(invalid()));
        // `re_decimal`: trailing `.0*` plus `re`-whitespace; no match
        // returns the original (so `int()` still rejects `\u{1c}`).
        assert_eq!(strip_re_decimal("1.000"), "1");
        assert_eq!(strip_re_decimal("1.5"), "1.5");
        assert_eq!(strip_re_decimal("100"), "100");
        assert_eq!(strip_re_decimal("5."), "5");
        assert_eq!(strip_re_decimal("5. "), "5");
        assert_eq!(strip_re_decimal("5.0\u{a0}"), "5");
        assert_eq!(strip_re_decimal("5.0\u{1c}"), "5");
        assert_eq!(strip_re_decimal("50\u{1c}"), "50\u{1c}");
        assert_eq!(strip_re_decimal("5.0.0"), "5.0");
        assert_eq!(validate_int(&json!("5\u{1c}")), Err(invalid()));
    }

    #[test]
    fn choice_and_network_coerce_like_drf() {
        assert_eq!(
            validate_str_choice(&json!("UTC"), TIMEZONE_CHOICES),
            Ok("UTC".to_owned())
        );
        assert_eq!(
            validate_str_choice(&json!("Mars/Olympus"), TIMEZONE_CHOICES),
            Err("\"Mars/Olympus\" is not a valid choice.".to_owned())
        );
        assert_eq!(validate_network(&json!(0)), Ok(0));
        assert_eq!(validate_network(&json!("2")), Ok(2));
        assert_eq!(
            validate_network(&json!(true)),
            Err("\"True\" is not a valid choice.".to_owned())
        );
        assert_eq!(
            validate_network(&json!(2.0)),
            Err("\"2.0\" is not a valid choice.".to_owned())
        );
        // `pytz.common_timezones`, pinned count.
        assert_eq!(TIMEZONE_CHOICES.len(), 433);
        assert!(TIMEZONE_CHOICES.contains(&"UTC"));
    }

    #[test]
    fn datetime_parsing_matches_drf_iso8601() {
        let utc = chrono_tz::UTC;
        let parsed = parse_drf_datetime("2026-10-02T22:22:38.379224Z", utc)
            .ok()
            .unwrap();
        assert_eq!(parsed.to_string(), "2026-10-02 22:22:38.379224 UTC");
        let parsed = parse_drf_datetime("2026-10-02T22:22:38+00:00", utc)
            .ok()
            .unwrap();
        assert_eq!(parsed.timestamp(), 1790979758);
        // Naive reads in the request user's zone.
        let eastern: Tz = "America/New_York".parse().unwrap();
        let parsed = parse_drf_datetime("2026-10-02 22:22:38", eastern)
            .ok()
            .unwrap();
        assert_eq!(parsed.to_string(), "2026-10-03 02:22:38 UTC");
        // Date-only: midnight, naive → user zone.
        let parsed = parse_drf_datetime("2026-10-02", eastern).ok().unwrap();
        assert_eq!(parsed.to_string(), "2026-10-02 04:00:00 UTC");
        // Lowercase `z` rejected; lowercase `t` accepted.
        assert!(parse_drf_datetime("2026-10-02T22:22:38z", utc)
            .ok()
            .is_none());
        assert!(parse_drf_datetime("2026-10-02t22:22:38", utc)
            .ok()
            .is_some());
        // Leading whitespace rejected; trailing (no tz) accepted.
        assert!(parse_drf_datetime(" 2026-10-02T22:22:38", utc)
            .ok()
            .is_none());
        assert!(parse_drf_datetime("2026-10-02T22:22:38 ", utc)
            .ok()
            .is_some());
        assert!(parse_drf_datetime("2026-10-02T22:22:38+00:00 ", utc)
            .ok()
            .is_none());
        // `re` `$` matches before one trailing newline (regex path
        // only): `…Z\n` / `…+00:00\n` valid, double newline not.
        assert!(parse_drf_datetime("2026-10-02T22:22:38Z\n", utc)
            .ok()
            .is_some());
        assert!(parse_drf_datetime("2026-10-02T22:22:38+00:00\n", utc)
            .ok()
            .is_some());
        assert!(parse_drf_datetime("2026-10-02T22:22:38Z\n\n", utc)
            .ok()
            .is_none());
        assert!(parse_drf_datetime("2026-10-02T22:22:38+00:00\r", utc)
            .ok()
            .is_none());
        // Fractions truncate to micros (never round).
        let parsed = parse_drf_datetime("2026-10-02T22:22:38.123456789Z", utc)
            .ok()
            .unwrap();
        assert_eq!(parsed.to_string(), "2026-10-02 22:22:38.123456 UTC");
        // Offset shapes: `+HHMM`, `+HH`, seconds, space separator
        // without seconds, comma fractions, single-digit fields.
        assert_eq!(
            parse_drf_datetime("2026-10-02T22:22:38+0000", utc)
                .ok()
                .unwrap()
                .to_string(),
            "2026-10-02 22:22:38 UTC"
        );
        assert_eq!(
            parse_drf_datetime("2026-10-02T22:22:38+00", utc)
                .ok()
                .unwrap()
                .to_string(),
            "2026-10-02 22:22:38 UTC"
        );
        assert_eq!(
            parse_drf_datetime("2026-10-02T22:22:38+01:02:03", utc)
                .ok()
                .unwrap()
                .to_string(),
            "2026-10-02 21:20:35 UTC"
        );
        assert_eq!(
            parse_drf_datetime("2026-10-02 22:22+00:00", utc)
                .ok()
                .unwrap()
                .to_string(),
            "2026-10-02 22:22:00 UTC"
        );
        let parsed = parse_drf_datetime("2026-10-02T22:22:38,5Z", utc)
            .ok()
            .unwrap();
        assert_eq!(parsed.to_string(), "2026-10-02 22:22:38.500 UTC");
        assert!(parse_drf_datetime("2026-1-2T1:2:3", utc).ok().is_some());
        // Basic and week dates; any single-char separator.
        assert!(parse_drf_datetime("20261002T222238Z", utc).ok().is_some());
        assert_eq!(
            parse_drf_datetime("2024-W01-1", utc)
                .ok()
                .unwrap()
                .to_string(),
            "2024-01-01 00:00:00 UTC"
        );
        assert_eq!(
            parse_drf_datetime("2026-10-02x22:22:38", utc)
                .ok()
                .unwrap()
                .to_string(),
            "2026-10-02 22:22:38 UTC"
        );
        // `+`/`-` in separator position separate (05:30 naive).
        assert_eq!(
            parse_drf_datetime("2026-10-02+05:30", utc)
                .ok()
                .unwrap()
                .to_string(),
            "2026-10-02 05:30:00 UTC"
        );
        // DST: ambiguous and nonexistent wall times resolve `fold=0`
        // (first occurrence / pre-transition offset), never 400.
        assert_eq!(
            parse_drf_datetime("2024-11-03T01:30:00", eastern)
                .ok()
                .unwrap()
                .to_string(),
            "2024-11-03 05:30:00 UTC"
        );
        assert_eq!(
            parse_drf_datetime("2024-03-10T02:30:00", eastern)
                .ok()
                .unwrap()
                .to_string(),
            "2024-03-10 07:30:00 UTC"
        );
        // Rejects.
        for bad in [
            "not-a-date",
            "",
            "2024-13-01",
            "2026-10-02T24:00:00",
            "2026-10-02T00:00:00+99:99",
            "2026-10-02T00:00:00+05:30x",
            "20240101000000",
            "2026-10-02T",
        ] {
            assert!(parse_drf_datetime(bad, utc).ok().is_none(), "{bad:?}");
        }
        assert_eq!(
            validate_datetime(&json!("nope"), utc, true),
            Err(INVALID_DATETIME_MESSAGE.to_owned())
        );
        // `astimezone` overflow is its own 400, not `invalid`.
        assert_eq!(
            validate_datetime(&json!("9999-12-31T23:00:00-14:00"), utc, true),
            Err(OVERFLOW_DATETIME_MESSAGE.to_owned())
        );
        assert_eq!(
            validate_datetime(&json!("0001-01-01T00:00:00+14:00"), utc, true),
            Err(OVERFLOW_DATETIME_MESSAGE.to_owned())
        );
        // Long fractions scan to the tz; short ones stay strict.
        assert_eq!(
            parse_drf_datetime("2026-10-02T22:22:38.123456xyz+00:00", utc)
                .ok()
                .unwrap()
                .to_string(),
            "2026-10-02 22:22:38.123456 UTC"
        );
        assert!(parse_drf_datetime("2026-10-02T22:22:38.12345x+00:00", utc)
            .ok()
            .is_none());
        // Week dates resolving past `9999` are invalid.
        assert!(parse_drf_datetime("9999-W52-7", utc).ok().is_none());
        assert!(parse_drf_datetime("9999-W52-1", utc).ok().is_some());
    }

    #[test]
    fn base_branch_regex_matches() {
        assert_eq!(validate_base_branch("main"), Ok("main".to_owned()));
        assert_eq!(
            validate_base_branch("a/b_c.d-e1"),
            Ok("a/b_c.d-e1".to_owned())
        );
        assert!(validate_base_branch("has space").is_err());
        assert!(validate_base_branch("semi;colon").is_err());
    }

    #[test]
    fn fk_classification_matches_drf() {
        let id = uuid::Uuid::nil();
        assert_eq!(classify_fk_input(&json!(id.to_string())), Ok(id));
        assert_eq!(
            classify_fk_input(&json!("xyz")),
            Err("\u{201c}xyz\u{201d} is not a valid UUID.".to_owned())
        );
        assert_eq!(
            classify_fk_input(&json!(true)),
            Err("Incorrect type. Expected pk value, received bool.".to_owned())
        );
        assert_eq!(classify_fk_input(&json!(0)), Ok(uuid::Uuid::from_u128(0)));
        assert!(classify_fk_input(&json!(1.5)).is_err());
    }

    #[test]
    fn write_field_order_pins_put_required_positions() {
        // Error-dict order is DRF serializer order (`PUT {}` reports
        // `[deleted_at, name, identifier, workspace]`); relations sort
        // last, `default_state` final.
        let position = |field: &str| WRITE_FIELDS.iter().position(|name| *name == field).unwrap();
        assert_eq!(WRITE_FIELDS[0], "deleted_at");
        assert!(position("deleted_at") < position("name"));
        assert!(position("name") < position("identifier"));
        assert!(position("identifier") < position("workspace"));
        assert_eq!(WRITE_FIELDS[WRITE_FIELDS.len() - 1], "default_state");
        assert_eq!(WRITE_FIELDS[WRITE_FIELDS.len() - 6], "workspace");
        assert_eq!(WRITE_FIELDS.len(), 43);
    }

    #[test]
    fn parse_body_shapes_match_drf() {
        assert!(parse_body(b"").unwrap().is_empty());
        assert_eq!(parse_body(br#"{"a":1}"#).unwrap().len(), 1);
        let Err(Denial::Raw(status, body)) = parse_body(b"[1]") else {
            panic!("expected raw 400");
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("but got list"));
        // PUT `null` carries the `errors`-property message, not the
        // non-dictionary branch (DRF `serializers.py:580`).
        let Err(Denial::Raw(status, body)) = parse_body(b"null") else {
            panic!("expected raw 400");
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "{\"non_field_errors\":[\"No data provided\"]}");
        let Err(Denial::Raw(status, _)) = parse_body(b"{oops") else {
            panic!("expected raw 400");
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // Mapping-style bodies (create/PATCH/identifiers/user-views)
        // 500 on every non-dict shape (`AttributeError` / `TypeError`).
        for raw in [b"[1]".as_slice(), b"\"x\"", b"5", b"null", b"true"] {
            assert!(
                matches!(parse_get_body(raw), Err(Denial::ServerError)),
                "shape {raw:?} must 500"
            );
        }
    }

    #[test]
    fn datetime_iso8601_literal_fallback() {
        // PIDASHCONV-773: DRF `to_internal_value` falls through to
        // `strptime(value, 'iso-8601')` when `parse_datetime` returns None;
        // the literal matches case-insensitively and yields naive
        // 1900-01-01 in the request zone (probed live both backends).
        let utc = chrono_tz::UTC;
        for text in [
            "iso-8601", "ISO-8601", "Iso-8601", "iSo-8601", "isO-8601", "ISo-8601", "IsO-8601",
            "iSO-8601",
        ] {
            let parsed = parse_drf_datetime(text, utc).ok().expect(text);
            assert_eq!(parsed.to_rfc3339(), "1900-01-01T00:00:00+00:00", "{text:?}");
        }
        // Near-misses stay invalid (exact match, both sides probed).
        for text in [
            "iso8601",
            "xiso-8601",
            "iso-8601x",
            " iso-8601",
            "iso-8601 ",
            "iso-8601\n",
            "\tiso-8601",
        ] {
            assert!(parse_drf_datetime(text, utc).ok().is_none(), "{text:?}");
        }
    }

    #[test]
    fn python_datetime_and_float_match() {
        let zoned = chrono::DateTime::parse_from_rfc3339("2026-10-02T22:22:38.379224Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            render_python_datetime(&zoned),
            "2026-10-02 22:22:38.379224+00:00"
        );
        let zoned = chrono::DateTime::parse_from_rfc3339("2026-10-02T22:22:38Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(render_python_datetime(&zoned), "2026-10-02 22:22:38+00:00");
        assert_eq!(python_float(&json!(1.5)), Some(1.5));
        assert_eq!(python_float(&json!(true)), Some(1.0));
        assert_eq!(python_float(&json!(" 2.5 ")), Some(2.5));
        assert_eq!(python_float(&json!("1_0")), Some(10.0));
        assert!(python_float(&json!("inf")).unwrap().is_infinite());
        assert!(python_float(&json!("")).is_none());
        assert!(python_float(&json!("abc")).is_none());
        assert!(python_float(&Value::Null).is_none());
        assert!(python_float(&json!([1])).is_none());
        // Underscores only between digits; never in `inf`/`nan`.
        assert_eq!(python_float(&json!("1_0e1_0")), Some(1e11));
        assert_eq!(python_float(&json!("1.0_0")), Some(1.0));
        for bad in [
            "1_",
            "_1",
            "1__0",
            "1_.0",
            "1._0",
            "1e_1",
            "inf_inity",
            "nan_1",
        ] {
            assert!(python_float(&json!(bad)).is_none(), "{bad:?}");
        }
        // Unicode decimal digits (but U+001C padding raises).
        assert_eq!(python_float(&json!("５")), Some(5.0));
        assert_eq!(python_float(&json!("１_２")), Some(12.0));
        assert!(python_float(&json!("\u{1c}1.5")).is_none());
        assert_eq!(python_float(&json!("\u{85}1.5")), Some(1.5));
    }

    #[test]
    fn python_text_and_render_helpers_match() {
        assert_eq!(strip_py("\u{1c}a\u{1c}"), "a");
        assert_eq!(nd_value('５'), Some(5));
        assert_eq!(nd_value('²'), None);
        assert_eq!(nd_value('7'), Some(7));
        assert_eq!(
            escape_u2028("a\u{2028}b\u{2029}c".to_owned()),
            "a\\u2028b\\u2029c"
        );
        assert_eq!(escape_u2028("plain".to_owned()), "plain");
        assert_eq!(i32_or_500(12).unwrap(), 12);
        assert!(matches!(i32_or_500(1 << 40), Err(Denial::ServerError)));
        assert!(matches!(i32_or_500(i64::MIN), Err(Denial::ServerError)));
        // Raw `if intake_view:` truthiness (validated-bool independent).
        assert!(python_truthy(&json!("false")));
        assert!(python_truthy(&json!(1)));
        assert!(!python_truthy(&json!(0)));
        assert!(!python_truthy(&json!(0.0)));
        assert!(!python_truthy(&json!("")));
        assert!(!python_truthy(&Value::Null));
        assert!(!python_truthy(&json!([])));
        assert!(!python_truthy(&json!({})));
    }

    #[test]
    fn scope_and_order_helpers_match() {
        assert!(scope_join(true, false).starts_with("INNER JOIN"));
        assert!(scope_where(true, false).contains("pm_scope.member_id = $2"));
        assert!(!scope_where(true, false).contains("p.network"));
        assert!(scope_join(false, true).starts_with("LEFT OUTER JOIN"));
        assert!(scope_where(false, true).contains("p.network = 2"));
        assert_eq!(scope_join(false, false), "");
        assert_eq!(scope_where(false, false), "TRUE");
        assert_eq!(order_column("created_at").unwrap(), "p.created_at");
        assert_eq!(order_column("workspace").unwrap(), "p.workspace_id");
        assert_eq!(order_column("sort_order").unwrap(), "sort_order");
        assert_eq!(order_column("pk").unwrap(), "p.id");
        assert_eq!(order_column("workspace__slug").unwrap(), "p.workspace_id");
        assert!(matches!(order_column("nope"), Err(Denial::ServerError)));
    }

    #[test]
    fn task_messages_match_l8_shapes() {
        let id = uuid::Uuid::nil();
        let message = soft_delete_message(&id);
        assert_eq!(
            message.task,
            pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK
        );
        assert_eq!(message.args.len(), 3);
        assert_eq!(message.kwargs.get("using"), Some(&Value::Null));
        let message = recent_visited_message(&id, &id, "ws");
        assert_eq!(message.args.len(), 0);
        assert!(message.kwargs.contains_key("entity_identifier"));
        let message = webhook_destroy_message(&id, &id, "ws", "http://x");
        assert!(message.kwargs.contains_key("event_id"));
    }
}
