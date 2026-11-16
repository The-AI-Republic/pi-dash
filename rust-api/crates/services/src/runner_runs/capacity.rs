//! Dev-machine run-queue capacity contract helpers (NEWPIDASH-241, T1-T2).
//!
//! Rust-only port of Django `capacity.py` (origin/pi-dash/pdashoss01-372)
//! per `.ai_design/dev_machine_run_queue/implementation.md` §3.
//! Keep import-light: no DB or HTTP; callers handle transactions and locks.
//! Runner → Session → Run order is owned by callers (session_open / capacity /
//! matcher / lifecycle) with remote I/O outside transactions.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use uuid::Uuid;

pub const PROTOCOL_LEGACY: i16 = 0;
pub const PROTOCOL_MACHINE_QUEUE: i16 = 1;
pub const CAPABILITY_MACHINE_QUEUE: &str = "machine_run_queue_v1";

pub const CAPACITY_BODY_LIMIT_BYTES: usize = 16 * 1024;
pub const REVISION_MAX: u64 = u64::MAX;
pub const CAPACITY_FRESH_SECS: i64 = 90;
pub const CAPACITY_MIN_LIMIT: u64 = 1;
pub const CAPACITY_MAX_LIMIT: u64 = 65535;

pub const PHASE_WAITING: &str = "waiting_for_capacity";
pub const PHASE_PREPARING: &str = "preparing";

pub const CAPACITY_ACTIONS: &[&str] = &["waiting", "admit", "defer", "reconcile"];

/// True when a session's capabilities advertise machine_run_queue_v1.
pub fn supports_machine_queue(capabilities: &Value) -> bool {
    if let Some(arr) = capabilities.as_array() {
        return arr.iter().any(|v| {
            v.as_str()
                .map(|s| s == CAPABILITY_MACHINE_QUEUE)
                .unwrap_or(false)
        });
    }
    false
}

/// Assignment protocol for a fresh assignment to a runner whose active
/// session advertises `capabilities`.
pub fn protocol_for_capabilities(capabilities: &Value) -> i16 {
    if supports_machine_queue(capabilities) {
        PROTOCOL_MACHINE_QUEUE
    } else {
        PROTOCOL_LEGACY
    }
}

/// Mint a fresh attempt token (UUIDv4) for a new assignment.
/// Centralized helper for pod drain, dedicated/direct, requeue/reassignment.
pub fn mint_attempt_token() -> Uuid {
    Uuid::new_v4()
}

/// Create a fresh attempt (token + protocol) from session capabilities.
/// All new assignments call this; redelivery preserves the stored token instead.
pub fn mint_for_capabilities(capabilities: &Value) -> (Uuid, i16) {
    (
        mint_attempt_token(),
        protocol_for_capabilities(capabilities),
    )
}

/// u64 revision or None when invalid. Rejects bools, floats,
/// negatives, overflow and non-ints.
pub fn parse_revision(value: &Value) -> Option<u64> {
    if value.is_boolean() {
        return None;
    }
    if let Some(u) = value.as_u64() {
        return Some(u);
    }
    if let Some(i) = value.as_i64() {
        if i < 0 {
            return None;
        }
        return Some(i as u64);
    }
    None
}

/// UUID for a valid UUID string, else None.
/// Mirrors Python `uuid.UUID(str(value))` for the common hyphenated form.
pub fn parse_uuid(value: &Value) -> Option<Uuid> {
    let s = value.as_str()?;
    Uuid::parse_str(s).ok()
}

/// Daemon-advertised session capabilities (additive, defaulted).
/// Missing/non-list yields []; entries trimmed to 128 chars, capped 32.
pub fn parse_session_capabilities(body: &Map<String, Value>) -> Vec<String> {
    let Some(Value::Array(arr)) = body.get("capabilities") else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .map(|s| s.chars().take(128).collect::<String>())
        .take(32)
        .collect()
}

/// Count field validation: positive (≥1) or allow_zero (≥0).
pub fn validate_count(value: &Value, allow_zero: bool, positive: bool) -> Option<u64> {
    if value.is_boolean() {
        return None;
    }
    if let Some(u) = value.as_u64() {
        if positive {
            return if u >= 1 { Some(u) } else { None };
        }
        if allow_zero {
            return Some(u);
        }
        return if u > 0 { Some(u) } else { None };
    }
    if let Some(i) = value.as_i64() {
        if i < 0 {
            return None;
        }
        let u = i as u64;
        if positive {
            return if u >= 1 { Some(u) } else { None };
        }
        if allow_zero {
            return Some(u);
        }
        return if u > 0 { Some(u) } else { None };
    }
    None
}

/// Exact attempt-token equality. No stored token only matches tokenless.
pub fn token_matches(stored: Option<Uuid>, provided: Option<Uuid>) -> bool {
    match stored {
        None => provided.is_none(),
        Some(s) => provided.is_some_and(|p| p == s),
    }
}

/// Fresh same-attempt waiting check (90s server-receipt freshness).
/// `capacity_wait` is the stored JSON object; `reported_at` is RFC3339
/// string. Falls back to fresh when reported_at absent (back-compat earlier
/// in Django; every Rust store path writes it).
pub fn is_fresh_capacity_wait(
    capacity_wait: &Value,
    current_token: Option<Uuid>,
    now: DateTime<Utc>,
) -> bool {
    let Some(obj) = capacity_wait.as_object() else {
        return false;
    };
    if obj.get("phase").and_then(Value::as_str) != Some(PHASE_WAITING) {
        return false;
    }
    let Some(token_str) = obj.get("assignment_token").and_then(Value::as_str) else {
        return false;
    };
    let Ok(payload_token) = Uuid::parse_str(token_str) else {
        return false;
    };
    let Some(cur) = current_token else {
        return false;
    };
    if cur != payload_token {
        return false;
    }
    let Some(reported_raw) = obj.get("reported_at") else {
        return true; // back-compat: treat as fresh when absent
    };
    let Some(reported_str) = reported_raw.as_str() else {
        return false;
    };
    // RFC3339 parse supporting Z and +00:00.
    let normalized = reported_str.replace('Z', "+00:00");
    let Ok(reported) = DateTime::parse_from_rfc3339(&normalized) else {
        return false;
    };
    let reported = reported.with_timezone(&Utc);
    (now - reported).num_seconds() <= CAPACITY_FRESH_SECS
}

/// Apply poll's held-token/capacity snapshot (T2).
/// This is pure validation for the poll path: it returns the reap entry
/// (possibly stripped) and the cw to persist. The actual DB write is done
/// by the caller under Runner → Run lock order.
/// Returns (reap_entry, Option<cw>) where cw is the new capacity_wait to store.
pub fn apply_poll_snapshot(
    status_entry: &Map<String, Value>,
    stored_wait: Option<&Value>,
    stored_token: Option<Uuid>,
    stored_protocol: i16,
    session_id: Uuid,
    now: DateTime<Utc>,
) -> (Map<String, Value>, Option<Value>) {
    let mut reap_entry = status_entry.clone();
    let held_raw = reap_entry
        .get("in_flight_run")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if held_raw.is_empty() {
        return (reap_entry, None);
    }
    let _in_flight = match Uuid::parse_str(&held_raw) {
        Ok(u) => u,
        Err(_) => return (reap_entry, None),
    };
    // For protocol-1, need token check
    let held_token_raw = reap_entry.get("held_assignment_token").cloned();
    let snapshot = reap_entry.get("capacity").cloned();
    let snapshot_dict: Option<&Map<String, Value>> = match snapshot.as_ref() {
        Some(Value::Object(m)) => Some(m),
        _ => None,
    };
    if held_token_raw.is_none() && snapshot.is_none() {
        return (reap_entry, None);
    }
    // Only protocol-1 has fencing
    if stored_protocol != PROTOCOL_MACHINE_QUEUE {
        return (reap_entry, None);
    }
    let provided = held_token_raw.as_ref().and_then(|v| {
        if v.is_null() {
            None
        } else {
            cap_parse_uuid_from_value(v)
        }
    });
    if !token_matches(stored_token, provided) {
        // Strip in_flight so reaper doesn't suppress
        reap_entry.insert("in_flight_run".to_string(), Value::Null);
        return (reap_entry, None);
    }
    // Validate snapshot
    if snapshot_dict.is_none() {
        return (reap_entry, None);
    }
    let snapshot_dict = snapshot_dict.unwrap();
    // revision: if present must parse, else use stored
    let stored_rev = stored_wait
        .and_then(|v| v.get("revision"))
        .and_then(Value::as_u64)
        .or_else(|| {
            stored_wait
                .and_then(|v| v.get("revision"))
                .and_then(Value::as_i64)
                .map(|i| i as u64)
        });
    let revision: Option<u64> = if let Some(raw_rev) = snapshot_dict.get("revision") {
        if raw_rev.is_null() {
            stored_rev
        } else {
            match parse_revision(raw_rev) {
                Some(r) => Some(r),
                None => return (reap_entry, None),
            }
        }
    } else {
        stored_rev
    };
    if let Some(rev) = revision {
        if let Some(stored) = stored_rev {
            if rev < stored {
                return (reap_entry, None);
            }
        }
    }
    let raw_phase = snapshot_dict
        .get("phase")
        .and_then(Value::as_str)
        .map(|s| s.to_owned());
    if let Some(ref phase) = raw_phase {
        if phase != PHASE_WAITING && phase != PHASE_PREPARING {
            return (reap_entry, None);
        }
    }
    // queued_at
    let queued_at: Option<String> = snapshot_dict
        .get("queued_at")
        .and_then(|v| if v.is_null() { None } else { Some(v.clone()) })
        .map(|v| v.as_str().unwrap_or("").to_owned())
        .or_else(|| {
            stored_wait
                .and_then(|v| v.get("queued_at"))
                .and_then(Value::as_str)
                .map(|s| s.to_owned())
        });
    if let Some(q) = snapshot_dict.get("queued_at") {
        if !q.is_null() {
            if let Some(s) = q.as_str() {
                if parse_rfc3339_str(s).is_none() {
                    return (reap_entry, None);
                }
            } else {
                return (reap_entry, None);
            }
        }
    }
    let queue_position: Option<u64> = if let Some(v) = snapshot_dict.get("queue_position") {
        if v.is_null() {
            None
        } else {
            match validate_count(v, false, true) {
                Some(n) => Some(n),
                None => return (reap_entry, None),
            }
        }
    } else {
        stored_wait
            .and_then(|v| v.get("queue_position"))
            .and_then(Value::as_u64)
    };
    let active: Option<u64> = if let Some(v) = snapshot_dict.get("active") {
        if v.is_null() {
            None
        } else {
            match validate_count(v, true, false) {
                Some(n) => Some(n),
                None => return (reap_entry, None),
            }
        }
    } else {
        stored_wait
            .and_then(|v| v.get("active"))
            .and_then(Value::as_u64)
    };
    let limit: Option<u64> = if let Some(v) = snapshot_dict.get("limit") {
        if v.is_null() {
            None
        } else {
            match validate_count(v, false, true) {
                Some(n) if (CAPACITY_MIN_LIMIT..=CAPACITY_MAX_LIMIT).contains(&n) => Some(n),
                _ => return (reap_entry, None),
            }
        }
    } else {
        stored_wait
            .and_then(|v| v.get("limit"))
            .and_then(Value::as_u64)
    };
    let stored_phase = stored_wait
        .and_then(|v| v.get("phase"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let phase = raw_phase.unwrap_or_else(|| {
        if stored_phase.is_empty() {
            PHASE_WAITING.to_owned()
        } else {
            stored_phase.clone()
        }
    });
    let phase = if stored_phase == PHASE_PREPARING && phase == PHASE_WAITING {
        PHASE_PREPARING.to_owned()
    } else {
        phase
    };
    let queue_position = if phase == PHASE_PREPARING {
        None
    } else {
        queue_position
    };
    let mut cw = serde_json::json!({
        "phase": phase,
        "assignment_token": stored_token.map(|u| Value::String(u.to_string())).unwrap_or(Value::Null),
        "session_id": session_id.to_string(),
        "revision": revision,
        "queued_at": queued_at.map(Value::String).unwrap_or(Value::Null),
        "reported_at": rfc3339_now_string_pub(now),
        "queue_position": queue_position.map(Value::from).unwrap_or(Value::Null),
        "active": active.map(Value::from).unwrap_or(Value::Null),
        "limit": limit.map(Value::from).unwrap_or(Value::Null),
    });
    if phase == PHASE_PREPARING {
        if let Some(admission) = stored_wait
            .and_then(|v| v.get("admission_id"))
            .and_then(Value::as_str)
        {
            cw.as_object_mut().unwrap().insert(
                "admission_id".to_string(),
                Value::String(admission.to_owned()),
            );
        }
    }
    (reap_entry, Some(cw))
}

fn cap_parse_uuid_from_value(v: &Value) -> Option<Uuid> {
    v.as_str().and_then(|s| Uuid::parse_str(s).ok())
}
fn parse_rfc3339_str(s: &str) -> Option<DateTime<Utc>> {
    let normalized = s.replace('Z', "+00:00");
    chrono::DateTime::parse_from_rfc3339(&normalized)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}
fn rfc3339_now_string_pub(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}

/// Fence protocol-1 attempt writes (Accept/Started/terminal).
/// For protocol-1, require session (when require_session) and exact token.
/// Returns Err(Response) with 400/409 as appropriate, Ok otherwise.
/// Must be called inside Runner → Run locked transaction (no I/O).
pub fn fence_attempt(
    protocol: i16,
    session_id: Option<Uuid>,
    active_session_id: Option<Uuid>,
    provided_token: Option<Uuid>,
    stored_token: Option<Uuid>,
    require_session: bool,
) -> Result<(), String> {
    if protocol != PROTOCOL_MACHINE_QUEUE {
        return Ok(());
    }
    if require_session {
        let Some(sid) = session_id else {
            return Err("session_id_required".to_owned());
        };
        let Some(active) = active_session_id else {
            return Err("session_evicted".to_owned());
        };
        if sid != active {
            return Err("session_evicted".to_owned());
        }
    }
    if !token_matches(stored_token, provided_token) {
        return Err("stale_assignment".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn revision_validation() {
        assert_eq!(parse_revision(&json!(0)), Some(0));
        assert_eq!(parse_revision(&json!(u64::MAX)), Some(u64::MAX));
        assert_eq!(parse_revision(&json!(-1)), None);
        assert_eq!(parse_revision(&json!(1.5)), None);
        assert_eq!(parse_revision(&json!(true)), None);
        assert_eq!(parse_revision(&json!("3")), None);
        // float that looks like int should be rejected
        assert_eq!(parse_revision(&json!(3.0)), None);
    }

    #[test]
    fn uuid_parse_and_token_match() {
        let v = json!("00000000-0000-0000-0000-000000000001");
        assert!(parse_uuid(&v).is_some());
        assert!(parse_uuid(&json!("not-a-uuid")).is_none());
        let a = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let b = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        assert!(token_matches(None, None));
        assert!(!token_matches(None, Some(a)));
        assert!(!token_matches(Some(a), None));
        assert!(token_matches(Some(a), Some(a)));
        assert!(!token_matches(Some(a), Some(b)));
    }

    #[test]
    fn capabilities_and_protocol() {
        assert!(supports_machine_queue(&json!(["machine_run_queue_v1"])));
        assert!(supports_machine_queue(&json!([
            "machine_run_queue_v1",
            "other"
        ])));
        assert!(!supports_machine_queue(&json!(["other"])));
        assert!(!supports_machine_queue(&json!([])));
        assert!(!supports_machine_queue(&json!("machine_run_queue_v1")));
        assert_eq!(
            protocol_for_capabilities(&json!(["machine_run_queue_v1"])),
            1
        );
        assert_eq!(protocol_for_capabilities(&json!([])), 0);
    }

    #[test]
    fn session_capabilities_parsing() {
        let body = json!({"capabilities": ["machine_run_queue_v1", "agent:muse_code"]});
        let caps = parse_session_capabilities(body.as_object().unwrap());
        assert_eq!(caps, vec!["machine_run_queue_v1", "agent:muse_code"]);
        // missing / non-list
        assert!(parse_session_capabilities(json!({}).as_object().unwrap()).is_empty());
        assert!(
            parse_session_capabilities(json!({"capabilities": "nope"}).as_object().unwrap())
                .is_empty()
        );
        // non-string entries filtered, length cap
        let body2 = json!({"capabilities": [1, "ok", 2, "x".repeat(200)]});
        let caps2 = parse_session_capabilities(body2.as_object().unwrap());
        assert_eq!(caps2.len(), 2);
        assert!(caps2[1].len() <= 128);
    }

    #[test]
    fn count_validation() {
        assert_eq!(validate_count(&json!(0), true, false), Some(0));
        assert_eq!(validate_count(&json!(0), false, true), None);
        assert_eq!(validate_count(&json!(1), false, true), Some(1));
        assert_eq!(validate_count(&json!(-1), true, false), None);
        assert_eq!(validate_count(&json!(1.5), true, false), None);
        assert_eq!(validate_count(&json!(true), true, false), None);
    }

    #[test]
    fn fresh_wait_freshness() {
        let now = Utc::now();
        let tok = Uuid::parse_str("00000000-0000-0000-0000-000000000011").unwrap();
        let tok_str = tok.to_string();
        let fresh = json!({
            "phase": "waiting_for_capacity",
            "assignment_token": tok_str,
            "reported_at": now.to_rfc3339()
        });
        assert!(is_fresh_capacity_wait(&fresh, Some(tok), now));
        // stale: reported 100s ago
        let stale = json!({
            "phase": "waiting_for_capacity",
            "assignment_token": tok_str,
            "reported_at": (now - chrono::Duration::seconds(100)).to_rfc3339()
        });
        assert!(!is_fresh_capacity_wait(&stale, Some(tok), now));
        // token mismatch
        let other = Uuid::parse_str("00000000-0000-0000-0000-000000000012").unwrap();
        assert!(!is_fresh_capacity_wait(&fresh, Some(other), now));
        // preparing never qualifies
        let prep = json!({"phase": "preparing", "assignment_token": tok_str});
        assert!(!is_fresh_capacity_wait(&prep, Some(tok), now));
        // missing reported_at treated as fresh (back-compat)
        let no_time = json!({"phase": "waiting_for_capacity", "assignment_token": tok_str});
        assert!(is_fresh_capacity_wait(&no_time, Some(tok), now));
    }
}
