//! A configuration value.
//!
//! The Python registry holds heterogeneous defaults (`None`, strings, ints,
//! floats, bools) and the environment/DB always yield strings, so one enum
//! carries every shape instead of forcing everything through `String` and
//! losing the difference between `None` and `""` or `False` and `"False"`.

/// One configuration value, mirroring what Python's `get_config` returns.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    /// Missing: env var unset, no DB row, or an explicit `None` default.
    Null,
    /// A string: every env-var read and every stored DB row value.
    Str(String),
    /// An integer registry default (e.g. `FILE_SIZE_LIMIT = 5242880`).
    Int(i64),
    /// A float registry default (e.g. `REDIS_SOCKET_CONNECT_TIMEOUT = 2.0`).
    Float(f64),
    /// A boolean registry default (e.g. `GITHUB_ACCESS_TOKEN = False`).
    Bool(bool),
}

impl ConfigValue {
    /// The project's `get_bool` convention: truthy iff the value is exactly
    /// the string `"1"`. An integer `1` or boolean `true` is NOT truthy here —
    /// Python compares with `== "1"`, so only `Str("1")` passes.
    pub fn is_flag_set(&self) -> bool {
        matches!(self, ConfigValue::Str(s) if s == "1")
    }

    /// Coerce like Python's `int(value)`: integers pass through, floats
    /// truncate toward zero, bools become 0/1, strings parse with Python's
    /// rules (surrounding whitespace, one leading sign, digits with single
    /// underscores between them). Anything else — `Null`, unparseable
    /// strings, out-of-range floats — is `None` and the caller falls back.
    pub fn to_int(&self) -> Option<i64> {
        match self {
            ConfigValue::Null => None,
            ConfigValue::Bool(b) => Some(i64::from(*b)),
            ConfigValue::Int(i) => Some(*i),
            ConfigValue::Float(f) => {
                if f.is_finite() && *f >= i64::MIN as f64 && *f <= i64::MAX as f64 {
                    Some(*f as i64)
                } else {
                    None
                }
            }
            ConfigValue::Str(s) => parse_python_int(s),
        }
    }

    /// Coerce like Python's `float(value)`: integers and bools widen,
    /// floats pass through, strings parse (including `"inf"`, `"nan"` and
    /// exponents, like Python). Anything else is `None`.
    pub fn to_float(&self) -> Option<f64> {
        match self {
            ConfigValue::Null => None,
            ConfigValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            ConfigValue::Int(i) => Some(*i as f64),
            ConfigValue::Float(f) => Some(*f),
            ConfigValue::Str(s) => parse_python_float(s),
        }
    }

    /// String view: `Str` as-is, numbers and bools in their plain rendering,
    /// `Null` as `None`. Used for diagnostics, never for wire output.
    pub fn as_display(&self) -> Option<String> {
        match self {
            ConfigValue::Null => None,
            ConfigValue::Str(s) => Some(s.clone()),
            ConfigValue::Int(i) => Some(i.to_string()),
            ConfigValue::Float(f) => Some(float_repr(*f)),
            ConfigValue::Bool(b) => Some(b.to_string()),
        }
    }
}

/// Parse a string with CPython `float()` rules for the common cases:
/// surrounding whitespace ignored, decimal/integer/exponent forms plus
/// case-insensitive `inf`/`infinity`/`nan` with optional sign. Hex float
/// syntax and underscores are rejected (Python's `float()` rejects
/// underscores too). Unrepresentable magnitudes yield `None` here rather
/// than Python's `inf`.
fn parse_python_float(s: &str) -> Option<f64> {
    let t = s.trim_matches(|c: char| c.is_whitespace());
    if t.is_empty() || t.contains('_') {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    if ["inf", "infinity", "+inf", "+infinity", "-inf", "-infinity"].contains(&lower.as_str())
        || ["nan", "+nan", "-nan"].contains(&lower.as_str())
    {
        return t.parse().ok();
    }
    let f: f64 = t.parse().ok()?;
    if f.is_finite() {
        Some(f)
    } else {
        None
    }
}

/// Render a float the way Python's `str()` does for whole values (`2.0`, not
/// `2`), so registry defaults print recognisably in diagnostics.
fn float_repr(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// Parse a string with CPython `int(s, 10)` rules for the common cases:
/// surrounding ASCII whitespace is ignored, one leading `+`/`-` is allowed,
/// and single underscores may separate digits (`"1_0"` is 10). Hex, octal and
/// binary prefixes, exponents and decimal points are rejected, as is an
/// empty digit run. Overflow is `None` (Python would yield a big int; config
/// magnitudes always fit `i64`).
fn parse_python_int(s: &str) -> Option<i64> {
    let t = s.trim_matches(|c: char| c.is_whitespace());
    let (t, negative) = match t.strip_prefix(['+', '-']) {
        Some(rest) => (rest, s.trim_start().starts_with('-')),
        None => (t, false),
    };
    if t.is_empty() {
        return None;
    }
    let mut digits = String::with_capacity(t.len());
    let mut prev_underscore = true; // leading '_' is invalid
    let mut seen_digit = false;
    for c in t.chars() {
        if c == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else if c.is_ascii_digit() {
            digits.push(c);
            prev_underscore = false;
            seen_digit = true;
        } else {
            return None;
        }
    }
    if !seen_digit || prev_underscore {
        return None;
    }
    let magnitude: i64 = digits.parse().ok()?;
    if negative {
        magnitude.checked_neg()
    } else {
        Some(magnitude)
    }
}

impl From<String> for ConfigValue {
    fn from(s: String) -> Self {
        ConfigValue::Str(s)
    }
}

impl From<&str> for ConfigValue {
    fn from(s: &str) -> Self {
        ConfigValue::Str(s.to_owned())
    }
}

impl From<i64> for ConfigValue {
    fn from(i: i64) -> Self {
        ConfigValue::Int(i)
    }
}

impl From<i32> for ConfigValue {
    fn from(i: i32) -> Self {
        ConfigValue::Int(i64::from(i))
    }
}

impl From<f64> for ConfigValue {
    fn from(f: f64) -> Self {
        ConfigValue::Float(f)
    }
}

impl From<bool> for ConfigValue {
    fn from(b: bool) -> Self {
        ConfigValue::Bool(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_coercion_matches_python() {
        assert_eq!(ConfigValue::from(2).to_float(), Some(2.0));
        assert_eq!(ConfigValue::from(true).to_float(), Some(1.0));
        assert_eq!(ConfigValue::from(2.5).to_float(), Some(2.5));
        assert_eq!(ConfigValue::from("5.0").to_float(), Some(5.0));
        assert_eq!(ConfigValue::from(" 1e3 ").to_float(), Some(1000.0));
        assert_eq!(ConfigValue::Null.to_float(), None);
        assert_eq!(ConfigValue::from("abc").to_float(), None);
        assert_eq!(ConfigValue::from("1_0").to_float(), None);
        assert_eq!(ConfigValue::from("1e999").to_float(), None);
    }

    #[test]
    fn flag_is_only_string_one() {
        assert!(ConfigValue::from("1").is_flag_set());
        assert!(!ConfigValue::from("true").is_flag_set());
        assert!(!ConfigValue::from("01").is_flag_set());
        assert!(!ConfigValue::from(1).is_flag_set());
        assert!(!ConfigValue::from(true).is_flag_set());
        assert!(!ConfigValue::Null.is_flag_set());
    }

    #[test]
    fn int_coercion_matches_python() {
        assert_eq!(ConfigValue::from("587").to_int(), Some(587));
        assert_eq!(ConfigValue::from("  -42 ").to_int(), Some(-42));
        assert_eq!(ConfigValue::from("+7").to_int(), Some(7));
        assert_eq!(ConfigValue::from("1_0").to_int(), Some(10));
        assert_eq!(ConfigValue::from(5242880).to_int(), Some(5242880));
        assert_eq!(ConfigValue::from(2.9).to_int(), Some(2));
        assert_eq!(ConfigValue::from(-2.9).to_int(), Some(-2));
        assert_eq!(ConfigValue::from(true).to_int(), Some(1));
        assert_eq!(ConfigValue::from(false).to_int(), Some(0));
        assert_eq!(ConfigValue::Null.to_int(), None);
        assert_eq!(ConfigValue::from("5.0").to_int(), None);
        assert_eq!(ConfigValue::from("not-a-number").to_int(), None);
        assert_eq!(ConfigValue::from("").to_int(), None);
        assert_eq!(ConfigValue::from("0x10").to_int(), None);
        assert_eq!(ConfigValue::from("1__0").to_int(), None);
        assert_eq!(ConfigValue::from("_10").to_int(), None);
        assert_eq!(ConfigValue::from("10_").to_int(), None);
        assert_eq!(ConfigValue::from("9999999999999999999999").to_int(), None);
    }
}
