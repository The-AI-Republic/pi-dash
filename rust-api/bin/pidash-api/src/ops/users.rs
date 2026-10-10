//! Users + membership ops commands (D-37, F37-03/F37-04).
//!
//! Clap wiring and stdio orchestration for `activate_user`,
//! `reset_password`, `create_instance_admin`, `create_project_member`
//! and `create_dummy_data`. Prompts mirror `input()` / `getpass()`
//! byte for byte (universal newlines, one trailing `\n` stripped,
//! getpass fallback bytes on stderr); the `services` kernels own the
//! messages and the `db` layer owns the SQL.
//!
//! Exit codes mirror the commands: `0` on success (including the
//! two commands that swallow their own errors onto stdout),
//! `1` on `CommandError` / crash branches, `2` on clap usage errors.

use crate::ops::OpsIo;
use clap::Args;

// ---------------------------------------------------------------------------
// Arguments (flags named verbatim; argparse `nargs="?"` becomes
// `num_args(0..=1)`: absent and bare both read `None`)
// ---------------------------------------------------------------------------

/// `activate_user email` (`activate_user.py:15-17`).
#[derive(Debug, Args)]
pub struct ActivateUserArgs {
    /// user email
    pub email: String,
}

/// `reset_password email` (`reset_password.py:21-23`).
#[derive(Debug, Args)]
pub struct ResetPasswordArgs {
    /// user email
    pub email: String,
}

/// `create_instance_admin admin_email`
/// (`create_instance_admin.py:16-18`).
#[derive(Debug, Args)]
pub struct CreateInstanceAdminArgs {
    /// Instance Admin Email
    pub admin_email: String,
}

/// `create_project_member [--project_id] [--user_email] [--role]`
/// (`create_project_member.py:22-26`). `--role` stays a string:
/// argparse `type=int` accepts surrounding whitespace, signs and
/// underscores (`1_5` → 15), which clap's integer parser would
/// reject — the services `py_int` kernel parses it instead.
#[derive(Debug, Args)]
pub struct CreateProjectMemberArgs {
    /// Project ID
    #[arg(long = "project_id", num_args(0..=1), allow_negative_numbers(true))]
    pub project_id: Option<String>,
    /// User Email
    #[arg(long = "user_email", num_args(0..=1), allow_negative_numbers(true))]
    pub user_email: Option<String>,
    /// Role of the user in the project
    #[arg(long = "role", num_args(0..=1), allow_negative_numbers(true))]
    pub role: Option<String>,
}

/// `create_dummy_data` (no arguments; fully interactive).
#[derive(Debug, Args)]
pub struct CreateDummyDataArgs {}

// ---------------------------------------------------------------------------
// `input()` / `getpass` line reading over universal newlines
// ---------------------------------------------------------------------------

/// How a prompt read ended without a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadEnd {
    /// End of stdin before any byte (`EOFError`).
    Eof,
    /// Invalid UTF-8; carries the CPython codec message
    /// (`'utf-8' codec can't decode byte ...`).
    Decode(String),
    /// An OS read failure; carries the raw error text.
    Io(String),
}

/// Read one `input()` line: `sys.stdin.readline` semantics —
/// universal newlines (`\n`, `\r`, `\r\n` all terminate, like the
/// text layer), the terminator stripped, `None` on EOF before any
/// byte, trailing partial bytes returned as the final line.
pub fn read_py_line(stdin: &mut dyn std::io::BufRead) -> Result<Option<String>, ReadEnd> {
    let mut raw: Vec<u8> = Vec::new();
    loop {
        let avail = stdin.fill_buf().map_err(|e| ReadEnd::Io(e.to_string()))?;
        if avail.is_empty() {
            if raw.is_empty() {
                return Ok(None);
            }
            return decode_line(&raw);
        }
        match avail.iter().position(|b| *b == b'\n' || *b == b'\r') {
            None => {
                let len = avail.len();
                raw.extend_from_slice(avail);
                stdin.consume(len);
            }
            Some(pos) => {
                raw.extend_from_slice(&avail[..pos]);
                let term = avail[pos];
                let mut consume = pos + 1;
                if term == b'\r' {
                    if avail.get(pos + 1) == Some(&b'\n') {
                        consume += 1;
                    } else if pos + 1 == avail.len() {
                        // `\r` straddles the buffer edge: peek for `\n`.
                        stdin.consume(consume);
                        let next = stdin.fill_buf().map_err(|e| ReadEnd::Io(e.to_string()))?;
                        if next.first() == Some(&b'\n') {
                            stdin.consume(1);
                        }
                        return decode_line(&raw);
                    }
                }
                stdin.consume(consume);
                return decode_line(&raw);
            }
        }
    }
}

fn decode_line(raw: &[u8]) -> Result<Option<String>, ReadEnd> {
    match String::from_utf8(raw.to_vec()) {
        Ok(line) => Ok(Some(line)),
        Err(error) => {
            let valid_up_to = error.utf8_error().valid_up_to();
            let byte = raw.get(valid_up_to).copied().unwrap_or(0);
            let reason = match error.utf8_error().error_len() {
                None => "unexpected end of data",
                Some(_) if (0xC2..=0xF4).contains(&byte) => "invalid continuation byte",
                Some(_) => "invalid start byte",
            };
            Err(ReadEnd::Decode(format!(
                "'utf-8' codec can't decode byte 0x{byte:02x} in position {valid_up_to}: {reason}"
            )))
        }
    }
}

/// `input(prompt)`: prompt to stdout (flushed, no newline), line from
/// stdin. `ReadEnd::Eof` is `EOFError("EOF when reading a line")`.
pub fn ask(io: &mut OpsIo<'_>, prompt: &str) -> Result<String, ReadEnd> {
    io.stdout
        .write_all(prompt.as_bytes())
        .map_err(|e| ReadEnd::Io(format!("stdout write failed: {e}")))?;
    io.stdout
        .flush()
        .map_err(|e| ReadEnd::Io(format!("stdout flush failed: {e}")))?;
    read_py_line(io.stdin)?.ok_or(ReadEnd::Eof)
}

/// `getpass.getpass(prompt)` on a pipeless terminal: the
/// `fallback_getpass` bytes — `Warning: Password input may be
/// echoed.` plus the prompt on stderr, the line from stdin with one
/// trailing `\n` stripped, then `\n` on stderr. (The
/// `GetPassWarning` header is interpreter noise and is not ported;
/// echo stays on — without `unsafe` there is no termios control.)
/// EOF raises bare `EOFError` before the trailing `\n` is written.
pub fn getpass(io: &mut OpsIo<'_>, prompt: &str) -> Result<String, ReadEnd> {
    use pidash_services::ops::users as kernel;
    io.stderr
        .write_all(kernel::GETPASS_ECHO_WARNING.as_bytes())
        .map_err(|e| ReadEnd::Io(format!("stderr write failed: {e}")))?;
    io.stderr
        .write_all(b"\n")
        .map_err(|e| ReadEnd::Io(format!("stderr write failed: {e}")))?;
    io.stderr
        .write_all(prompt.as_bytes())
        .map_err(|e| ReadEnd::Io(format!("stderr write failed: {e}")))?;
    io.stderr
        .flush()
        .map_err(|e| ReadEnd::Io(format!("stderr flush failed: {e}")))?;
    let line = read_py_line(io.stdin)?.ok_or(ReadEnd::Eof)?;
    io.stderr
        .write_all(b"\n")
        .map_err(|e| ReadEnd::Io(format!("stderr write failed: {e}")))?;
    Ok(line)
}

// ---------------------------------------------------------------------------
// Output + error plumbing
// ---------------------------------------------------------------------------

/// `self.stdout.write(s)` / `print(x)`: content plus `\n` on stdout.
fn emit_stdout(io: &mut OpsIo<'_>, content: &str) {
    let _ = writeln!(io.stdout, "{content}");
}

/// `self.stderr.write(s)` / uncaught-`CommandError` / tracebacks:
/// content plus `\n` on stderr.
fn emit_stderr(io: &mut OpsIo<'_>, content: &str) {
    let _ = writeln!(io.stderr, "{content}");
}

/// The server `DETAIL` field off a database error, via the Postgres
/// error type.
fn db_error_detail(error: &sqlx::Error) -> Option<String> {
    error
        .as_database_error()
        .and_then(|e| {
            e.as_error()
                .downcast_ref::<sqlx::postgres::PgDatabaseError>()
        })
        .and_then(|pg| pg.detail())
        .map(str::to_owned)
}

/// Split a `sqlx::Error` into the server fields the Django
/// renderers need: `(sqlstate, message, detail)`. Non-server
/// failures map to the operational/generic fallbacks.
fn db_error_parts(error: &sqlx::Error) -> (String, String, Option<String>) {
    match error {
        sqlx::Error::Database(db_error) => (
            db_error.code().unwrap_or_default().into_owned(),
            db_error.message().to_owned(),
            db_error_detail(error),
        ),
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => {
            ("08006".to_owned(), error.to_string(), None)
        }
        _ => ("XX000".to_owned(), error.to_string(), None),
    }
}

/// Render an uncaught DB error as the chained psycopg → Django
/// traceback skeleton on stderr.
fn emit_db_skeleton(io: &mut OpsIo<'_>, error: &sqlx::Error) {
    use pidash_services::ops::users as kernel;
    let (code, message, detail) = db_error_parts(error);
    // `08006` (connection failure) renders as the operational pair,
    // which the state taxonomy below does not name.
    if code == "08006" {
        let rendered = kernel::printed_db_error(&message, detail.as_deref());
        emit_stderr(
            io,
            &kernel::chained_skeleton(
                &format!("psycopg.errors.OperationalError: {rendered}"),
                kernel::ChainSentence::DirectCause,
                &format!("django.db.utils.OperationalError: {rendered}"),
            ),
        );
        return;
    }
    emit_stderr(
        io,
        &kernel::db_error_skeleton(&code, &message, detail.as_deref()),
    );
}

/// `str(e)` for a DB error the dummy-data command catches: the
/// server message plus `DETAIL`, with no exception-class prefix.
fn caught_db_error_text(error: &sqlx::Error) -> String {
    use pidash_services::ops::users as kernel;
    match error {
        sqlx::Error::Database(db_error) => {
            kernel::printed_db_error(db_error.message(), db_error_detail(error).as_deref())
        }
        _ => error.to_string(),
    }
}

// ---------------------------------------------------------------------------
// `activate_user` (`activate_user.py:19-38`)
// ---------------------------------------------------------------------------

/// `activate_user email`: missing/unknown users raise `CommandError`
/// (stderr, exit 1); found users flip `is_active` through `save()`.
pub async fn run_activate_user(
    pool: &sqlx::PgPool,
    args: &ActivateUserArgs,
    io: &mut OpsIo<'_>,
) -> Result<i32, clap::Error> {
    use pidash_db::ops::users as queries;
    use pidash_services::ops::users as kernel;

    if args.email.is_empty() {
        emit_stderr(io, &kernel::command_error_line("Error: Email is required"));
        return Ok(1);
    }
    let user = match queries::user_by_email(pool, &args.email).await {
        Ok(row) => row,
        Err(error) => {
            emit_db_skeleton(io, &error);
            return Ok(1);
        }
    };
    let Some(user) = user else {
        emit_stderr(
            io,
            &kernel::command_error_line(&format!(
                "Error: User with {} does not exists",
                args.email
            )),
        );
        return Ok(1);
    };
    let write = kernel::apply_user_save(
        &kernel::UserSaveInput {
            email: user.email,
            display_name: user.display_name,
            is_staff: user.is_staff,
            is_superuser: user.is_superuser,
            token: user.token,
            token_updated_at: user.token_updated_at,
        },
        &kernel::new_user_token(),
        chrono::Utc::now(),
    );
    if let Err(error) = queries::save_user_activation(pool, user.id, &write).await {
        emit_db_skeleton(io, &error);
        return Ok(1);
    }
    emit_stdout(io, kernel::ACTIVATE_SUCCESS);
    Ok(0)
}

// ---------------------------------------------------------------------------
// `reset_password` (`reset_password.py:25-66`)
// ---------------------------------------------------------------------------

/// Read one getpass password, rendering the EOF / decode / IO
/// failures as single-traceback skeletons on stderr.
fn read_reset_password(io: &mut OpsIo<'_>, prompt: &str) -> Result<String, i32> {
    use pidash_services::ops::users as kernel;
    match getpass(io, prompt) {
        Ok(password) => Ok(password),
        Err(ReadEnd::Eof) => {
            emit_stderr(io, &kernel::getpass_eof_skeleton());
            Err(1)
        }
        Err(ReadEnd::Decode(message)) => {
            emit_stderr(
                io,
                &kernel::single_skeleton(&format!("UnicodeDecodeError: {message}")),
            );
            Err(1)
        }
        Err(ReadEnd::Io(message)) => {
            emit_stderr(io, &kernel::single_skeleton(&format!("OSError: {message}")));
            Err(1)
        }
    }
}

/// `reset_password email`: email/user errors go to stderr with exit
/// 0, mismatches and blanks likewise, weak passwords raise
/// `CommandError` (exit 1), accepted ones re-hash through `save()`.
pub async fn run_reset_password(
    pool: &sqlx::PgPool,
    args: &ResetPasswordArgs,
    io: &mut OpsIo<'_>,
) -> Result<i32, clap::Error> {
    use pidash_db::ops::users as queries;
    use pidash_services::ops::users as kernel;

    if args.email.is_empty() {
        emit_stderr(io, "Error: Email is required");
        return Ok(0);
    }
    let user = match queries::user_by_email(pool, &args.email).await {
        Ok(row) => row,
        Err(error) => {
            emit_db_skeleton(io, &error);
            return Ok(1);
        }
    };
    let Some(user) = user else {
        emit_stderr(
            io,
            &format!("Error: User with {} does not exists", args.email),
        );
        return Ok(0);
    };
    let password = match read_reset_password(io, kernel::GETPASS_PASSWORD) {
        Ok(password) => password,
        Err(code) => return Ok(code),
    };
    let confirm = match read_reset_password(io, kernel::GETPASS_PASSWORD_AGAIN) {
        Ok(password) => password,
        Err(code) => return Ok(code),
    };
    if password != confirm {
        emit_stderr(io, "Error: Your passwords didn't match.");
        return Ok(0);
    }
    if kernel::py_strip(&password).is_empty() {
        emit_stderr(io, "Error: Blank passwords aren't allowed.");
        return Ok(0);
    }
    if kernel::is_weak_password(kernel::password_score(&password)) {
        emit_stderr(
            io,
            &kernel::command_error_line("Password is too common please set a complex password"),
        );
        return Ok(1);
    }
    let encoded = kernel::encode_password(&password);
    let write = kernel::apply_user_save(
        &kernel::UserSaveInput {
            email: user.email,
            display_name: user.display_name,
            is_staff: user.is_staff,
            is_superuser: user.is_superuser,
            token: user.token,
            token_updated_at: user.token_updated_at,
        },
        &kernel::new_user_token(),
        chrono::Utc::now(),
    );
    if let Err(error) = queries::save_user_password(pool, user.id, &encoded, &write).await {
        emit_db_skeleton(io, &error);
        return Ok(1);
    }
    emit_stdout(io, kernel::RESET_PASSWORD_SUCCESS);
    Ok(0)
}

// ---------------------------------------------------------------------------
// `create_instance_admin` (`create_instance_admin.py:20-43`)
// ---------------------------------------------------------------------------

/// The `except Exception` tail: `print(e)` on stdout, then the outer
/// `CommandError("Failed to create the instance admin.")` on stderr,
/// exit 1.
fn emit_instance_admin_failure(io: &mut OpsIo<'_>, printed: &str) -> i32 {
    use pidash_services::ops::users as kernel;
    emit_stdout(io, printed);
    emit_stderr(
        io,
        &kernel::command_error_line("Failed to create the instance admin."),
    );
    1
}

/// `create_instance_admin admin_email`: `get_or_create` for
/// `(user, oldest instance, role 20)`; duplicates and DB failures
/// print the inner error on stdout and raise the outer one.
pub async fn run_create_instance_admin(
    pool: &sqlx::PgPool,
    args: &CreateInstanceAdminArgs,
    io: &mut OpsIo<'_>,
) -> Result<i32, clap::Error> {
    use pidash_db::ops::users as queries;
    use pidash_services::ops::users as kernel;

    if args.admin_email.is_empty() {
        emit_stderr(
            io,
            &kernel::command_error_line("Please provide the email of the admin."),
        );
        return Ok(1);
    }
    let user = match queries::user_by_email(pool, &args.admin_email).await {
        Ok(row) => row,
        Err(error) => {
            emit_db_skeleton(io, &error);
            return Ok(1);
        }
    };
    let Some(user) = user else {
        emit_stderr(
            io,
            &kernel::command_error_line("User with the provided email does not exist."),
        );
        return Ok(1);
    };
    let instance = match queries::oldest_instance(pool).await {
        Ok(instance) => instance,
        Err(error) => {
            return Ok(emit_instance_admin_failure(
                io,
                &caught_db_error_text(&error),
            ));
        }
    };
    let Some(instance_id) = instance else {
        // `get_or_create(user, instance=None, role=20)`: the `get()`
        // (`instance_id = NULL`) can never match, so the `create()`
        // runs and the not-null violation prints.
        let error = match queries::instance_admin_create(pool, user.id, None).await {
            Ok(_) => {
                emit_stdout(io, kernel::INSTANCE_ADMIN_SUCCESS);
                return Ok(0);
            }
            Err(error) => error,
        };
        return Ok(emit_instance_admin_failure(
            io,
            &caught_db_error_text(&error),
        ));
    };
    match queries::instance_admin_get(pool, user.id, instance_id).await {
        Ok(Some(_)) => {
            return Ok(emit_instance_admin_failure(
                io,
                "The provided email is already an instance admin.",
            ));
        }
        Ok(None) => {}
        Err(error) => {
            return Ok(emit_instance_admin_failure(
                io,
                &caught_db_error_text(&error),
            ));
        }
    }
    match queries::instance_admin_create(pool, user.id, Some(instance_id)).await {
        Ok(_) => {
            emit_stdout(io, kernel::INSTANCE_ADMIN_SUCCESS);
            Ok(0)
        }
        Err(error) => {
            // `get_or_create` retries the `get()` after an
            // `IntegrityError`: a hit is the duplicate-admin path, a
            // miss re-raises the original, and any other failure
            // propagates instead.
            match queries::instance_admin_get(pool, user.id, instance_id).await {
                Ok(Some(_)) => Ok(emit_instance_admin_failure(
                    io,
                    "The provided email is already an instance admin.",
                )),
                Ok(None) => Ok(emit_instance_admin_failure(
                    io,
                    &caught_db_error_text(&error),
                )),
                Err(retry_error) => Ok(emit_instance_admin_failure(
                    io,
                    &caught_db_error_text(&retry_error),
                )),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// `create_project_member` (`create_project_member.py:28-72`)
// ---------------------------------------------------------------------------

/// The BUG-3 crash: every `CommandError` the handler catches
/// re-raises as `AttributeError` (chained skeleton, exit 1).
fn emit_member_crash(io: &mut OpsIo<'_>, inner_message: &str) -> i32 {
    use pidash_services::ops::users as kernel;
    emit_stderr(io, &kernel::member_command_crash_skeleton(inner_message));
    1
}

/// `create_project_member`: the `Role:` line prints before the
/// lookups; misses crash (BUG-3); hits upsert the membership and
/// `get_or_create` the property.
pub async fn run_create_project_member(
    pool: &sqlx::PgPool,
    args: &CreateProjectMemberArgs,
    io: &mut OpsIo<'_>,
) -> Result<i32, clap::Error> {
    use pidash_db::ops::users as queries;
    use pidash_services::ops::users as kernel;

    let project_id_raw = args.project_id.as_deref().unwrap_or("");
    let user_email = args.user_email.as_deref().unwrap_or("");
    if project_id_raw.is_empty() {
        return Ok(emit_member_crash(io, "Project ID is required"));
    }
    if user_email.is_empty() {
        return Ok(emit_member_crash(io, "User Email is required"));
    }
    // `options.get("role", 20)`: argparse stores `None` for an
    // absent (or bare) `--role`, so the `20` default never fires.
    let role: Option<i64> = match args.role.as_deref() {
        None => None,
        Some(raw) => match kernel::py_int(raw) {
            Ok(value) => Some(value),
            Err(message) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("invalid value '{raw}' for '--role <ROLE>': {message}\n"),
                ));
            }
        },
    };
    emit_stdout(io, &kernel::role_line(role));
    let user = match queries::user_by_email(pool, user_email).await {
        Ok(row) => row,
        Err(error) => {
            emit_db_skeleton(io, &error);
            return Ok(1);
        }
    };
    let Some(user) = user else {
        return Ok(emit_member_crash(io, "User not found"));
    };
    let Some(project_id) = kernel::parse_uuid_field(project_id_raw) else {
        emit_stderr(io, &kernel::invalid_uuid_skeleton(project_id_raw));
        return Ok(1);
    };
    let project = match queries::project_lookup(pool, project_id).await {
        Ok(row) => row,
        Err(error) => {
            emit_db_skeleton(io, &error);
            return Ok(1);
        }
    };
    let Some(project) = project else {
        return Ok(emit_member_crash(io, "Project not found"));
    };
    match queries::workspace_member_active_exists(pool, project.workspace_id, user.id).await {
        Ok(true) => {}
        Ok(false) => return Ok(emit_member_crash(io, "User not member in workspace")),
        Err(error) => {
            emit_db_skeleton(io, &error);
            return Ok(1);
        }
    }
    let props = queries::project_member_props_json();
    let preferences = queries::project_preferences_json();
    match queries::project_member_exists(pool, project.id, user.id).await {
        Ok(true) => {
            if let Err(error) =
                queries::project_member_update(pool, project.id, user.id, role).await
            {
                emit_db_skeleton(io, &error);
                return Ok(1);
            }
        }
        Ok(false) => {
            if let Err(code) = create_project_member_with_hook(
                pool,
                io,
                &project,
                &user,
                role,
                &props,
                &preferences,
            )
            .await
            {
                return Ok(code);
            }
        }
        Err(error) => {
            emit_db_skeleton(io, &error);
            return Ok(1);
        }
    }
    // `ProjectUserProperty.objects.get_or_create(user=user,
    // project=project)` (`:65`).
    match queries::property_get(pool, user.id, project.id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            let filters = queries::property_filters_json();
            let display_filters = queries::property_display_filters_json();
            let display_properties = queries::property_display_properties_json();
            if let Err(error) = queries::property_create(
                pool,
                project.workspace_id,
                project.id,
                user.id,
                65535.0,
                &filters,
                &display_filters,
                &display_properties,
                &preferences,
            )
            .await
            {
                match queries::property_get(pool, user.id, project.id).await {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        emit_db_skeleton(io, &error);
                        return Ok(1);
                    }
                    Err(retry_error) => {
                        emit_db_skeleton(io, &retry_error);
                        return Ok(1);
                    }
                }
            }
        }
        Err(error) => {
            emit_db_skeleton(io, &error);
            return Ok(1);
        }
    }
    emit_stdout(io, &kernel::member_added_line(user_email, project_id_raw));
    Ok(0)
}

/// The create arm (`:60-62`) with the `ProjectMember.save()` hook
/// (`db/models/project.py:348-364`): the property row (minimum
/// sort for this member in the workspace minus 10000, else the
/// `65535` default) persists BEFORE the member row, so a member
/// failure orphans it. `Err` is the exit code (already emitted).
async fn create_project_member_with_hook(
    pool: &sqlx::PgPool,
    io: &mut OpsIo<'_>,
    project: &pidash_db::ops::users::ProjectRef,
    user: &pidash_db::ops::users::UserRow,
    role: Option<i64>,
    props: &serde_json::Value,
    preferences: &serde_json::Value,
) -> Result<(), i32> {
    use pidash_db::ops::users as queries;
    let min_sort = match queries::property_min_sort(pool, project.workspace_id, user.id).await {
        Ok(min) => min,
        Err(error) => {
            emit_db_skeleton(io, &error);
            return Err(1);
        }
    };
    let hook_sort = min_sort.map(|min| min - 10000.0).unwrap_or(65535.0);
    if let Err(error) = queries::property_create(
        pool,
        project.workspace_id,
        project.id,
        user.id,
        hook_sort,
        &queries::property_filters_json(),
        &queries::property_display_filters_json(),
        &queries::property_display_properties_json(),
        preferences,
    )
    .await
    {
        emit_db_skeleton(io, &error);
        return Err(1);
    }
    if let Err(error) = queries::project_member_create(
        pool,
        project.id,
        project.workspace_id,
        user.id,
        role,
        props,
        preferences,
    )
    .await
    {
        emit_db_skeleton(io, &error);
        return Err(1);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `create_dummy_data` (`create_dummy_data.py:16-74`)
// ---------------------------------------------------------------------------

/// One `input()` answer: `ReadEnd::Eof` is `EOFError("EOF when
/// reading a line")`, decode failures carry the codec message.
fn ask_line(io: &mut OpsIo<'_>, prompt: &str) -> Result<String, String> {
    use pidash_services::ops::users as kernel;
    match ask(io, prompt) {
        Ok(line) => Ok(line),
        Err(ReadEnd::Eof) => Err(kernel::INPUT_EOF_MESSAGE.to_owned()),
        Err(ReadEnd::Decode(message)) | Err(ReadEnd::Io(message)) => Err(message),
    }
}

/// `create_dummy_data`: every failure — validations, `int()` parses,
/// DB errors, task errors — lands on stdout as `Command errored
/// out {e}` with exit 0 (`:72-74`). The inner function returns
/// `str(e)` content; nothing here ever touches stderr.
pub async fn run_create_dummy_data(
    pool: &sqlx::PgPool,
    _args: &CreateDummyDataArgs,
    io: &mut OpsIo<'_>,
) -> Result<i32, clap::Error> {
    use pidash_services::ops::users as kernel;
    match run_create_dummy_data_inner(pool, io).await {
        Ok(()) => Ok(0),
        Err(message) => {
            emit_stdout(io, &kernel::dummy_data_error_line(&message));
            Ok(0)
        }
    }
}

async fn run_create_dummy_data_inner(
    pool: &sqlx::PgPool,
    io: &mut OpsIo<'_>,
) -> Result<(), String> {
    use pidash_db::ops::users as queries;
    use pidash_services::ops::users as kernel;

    let workspace_name = ask_line(io, kernel::PROMPT_WORKSPACE_NAME)?;
    let workspace_slug = ask_line(io, kernel::PROMPT_WORKSPACE_SLUG)?;
    if workspace_slug.is_empty() {
        return Err("Workspace slug is required".to_owned());
    }
    let slug_taken = queries::workspace_slug_exists(pool, &workspace_slug)
        .await
        .map_err(|e| caught_db_error_text(&e))?;
    if slug_taken {
        return Err("Workspace already exists".to_owned());
    }
    let creator = ask_line(io, kernel::PROMPT_CREATOR_EMAIL)?;
    let creator_known = if creator.is_empty() {
        false
    } else {
        queries::user_exists_by_email(pool, &creator)
            .await
            .map_err(|e| caught_db_error_text(&e))?
    };
    if !creator_known {
        return Err("User email is required and should have signed in pi dash".to_owned());
    }
    let owner = queries::user_by_email(pool, &creator)
        .await
        .map_err(|e| caught_db_error_text(&e))?;
    let Some(owner) = owner else {
        return Err("User matching query does not exist.".to_owned());
    };
    let members_raw = ask_line(io, kernel::PROMPT_MEMBER_EMAILS)?;
    let members = kernel::split_member_emails(&members_raw);
    let workspace_id = queries::workspace_create(
        pool,
        &workspace_name,
        &workspace_slug,
        owner.id,
        &kernel::random_color(),
    )
    .await
    .map_err(|e| caught_db_error_text(&e))?;
    let props = queries::workspace_member_props_json();
    let issue_props = queries::workspace_issue_props_json();
    queries::workspace_member_create(pool, workspace_id, owner.id, 20, &props, &issue_props)
        .await
        .map_err(|e| caught_db_error_text(&e))?;
    let member_ids = queries::user_ids_by_emails(pool, &members)
        .await
        .map_err(|e| caught_db_error_text(&e))?;
    queries::workspace_members_bulk_create(pool, workspace_id, &member_ids, &props, &issue_props)
        .await
        .map_err(|e| caught_db_error_text(&e))?;
    let project_count_raw = ask_line(io, kernel::PROMPT_PROJECT_COUNT)?;
    let project_count = kernel::py_int(&project_count_raw)?;
    for (index, _) in (0..project_count).enumerate() {
        emit_stdout(io, &kernel::project_details_line(index + 1));
        let mut counts = Vec::with_capacity(5);
        for prompt in kernel::DummyPrompts::count_prompts() {
            let raw = ask_line(io, prompt)?;
            counts.push(kernel::py_int(&raw)?);
        }
        // The synchronous task call (`:57-68`, plain call — not
        // `.delay()`): the ported task runs inline on this pool.
        let task_args = pidash_services::tasks_cleanup::dummy_data::CreateDummyDataArgs {
            slug: workspace_slug.clone(),
            email: creator.clone(),
            members: members.clone(),
            issue_count: counts[0],
            cycle_count: counts[1],
            module_count: counts[2],
            pages_count: counts[3],
            intake_issue_count: counts[4],
        };
        pidash_jobs::tasks_cleanup::dummy_data::PgDummyData::new(pool.clone())
            .run(&task_args)
            .await?;
    }
    emit_stdout(io, kernel::DUMMY_DATA_SUCCESS);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::OpsCommand;
    use clap::Parser;
    use std::io::Cursor;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: OpsCommand,
    }

    fn test_io(stdin_bytes: &[u8]) -> (Cursor<Vec<u8>>, Vec<u8>, Vec<u8>) {
        (Cursor::new(stdin_bytes.to_vec()), Vec::new(), Vec::new())
    }

    #[test]
    fn subcommands_keep_python_snake_case_names() {
        for argv in [
            vec!["pidash-api", "activate_user", "a@b.com"],
            vec!["pidash-api", "reset_password", "a@b.com"],
            vec!["pidash-api", "create_instance_admin", "a@b.com"],
            vec![
                "pidash-api",
                "create_project_member",
                "--project_id",
                "p",
                "--user_email",
                "u",
            ],
            vec!["pidash-api", "create_dummy_data"],
        ] {
            TestCli::try_parse_from(argv).expect("ops argv parses");
        }
        // Kebab-case aliases do not exist.
        assert!(TestCli::try_parse_from(["pidash-api", "activate-user", "a@b.com"]).is_err());
    }

    #[test]
    fn member_flags_mirror_nargs_question_mark() {
        // Absent flags read None.
        let cli = TestCli::try_parse_from(["pidash-api", "create_project_member"]).unwrap();
        let OpsCommand::CreateProjectMember(args) = cli.command else {
            panic!("wrong variant");
        };
        assert_eq!(args.project_id, None);
        assert_eq!(args.user_email, None);
        assert_eq!(args.role, None);
        // Bare flags read None too (argparse const=None).
        let cli = TestCli::try_parse_from([
            "pidash-api",
            "create_project_member",
            "--project_id",
            "--user_email",
            "--role",
        ])
        .unwrap();
        let OpsCommand::CreateProjectMember(args) = cli.command else {
            panic!("wrong variant");
        };
        assert_eq!(args.project_id, None);
        assert_eq!(args.user_email, None);
        assert_eq!(args.role, None);
        // Values, equals form, and negative numbers pass through raw.
        let cli = TestCli::try_parse_from([
            "pidash-api",
            "create_project_member",
            "--project_id=p",
            "--user_email",
            "u@e.com",
            "--role",
            "-5",
        ])
        .unwrap();
        let OpsCommand::CreateProjectMember(args) = cli.command else {
            panic!("wrong variant");
        };
        assert_eq!(args.project_id.as_deref(), Some("p"));
        assert_eq!(args.user_email.as_deref(), Some("u@e.com"));
        assert_eq!(args.role.as_deref(), Some("-5"));
        // Underscored ints pass through raw for the py_int kernel.
        let cli = TestCli::try_parse_from([
            "pidash-api",
            "create_project_member",
            "--project_id",
            "p",
            "--user_email",
            "u",
            "--role",
            "1_5",
        ])
        .unwrap();
        let OpsCommand::CreateProjectMember(args) = cli.command else {
            panic!("wrong variant");
        };
        assert_eq!(args.role.as_deref(), Some("1_5"));
    }

    #[test]
    fn positionals_accept_empty_strings() {
        let cli = TestCli::try_parse_from(["pidash-api", "activate_user", ""]).unwrap();
        let OpsCommand::ActivateUser(args) = cli.command else {
            panic!("wrong variant");
        };
        assert_eq!(args.email, "");
    }

    #[test]
    fn line_reader_matches_readline_newlines() {
        // `\n`, `\r`, `\r\n` all terminate; terminator stripped.
        let mut cursor = Cursor::new(b"a\nb\rc\r\nd".to_vec());
        assert_eq!(read_py_line(&mut cursor), Ok(Some("a".to_owned())));
        assert_eq!(read_py_line(&mut cursor), Ok(Some("b".to_owned())));
        assert_eq!(read_py_line(&mut cursor), Ok(Some("c".to_owned())));
        // Trailing partial bytes are the final line.
        assert_eq!(read_py_line(&mut cursor), Ok(Some("d".to_owned())));
        assert_eq!(read_py_line(&mut cursor), Ok(None));
        assert_eq!(read_py_line(&mut cursor), Ok(None));
        // Only ONE trailing `\n` is stripped.
        let mut cursor = Cursor::new(b"x\n\n".to_vec());
        assert_eq!(read_py_line(&mut cursor), Ok(Some("x".to_owned())));
        assert_eq!(read_py_line(&mut cursor), Ok(Some("".to_owned())));
        assert_eq!(read_py_line(&mut cursor), Ok(None));
    }

    #[test]
    fn line_reader_reports_codec_errors_like_cpython() {
        let mut cursor = Cursor::new(b"ab\xffcd\n".to_vec());
        assert_eq!(
            read_py_line(&mut cursor),
            Err(ReadEnd::Decode(
                "'utf-8' codec can't decode byte 0xff in position 2: invalid start byte".to_owned()
            ))
        );
        let mut cursor = Cursor::new(b"ab\xc3".to_vec());
        assert_eq!(
            read_py_line(&mut cursor),
            Err(ReadEnd::Decode(
                "'utf-8' codec can't decode byte 0xc3 in position 2: unexpected end of data"
                    .to_owned()
            ))
        );
        let mut cursor = Cursor::new(b"ab\xc2Xcd\n".to_vec());
        assert_eq!(
            read_py_line(&mut cursor),
            Err(ReadEnd::Decode(
                "'utf-8' codec can't decode byte 0xc2 in position 2: invalid continuation byte"
                    .to_owned()
            ))
        );
    }

    #[test]
    fn ask_writes_bare_prompt_then_reads() {
        let (mut stdin, mut stdout, mut stderr) = test_io(b"answer\n");
        let mut io = OpsIo {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        assert_eq!(ask(&mut io, "Workspace Name: "), Ok("answer".to_owned()));
        assert_eq!(stdout, b"Workspace Name: ");
        assert!(stderr.is_empty());
    }

    #[test]
    fn ask_reports_eof_without_consuming_prompt() {
        let (mut stdin, mut stdout, mut stderr) = test_io(b"");
        let mut io = OpsIo {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        assert_eq!(ask(&mut io, "Workspace Name: "), Err(ReadEnd::Eof));
        assert_eq!(stdout, b"Workspace Name: ");
    }

    #[test]
    fn getpass_emits_fallback_bytes_on_stderr() {
        let (mut stdin, mut stdout, mut stderr) = test_io(b"secret\n");
        let mut io = OpsIo {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        assert_eq!(getpass(&mut io, "Password: "), Ok("secret".to_owned()));
        assert!(stdout.is_empty());
        assert_eq!(
            stderr,
            b"Warning: Password input may be echoed.\nPassword: \n"
        );
    }

    #[test]
    fn getpass_strips_one_newline_and_skips_it_on_eof() {
        let (mut stdin, mut stdout, mut stderr) = test_io(b"pw\r\n");
        let mut io = OpsIo {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        // Universal newlines: `\r\n` terminates (the `\r` never
        // reaches the password, as under `sys.stdin.readline`).
        assert_eq!(getpass(&mut io, "Password: "), Ok("pw".to_owned()));

        let (mut stdin, mut stdout, mut stderr) = test_io(b"");
        let mut io = OpsIo {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        assert_eq!(getpass(&mut io, "Password: "), Err(ReadEnd::Eof));
        // No trailing `\n`: `unix_getpass` raises before writing it.
        assert_eq!(
            stderr,
            b"Warning: Password input may be echoed.\nPassword: "
        );
    }

    #[test]
    fn cli_parses_ops_activate_user() {
        let ops = crate::Cli::try_parse_from(["pidash-api", "ops", "activate_user", "a@b.com"])
            .expect("ops activate_user");
        assert!(matches!(
            ops.mode,
            crate::Mode::Ops {
                command: OpsCommand::ActivateUser(_)
            }
        ));
    }
}
