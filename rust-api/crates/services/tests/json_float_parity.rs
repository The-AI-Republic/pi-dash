//! PIDASHCONV-705: JSON float parsing must be correctly rounded (CPython parity).
//!
//! Without serde_json's `float_roundtrip` feature, floats parse via naive
//! significand-times-pow10 math that lands 1 ulp off Python on knife-edge
//! long decimals. This crate's standalone graph has no
//! `arbitrary_precision`, so it is the graph the workspace `float_roundtrip`
//! feature (enabled on pidash-types) repairs. Each vector pins the exact f64
//! bits CPython's `float()` produces; the api-graph half of this check lives
//! in `crates/api/tests/json_float_parity.rs` and both must agree.

/// (JSON number text, CPython `float(text)` bits).
const VECTORS: &[(&str, u64)] = &[
    ("123456789.12345679", 0x419d6f34547e6b75),
    ("1.59773974325455500e250", 0x73e1da3f024c95bf),
    ("6.45497157590288052e276", 0x79674e11a59b73b4),
    ("3.23793006326237676e218", 0x6d4d5a297ad77bca),
    // Short realistic decimals: exact under both parsers (no-overcorrection guard).
    ("12.5", 0x4029000000000000),
    ("100.00", 0x4059000000000000),
    ("1e20", 0x4415af1d78b58c40),
];

#[test]
fn long_decimal_floats_parse_like_cpython() {
    for (text, want) in VECTORS {
        let direct: f64 = serde_json::from_str(text).expect("valid JSON number");
        assert_eq!(direct.to_bits(), *want, "f64 mis-parse of {text}");
        let value: serde_json::Value = serde_json::from_str(text).expect("valid JSON");
        assert_eq!(
            value.as_f64().expect("number is f64").to_bits(),
            *want,
            "Value::as_f64 mis-parse of {text}"
        );
    }
}
