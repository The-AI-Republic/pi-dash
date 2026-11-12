//! Iterative compact-JSON encoder for `serde_json::Value` (PIDASHCONV-626).
//!
//! `serde_json::to_string`/`to_vec` recurse per nesting level, which aborts
//! the process (stack overflow) on the ~9900-deep payloads the cycle ports
//! legitimately accept and enqueue (Django dumps them fine — its C encoder
//! budget covers them — so the Rust side must 200, not crash). This writer
//! emits byte-identical compact JSON (same separators, same per-scalar
//! rendering, insertion-ordered keys under `preserve_order`) with an
//! explicit stack.
//!
//! Used by the queue bind ([`crate::queue`]) and the Celery wire builder
//! ([`crate::celery`]), the two places a whole job payload serializes.
//!
//! The same depth also kills the *drop*: serde's `Map` drop recurses
//! through IndexMap/hashbrown glue (~8 frames per level), so dropping a
//! ~9900-deep object chain overflows a 2MB worker. [`drop_value_deep`]
//! dismantles iteratively; [`NewJob`](crate::queue::NewJob) drops through
//! it, keeping every enqueue site safe by construction.

use serde_json::Value;

/// Compact-JSON bytes for `value`, byte-identical to
/// `serde_json::to_vec` for every input.
pub fn to_compact_vec(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_compact(&mut out, value);
    out
}

/// Compact-JSON text for `value` (see [`to_compact_vec`]).
pub fn to_compact_string(value: &Value) -> String {
    // The writer emits ASCII structure plus `serde_json`-escaped strings,
    // so the bytes are always valid UTF-8.
    String::from_utf8(to_compact_vec(value)).expect("compact JSON is UTF-8")
}

/// Append compact JSON for `value` to `out` (see [`to_compact_vec`]).
pub fn write_compact(out: &mut Vec<u8>, value: &Value) {
    drive(out, vec![Task::Emit(value)]);
}

/// Append a compact array of borrowed items (see [`to_compact_vec`]).
pub fn write_compact_array(out: &mut Vec<u8>, items: &[Value]) {
    out.push(b'[');
    let mut tasks = vec![Task::Seal(b']')];
    push_items(&mut tasks, items);
    drive(out, tasks);
}

/// Append a compact object of borrowed entries (see [`to_compact_vec`]).
pub fn write_compact_map(out: &mut Vec<u8>, map: &serde_json::Map<String, Value>) {
    out.push(b'{');
    let mut tasks = vec![Task::Seal(b'}')];
    push_entries(&mut tasks, map);
    drive(out, tasks);
}

/// Destroy `value` without recursing: containers are emptied onto an
/// explicit work stack so every implicit drop in this loop is a scalar, an
/// emptied `Vec`, or an exhausted map iterator — all flat. Wide values use
/// heap proportional to their own size; deep chains use one slot at a time.
pub fn drop_value_deep(value: Value) {
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            Value::Array(mut items) => {
                // Moved out wholesale; the emptied `Vec` drops flat.
                stack.append(&mut items);
            }
            Value::Object(map) => {
                // `into_iter` moves the map into the iterator without
                // dropping elements; `extend` exhausts it, keys drop flat.
                stack.extend(map.into_iter().map(|(_, member)| member));
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
}

enum Task<'v> {
    Emit(&'v Value),
    Key(&'v str),
    Lit(&'static str),
    Seal(u8),
}

fn push_items<'v>(tasks: &mut Vec<Task<'v>>, items: &'v [Value]) {
    for (index, item) in items.iter().rev().enumerate() {
        if index > 0 {
            tasks.push(Task::Lit(","));
        }
        tasks.push(Task::Emit(item));
    }
}

fn push_entries<'v>(tasks: &mut Vec<Task<'v>>, map: &'v serde_json::Map<String, Value>) {
    for (index, (name, member)) in map.iter().rev().enumerate() {
        if index > 0 {
            tasks.push(Task::Lit(","));
        }
        tasks.push(Task::Emit(member));
        tasks.push(Task::Lit(":"));
        tasks.push(Task::Key(name));
    }
}

fn drive(out: &mut Vec<u8>, mut stack: Vec<Task<'_>>) {
    fn scalar(out: &mut Vec<u8>, value: &Value) {
        match value {
            Value::Null => out.extend_from_slice(b"null"),
            Value::Bool(true) => out.extend_from_slice(b"true"),
            Value::Bool(false) => out.extend_from_slice(b"false"),
            // Per-scalar serde rendering: numbers keep serde's exact text
            // (including `arbitrary_precision` spellings), strings keep
            // serde's exact escaping. Neither can nest, so neither can
            // overflow.
            Value::Number(number) => out.extend_from_slice(number.to_string().as_bytes()),
            Value::String(text) => {
                out.extend_from_slice(serde_json::to_string(text).expect("escape").as_bytes());
            }
            Value::Array(_) | Value::Object(_) => unreachable!("scalars only"),
        }
    }
    while let Some(task) = stack.pop() {
        match task {
            Task::Emit(Value::Array(items)) => {
                out.push(b'[');
                stack.push(Task::Seal(b']'));
                push_items(&mut stack, items);
            }
            Task::Emit(Value::Object(map)) => {
                out.push(b'{');
                stack.push(Task::Seal(b'}'));
                push_entries(&mut stack, map);
            }
            Task::Emit(leaf) => scalar(out, leaf),
            Task::Key(name) => {
                out.extend_from_slice(serde_json::to_string(name).expect("escape").as_bytes());
            }
            Task::Lit(text) => out.extend_from_slice(text.as_bytes()),
            Task::Seal(closer) => out.push(closer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn byte_identical_to_serde() {
        let values = vec![
            json!(null),
            json!(true),
            json!(false),
            json!(0),
            json!(-0.0),
            json!(1.5),
            json!(1e100),
            json!(""),
            json!("a\"b\\c\nd\u{1}\u{7f}é中😀"),
            json!([]),
            json!({}),
            json!([1, "x", null, true, [[]], {"a": {}}]),
            json!({"b": 1, "a": [1, {"c": "d\"e"}]}),
            json!({"": {"": [{"": ""}]}}),
            json!([[[[["deep"]]]]]),
            serde_json::from_str::<Value>("9007199254740993").unwrap(),
            serde_json::from_str::<Value>("3.141592653589793238462643383279").unwrap(),
            serde_json::from_str::<Value>("-0").unwrap(),
        ];
        for value in &values {
            assert_eq!(
                to_compact_vec(value),
                serde_json::to_vec(value).unwrap(),
                "{value:?}"
            );
        }
    }

    #[test]
    fn deep_drop_survives_small_stack() {
        // The real killer shape: nested OBJECTS (gdb showed the abort in
        // IndexMap/hashbrown drop glue, ~8 frames per level — array chains
        // drop cheaply and masked this). A plain `drop` of this value
        // aborts a 2MB thread; the dismantler must not.
        let mut deep = json!(1);
        for _ in 0..9939 {
            deep = Value::Object(serde_json::Map::from_iter([("a".to_owned(), deep)]));
        }
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || drop_value_deep(deep))
            .expect("spawn")
            .join()
            .expect("no stack overflow");
        // Wide values dismantle too (siblings share the work stack).
        let wide = Value::Array(vec![json!({"k": [1, "s", null, true]}); 1000]);
        drop_value_deep(wide);
    }

    #[test]
    fn deep_payload_survives_small_stack() {
        let mut value = json!(1);
        for _ in 0..9939 {
            value = Value::Array(vec![value]);
        }
        let text = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || to_compact_vec(&value))
            .expect("spawn")
            .join()
            .expect("no stack overflow");
        assert_eq!(text.len(), 9939 + 1 + 9939);
        assert!(text.starts_with(b"["));
        assert!(text.ends_with(b"]"));
    }
}
