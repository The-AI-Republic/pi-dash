//! Loop eligibility predicate logic (D-03, stage 4).
//!
//! Pure port of `apps/api/pi_dash/loop/eligibility.py:119-154`
//! ([`check`]) and the settings-read helpers
//! (`loop/views.py:24-49`, [`master_enabled`] / [`job_enabled`]).
//! The SQL half of this issue lives in `pidash-db`
//! (`db/src/loop/queries.rs`); this module takes already-resolved
//! inputs so the precedence order is unit-testable with no database.
//!
//! Precedence is fixed so the recorded skip reason is deterministic
//! (`eligibility.py:122-124`): master pause → job opt-out →
//! membership/role → LLM credentials. Each argument traces to the
//! queryset or row read that resolves it:
//!
//! | Argument | Python source |
//! | --- | --- |
//! | `master_paused` | `_master_paused_q` (`:77-85`) / `check` `:126-129` |
//! | `job_opted_out` | `_job_off_q` (`:66-74`) / `check` `:131-134` |
//! | `membership_role` | `_member_q` (`:54-63`) / `check` `:136-150` |
//! | `min_role` | `job.min_role` (`check` `:149`) |
//! | `has_llm` | `llm_available_q` (`:38-42`) / `user_has_llm` (`:45-51`) |
//!
//! The Cloud seam note (`eligibility.py:11-17`) applies unchanged: the
//! BYOK check behind `has_llm` is the one overridable function, shared
//! by the scanner pre-filter and the fire-time re-check so they cannot
//! diverge.

use std::collections::HashSet;

use pidash_db::r#loop::SkipReason;

/// Fire-time re-check of one claimed target
/// (`eligibility.py:119-154`, `check`).
///
/// Returns the [`SkipReason`] that `last_skip_reason` must record, or
/// `None` when the target is eligible. `membership_role = None` means
/// no active membership row (`membership is None`, `:147-148`);
/// otherwise the role value compares against the job's `min_role`
/// (`:149-150`, the row form of the `role__gte` annotation).
pub fn check(
    master_paused: bool,
    job_opted_out: bool,
    membership_role: Option<i32>,
    min_role: i32,
    has_llm: bool,
) -> Option<SkipReason> {
    if master_paused {
        return Some(SkipReason::MasterPaused);
    }
    if job_opted_out {
        return Some(SkipReason::UserDisabled);
    }
    let role = match membership_role {
        None => return Some(SkipReason::MembershipGone),
        Some(role) => role,
    };
    if role < min_role {
        return Some(SkipReason::MinRole);
    }
    if !has_llm {
        return Some(SkipReason::LlmConfigMissing);
    }
    None
}

/// Master-switch read (`views.py:24-30`, `_master_enabled`): the live
/// NULL-job row's `enabled`, or `true` when the row is absent
/// (`True if pref is None else bool(pref)`).
pub fn master_enabled(stored: Option<bool>) -> bool {
    stored.unwrap_or(true)
}

/// Per-job card state (`views.py:33-40` + `:47`, `_job_enabled_map` /
/// `_settings_payload`): a job is enabled unless its id is in the
/// user's live opt-out set.
pub fn job_enabled(off_job_ids: &HashSet<uuid::Uuid>, job_id: &uuid::Uuid) -> bool {
    !off_job_ids.contains(job_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(n: u8) -> uuid::Uuid {
        uuid::Uuid::from_bytes([n; 16])
    }

    /// Every precedence level wins over everything below it, and the
    /// fully-eligible input returns `None`. Vectors trace to
    /// `eligibility.py:126-154`.
    #[test]
    fn check_precedence_vectors() {
        // Eligible on all inputs — dispatches.
        assert_eq!(check(false, false, Some(15), 15, true), None);
        // Master pause beats everything, even a fully eligible edge.
        assert_eq!(
            check(true, true, None, 15, false),
            Some(SkipReason::MasterPaused)
        );
        // Job opt-out beats membership/role/LLM.
        assert_eq!(
            check(false, true, None, 15, false),
            Some(SkipReason::UserDisabled)
        );
        // Gone membership beats a low role and missing creds…
        assert_eq!(
            check(false, false, None, 15, true),
            Some(SkipReason::MembershipGone)
        );
        // …and a present-but-low role reports min_role, not gone.
        assert_eq!(
            check(false, false, Some(5), 15, true),
            Some(SkipReason::MinRole)
        );
        // Equal role passes (gte, not gt).
        assert_eq!(
            check(false, false, Some(15), 15, false),
            Some(SkipReason::LlmConfigMissing)
        );
        // Missing creds are last.
        assert_eq!(
            check(false, false, Some(20), 15, false),
            Some(SkipReason::LlmConfigMissing)
        );
        assert_eq!(check(false, false, Some(20), 15, true), None);
    }

    /// The eligible fixture row (member, role 15, LLM creds) must
    /// check out as eligible, and the excluded guest row (role 5 <
    /// min_role 15) as `min_role` — mirroring
    /// `eligible_due_targets.rows.json`.
    #[test]
    fn check_replays_eligible_fixture_verdicts() {
        let path = format!(
            "{}/../../fixtures/loop/queries/eligible_due_targets.rows.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let fixture: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("eligible fixture exists"))
                .expect("eligible fixture parses");
        assert_eq!(fixture["rows"][0]["user_email"], "fxloop-member@e.com");
        assert_eq!(check(false, false, Some(15), 15, true), None);
        assert_eq!(fixture["excluded"][0]["check"], "min_role");
        assert_eq!(
            check(false, false, Some(5), 15, true),
            Some(SkipReason::MinRole)
        );
        assert_eq!(SkipReason::MinRole.as_str(), "min_role");
    }

    /// Absent master row reads as enabled; stored values pass through
    /// (`views.py:30`).
    #[test]
    fn master_enabled_defaults_true() {
        assert!(master_enabled(None));
        assert!(master_enabled(Some(true)));
        assert!(!master_enabled(Some(false)));
    }

    /// Only live opt-out ids disable their card (`views.py:33-40`).
    #[test]
    fn job_enabled_follows_off_set() {
        let off: HashSet<uuid::Uuid> = [uid(1)].into_iter().collect();
        assert!(!job_enabled(&off, &uid(1)));
        assert!(job_enabled(&off, &uid(2)));
        assert!(job_enabled(&HashSet::new(), &uid(1)));
    }
}
