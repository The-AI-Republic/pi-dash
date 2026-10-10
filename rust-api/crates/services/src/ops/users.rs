//! Users + membership ops commands (D-37, F37-03/F37-04).
//!
//! Pure kernels for `db/management/commands/{activate_user,
//! reset_password,create_instance_admin,create_project_member,
//! create_dummy_data}.py`: success/error message builders, the JSON
//! model-default documents, `User.save()` semantics, the PBKDF2 + zxcvbn
//! password primitives, the `int()` parser, and the stderr renderers for
//! every failure shape (uncaught `CommandError`, the getpass fallback
//! bytes, and the traceback skeletons for the crash branches).
//!
//! Byte conventions (Django 4.2, piped stdio — recorded against the
//! oracle 2026-10-10): `self.stdout.write(s)` / `print(x)` emit `s` +
//! `\n` on stdout; `self.stderr.write(s)` emits `s` + `\n` on stderr;
//! an uncaught `CommandError(msg)` emits `CommandError: {msg}` + `\n`
//! on stderr with exit 1; `self.style.*` is the identity when piped.
//! Every builder below returns content WITHOUT the trailing newline;
//! the `bin` layer terminates each line. Prompts carry no newline.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-1 (`activate_user.py:32`, `reset_password.py:39`): `does not
//!   exists` grammar. Kept verbatim.
//! * BUG-2 (`create_project_member.py:37`): `options.get('role', 20)`
//!   never sees its default — argparse stores `None` for an absent
//!   `--role`, so the role is `None` (`Role: None`, `NULL` write).
//! * BUG-3 (`create_project_member.py:70-72`): the `except CommandError`
//!   handler passes the exception object to `style.ERROR`, which is the
//!   identity when piped, so `stdout.write` raises `AttributeError`
//!   (`'CommandError' object has no attribute 'endswith'`) — every
//!   `CommandError` in this command crashes with exit 1 instead of
//!   printing. The port renders the chained-traceback skeleton.
//! * BUG-4 (`dummy_data_task.py:153`, via the synchronous task call):
//!   `create_cycles` writes `cycle_count + 1` rows. Owned by the task
//!   port; the command passes counts through untouched.
//! * BUG-5 (`create_dummy_data.py:59-70`): the task runs as a plain
//!   synchronous call, yet the command reports `Data is pushed to the
//!   queue`. Both the direct call and the message are ported.
//!
//! Deliberate deviations (documented, not bugs):
//!
//! * Traceback frames (file paths, line numbers, carets, source
//!   echoes) embed the checkout/venv layout, so the port renders the
//!   deterministic skeleton — `Traceback (most recent call last):`
//!   headers, the exception lines, the chaining sentence and the blank
//!   separator lines — and drops the frames.
//! * `DETAIL: Failing row contains (...)` embeds fresh timestamps and
//!   UUIDs (nondeterministic even between two Python runs); the shape
//!   is ported, the row content is asserted by pattern.
//! * The `GetPassWarning` header and Django `RuntimeWarning`s are
//!   interpreter noise (stdlib/venv paths); only the program bytes
//!   (`Warning: Password input may be echoed.`, the prompts) are
//!   ported.
//! * `int()` accepts non-ASCII decimal digits and Unicode whitespace
//!   padding; the port accepts ASCII only (same simplification as the
//!   `integrations` `py_int_from_str` precedent).
//! * argparse usage/error texts (`manage.py ...`, exit 2) belong to a
//!   different binary; the `ops` CLI is clap and renders clap errors.

use chrono::{DateTime, Utc};
use pidash_db::ops::users::UserSaveWrite;

// ---------------------------------------------------------------------------
// Message builders
// ---------------------------------------------------------------------------

/// `User activated successfully` (`activate_user.py:38`).
pub const ACTIVATE_SUCCESS: &str = "User activated successfully";

/// `User password updated successfully` (`reset_password.py:66`).
pub const RESET_PASSWORD_SUCCESS: &str = "User password updated successfully";

/// `Successfully created the admin` (`create_instance_admin.py:40`).
pub const INSTANCE_ADMIN_SUCCESS: &str = "Successfully created the admin";

/// `Data is pushed to the queue` (`create_dummy_data.py:70`).
pub const DUMMY_DATA_SUCCESS: &str = "Data is pushed to the queue";

/// `Role: {role}` via plain `print` (`create_project_member.py:39`).
/// `None` renders `None`, exactly like Python `str(None)`.
pub fn role_line(role: Option<i64>) -> String {
    match role {
        Some(value) => format!("Role: {value}"),
        None => "Role: None".to_owned(),
    }
}

/// `User {email} added to project {project_id}`
/// (`create_project_member.py:68`).
pub fn member_added_line(user_email: &str, project_id: &str) -> String {
    format!("User {user_email} added to project {project_id}")
}

/// `Command errored out {e}` (`create_dummy_data.py:73`).
pub fn dummy_data_error_line(error: &str) -> String {
    format!("Command errored out {error}")
}

/// `Please provide the following details for project {i + 1}:`
/// (`create_dummy_data.py:50` — 1-based display of the 0-based loop).
pub fn project_details_line(one_based_index: usize) -> String {
    format!("Please provide the following details for project {one_based_index}:")
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

/// `input("Workspace Name: ")` (`create_dummy_data.py:18`).
pub const PROMPT_WORKSPACE_NAME: &str = "Workspace Name: ";
/// `input("Workspace slug: ")` (`create_dummy_data.py:19`).
pub const PROMPT_WORKSPACE_SLUG: &str = "Workspace slug: ";
/// `input("Your email: ")` (`create_dummy_data.py:27`).
pub const PROMPT_CREATOR_EMAIL: &str = "Your email: ";
/// `input("Enter Member emails (comma separated): ")`
/// (`create_dummy_data.py:34`).
pub const PROMPT_MEMBER_EMAILS: &str = "Enter Member emails (comma separated): ";
/// `int(input("Number of projects to be created: "))`
/// (`create_dummy_data.py:47`).
pub const PROMPT_PROJECT_COUNT: &str = "Number of projects to be created: ";
/// Per-project `int(input(...))` prompts (`create_dummy_data.py:51-55`).
pub const PROMPT_ISSUE_COUNT: &str = "Number of issues to be created: ";
/// `create_dummy_data.py:52`.
pub const PROMPT_CYCLE_COUNT: &str = "Number of cycles to be created: ";
/// `create_dummy_data.py:53`.
pub const PROMPT_MODULE_COUNT: &str = "Number of modules to be created: ";
/// `create_dummy_data.py:54`.
pub const PROMPT_PAGES_COUNT: &str = "Number of pages to be created: ";
/// `create_dummy_data.py:55`.
pub const PROMPT_INTAKE_ISSUE_COUNT: &str = "Number of intake issues to be created: ";

impl DummyPrompts {
    /// Shorthand for the five per-project count prompts in loop order.
    pub fn count_prompts() -> [&'static str; 5] {
        [
            PROMPT_ISSUE_COUNT,
            PROMPT_CYCLE_COUNT,
            PROMPT_MODULE_COUNT,
            PROMPT_PAGES_COUNT,
            PROMPT_INTAKE_ISSUE_COUNT,
        ]
    }
}

/// Marker namespace for the dummy-data prompt grouping.
pub struct DummyPrompts {
    _private: (),
}

/// `getpass.getpass('Password: ')` (`reset_password.py:43`).
pub const GETPASS_PASSWORD: &str = "Password: ";
/// `getpass.getpass('Password (again): ')` (`reset_password.py:44`).
pub const GETPASS_PASSWORD_AGAIN: &str = "Password (again): ";
/// `fallback_getpass` warning line (CPython `getpass.py`): printed to
/// stderr before each prompt when echo cannot be controlled.
pub const GETPASS_ECHO_WARNING: &str = "Warning: Password input may be echoed.";

// ---------------------------------------------------------------------------
// Python `str.strip()` + `User.save()` email semantics
// ---------------------------------------------------------------------------

/// Python `str.strip()` with no args: Unicode whitespace plus the four
/// ASCII control chars (`\x1c`-`\x1f`) CPython's `Py_UNICODE_ISSPACE`
/// accepts but Rust's `char::is_whitespace` does not. Same recipe as
/// the `app_workspace` `py_strip` precedent.
pub fn py_strip(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

/// `User.save()` (`db/models/user.py:169-182`): `email.lower().strip()`
/// plus the `display_name` fill. `split("@")` never returns an empty
/// list, so the `else` (random letters) is dead — the fill is always
/// the pre-`@` segment, which may itself be empty.
pub fn normalize_saved_email(email: &str) -> String {
    py_strip(&email.to_lowercase()).to_owned()
}

/// The `display_name` fill for a user whose stored name is empty.
pub fn display_name_for_saved_email(normalized_email: &str) -> String {
    normalized_email.split('@').next().unwrap_or("").to_owned()
}

/// Inputs `User.save()` reads beyond the command's own field writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSaveInput {
    pub email: String,
    pub display_name: String,
    pub is_staff: bool,
    pub is_superuser: bool,
    pub token: String,
    pub token_updated_at: Option<DateTime<Utc>>,
}

/// The `User.save()` outputs the commands persist (`user.py:169-187`):
/// normalized email, filled display name, staff escalation, and the
/// token rotation (`uuid4hex` x 2 + `token_updated_at = now`) which
/// runs exactly when `token_updated_at` is already set. The struct
/// lives in the `db` layer (it is the UPDATE's write model) and is
/// computed here.
pub fn apply_user_save(
    input: &UserSaveInput,
    fresh_token: &str,
    now: DateTime<Utc>,
) -> UserSaveWrite {
    let email = normalize_saved_email(&input.email);
    let display_name = if input.display_name.is_empty() {
        display_name_for_saved_email(&email)
    } else {
        input.display_name.clone()
    };
    let is_staff = input.is_staff || input.is_superuser;
    let (token, token_updated_at) = match input.token_updated_at {
        Some(_) => (fresh_token.to_owned(), Some(now)),
        None => (input.token.clone(), None),
    };
    UserSaveWrite {
        email,
        display_name,
        is_staff,
        token,
        token_updated_at,
    }
}

/// `uuid.uuid4().hex + uuid.uuid4().hex` (`user.py:174`): 64 lowercase
/// hex chars.
pub fn new_user_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

// ---------------------------------------------------------------------------
// Password primitives (`reset_password.py:56-64`)
// ---------------------------------------------------------------------------

/// PBKDF2 iteration count Django 4.2 writes for newly set passwords.
pub const PBKDF2_ITERATIONS: u32 = 600_000;
/// Salt length `make_password` generates (22 alphanumerics).
pub const PASSWORD_SALT_LEN: usize = 22;
/// `zxcvbn(password)["score"] < 3` rejects (`reset_password.py:58`).
pub const PASSWORD_MIN_SCORE: u8 = 3;

/// `make_password(password)`: fresh 22-alphanumeric salt (same UUID
/// recipe as the `auth_session` `encode_password` precedent) + the
/// read-only auth kernel's `hash_password` at the pinned count.
pub fn encode_password(password: &str) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut salt = String::with_capacity(PASSWORD_SALT_LEN);
    while salt.len() < PASSWORD_SALT_LEN {
        for byte in uuid::Uuid::new_v4().as_bytes() {
            if salt.len() == PASSWORD_SALT_LEN {
                break;
            }
            salt.push(ALPHABET[*byte as usize % ALPHABET.len()] as char);
        }
    }
    pidash_auth::password::hash_password(password, &salt, PBKDF2_ITERATIONS)
}

/// `zxcvbn(password)["score"]` on the 0-4 scale.
pub fn password_score(password: &str) -> u8 {
    zxcvbn::zxcvbn(password, &[]).score() as u8
}

/// `results["score"] < 3` (`reset_password.py:58`).
pub fn is_weak_password(score: u8) -> bool {
    score < PASSWORD_MIN_SCORE
}

// ---------------------------------------------------------------------------
// `int()` parser (`--role` via argparse `type=int`, dummy counts via
// `int(input(...))`)
// ---------------------------------------------------------------------------

/// `int(s, 10)` over ASCII: surrounding ASCII whitespace, one leading
/// sign, digits with single underscores between digits
/// (`[0-9](_?[0-9])*` — `1_5` parses, `1__5`/`_5`/`5_`/`+_5` do not).
/// Overflow past `i64` (Python ints are unbounded) and non-ASCII
/// digits fail with the same literal error.
pub fn py_int(raw: &str) -> Result<i64, String> {
    if let Some(value) = py_int_value(raw) {
        return Ok(value);
    }
    Err(format!("invalid literal for int() with base 10: '{raw}'"))
}

fn py_int_value(raw: &str) -> Option<i64> {
    let trimmed = raw.trim_matches(|c: char| c.is_ascii_whitespace());
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    let mut chars = digits.chars().peekable();
    match chars.next() {
        Some(first) if first.is_ascii_digit() => {}
        _ => return None,
    }
    let mut canonical = String::with_capacity(digits.len());
    let mut prev_underscore = false;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else if ch.is_ascii_digit() {
            prev_underscore = false;
            canonical.push(ch);
        } else {
            return None;
        }
    }
    if prev_underscore {
        return None;
    }
    let signed = if trimmed.starts_with('-') {
        format!("-{canonical}")
    } else {
        canonical
    };
    signed.parse::<i64>().ok()
}

// ---------------------------------------------------------------------------
// Dummy-data helpers
// ---------------------------------------------------------------------------

/// `members.split(",") if members != "" else []`
/// (`create_dummy_data.py:35`): no stripping — `"a, b"` keeps the
/// space and the spaced address never matches `email__in`.
pub fn split_member_emails(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(',').map(str::to_owned).collect()
}

/// `random_color` (`utils/color.py:12-17`): `#` + 6 draws over
/// `string.hexdigits` (`0123456789abcdefABCDEF`, uniform).
pub fn random_color() -> String {
    use rand::seq::IndexedRandom;
    const HEXDIGITS: &[u8] = b"0123456789abcdefABCDEF";
    let mut rng = rand::rng();
    let mut color = String::with_capacity(7);
    color.push('#');
    for _ in 0..6 {
        color.push(*HEXDIGITS.choose(&mut rng).unwrap_or(&b'0') as char);
    }
    color
}

// ---------------------------------------------------------------------------
// Stderr renderers (content without the trailing newline; the `bin`
// layer terminates every line)
// ---------------------------------------------------------------------------

/// Uncaught `CommandError(msg)` via `run_from_argv`
/// (`django/core/management/base.py`): `CommandError: {msg}` on
/// stderr, exit 1.
pub fn command_error_line(message: &str) -> String {
    format!("CommandError: {message}")
}

/// The two chaining sentences CPython prints between chained
/// tracebacks, each followed by a blank line (and a blank line
/// follows the inner exception block too).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainSentence {
    /// `raise ...` inside an `except` block (`raise ... from` unset).
    DuringHandling,
    /// `raise ... from ...` (Django's `wrap_database_errors`).
    DirectCause,
}

impl ChainSentence {
    fn text(self) -> &'static str {
        match self {
            ChainSentence::DuringHandling => {
                "During handling of the above exception, another exception occurred:"
            }
            ChainSentence::DirectCause => {
                "The above exception was the direct cause of the following exception:"
            }
        }
    }
}

/// A chained traceback with the frames stripped: both `Traceback
/// (most recent call last):` headers, the exception line(s), the
/// chaining sentence, and the two blank separator lines. Multi-line
/// exception texts (DB errors carry `DETAIL:`) render on both the
/// inner and the outer block, as CPython prints them.
pub fn chained_skeleton(inner: &str, sentence: ChainSentence, outer: &str) -> String {
    format!(
        "Traceback (most recent call last):\n{inner}\n\n{}\n\nTraceback (most recent call last):\n{outer}",
        sentence.text()
    )
}

/// A single (unchained) traceback with the frames stripped.
pub fn single_skeleton(exception_line: &str) -> String {
    format!("Traceback (most recent call last):\n{exception_line}")
}

/// `create_project_member.py:70-72` crash (BUG-3): the caught
/// `CommandError` re-raises as `AttributeError` inside the handler.
/// Inner line keeps the `django.core.management.base.` module path,
/// exactly as CPython renders the inner exception.
pub fn member_command_crash_skeleton(inner_message: &str) -> String {
    chained_skeleton(
        &format!("django.core.management.base.CommandError: {inner_message}"),
        ChainSentence::DuringHandling,
        "AttributeError: 'CommandError' object has no attribute 'endswith'",
    )
}

/// `str(exc)` for a DB error as `print(e)` renders it
/// (`create_instance_admin.py:42`): the server message plus
/// `DETAIL:  {detail}` when the server sent one. No exception-class
/// prefix — that only appears in tracebacks.
pub fn printed_db_error(message: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) => format!("{message}\nDETAIL:  {detail}"),
        None => message.to_owned(),
    }
}

/// Map a Postgres SQLSTATE to the `(psycopg, django)` exception pair
/// Django's `wrap_database_errors` renders. The four states the
/// commands observably raise keep their concrete subclasses;
/// anything else falls back to the class-prefix taxonomy
/// (`23` → integrity, `22` → data, else generic database error).
pub fn db_exception_pair(sql_state: &str) -> (&'static str, &'static str) {
    match sql_state {
        "23502" => (
            "psycopg.errors.NotNullViolation",
            "django.db.utils.IntegrityError",
        ),
        "23505" => (
            "psycopg.errors.UniqueViolation",
            "django.db.utils.IntegrityError",
        ),
        "23514" => (
            "psycopg.errors.CheckViolation",
            "django.db.utils.IntegrityError",
        ),
        "22003" => (
            "psycopg.errors.NumericValueOutOfRange",
            "django.db.utils.DataError",
        ),
        _ if sql_state.starts_with("23") => (
            "psycopg.errors.IntegrityError",
            "django.db.utils.IntegrityError",
        ),
        _ if sql_state.starts_with("22") => {
            ("psycopg.errors.DataError", "django.db.utils.DataError")
        }
        _ => ("psycopg.errors.Error", "django.db.utils.DatabaseError"),
    }
}

/// An uncaught DB error (`wrap_database_errors`, direct-cause chain):
/// the psycopg line and the Django line carry the same server
/// message + `DETAIL`.
pub fn db_error_skeleton(sql_state: &str, message: &str, detail: Option<&str>) -> String {
    let (psycopg_class, django_class) = db_exception_pair(sql_state);
    let rendered = printed_db_error(message, detail);
    chained_skeleton(
        &format!("{psycopg_class}: {rendered}"),
        ChainSentence::DirectCause,
        &format!("{django_class}: {rendered}"),
    )
}

/// `Project.objects.filter(pk=project_id)` with an invalid UUID
/// (`fields/__init__.py:to_python`): `uuid.UUID(value)` raises
/// `ValueError: badly formed hexadecimal UUID string`, wrapped as
/// `ValidationError: ['“{value}” is not a valid UUID.']` (U+201C /
/// U+201D quotes, Django 4.2 text).
pub fn invalid_uuid_skeleton(value: &str) -> String {
    chained_skeleton(
        "ValueError: badly formed hexadecimal UUID string",
        ChainSentence::DuringHandling,
        &format!("django.core.exceptions.ValidationError: ['\u{201c}{value}\u{201d} is not a valid UUID.']"),
    )
}

/// Parse a UUID field the way `uuid.UUID(value)` does: strip every
/// `urn:`/`uuid:` (case-sensitive, anywhere), strip surrounding
/// braces, drop every hyphen wherever it sits, then require exactly
/// 32 ASCII hex digits. Surrounding whitespace never parses (it
/// breaks the length check), and neither do `int(x, 16)` quirks the
/// port declines (`_` separators, `+`/`-` signs — absurd inputs no
/// operator types).
pub fn parse_uuid_field(value: &str) -> Option<uuid::Uuid> {
    let no_urn = value.replace("urn:", "").replace("uuid:", "");
    let no_braces = no_urn.trim_matches(|c| c == '{' || c == '}');
    let hex: String = no_braces.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    uuid::Uuid::parse_str(&hex).ok()
}

/// Bare `EOFError` from `getpass` hitting end-of-stdin
/// (`getpass.py:_raw_input`): uncaught, single traceback.
pub fn getpass_eof_skeleton() -> String {
    single_skeleton("EOFError")
}

/// `input()` hitting end-of-stdin raises
/// `EOFError("EOF when reading a line")`, which
/// `create_dummy_data.py:72-74` catches like any other error.
pub const INPUT_EOF_MESSAGE: &str = "EOF when reading a line";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_line_renders_none_like_python() {
        assert_eq!(role_line(None), "Role: None");
        assert_eq!(role_line(Some(15)), "Role: 15");
        assert_eq!(role_line(Some(-5)), "Role: -5");
        assert_eq!(role_line(Some(9999999999)), "Role: 9999999999");
    }

    #[test]
    fn member_added_line_matches_python_fstring() {
        assert_eq!(
            member_added_line("m@example.com", "4a241c2d-0443-4bc8-b7a8-f5f902402ed3"),
            "User m@example.com added to project 4a241c2d-0443-4bc8-b7a8-f5f902402ed3"
        );
    }

    #[test]
    fn dummy_data_lines_match_python() {
        assert_eq!(
            dummy_data_error_line("Workspace slug is required"),
            "Command errored out Workspace slug is required"
        );
        assert_eq!(
            dummy_data_error_line("invalid literal for int() with base 10: 'abc'"),
            "Command errored out invalid literal for int() with base 10: 'abc'"
        );
        assert_eq!(
            project_details_line(1),
            "Please provide the following details for project 1:"
        );
        assert_eq!(
            project_details_line(2),
            "Please provide the following details for project 2:"
        );
    }

    #[test]
    fn py_int_matrix_matches_cpython() {
        for (raw, expected) in [
            ("15", 15),
            ("  15  ", 15),
            ("+15", 15),
            ("-5", -5),
            ("-0", 0),
            ("1_5", 15),
            ("1_2_3", 123),
            ("0", 0),
            ("9999999999", 9999999999),
        ] {
            assert_eq!(py_int(raw), Ok(expected), "raw={raw:?}");
        }
        for raw in [
            "",
            "   ",
            "abc",
            "1.0",
            "0x11",
            "--5",
            "++5",
            "+-5",
            "_5",
            "5_",
            "+_5",
            "1__5",
            "-",
            "+",
            "_",
            "１２３",
            "99999999999999999999999999",
        ] {
            assert_eq!(
                py_int(raw),
                Err(format!("invalid literal for int() with base 10: '{raw}'")),
                "raw={raw:?}"
            );
        }
    }

    #[test]
    fn py_strip_matches_python_control_set() {
        assert_eq!(py_strip("  a@b.com\n"), "a@b.com");
        assert_eq!(py_strip("\u{1c}a@b.com\u{1f}"), "a@b.com");
        assert_eq!(py_strip("a@b.com"), "a@b.com");
    }

    #[test]
    fn saved_email_normalizes_and_fills() {
        assert_eq!(
            normalize_saved_email("EDGE1@EXAMPLE.COM"),
            "edge1@example.com"
        );
        assert_eq!(normalize_saved_email("  a@B.com\t"), "a@b.com");
        assert_eq!(display_name_for_saved_email("edge1@example.com"), "edge1");
        assert_eq!(display_name_for_saved_email("a@b@c"), "a");
        assert_eq!(display_name_for_saved_email("@x"), "");
    }

    #[test]
    fn user_save_applies_all_branches() {
        let now = Utc::now();
        let out = apply_user_save(
            &UserSaveInput {
                email: "EDGE1@EXAMPLE.COM".to_owned(),
                display_name: String::new(),
                is_staff: false,
                is_superuser: true,
                token: "tok".to_owned(),
                token_updated_at: Some(now),
            },
            "fresh",
            now,
        );
        assert_eq!(out.email, "edge1@example.com");
        assert_eq!(out.display_name, "edge1");
        assert!(out.is_staff);
        assert_eq!(out.token, "fresh");
        assert_eq!(out.token_updated_at, Some(now));

        let kept = apply_user_save(
            &UserSaveInput {
                email: "a@b.com".to_owned(),
                display_name: "keep".to_owned(),
                is_staff: false,
                is_superuser: false,
                token: "tok".to_owned(),
                token_updated_at: None,
            },
            "fresh",
            now,
        );
        assert_eq!(kept.display_name, "keep");
        assert!(!kept.is_staff);
        assert_eq!(kept.token, "tok");
        assert_eq!(kept.token_updated_at, None);
    }

    #[test]
    fn user_token_is_64_hex() {
        let token = new_user_token();
        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn password_scores_agree_with_python_zxcvbn_4_4_28() {
        // Python oracle scores (venv, zxcvbn==4.4.28): password123=0,
        // Sup3rS3cur3!=3, Tr0ub4dor&3=4. The port must agree on both
        // sides of the score<3 gate.
        assert_eq!(password_score("password123"), 0);
        assert_eq!(password_score("Sup3rS3cur3!"), 3);
        assert_eq!(password_score("Tr0ub4dor&3"), 4);
        assert!(is_weak_password(0));
        assert!(is_weak_password(2));
        assert!(!is_weak_password(3));
        assert!(!is_weak_password(4));
    }

    #[test]
    fn encoded_password_verifies_through_auth_kernel() {
        let encoded = encode_password("Tr0ub4dor&3");
        let mut parts = encoded.split('$');
        assert_eq!(parts.next(), Some("pbkdf2_sha256"));
        assert_eq!(parts.next(), Some("600000"));
        let salt = parts.next().unwrap_or_default();
        assert_eq!(salt.len(), 22);
        assert!(salt.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert!(parts.next().is_some());
        assert_eq!(parts.next(), None);
        assert_eq!(
            pidash_auth::password::verify_password("Tr0ub4dor&3", &encoded),
            Ok(true)
        );
        assert_eq!(
            pidash_auth::password::verify_password("wrong", &encoded),
            Ok(false)
        );
    }

    #[test]
    fn split_members_keeps_spaces_and_empty() {
        assert!(split_member_emails("").is_empty());
        assert_eq!(split_member_emails("a@x"), vec!["a@x".to_owned()]);
        assert_eq!(
            split_member_emails("m1@example.com, m2@example.com"),
            vec!["m1@example.com".to_owned(), " m2@example.com".to_owned()]
        );
    }

    #[test]
    fn random_color_matches_hexdigits_shape() {
        for _ in 0..50 {
            let color = random_color();
            assert_eq!(color.len(), 7);
            assert!(color.starts_with('#'));
            assert!(color[1..]
                .bytes()
                .all(|b| b"0123456789abcdefABCDEF".contains(&b)));
        }
    }

    #[test]
    fn command_error_line_matches_run_from_argv() {
        assert_eq!(
            command_error_line("Error: User with a@b does not exists"),
            "CommandError: Error: User with a@b does not exists"
        );
    }

    #[test]
    fn member_crash_skeleton_matches_oracle() {
        assert_eq!(
            member_command_crash_skeleton("Project ID is required"),
            "Traceback (most recent call last):\n\
             django.core.management.base.CommandError: Project ID is required\n\
             \n\
             During handling of the above exception, another exception occurred:\n\
             \n\
             Traceback (most recent call last):\n\
             AttributeError: 'CommandError' object has no attribute 'endswith'"
        );
    }

    #[test]
    fn db_skeleton_carries_detail_on_both_blocks() {
        let skeleton = db_error_skeleton(
            "23502",
            "null value in column \"role\" of relation \"project_members\" violates not-null constraint",
            Some("Failing row contains (1, 2)."),
        );
        assert!(skeleton.starts_with(
            "Traceback (most recent call last):\npsycopg.errors.NotNullViolation: null value"
        ));
        assert!(skeleton.contains(
            "\n\nThe above exception was the direct cause of the following exception:\n\n"
        ));
        assert!(skeleton.ends_with("django.db.utils.IntegrityError: null value in column \"role\" of relation \"project_members\" violates not-null constraint\nDETAIL:  Failing row contains (1, 2)."));
        assert_eq!(skeleton.matches("DETAIL:").count(), 2);
        let no_detail = db_error_skeleton("22003", "smallint out of range", None);
        assert_eq!(
            no_detail,
            "Traceback (most recent call last):\n\
             psycopg.errors.NumericValueOutOfRange: smallint out of range\n\
             \n\
             The above exception was the direct cause of the following exception:\n\
             \n\
             Traceback (most recent call last):\n\
             django.db.utils.DataError: smallint out of range"
        );
    }

    #[test]
    fn db_taxonomy_maps_sqlstates() {
        assert_eq!(
            db_exception_pair("23502"),
            (
                "psycopg.errors.NotNullViolation",
                "django.db.utils.IntegrityError"
            )
        );
        assert_eq!(
            db_exception_pair("23505"),
            (
                "psycopg.errors.UniqueViolation",
                "django.db.utils.IntegrityError"
            )
        );
        assert_eq!(
            db_exception_pair("23514"),
            (
                "psycopg.errors.CheckViolation",
                "django.db.utils.IntegrityError"
            )
        );
        assert_eq!(
            db_exception_pair("23000").1,
            "django.db.utils.IntegrityError"
        );
        assert_eq!(db_exception_pair("22001").1, "django.db.utils.DataError");
        assert_eq!(
            db_exception_pair("XX000"),
            ("psycopg.errors.Error", "django.db.utils.DatabaseError")
        );
    }

    #[test]
    fn uuid_forms_match_cpython_constructor() {
        let canonical = "4a241c2d-0443-4bc8-b7a8-f5f902402ed3";
        let expected = uuid::Uuid::parse_str(canonical).unwrap();
        assert_eq!(parse_uuid_field(canonical), Some(expected));
        assert_eq!(
            parse_uuid_field("4a241c2d04434bc8b7a8f5f902402ed3"),
            Some(expected)
        );
        assert_eq!(
            parse_uuid_field("{4a241c2d-0443-4bc8-b7a8-f5f902402ed3}"),
            Some(expected)
        );
        assert_eq!(
            parse_uuid_field("urn:uuid:4a241c2d-0443-4bc8-b7a8-f5f902402ed3"),
            Some(expected)
        );
        assert_eq!(
            parse_uuid_field("4A241C2D-0443-4BC8-B7A8-F5F902402ED3"),
            Some(expected)
        );
        assert_eq!(
            parse_uuid_field("4a241c2d04434bc8-b7a8f5f902402ed3"),
            Some(expected)
        );
        assert_eq!(
            parse_uuid_field("urn:uuid:{4a241c2d-0443-4bc8-b7a8-f5f902402ed3}"),
            Some(expected)
        );
        assert_eq!(parse_uuid_field("xyz"), None);
        assert_eq!(
            parse_uuid_field(" 4a241c2d-0443-4bc8-b7a8-f5f902402ed3 "),
            None
        );
        assert_eq!(parse_uuid_field(""), None);
        assert_eq!(
            parse_uuid_field("4a241c2d-0443-bc8b-b7a8-f5f902402ed"),
            None
        );
    }

    #[test]
    fn invalid_uuid_skeleton_matches_oracle() {
        assert_eq!(
            invalid_uuid_skeleton("xyz"),
            "Traceback (most recent call last):\n\
             ValueError: badly formed hexadecimal UUID string\n\
             \n\
             During handling of the above exception, another exception occurred:\n\
             \n\
             Traceback (most recent call last):\n\
             django.core.exceptions.ValidationError: ['\u{201c}xyz\u{201d} is not a valid UUID.']"
        );
    }

    #[test]
    fn getpass_eof_skeleton_is_bare() {
        assert_eq!(
            getpass_eof_skeleton(),
            "Traceback (most recent call last):\nEOFError"
        );
    }

    #[test]
    fn printed_db_error_has_no_class_prefix() {
        assert_eq!(printed_db_error("msg", Some("det")), "msg\nDETAIL:  det");
        assert_eq!(printed_db_error("msg", None), "msg");
    }
}
