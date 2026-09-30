#![forbid(unsafe_code)]

//! D-21 model columns + field semantics (PIDASHCONV-404).
//!
//! Ports the column lists, defaults, indexes, constraints, manager scopes,
//! and save-rule helpers for `file_assets`, `stickies`, `intakes`, and
//! `intake_issues` as the api-v1 views see them, adopting the Django-owned
//! schema column-for-column. Migrations are not ported; Django stays
//! schema owner until switchover.
//!
//! Sources (`apps/api/`): `pi_dash/db/models/asset.py:28-110`
//! (`FileAsset` L45-62, `Meta` L64-74, `asset_url` L79-100,
//! `EntityTypeContext` L33-43); `pi_dash/db/models/sticky.py:16-60`
//! (`Sticky` L17-30, `Meta` L32-36, `save` L38-53);
//! `pi_dash/db/models/intake.py:12-80` (`Intake` L12-35, `SourceType`
//! L38-39, `IntakeIssueStatus` L42-47, `IntakeIssue` L50-84); audit
//! columns `pi_dash/db/mixins.py:16-89`, UUID pk
//! `pi_dash/db/models/base.py:17-21`.
//!
//! Fixtures: `rust-api/fixtures/v1_assets/fx-model-fileasset.json`
//! (`FileAsset` columns, `asset_url` goldens, `FileAssetSerializer` read
//! shape), `fx-model-sticky.json` (`Sticky` columns, `save` goldens),
//! `fx-model-intake.json` (`Intake` / `IntakeIssue` columns, enums,
//! constraints). The `#[cfg(test)]` suite replays all three.
//!
//! Reads serve from the per-table soft-delete views (`<table>_active`,
//! [`crate::soft_delete::active_view_ddl`]) wherever Django uses its
//! default managers; writes hit the tables so the partial unique indexes
//! keep working. Every application-level default below must be supplied
//! explicitly on insert — the live tables carry no `column_default`.
//!
//! # Where the neighboring behavior lives
//!
//! * `asset_url` (unit 1, table-adjacent) is a types-layer kernel:
//!   `pidash-types` `v1_assets::file_asset::asset_url` (PIDASHCONV-404
//!   splits it there with the unit-5 read shape). It mirrors
//!   [`crate::app_assets::columns::asset_url`] branch for branch; the
//!   queries layer (PIDASHCONV-409) resolves the slug/ids and owns the
//!   missing-workspace 500.
//! * `get_upload_path` (`asset.py:17-20`) and the `file_size` message
//!   (`asset.py:23-25`) are ported once for this table in
//!   [`crate::app_assets::columns`]
//!   ([`crate::app_assets::columns::upload_path_key`],
//!   [`crate::app_assets::columns::FILE_SIZE_MESSAGE`]); this module
//!   does not duplicate them.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * `DRAFT_ISSUE_ATTACHMENT` has no `asset_url` branch
//!   (`asset.py:79-100` enumerates 9 of the 10 `EntityTypeContext`
//!   values): the property returns `None`. Ported as-is.
//! * `file_size` is defined but wired to no field (the `FileField` at
//!   `asset.py:46` carries no `validators=`): dead code. There is
//!   deliberately no validator const here; uploads are capped by
//!   `min(size, FILE_SIZE_LIMIT)` in the views.
//! * Generic post hard-codes `entity_type = ISSUE_ATTACHMENT`
//!   (`api/views/asset.py:562`) for every workspace asset, so `asset_url`
//!   for generic uploads always renders the attachment shape.
//! * `Sticky.save` recomputes `description_stripped` on EVERY save
//!   (create and update) but touches `sort_order` only on create
//!   (`sticky.py:38-52`). Concurrent creates read the same max and
//!   collide (no locking); ported as-is.
//! * `Sticky.__str__` is `str(self.name)` (`sticky.py:56-57`): a null
//!   name renders `"None"`. [`sticky::Sticky`]'s `Display` ports that
//!   exactly. `Intake.__str__` (`f"{name} <{project.name}>"`,
//!   `intake.py:19-21`) and `IntakeIssue.__str__`
//!   (`f"{issue.name} <{intake.name}>"`, `intake.py:82-84`) dereference
//!   joined rows the row structs do not carry; there is deliberately no
//!   `Display` for those two (a `Display` rendering the id in place
//!   would invent a shape Python never emits).
//! * `Display` impls below format exact row data only and never panic;
//!   nullable dereferences that would crash Python have no Rust
//!   equivalent by construction.

use crate::license::models::OnDelete;

use super::entities;

/// Base columns every table inherits, in Django `_meta` field order:
/// `BaseModel.id` (`db/models/base.py:17-18`), then the audit columns
/// (`TimeAuditModel.created_at/updated_at`, `db/mixins.py:16-23`;
/// `UserAuditModel.created_by/updated_by`, `db/mixins.py:26-42`;
/// `SoftDeleteModel.deleted_at`, `db/mixins.py:61-64`). FK attnames
/// (`created_by_id`, `updated_by_id`).
pub const BASE_COLUMNS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
];

/// A `Meta.indexes` entry: the index name plus its column list.
pub struct Index {
    /// Index name as created by Django.
    pub name: &'static str,
    /// Indexed columns in order.
    pub columns: &'static [&'static str],
}

/// `created_by` / `updated_by` audit FKs: `SET_NULL`, nullable
/// (from `UserAuditModel`, `db/mixins.py:26-42`). Shared by all four
/// tables.
pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
/// See [`CREATED_BY_ON_DELETE`].
pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

/// `file_assets` table (`db/models/asset.py:28-110`,
/// `db_table = "file_assets"`), api-v1 view.
pub mod file_asset {
    use super::{Index, OnDelete};

    /// Physical table (`Meta.db_table`, `asset.py:67`).
    pub const TABLE: &str = "file_assets";

    /// Soft-delete read view (`file_assets_active`).
    pub const VIEW: &str = "file_assets_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`,
    /// `asset.py:68`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order: [`super::BASE_COLUMNS`],
    /// then `asset.py:45-62`. FK entries use the Django attnames
    /// (`user_id`, `workspace_id`, …).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "attributes",
        "asset",
        "user_id",
        "workspace_id",
        "draft_issue_id",
        "project_id",
        "issue_id",
        "comment_id",
        "page_id",
        "entity_type",
        "entity_identifier",
        "is_deleted",
        "is_archived",
        "external_id",
        "external_source",
        "size",
        "is_uploaded",
        "storage_metadata",
    ];

    /// Django field defaults in [`COLUMNS`] order (`None` = no default).
    /// `attributes`/`storage_metadata` default to `dict` (`:45,:62`), the
    /// booleans to `false` (`:56,:57,:61`), `size` to `0` (`:60`); `id` is
    /// a `uuid4` primary key (`base.py:18`).
    pub const DEFAULTS: &[Option<&str>] = &[
        None,          // id (uuid4 pk)
        None,          // created_at (auto_now_add)
        None,          // updated_at (auto_now)
        None,          // created_by_id (SET_NULL)
        None,          // updated_by_id (SET_NULL)
        None,          // deleted_at
        Some("dict"),  // attributes (JSONField)
        None,          // asset (FileField, max_length=800)
        None,          // user_id (CASCADE)
        None,          // workspace_id (CASCADE)
        None,          // draft_issue_id (CASCADE)
        None,          // project_id (CASCADE)
        None,          // issue_id (CASCADE)
        None,          // comment_id (CASCADE)
        None,          // page_id (CASCADE)
        None,          // entity_type (varchar(255) null blank, no choices=)
        None,          // entity_identifier (varchar(255) null blank)
        Some("false"), // is_deleted
        Some("false"), // is_archived
        None,          // external_id (varchar(255) null blank)
        None,          // external_source (varchar(255) null blank)
        Some("0"),     // size (FloatField)
        Some("false"), // is_uploaded
        Some("dict"),  // storage_metadata (JSONField null blank)
    ];

    /// `Meta.indexes` (`asset.py:69-74`) in declaration order.
    pub const INDEXES: &[Index] = &[
        Index {
            name: "asset_entity_type_idx",
            columns: &["entity_type"],
        },
        Index {
            name: "asset_entity_identifier_idx",
            columns: &["entity_identifier"],
        },
        Index {
            name: "asset_entity_idx",
            columns: &["entity_type", "entity_identifier"],
        },
        Index {
            name: "asset_asset_idx",
            columns: &["asset"],
        },
    ];

    /// All `EntityTypeContext` values (`asset.py:33-43`) in source
    /// definition order. `entity_type` passes no `choices=` (`:54`); the
    /// values are enforced only in view code, so the column stays an
    /// unconstrained varchar — never assume a DB constraint.
    pub const ENTITY_TYPES: &[&str] = &[
        "ISSUE_ATTACHMENT",
        "ISSUE_DESCRIPTION",
        "COMMENT_DESCRIPTION",
        "PAGE_DESCRIPTION",
        "USER_COVER",
        "USER_AVATAR",
        "WORKSPACE_LOGO",
        "PROJECT_COVER",
        "DRAFT_ISSUE_ATTACHMENT",
        "DRAFT_ISSUE_DESCRIPTION",
    ];

    /// `asset` bound (`asset.py:46`, `FileField(max_length=800)`).
    pub const ASSET_MAX_LENGTH: usize = 800;

    /// `entity_type` / `entity_identifier` / `external_id` /
    /// `external_source` bound (`:54-55,:58-59`, `max_length=255`).
    pub const VARCHAR_MAX_LENGTH: usize = 255;

    /// `user` FK: `CASCADE`, nullable (`asset.py:47`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE`, nullable (`asset.py:48`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `draft_issue` FK: `CASCADE`, nullable (`asset.py:49`).
    pub const DRAFT_ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE`, nullable (`asset.py:50`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK: `CASCADE`, nullable (`asset.py:51`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `comment` FK: `CASCADE`, nullable (`asset.py:52`).
    pub const COMMENT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `page` FK: `CASCADE`, nullable (`asset.py:53`).
    pub const PAGE_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// One `file_assets` row. `attributes` stores `{}`, never `NULL`
    /// (`JSONField(default=dict)`); `entity_type`/`entity_identifier`
    /// are nullable with no choices constraint; `size` is a float byte
    /// count defaulting to `0`; `storage_metadata` is nullable
    /// (`default=dict, null=True, blank=True`).
    /// Python `FloatField` (C double) maps to `f64` (see Semantic traps).
    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct FileAsset {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub attributes: serde_json::Value,
        pub asset: String,
        pub user_id: Option<uuid::Uuid>,
        pub workspace_id: Option<uuid::Uuid>,
        pub draft_issue_id: Option<uuid::Uuid>,
        pub project_id: Option<uuid::Uuid>,
        pub issue_id: Option<uuid::Uuid>,
        pub comment_id: Option<uuid::Uuid>,
        pub page_id: Option<uuid::Uuid>,
        pub entity_type: Option<String>,
        pub entity_identifier: Option<String>,
        pub is_deleted: bool,
        pub is_archived: bool,
        pub external_id: Option<String>,
        pub external_source: Option<String>,
        pub size: f64,
        pub is_uploaded: bool,
        pub storage_metadata: Option<serde_json::Value>,
    }

    impl std::fmt::Display for FileAsset {
        /// `__str__` (`asset.py:76-77`): the storage key, not the id.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.asset)
        }
    }
}

/// `stickies` table (`db/models/sticky.py:16-60`,
/// `db_table = "stickies"`), api-v1 view.
pub mod sticky {
    use super::OnDelete;

    /// Physical table (`Meta.db_table`, `sticky.py:35`).
    pub const TABLE: &str = "stickies";

    /// Soft-delete read view (`stickies_active`).
    pub const VIEW: &str = "stickies_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`,
    /// `sticky.py:36`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order: [`super::BASE_COLUMNS`],
    /// then `sticky.py:17-30`. FK entries use the Django attnames
    /// (`workspace_id`, `owner_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "name",
        "description",
        "description_html",
        "description_stripped",
        "description_binary",
        "logo_props",
        "color",
        "background_color",
        "workspace_id",
        "owner_id",
        "sort_order",
    ];

    /// Django field defaults in [`COLUMNS`] order (`None` = no default).
    /// `description`/`logo_props` default to `dict` (`:19,:25`),
    /// `description_html` to `"<p></p>"` (`:20`), `sort_order` to
    /// `65535` (`:30`); `id` is a `uuid4` primary key (`base.py:18`).
    pub const DEFAULTS: &[Option<&str>] = &[
        None,                // id (uuid4 pk)
        None,                // created_at (auto_now_add)
        None,                // updated_at (auto_now)
        None,                // created_by_id (SET_NULL)
        None,                // updated_by_id (SET_NULL)
        None,                // deleted_at
        None,                // name (TextField null blank)
        Some("dict"),        // description (JSONField blank)
        Some("\"<p></p>\""), // description_html (TextField blank)
        None,                // description_stripped (TextField null blank)
        None,                // description_binary (BinaryField null)
        Some("dict"),        // logo_props (JSONField)
        None,                // color (varchar(255) null blank)
        None,                // background_color (varchar(255) null blank)
        None,                // workspace_id (CASCADE, required)
        None,                // owner_id (CASCADE, required)
        Some("65535"),       // sort_order (FloatField)
    ];

    /// `Meta.indexes`: none (`sticky.py:32-36` declares no indexes).
    pub const INDEXES: &[super::Index] = &[];

    /// Default `sort_order` (`sticky.py:30`): kept when the workspace
    /// has no rows yet (`sticky.py:51-52`, `last_id is None` branch).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;

    /// `sort_order` step for creates (`sticky.py:52`, `last_id + 10000`).
    pub const SORT_ORDER_STEP: f64 = 10000.0;

    /// `color` / `background_color` bound (`sticky.py:26-27`,
    /// `max_length=255`).
    pub const VARCHAR_MAX_LENGTH: usize = 255;

    /// `workspace` FK: `CASCADE`, required (`sticky.py:28`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `owner` FK: `CASCADE`, required (`sticky.py:29`).
    pub const OWNER_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// One `stickies` row. `description` stores `{}`, never `NULL`
    /// (`JSONField(blank=True, default=dict)`); `description_html`
    /// stores `"<p></p>"` by default, never `NULL`; `description_binary`
    /// is nullable with `blank=False` (no `blank=True`, `:23`);
    /// `workspace_id`/`owner_id` are required; `sort_order` is a float.
    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct Sticky {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub name: Option<String>,
        pub description: serde_json::Value,
        pub description_html: String,
        pub description_stripped: Option<String>,
        pub description_binary: Option<Vec<u8>>,
        pub logo_props: serde_json::Value,
        pub color: Option<String>,
        pub background_color: Option<String>,
        pub workspace_id: uuid::Uuid,
        pub owner_id: uuid::Uuid,
        pub sort_order: f64,
    }

    impl std::fmt::Display for Sticky {
        /// `__str__` (`sticky.py:56-57`): `str(self.name)` — a null name
        /// renders `"None"`, ported exactly.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match &self.name {
                Some(name) => write!(f, "{name}"),
                None => write!(f, "None"),
            }
        }
    }

    /// `Sticky.save` stripped-text rule (`sticky.py:38-44`): `None` when
    /// the html is empty or `None`, else [`strip_tags_ml`]. Recomputed on
    /// EVERY save (create and update); `sort_order` is touched only on
    /// create.
    pub fn stripped_description(html: Option<&str>) -> Option<String> {
        match html {
            None => None,
            Some("") => None,
            Some(h) => Some(strip_tags_ml(h)),
        }
    }

    /// `Sticky.save` sequence rule (`sticky.py:45-52`): on create only
    /// (`_state.adding`), `sort_order` becomes the workspace max plus
    /// [`SORT_ORDER_STEP`]; when no rows exist yet the field default
    /// ([`DEFAULT_SORT_ORDER`]) is kept. `existing_max` is
    /// `Sticky.objects.filter(workspace=…).aggregate(Max("sort_order"))`
    /// (per-workspace scope; other workspaces' rows are ignored).
    /// No transaction or locking: concurrent creates read the same max
    /// and collide — ported as-is.
    pub fn sort_order_on_create(existing_max: Option<f64>) -> f64 {
        match existing_max {
            None => DEFAULT_SORT_ORDER,
            Some(max) => max + SORT_ORDER_STEP,
        }
    }

    /// `pi_dash.utils.html_processor.strip_tags` (`html_processor.py:11-31`):
    /// the `MLStripper` (`HTMLParser` with `convert_charrefs=True`, CPython
    /// 3.12 — the project's runtime): tags dropped, character references in
    /// text decoded. This is NOT Django's `strip_tags` (regex, entities
    /// kept). Transliterated from CPython 3.12 `html.parser.HTMLParser`
    /// (`feed` path only — `Sticky.save` feeds once and never calls
    /// `close()`, so a trailing incomplete construct is dropped, not
    /// flushed) with `html.unescape` for text chunks ([`unescape`]).
    ///
    /// Behavior pinned by probe (`/tmp/probe6.py`, Python 3.12.14):
    /// comments/declarations/PIs dropped; `script`/`style`/`xmp`/`iframe`/
    /// `noembed`/`noframes` content raw (entities kept); `title`/`textarea`
    /// content decoded; an unclosed `<` is kept verbatim but a trailing
    /// `&...` or `<tag...` construct drops everything from the `&`/`<`
    /// (`R&D` → `""`, `a < b` → `"a < b"`); `</div class="a>b">` is one
    /// end tag on 3.12 (quote-aware `locatetagend`), unlike 3.9.
    /// Differential-fuzzed against the real `MLStripper` (see PR).
    pub fn strip_tags_ml(html: &str) -> String {
        Stripper::new(html).run()
    }

    /// `html.unescape` (CPython 3.12 `html/__init__.py`): decode every
    /// `&...` reference in a tag-free chunk — full `html5` table
    /// ([`super::entities::lookup`]) with longest-prefix fallback, numeric
    /// references with the invalid-charref/codepoint tables. Unknown or
    /// unterminated sequences stay verbatim; single left-to-right pass
    /// with no rescan (`&amp;amp;` → `&amp;`).
    pub fn unescape(text: &str) -> String {
        if !text.contains('&') {
            return text.to_owned();
        }
        let b = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut pos = 0;
        let mut i = 0;
        while i < b.len() {
            if b[i] != b'&' {
                i += 1;
                continue;
            }
            if let Some((body, end)) = scan_reference(b, i + 1) {
                out.push_str(&text[pos..i]);
                replace_reference(&mut out, body);
                pos = end;
                i = end;
            } else {
                i += 1;
            }
        }
        out.push_str(&text[pos..]);
        out
    }

    /// Scan one `&...` reference body starting at `start` (just past the
    /// `&`), mirroring `_charref =
    /// &(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\f <&#;]{1,32};?)`.
    /// Returns the body (WITHOUT `&`, WITH trailing `;` when present) and
    /// the byte offset just past it. `None` means "no reference here" —
    /// the `&` stays literal.
    fn scan_reference(b: &[u8], start: usize) -> Option<(&str, usize)> {
        let rest = b.get(start..)?;
        if rest.first() == Some(&b'#') {
            if rest.get(1) == Some(&b'x') || rest.get(1) == Some(&b'X') {
                let mut k = start + 2;
                while k < b.len() && b[k].is_ascii_hexdigit() {
                    k += 1;
                }
                if k == start + 2 {
                    return None;
                }
                let mut end = k;
                if b.get(end) == Some(&b';') {
                    end += 1;
                }
                std::str::from_utf8(&b[start..end]).ok().map(|s| (s, end))
            } else {
                let mut k = start + 1;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                if k == start + 1 {
                    return None;
                }
                let mut end = k;
                if b.get(end) == Some(&b';') {
                    end += 1;
                }
                std::str::from_utf8(&b[start..end]).ok().map(|s| (s, end))
            }
        } else {
            let mut k = start;
            while k < b.len()
                && (k - start) < 32
                && !matches!(
                    b[k],
                    b'\t' | b'\n' | 0x0C | b' ' | b'<' | b'&' | b'#' | b';'
                )
            {
                k += 1;
            }
            if k == start {
                return None;
            }
            let mut end = k;
            if b.get(end) == Some(&b';') {
                end += 1;
            }
            std::str::from_utf8(&b[start..end]).ok().map(|s| (s, end))
        }
    }

    /// Render one scanned reference body (`_replace_charref`): numeric
    /// bodies decode with the invalid tables, named bodies hit the
    /// [`super::entities`] map with longest-prefix fallback, and anything
    /// else stays verbatim (`&` + body).
    fn replace_reference(out: &mut String, body: &str) {
        if let Some(rest) = body.strip_prefix('#') {
            let num: u64 = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
                u64::from_str_radix(hex.trim_end_matches(';'), 16).unwrap_or(u64::MAX)
            } else {
                rest.trim_end_matches(';').parse().unwrap_or(u64::MAX)
            };
            if let Some(mapped) = super::entities::invalid_charref(num) {
                out.push_str(mapped);
            } else if (0xD800..=0xDFFF).contains(&num) || num > 0x10FFFF {
                out.push('\u{FFFD}');
            } else if super::entities::is_invalid_codepoint(num) {
                // Invalid code points decode to the empty string: the
                // reference vanishes.
            } else if let Some(ch) = char::from_u32(num as u32) {
                out.push(ch);
            } else {
                out.push('\u{FFFD}');
            }
            return;
        }
        if let Some(hit) = super::entities::lookup(body) {
            out.push_str(hit);
            return;
        }
        // Longest-prefix fallback (`range(len(s)-1, 1, -1)`): prefixes
        // are ASCII table keys, so iterate char boundaries.
        let chars: Vec<(usize, char)> = body.char_indices().collect();
        let mut x = chars.len().saturating_sub(1);
        while x >= 2 {
            let end = chars[x].0;
            if let Some(hit) = super::entities::lookup(&body[..end]) {
                out.push_str(hit);
                out.push_str(&body[end..]);
                return;
            }
            x -= 1;
        }
        out.push('&');
        out.push_str(body);
    }

    /// CDATA elements (`HTMLParser.CDATA_CONTENT_ELEMENTS`, 3.12):
    /// content is raw (entities kept).
    const CDATA_ELEMENTS: &[&str] = &["script", "style", "xmp", "iframe", "noembed", "noframes"];

    /// RCDATA elements (`HTMLParser.RCDATA_CONTENT_ELEMENTS`, 3.12):
    /// content is decoded like normal text.
    const RCDATA_ELEMENTS: &[&str] = &["textarea", "title"];

    /// Whitespace of the tolerant tag patterns (`[\t\n\r\f ]`).
    fn is_tag_ws(b: u8) -> bool {
        matches!(b, b'\t' | b'\n' | 0x0C | b'\r' | b' ')
    }

    /// `str.strip()` space (`Py_UNICODE_ISSPACE`): Rust `White_Space`
    /// plus U+001C–U+001F (probed on 3.12: the sets differ only there).
    /// Used solely for the `end not in (">", "/>")` check, so
    /// `<a\x1c>` drops like Python instead of emitting as data.
    fn is_py_strip_space(c: char) -> bool {
        c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
    }

    /// ASCII letter (`[a-zA-Z]`, `starttagopen` / `endtagopen`).
    fn is_tag_letter(b: u8) -> bool {
        b.is_ascii_alphabetic()
    }

    /// Attribute-name lead-in (`(?<=['"\t\n\r\f /])`): the byte before
    /// the name must be a quote, tag whitespace, or `/`.
    fn is_attr_lead(prev: u8) -> bool {
        matches!(
            prev,
            b'\'' | b'"' | b'\t' | b'\n' | 0x0C | b'\r' | b' ' | b'/'
        )
    }

    /// Tag-name char (`[^\t\n\r\f />]`, `tagfind_tolerant` /
    /// `locatetagend`).
    fn is_tagname_char(b: u8) -> bool {
        !matches!(b, b'\t' | b'\n' | 0x0C | b'\r' | b' ' | b'/' | b'>')
    }

    /// Attribute-name tail char (`[^\t\n\r\f /=>]`).
    fn is_attr_tail(b: u8) -> bool {
        !matches!(b, b'\t' | b'\n' | 0x0C | b'\r' | b' ' | b'/' | b'=' | b'>')
    }

    /// Bare attribute-value char (`[^>\t\n\r\f ]`).
    fn is_bare_value_char(b: u8) -> bool {
        !matches!(b, b'>' | b'\t' | b'\n' | 0x0C | b'\r' | b' ')
    }

    /// `locatetagend.match(rawdata, pos)`: tolerant tag-end scan used by
    /// both `check_for_whole_start_tag` and 3.12 `parse_endtag`. Returns
    /// the match end; the caller checks `s[end-1] == '>'`.
    fn match_tag_end(b: &[u8], pos: usize) -> usize {
        let mut k = pos;
        while k < b.len() && is_tagname_char(b[k]) {
            k += 1;
        }
        k = skip_tag_ws_slash(b, k);
        while let Some((_, after_value)) = match_attr(b, k) {
            k = skip_tag_ws_slash(b, after_value);
        }
        if b.get(k) == Some(&b'>') {
            k += 1;
        }
        k
    }

    /// Skip `[\t\n\r\f /]*` (the `locatetagend` separator).
    fn skip_tag_ws_slash(b: &[u8], mut k: usize) -> usize {
        while k < b.len() && (is_tag_ws(b[k]) || b[k] == b'/') {
            k += 1;
        }
        k
    }

    /// Skip `(?:[\t\n\r\f ]|/(?!>))*` (the `attrfind_tolerant` trailer):
    /// whitespace, or `/` not followed by `>`.
    fn skip_attr_trailer(b: &[u8], mut k: usize) -> usize {
        while k < b.len() && (is_tag_ws(b[k]) || (b[k] == b'/' && b.get(k + 1) != Some(&b'>'))) {
            k += 1;
        }
        k
    }

    /// One tolerant attribute at `k`: returns `(after_name,
    /// after_value_or_name)`. `None` when no attribute starts here
    /// (bad lead-in or bad first name char). Mirrors the shared core of
    /// `locatetagend` and `attrfind_tolerant`.
    fn match_attr(b: &[u8], k: usize) -> Option<(usize, usize)> {
        if k == 0 || !is_attr_lead(b[k - 1]) {
            return None;
        }
        let first = *b.get(k)?;
        if matches!(first, b'\t' | b'\n' | 0x0C | b'\r' | b' ' | b'/' | b'>') {
            return None;
        }
        let mut n = k + 1;
        while n < b.len() && is_attr_tail(b[n]) {
            n += 1;
        }
        // Optional `WS* = WS* VALUE`. The trailing `WS*` backtracks:
        // when no value fits past all the whitespace (e.g. an unclosed
        // quote: `href= ' <1>`), the engine gives one whitespace back
        // and the value matches empty there (`href= ` → groups
        // `('href', '=', '')`). Try the latest value start first.
        let mut v = n;
        while v < b.len() && is_tag_ws(b[v]) {
            v += 1;
        }
        if b.get(v) != Some(&b'=') {
            return Some((n, n));
        }
        let eq_end = v + 1;
        v = eq_end;
        while v < b.len() && is_tag_ws(b[v]) {
            v += 1;
        }
        let mut start = v + 1;
        while start > eq_end {
            start -= 1;
            if let Some(end) = match_attr_value(b, start) {
                return Some((n, end));
            }
        }
        Some((n, n))
    }

    /// Attribute value at `v`: `'[^']*'`, `"[^"]*"`, or bare
    /// (`(?![\'"])[^>\t\n\r\f ]*`, possibly empty). `None` when none
    /// fits (a leading quote with no closer, or a quote-led bare).
    fn match_attr_value(b: &[u8], v: usize) -> Option<usize> {
        match b.get(v).copied() {
            // `'[^']*'` / `"[^"]*"`: scan to the matching closer.
            Some(q) if q == b'\'' || q == b'"' => {
                let mut k = v + 1;
                while k < b.len() && b[k] != q {
                    k += 1;
                }
                if k < b.len() {
                    Some(k + 1)
                } else {
                    None
                }
            }
            Some(_) => {
                let mut k = v;
                while k < b.len() && is_bare_value_char(b[k]) {
                    k += 1;
                }
                Some(k)
            }
            None => Some(v),
        }
    }

    /// Running `goahead(end=False)` state: the input, the output, the
    /// cdata element (lowercase tag, or `"plaintext"`), and whether text
    /// chunks decode references (`_escapable`: true outside CDATA and
    /// inside RCDATA).
    struct Stripper<'a> {
        html: &'a str,
        bytes: &'a [u8],
        out: String,
        cdata: Option<&'a str>,
        escapable: bool,
    }

    impl<'a> Stripper<'a> {
        fn new(html: &'a str) -> Self {
            Stripper {
                html,
                bytes: html.as_bytes(),
                out: String::with_capacity(html.len()),
                cdata: None,
                escapable: true,
            }
        }

        /// `tag` lowered for the CDATA/RCDATA comparison
        /// (`match.group(1).lower()`, full Unicode lower like Python's
        /// `str.lower`). Returns the lowercase tag and its match end.
        fn tagfind_lower(&self, i: usize) -> (String, usize) {
            let mut k = i;
            while k < self.bytes.len() && is_tagname_char(self.bytes[k]) {
                k += 1;
            }
            let tag = self.html[i..k].to_lowercase();
            (tag, skip_attr_trailer(self.bytes, k))
        }

        fn run(mut self) -> String {
            let n = self.bytes.len();
            let mut i = 0;
            while i < n {
                // Find the next chunk end `j`.
                let j = if self.cdata.is_none() {
                    match memchr_lt(self.bytes, i) {
                        Some(p) => p,
                        None => {
                            if tail_amp_drops(self.html, self.bytes, i) {
                                break;
                            }
                            self.push_chunk(i, n);
                            break;
                        }
                    }
                } else if self.cdata == Some("plaintext") {
                    // `interesting = re.compile(r'\Z')`: the rest is raw.
                    n
                } else {
                    match find_cdata_end(self.bytes, i, self.cdata.unwrap_or("")) {
                        Some(p) => p,
                        None => break,
                    }
                };
                if i < j {
                    self.push_chunk(i, j);
                }
                i = j;
                if i >= n {
                    break;
                }
                // `self.bytes[i] == b'<'` here.
                if self.cdata.is_some() {
                    // The `interesting` match guarantees `</tag`, but run
                    // the full end-tag path: it also covers `</tag ...>`.
                    match self.parse_endtag(i) {
                        Some(k) => i = k,
                        None => break,
                    }
                    continue;
                }
                if i + 1 < n && is_tag_letter(self.bytes[i + 1]) {
                    match self.parse_starttag(i) {
                        Some(k) => i = k,
                        None => break,
                    }
                } else if self.bytes[i..].starts_with(b"</") {
                    match self.parse_endtag(i) {
                        Some(k) => i = k,
                        None => break,
                    }
                } else if self.bytes[i..].starts_with(b"<!--") {
                    match parse_comment(self.bytes, i) {
                        Some(k) => i = k,
                        None => break,
                    }
                } else if self.bytes[i..].starts_with(b"<?") {
                    match parse_pi(self.bytes, i) {
                        Some(k) => i = k,
                        None => break,
                    }
                } else if self.bytes[i..].starts_with(b"<!") {
                    match parse_declaration(self.bytes, i) {
                        Some(k) => i = k,
                        None => break,
                    }
                } else if i + 1 < n {
                    self.out.push('<');
                    i += 1;
                } else {
                    break;
                }
            }
            self.out
        }

        /// Emit `html[i..j]`, decoded when escapable
        /// (`handle_data(unescape(…))` vs `handle_data(…)`).
        fn push_chunk(&mut self, i: usize, j: usize) {
            let chunk = &self.html[i..j];
            if self.escapable {
                self.out.push_str(&unescape(chunk));
            } else {
                self.out.push_str(chunk);
            }
        }

        /// `parse_starttag(i)` (`<` + ASCII letter at `i`): returns the
        /// tag end, or `None` for an incomplete tag. Drops the tag
        /// (`handle_starttag` is a no-op for stripping), except the
        /// `end not in (">", "/>")` fallback which emits the raw tag text
        /// as data. Sets CDATA/RCDATA mode for the content elements.
        fn parse_starttag(&mut self, i: usize) -> Option<usize> {
            let endpos = check_whole_start_tag(self.bytes, i)?;
            let (tag, mut k) = self.tagfind_lower(i + 1);
            // `while k < endpos: m = attrfind_tolerant.match(rawdata, k);
            // if not m: break; ...; k = m.end()`. A match always consumes
            // the NAME (≥1 char), so progress is guaranteed.
            while k < endpos {
                let Some((_, after)) = match_attr(self.bytes, k) else {
                    break;
                };
                k = skip_attr_trailer(self.bytes, after);
            }
            let end = self.html[k..endpos].trim_matches(is_py_strip_space);
            if end != ">" && end != "/>" {
                self.out.push_str(&self.html[i..endpos]);
                return Some(endpos);
            }
            if end == "/>" {
                return Some(endpos);
            }
            if CDATA_ELEMENTS.contains(&tag.as_str()) || tag == "plaintext" {
                self.cdata = Some(intern_cdata(&tag));
                self.escapable = false;
            } else if RCDATA_ELEMENTS.contains(&tag.as_str()) {
                self.cdata = Some(intern_cdata(&tag));
                self.escapable = true;
            }
            Some(endpos)
        }

        /// 3.12 `parse_endtag(i)` (`</` at `i`): returns the tag end, or
        /// `None` when no `>` follows. Always clears CDATA mode on
        /// success — even a non-matching end tag like `</b>` inside
        /// `<script>` would, but that path never dispatches there (the
        /// CDATA `interesting` match only fires on the element's own
        /// closer).
        fn parse_endtag(&mut self, i: usize) -> Option<usize> {
            let b = self.bytes;
            memchr_gt(b, i + 2)?;
            if !b.get(i + 2).is_some_and(|c| is_tag_letter(*c)) {
                if b.get(i + 2) == Some(&b'>') {
                    return Some(i + 3);
                }
                return parse_bogus_comment(b, i);
            }
            let j = match_tag_end(b, i + 2);
            if b.get(j - 1) != Some(&b'>') {
                return None;
            }
            self.cdata = None;
            self.escapable = true;
            Some(j)
        }
    }

    /// First `<` at or after `i` (`rawdata.find('<', i)`; `<` is ASCII
    /// so the byte index is a char boundary).
    fn memchr_lt(b: &[u8], i: usize) -> Option<usize> {
        b[i..].iter().position(|c| *c == b'<').map(|p| i + p)
    }

    /// First `>` at or after `i`.
    fn memchr_gt(b: &[u8], i: usize) -> Option<usize> {
        b[i..].iter().position(|c| *c == b'>').map(|p| i + p)
    }

    /// The no-`<`-ahead tail rule (`goahead`, 3.12): when no `<` follows,
    /// look for an `&` in the last 34 chars (`rfind('&', max(i,
    /// n-34))`); if one is found and no whitespace/`;` follows it
    /// anywhere (`[\t\n\r\f ;]`), the tail may be a cut-in-half
    /// charref, so drop everything from `i` (wait for more text that
    /// never comes on a single feed). Returns true for "drop the rest".
    /// Character — not byte — counting, exactly like the Python.
    fn tail_amp_drops(html: &str, bytes: &[u8], i: usize) -> bool {
        let n_chars = html.chars().count();
        let i_char = html[..i].chars().count();
        let start_char = i_char.max(n_chars.saturating_sub(34));
        let Some(start_byte) =
            html.char_indices()
                .nth(start_char)
                .map(|(b, _)| b)
                .or(if start_char >= n_chars {
                    Some(bytes.len())
                } else {
                    None
                })
        else {
            return false;
        };
        let Some(rel) = bytes[start_byte..].iter().rposition(|c| *c == b'&') else {
            return false;
        };
        let amp = start_byte + rel;
        !bytes[amp..]
            .iter()
            .any(|c| matches!(c, b'\t' | b'\n' | b'\r' | 0x0C | b' ' | b';'))
    }

    /// CDATA closer (`interesting = re.compile(r'</TAG(?=[\t\n\r\f />])',
    /// IGNORECASE|ASCII)`): `</` + the element name (ASCII
    /// case-insensitive) + a boundary byte. Returns the match start.
    fn find_cdata_end(b: &[u8], i: usize, tag: &str) -> Option<usize> {
        let mut k = i;
        while let Some(p) = memchr_lt(b, k) {
            let after = b.get(p + 1..)?;
            if after.len() > tag.len()
                && after[0] == b'/'
                && after[1..1 + tag.len()].eq_ignore_ascii_case(tag.as_bytes())
                && after
                    .get(1 + tag.len())
                    .is_some_and(|c| matches!(c, b'\t' | b'\n' | 0x0C | b'\r' | b' ' | b'/' | b'>'))
            {
                return Some(p);
            }
            k = p + 1;
        }
        None
    }

    /// `check_for_whole_start_tag(i)`: `locatetagend.match(rawdata, i+1)`
    /// must end on `>`, else the tag is incomplete (`-1` → `None`).
    fn check_whole_start_tag(b: &[u8], i: usize) -> Option<usize> {
        let j = match_tag_end(b, i + 1);
        if b.get(j - 1) == Some(&b'>') {
            Some(j)
        } else {
            None
        }
    }

    /// `parse_comment(i)` (`<!--` at `i`): `commentclose.search`
    /// (`--!?>`) from `i+4`, else the abrupt close (`-?>`) anchored at
    /// `i+4`, else incomplete (`None`). Comments are dropped
    /// (`handle_comment` is a no-op for stripping).
    fn parse_comment(b: &[u8], i: usize) -> Option<usize> {
        let mut p = i + 4;
        while p < b.len() {
            if b[p..].starts_with(b"--!>") {
                return Some(p + 4);
            }
            if b[p..].starts_with(b"-->") {
                return Some(p + 3);
            }
            p += 1;
        }
        // Abrupt close (`commentabruptclose.match(rawdata, i+4)`).
        if b[i + 4..].starts_with(b"->") {
            return Some(i + 6);
        }
        if b.get(i + 4) == Some(&b'>') {
            return Some(i + 5);
        }
        None
    }

    /// `parse_pi(i)` (`<?` at `i`): ends at the first `>` (`piclose`).
    /// Dropped.
    fn parse_pi(b: &[u8], i: usize) -> Option<usize> {
        memchr_gt(b, i + 2).map(|p| p + 1)
    }

    /// `parse_bogus_comment(i)` (`<!` or `</` at `i`): ends at the first
    /// `>` past `i+2`. Dropped.
    fn parse_bogus_comment(b: &[u8], i: usize) -> Option<usize> {
        memchr_gt(b, i + 2).map(|p| p + 1)
    }

    /// `parse_html_declaration(i)` (`<!` at `i`): `<![CDATA[` sections,
    /// `<!DOCTYPE …>` (first `>`, no quote awareness), `<![…]>`, else a
    /// bogus comment. All dropped (`handle_decl`/`unknown_decl` are
    /// no-ops for stripping).
    fn parse_declaration(b: &[u8], i: usize) -> Option<usize> {
        if b[i..].starts_with(b"<![CDATA[") {
            let rest = &b[i + 9..];
            return find_bytes(rest, b"]]>").map(|p| i + 9 + p + 3);
        }
        if b.len() >= i + 9 && b[i..i + 9].eq_ignore_ascii_case(b"<!doctype") {
            return memchr_gt(b, i + 9).map(|p| p + 1);
        }
        if b[i..].starts_with(b"<![") {
            return memchr_gt(b, i + 3).map(|p| p + 1);
        }
        parse_bogus_comment(b, i)
    }

    /// First occurrence of `needle` in `haystack` (byte offset).
    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    /// The CDATA tag slot: the only values are the six CDATA elements,
    /// the two RCDATA elements, and `"plaintext"`.
    fn intern_cdata(tag: &str) -> &'static str {
        match tag {
            "script" => "script",
            "style" => "style",
            "xmp" => "xmp",
            "iframe" => "iframe",
            "noembed" => "noembed",
            "noframes" => "noframes",
            "textarea" => "textarea",
            "title" => "title",
            _ => "plaintext",
        }
    }
} // mod sticky

/// `SourceType` (`db/models/intake.py:38-39`): single-value
/// `TextChoices`. `IntakeIssue.source` defaults to `"IN_APP"`
/// (`intake.py:70`); the views write `SourceType.IN_APP`
/// (`api/views/intake.py:204`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceType {
    InApp,
}

impl SourceType {
    /// The stored string (`SourceType.IN_APP.value`).
    pub fn as_str(self) -> &'static str {
        match self {
            SourceType::InApp => "IN_APP",
        }
    }

    /// All values in declaration order.
    pub const ALL: &[SourceType] = &[SourceType::InApp];
}

/// Error for unknown source-type strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSourceType(pub String);

impl std::fmt::Display for UnknownSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown intake source type: {}", self.0)
    }
}

impl std::error::Error for UnknownSourceType {}

impl std::str::FromStr for SourceType {
    type Err = UnknownSourceType;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "IN_APP" => Ok(SourceType::InApp),
            other => Err(UnknownSourceType(other.to_string())),
        }
    }
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `IntakeIssueStatus` (`db/models/intake.py:42-47`):
/// `PENDING = -2`, `REJECTED = -1`, `SNOOZED = 0`, `ACCEPTED = 1`,
/// `DUPLICATE = 2`. Application-level only; the column is a plain
/// integer with no `CHECK` constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntakeIssueStatus {
    Pending,
    Rejected,
    Snoozed,
    Accepted,
    Duplicate,
}

impl IntakeIssueStatus {
    /// The stored integer (`IntakeIssueStatus.PENDING.value` etc.).
    pub fn as_i32(self) -> i32 {
        match self {
            IntakeIssueStatus::Pending => -2,
            IntakeIssueStatus::Rejected => -1,
            IntakeIssueStatus::Snoozed => 0,
            IntakeIssueStatus::Accepted => 1,
            IntakeIssueStatus::Duplicate => 2,
        }
    }

    /// Parse a stored integer; `None` for values Django never writes.
    pub fn from_i32(value: i32) -> Option<IntakeIssueStatus> {
        match value {
            -2 => Some(IntakeIssueStatus::Pending),
            -1 => Some(IntakeIssueStatus::Rejected),
            0 => Some(IntakeIssueStatus::Snoozed),
            1 => Some(IntakeIssueStatus::Accepted),
            2 => Some(IntakeIssueStatus::Duplicate),
            _ => None,
        }
    }

    /// All five values in declaration order.
    pub const ALL: &[IntakeIssueStatus] = &[
        IntakeIssueStatus::Pending,
        IntakeIssueStatus::Rejected,
        IntakeIssueStatus::Snoozed,
        IntakeIssueStatus::Accepted,
        IntakeIssueStatus::Duplicate,
    ];

    /// The human label (`choices=` label, `intake.py:55-61`).
    pub fn label(self) -> &'static str {
        match self {
            IntakeIssueStatus::Pending => "Pending",
            IntakeIssueStatus::Rejected => "Rejected",
            IntakeIssueStatus::Snoozed => "Snoozed",
            IntakeIssueStatus::Accepted => "Accepted",
            IntakeIssueStatus::Duplicate => "Duplicate",
        }
    }
}

/// `intakes` table (`db/models/intake.py:12-35`,
/// `db_table = "intakes"`), api-v1 view.
pub mod intake {
    use super::OnDelete;

    /// Physical table (`Meta.db_table`, `intake.py:34`).
    pub const TABLE: &str = "intakes";

    /// Soft-delete read view (`intakes_active`).
    pub const VIEW: &str = "intakes_active";

    /// Default `ORDER BY` (`Meta.ordering = ("name",)`, `intake.py:35`).
    pub const ORDERING: &str = "name";

    /// `Meta.unique_together` (`intake.py:24`): Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["name", "project", "deleted_at"];

    /// Partial unique constraint backing the live-name scope
    /// (`intake.py:25-31`).
    pub const UNIQUE_NAME_PROJECT_NAME: &str = "intake_unique_name_project_when_deleted_at_null";
    /// Columns of [`UNIQUE_NAME_PROJECT_NAME`] (`intake.py:27`).
    pub const UNIQUE_NAME_PROJECT_COLUMNS: &[&str] = &["name", "project_id"];
    /// `WHERE` of the partial unique index (`intake.py:28`,
    /// `deleted_at__isnull=True`): the name is unique per project among
    /// live rows only.
    pub const UNIQUE_NAME_PROJECT_WHERE: &str = "deleted_at IS NULL";

    /// Columns in Django `_meta` field order: [`super::BASE_COLUMNS`],
    /// then `project_id`/`workspace_id` (`ProjectBaseModel`,
    /// `db/models/project.py:302-311`), then `intake.py:13-17`. FK
    /// entries use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "name",
        "description",
        "is_default",
        "view_props",
        "logo_props",
    ];

    /// Django field defaults in [`COLUMNS`] order (`None` = no default).
    /// `description` stores `""` (`TextField(blank=True)` without
    /// `null=True`, `:14`); `is_default` defaults to `false` (`:15`);
    /// `view_props`/`logo_props` default to `dict` (`:16-17`); `id` is a
    /// `uuid4` primary key (`base.py:18`).
    pub const DEFAULTS: &[Option<&str>] = &[
        None,          // id (uuid4 pk)
        None,          // created_at (auto_now_add)
        None,          // updated_at (auto_now)
        None,          // created_by_id (SET_NULL)
        None,          // updated_by_id (SET_NULL)
        None,          // deleted_at
        None,          // project_id (CASCADE, required)
        None,          // workspace_id (CASCADE, required)
        None,          // name (varchar(255), required)
        Some("\"\""),  // description (text blank, never NULL)
        Some("false"), // is_default
        Some("dict"),  // view_props (JSONField)
        Some("dict"),  // logo_props (JSONField)
    ];

    /// `Meta.indexes`: none (`intake.py:23-35` declares no indexes).
    pub const INDEXES: &[super::Index] = &[];

    /// `name` bound (`intake.py:13`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;

    /// `is_default` default (`intake.py:15`).
    pub const DEFAULT_IS_DEFAULT: bool = false;

    /// `project` FK: `CASCADE`, required (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE`, required (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// One `intakes` row. `description` stores `""`, never `NULL`
    /// (`blank=True` without `null=True`, `intake.py:14`); `view_props`
    /// and `logo_props` store `{}`, never `NULL`
    /// (`JSONField(default=dict)`, `intake.py:16-17`).
    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct Intake {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub name: String,
        pub description: String,
        pub is_default: bool,
        pub view_props: serde_json::Value,
        pub logo_props: serde_json::Value,
    }
}

/// `intake_issues` table (`db/models/intake.py:50-84`,
/// `db_table = "intake_issues"`), api-v1 view.
pub mod intake_issue {
    use super::{IntakeIssueStatus, OnDelete, SourceType};

    /// Physical table (`Meta.db_table`, `intake.py:79`).
    pub const TABLE: &str = "intake_issues";

    /// Soft-delete read view (`intake_issues_active`).
    pub const VIEW: &str = "intake_issues_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`,
    /// `intake.py:80`).
    pub const ORDERING: &str = "-created_at";

    /// `Meta.unique_together`: none (`intake.py:76-80` declares no
    /// `unique_together` and no `constraints`).
    pub const UNIQUE_TOGETHER: &[&str] = &[];

    /// `Meta.indexes`: none (`intake.py:76-80` declares no indexes).
    pub const INDEXES: &[super::Index] = &[];

    /// Columns in Django `_meta` field order: [`super::BASE_COLUMNS`],
    /// then `project_id`/`workspace_id` (`ProjectBaseModel`), then
    /// `intake.py:51-74`. FK entries use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "intake_id",
        "issue_id",
        "status",
        "snoozed_till",
        "duplicate_to_id",
        "source",
        "source_email",
        "external_source",
        "external_id",
        "extra",
    ];

    /// Django field defaults in [`COLUMNS`] order (`None` = no default).
    /// `status` defaults to `-2` (pending, `:61`); `source` defaults to
    /// `"IN_APP"` (`:70`, nullable); `extra` defaults to `dict` (`:74`).
    pub const DEFAULTS: &[Option<&str>] = &[
        None,               // id (uuid4 pk)
        None,               // created_at (auto_now_add)
        None,               // updated_at (auto_now)
        None,               // created_by_id (SET_NULL)
        None,               // updated_by_id (SET_NULL)
        None,               // deleted_at
        None,               // project_id (CASCADE, required)
        None,               // workspace_id (CASCADE, required)
        None,               // intake_id (CASCADE, required)
        None,               // issue_id (CASCADE, required)
        Some("-2"),         // status (int choices)
        None,               // snoozed_till (timestamptz null)
        None,               // duplicate_to_id (SET_NULL null)
        Some("\"IN_APP\""), // source (varchar(255) null blank)
        None,               // source_email (text null blank)
        None,               // external_source (varchar(255) null blank)
        None,               // external_id (varchar(255) null blank)
        Some("dict"),       // extra (JSONField)
    ];

    /// `status` default (`intake.py:61`): pending.
    pub const DEFAULT_STATUS: i32 = -2;
    /// [`DEFAULT_STATUS`] as the enum value.
    pub const DEFAULT_STATUS_ENUM: IntakeIssueStatus = IntakeIssueStatus::Pending;
    /// `source` default (`intake.py:70`, `default="IN_APP"`).
    pub const DEFAULT_SOURCE: &str = "IN_APP";
    /// [`DEFAULT_SOURCE`] as the enum value.
    pub const DEFAULT_SOURCE_ENUM: SourceType = SourceType::InApp;

    /// `source` bound (`intake.py:70`, `max_length=255`).
    pub const SOURCE_MAX_LENGTH: usize = 255;
    /// `external_source` bound (`intake.py:72`, `max_length=255`).
    pub const EXTERNAL_SOURCE_MAX_LENGTH: usize = 255;
    /// `external_id` bound (`intake.py:73`, `max_length=255`).
    pub const EXTERNAL_ID_MAX_LENGTH: usize = 255;

    /// `intake` FK: `CASCADE`, required (`intake.py:51`).
    pub const INTAKE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK: `CASCADE`, required (`intake.py:52`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `duplicate_to` FK: `SET_NULL`, nullable (`intake.py:64-69`).
    pub const DUPLICATE_TO_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `project` FK: `CASCADE`, required (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE`, required (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// One `intake_issues` row. Python `IntegerField` (unbounded) maps
    /// to `i32` for `status` (values -2..2, see Semantic traps);
    /// `source` is nullable with an `"IN_APP"` application default;
    /// `extra` stores `{}`, never `NULL`.
    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct IntakeIssue {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub intake_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub status: i32,
        pub snoozed_till: Option<chrono::DateTime<chrono::Utc>>,
        pub duplicate_to_id: Option<uuid::Uuid>,
        pub source: Option<String>,
        pub source_email: Option<String>,
        pub external_source: Option<String>,
        pub external_id: Option<String>,
        pub extra: serde_json::Value,
    }
}

#[cfg(test)]
mod tests {
    use super::file_asset as fa;
    use super::intake as it;
    use super::intake_issue as ii;
    use super::sticky as st;
    use super::{IntakeIssueStatus, SourceType, BASE_COLUMNS};

    fn fixture(name: &str) -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/v1_assets")
            .join(name);
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
        serde_json::from_str(&body).expect("fixture is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Full expected column list: the six base columns plus the
    /// fixture's model-declared columns (each entry's `name`).
    fn expected_columns(fixture: &serde_json::Value) -> Vec<String> {
        let mut cols: Vec<String> = owned(BASE_COLUMNS);
        for entry in fixture["columns"]
            .as_array()
            .expect("fixture columns must be an array")
        {
            cols.push(
                entry["name"]
                    .as_str()
                    .expect("column entry must carry a name")
                    .to_owned(),
            );
        }
        cols
    }

    #[test]
    fn fileasset_columns_match_fixture() {
        let fixture = fixture("fx-model-fileasset.json");
        assert_eq!(fixture["model"].as_str().unwrap(), "FileAsset");
        assert_eq!(fixture["db_table"].as_str().unwrap(), fa::TABLE);
        assert_eq!(fixture["meta"]["db_table"].as_str().unwrap(), fa::TABLE);
        let ordering: Vec<String> = fixture["ordering"]
            .as_array()
            .expect("ordering must be an array")
            .iter()
            .map(|v| v.as_str().expect("ordering entry").to_owned())
            .collect();
        assert_eq!(ordering, [fa::ORDERING.to_owned()]);
        assert_eq!(owned(fa::COLUMNS), expected_columns(&fixture));
        assert_eq!(fa::DEFAULTS.len(), fa::COLUMNS.len());
        let index_names: Vec<String> = fa::INDEXES.iter().map(|i| i.name.to_string()).collect();
        let fixture_names: Vec<String> = fixture["indexes"]
            .as_array()
            .expect("indexes must be an array")
            .iter()
            .map(|i| i["name"].as_str().expect("index name").to_owned())
            .collect();
        assert_eq!(index_names, fixture_names);
        for (rust, fx) in fa::INDEXES
            .iter()
            .zip(fixture["indexes"].as_array().unwrap())
        {
            let fields: Vec<String> = fx["fields"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f.as_str().unwrap().to_owned())
                .collect();
            assert_eq!(owned(rust.columns), fields, "index {}", rust.name);
        }
        let entity_types: Vec<String> = fixture["entity_types"]
            .as_array()
            .expect("entity_types must be an array")
            .iter()
            .map(|v| v.as_str().expect("entity type").to_owned())
            .collect();
        assert_eq!(owned(fa::ENTITY_TYPES), entity_types);
    }

    #[test]
    fn sticky_columns_match_fixture() {
        let fixture = fixture("fx-model-sticky.json");
        assert_eq!(fixture["model"].as_str().unwrap(), "Sticky");
        assert_eq!(fixture["db_table"].as_str().unwrap(), st::TABLE);
        assert_eq!(fixture["meta"]["db_table"].as_str().unwrap(), st::TABLE);
        let ordering: Vec<String> = fixture["ordering"]
            .as_array()
            .expect("ordering must be an array")
            .iter()
            .map(|v| v.as_str().expect("ordering entry").to_owned())
            .collect();
        assert_eq!(ordering, [st::ORDERING.to_owned()]);
        assert_eq!(owned(st::COLUMNS), expected_columns(&fixture));
        assert_eq!(st::DEFAULTS.len(), st::COLUMNS.len());
        assert!(st::INDEXES.is_empty(), "Sticky declares no indexes");
    }

    #[test]
    fn sticky_save_stripped_goldens() {
        let fixture = fixture("fx-model-sticky.json");
        let goldens = fixture["save"]["description_stripped"]["goldens"]
            .as_array()
            .expect("stripped goldens");
        assert!(!goldens.is_empty());
        for (i, golden) in goldens.iter().enumerate() {
            let html = golden["in"]["description_html"].as_str();
            let expected = golden["out"].as_str().map(str::to_owned);
            assert_eq!(
                st::stripped_description(html),
                expected,
                "stripped golden {i}"
            );
        }
    }

    #[test]
    fn sticky_save_sort_order_goldens() {
        let fixture = fixture("fx-model-sticky.json");
        let goldens = fixture["save"]["create_sequence"]["goldens"]
            .as_array()
            .expect("sequence goldens");
        assert!(!goldens.is_empty());
        for (i, golden) in goldens.iter().enumerate() {
            let existing_max = golden["in"]["existing_max"].as_f64();
            let expected = golden["out_sort_order"].as_f64().expect("out_sort_order");
            assert_eq!(
                st::sort_order_on_create(existing_max),
                expected,
                "sequence golden {i}"
            );
        }
    }

    #[test]
    fn intake_columns_match_fixture() {
        let fixture = fixture("fx-model-intake.json");
        let intake = &fixture["Intake"];
        assert_eq!(intake["meta"]["db_table"].as_str().unwrap(), it::TABLE);
        let ordering: Vec<String> = intake["meta"]["ordering"]
            .as_array()
            .expect("ordering must be an array")
            .iter()
            .map(|v| v.as_str().expect("ordering entry").to_owned())
            .collect();
        assert_eq!(ordering, [it::ORDERING.to_owned()]);
        let mut expected: Vec<String> = owned(BASE_COLUMNS);
        expected.push("project_id".to_owned());
        expected.push("workspace_id".to_owned());
        for entry in intake["columns"].as_array().expect("columns") {
            expected.push(entry["name"].as_str().expect("name").to_owned());
        }
        assert_eq!(owned(it::COLUMNS), expected);
        assert_eq!(it::DEFAULTS.len(), it::COLUMNS.len());
        let unique_together: Vec<String> = intake["meta"]["unique_together"]
            .as_array()
            .expect("unique_together")
            .iter()
            .map(|v| v.as_str().expect("entry").to_owned())
            .collect();
        assert_eq!(unique_together, owned(it::UNIQUE_TOGETHER));
        let constraint = &intake["meta"]["constraints"];
        assert_eq!(constraint.as_array().unwrap().len(), 1);
        assert_eq!(
            constraint[0]["name"].as_str().unwrap(),
            it::UNIQUE_NAME_PROJECT_NAME
        );
        let fields: Vec<String> = constraint[0]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap().to_owned())
            .collect();
        // Django field names (`name`, `project`); the attname form lives
        // in [`it::UNIQUE_NAME_PROJECT_COLUMNS`].
        assert_eq!(fields, ["name".to_owned(), "project".to_owned()]);
        assert_eq!(
            owned(it::UNIQUE_NAME_PROJECT_COLUMNS),
            ["name".to_owned(), "project_id".to_owned()]
        );
        assert_eq!(
            constraint[0]["condition"].as_str().unwrap(),
            it::UNIQUE_NAME_PROJECT_WHERE
        );
    }

    #[test]
    fn intake_issue_columns_match_fixture() {
        let fixture = fixture("fx-model-intake.json");
        let issue = &fixture["IntakeIssue"];
        assert_eq!(issue["meta"]["db_table"].as_str().unwrap(), ii::TABLE);
        let ordering: Vec<String> = issue["meta"]["ordering"]
            .as_array()
            .expect("ordering must be an array")
            .iter()
            .map(|v| v.as_str().expect("ordering entry").to_owned())
            .collect();
        assert_eq!(ordering, [ii::ORDERING.to_owned()]);
        let mut expected: Vec<String> = owned(BASE_COLUMNS);
        expected.push("project_id".to_owned());
        expected.push("workspace_id".to_owned());
        for entry in issue["columns"].as_array().expect("columns") {
            expected.push(entry["name"].as_str().expect("name").to_owned());
        }
        assert_eq!(owned(ii::COLUMNS), expected);
        assert_eq!(ii::DEFAULTS.len(), ii::COLUMNS.len());
        assert!(ii::INDEXES.is_empty(), "IntakeIssue declares no indexes");
        assert!(ii::UNIQUE_TOGETHER.is_empty(), "no unique_together");
    }

    #[test]
    fn intake_enums_match_fixture() {
        let fixture = fixture("fx-model-intake.json");
        let status = &fixture["IntakeIssueStatus"]["choices"];
        let expected = [
            ("PENDING", -2),
            ("REJECTED", -1),
            ("SNOOZED", 0),
            ("ACCEPTED", 1),
            ("DUPLICATE", 2),
        ];
        assert_eq!(IntakeIssueStatus::ALL.len(), expected.len());
        for (i, (name, value)) in expected.iter().enumerate() {
            let variant = IntakeIssueStatus::ALL[i];
            assert_eq!(variant.as_i32(), *value, "{name}");
            assert_eq!(IntakeIssueStatus::from_i32(*value), Some(variant));
        }
        assert_eq!(status["PENDING"].as_i64().unwrap(), -2);
        assert_eq!(status["REJECTED"].as_i64().unwrap(), -1);
        assert_eq!(status["SNOOZED"].as_i64().unwrap(), 0);
        assert_eq!(status["ACCEPTED"].as_i64().unwrap(), 1);
        assert_eq!(status["DUPLICATE"].as_i64().unwrap(), 2);
        assert_eq!(IntakeIssueStatus::from_i32(3), None);
        let source = &fixture["SourceType"]["choices"];
        assert_eq!(source["IN_APP"].as_str().unwrap(), "IN_APP");
        assert_eq!(SourceType::InApp.as_str(), "IN_APP");
        assert_eq!("IN_APP".parse::<SourceType>(), Ok(SourceType::InApp));
        assert!("EMAIL".parse::<SourceType>().is_err());
        assert_eq!(SourceType::InApp.to_string(), "IN_APP");
        assert_eq!(ii::DEFAULT_STATUS, IntakeIssueStatus::Pending.as_i32());
        assert_eq!(ii::DEFAULT_SOURCE, SourceType::InApp.as_str());
    }

    #[test]
    fn display_shapes() {
        let asset = fa::FileAsset {
            id: uuid::Uuid::nil(),
            created_at: chrono::DateTime::<chrono::Utc>::MIN_UTC,
            updated_at: chrono::DateTime::<chrono::Utc>::MIN_UTC,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            attributes: serde_json::Value::Object(Default::default()),
            asset: "acme/abc-image.png".to_owned(),
            user_id: None,
            workspace_id: None,
            draft_issue_id: None,
            project_id: None,
            issue_id: None,
            comment_id: None,
            page_id: None,
            entity_type: None,
            entity_identifier: None,
            is_deleted: false,
            is_archived: false,
            external_id: None,
            external_source: None,
            size: 0.0,
            is_uploaded: false,
            storage_metadata: None,
        };
        // `__str__` is the storage key, not the id.
        assert_eq!(asset.to_string(), "acme/abc-image.png");
        let named = st::Sticky {
            id: uuid::Uuid::nil(),
            created_at: chrono::DateTime::<chrono::Utc>::MIN_UTC,
            updated_at: chrono::DateTime::<chrono::Utc>::MIN_UTC,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            name: Some("hello".to_owned()),
            description: serde_json::Value::Object(Default::default()),
            description_html: "<p></p>".to_owned(),
            description_stripped: None,
            description_binary: None,
            logo_props: serde_json::Value::Object(Default::default()),
            color: None,
            background_color: None,
            workspace_id: uuid::Uuid::nil(),
            owner_id: uuid::Uuid::nil(),
            sort_order: st::DEFAULT_SORT_ORDER,
        };
        assert_eq!(named.to_string(), "hello");
        let unnamed = st::Sticky {
            name: None,
            ..named.clone()
        };
        // Null names render `"None"`, exactly like `str(None)`.
        assert_eq!(unnamed.to_string(), "None");
    }

    /// Probe-anchored `strip_tags_ml` vectors (Python 3.12.14,
    /// `/tmp/probe6.py` + `/tmp/probe5.py`): every entry is observed
    /// `MLStripper` output, not a hand expectation.
    #[test]
    fn strip_tags_ml_probe_vectors() {
        let cases = [
            ("", ""),
            ("<p></p>", ""),
            ("<p>hello <b>world</b></p>", "hello world"),
            ("<!-- hi -->x", "x"),
            ("<!-- hi", ""),
            ("<!DOCTYPE html>x", "x"),
            ("<![CDATA[x]]>y", "y"),
            ("<?php echo 1; ?>z", "z"),
            ("<script>a<b>&amp;</script>c", "a<b>&amp;c"),
            ("<style>a>b</style>d", "a>bd"),
            ("<SCRIPT>e</SCRIPT>f", "ef"),
            ("<xmp>a<b>&amp;</xmp>c", "a<b>&amp;c"),
            ("<title>a&amp;b</title>c", "a&bc"),
            ("<title>a</b>b</title>c", "a</b>bc"),
            ("<title>a</title x>b</title>c", "abc"),
            ("<textarea>&lt;x&gt;</textarea>y", "<x>y"),
            ("<noscript>x</noscript>y", "xy"),
            ("<plaintext>a<b", "a<b"),
            ("<plaintext>a&amp;b", "a&amp;b"),
            ("<script>x</b>y</script>z", "x</b>yz"),
            ("<script>a</script x>b</script>c", "abc"),
            ("<script>abc", ""),
            ("<script>a</SCRIPT>b", "ab"),
            ("<div<span>", ""),
            ("</div class=\"a>b\">", ""),
            ("</div foo=bar>baz", "baz"),
            ("<a href=\"x\">y</a>", "y"),
            ("<a href=\"x>y\">z</a>", "z"),
            ("<a href=x>y</a>", "y"),
            ("<a b=c\"d>e", "e"),
            ("<a =x>y</a>", "y"),
            ("<a href='unclosed>y", ""),
            ("<a/>b", "b"),
            ("<a b/ >c", "c"),
            ("<br>", ""),
            ("<br", ""),
            ("a < b", "a < b"),
            ("a <b c", "a "),
            ("<<x>>", "<>"),
            ("<3", "<3"),
            ("</>", ""),
            ("</ div>", ""),
            ("</br/>", ""),
            ("<DIV>X</DIV>", "X"),
            ("<p>a</p ><p>b</p>", "ab"),
            ("R&D", ""),
            ("R&D ", "R&D "),
            ("<div><span>", ""),
            ("<p>a<br", "a"),
            ("<p>a</p", "a"),
            ("<!-->", ""),
            ("<!--->", ""),
            ("<!--->x", "x"),
            ("<!-- a --!>z", "z"),
            ("<?a>b?>c", "b?>c"),
            ("<?a", ""),
            ("<!DOCTYPE html PUBLIC 'x>y'>z", "y'>z"),
            ("<![CDATA[>]]>v", "v"),
            ("<![foo]bar>u", "u"),
            ("<![foo>q", "q"),
            ("<!foo>p", "p"),
            ("<!foo", ""),
            ("<\x00>", "<\x00>"),
            ("<a\x1c>", ""),
            ("<a\x1c >", ""),
            ("<a\u{85}>", ""),
            ("<a\u{a0}>", ""),
            ("<a\u{a0}/>", ""),
            ("<a\u{202f}>", ""),
        ];
        for (input, expected) in cases {
            assert_eq!(st::strip_tags_ml(input), expected, "input {input:?}");
        }
    }

    /// Probe-anchored `unescape` vectors (Python 3.12.14 `html.unescape`).
    #[test]
    fn unescape_probe_vectors() {
        let cases = [
            ("&amp; &lt; &gt; &quot; &apos; &nbsp;", "& < > \" ' \u{a0}"),
            ("&#65;&#x41;&#X42;", "AAB"),
            ("&copy; &frac12; &fjlig;", "© ½ fj"),
            ("&bogus; &; &amp", "&bogus; &; &"),
            ("&amp;amp;", "&amp;"),
            ("&amp &amp;", "& &"),
            ("&notit; &not in", "¬it; ¬ in"),
            ("&notit x", "¬it x"),
            ("&notin; y", "∉ y"),
            ("&#38;#38;", "&#38;"),
            ("&#x26;#38;", "&#38;"),
            ("&#0;", "�"),
            ("&#13;", "\r"),
            ("&#128;", "€"),
            ("&#8;", ""),
            ("&#127;", ""),
            ("&#64976;", ""),
            ("&#173;", "\u{ad}"),
            ("&#x1F600;", "😀"),
            ("&#x110000;", "�"),
            ("&#1114111;", ""),
            ("&#1114112;", "�"),
            ("&AMP;&LT;&GT;&QUOT;", "&<>\""),
            ("& Razor", "& Razor"),
            ("&bogusx y", "&bogusx y"),
            ("&\rb", "&\rb"),
            ("&#12x;", "\x0cx;"),
            ("&#x4z;", "z;"),
            ("&#X41;", "A"),
            ("a&ampb c", "a&b c"),
            ("&ampb c", "&b c"),
        ];
        for (input, expected) in cases {
            assert_eq!(st::unescape(input), expected, "input {input:?}");
        }
    }
}
