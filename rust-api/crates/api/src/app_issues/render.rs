#![forbid(unsafe_code)]

//! The v2 `paginate()` page math (`utils/global_paginator.py`).
//!
//! `PaginateCursor(size:page:offset)` slices the annotated queryset over
//! `updated_at`; the offset part of the cursor is accepted but never read
//! (a ported quirk — `start_index` derives from `current_page` alone).
//! An unparsable cursor or a zero page size raises uncaught in Python and
//! lands in the generic 500.

/// One v2 page: slice bounds plus rendered cursor strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V2Page {
    pub page_size: i64,
    pub start: i64,
    pub end: i64,
    pub prev_cursor: String,
    pub cursor: String,
    pub next_cursor: Option<String>,
    pub prev_page_results: bool,
    pub next_page_results: bool,
    pub total_pages: i64,
}

/// Why a v2 page failed: an unparsable cursor or a zero page size both
/// raise uncaught in Python (generic 500).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V2PageError;

pub fn v2_page(cursor: Option<&str>, total_results: i64) -> Result<V2Page, V2PageError> {
    let fail = || V2PageError;
    const MAX_LIMIT: i64 = 1000;
    let (size, current) = match cursor {
        None => (MAX_LIMIT, 0),
        Some(raw) => {
            let bits: Vec<&str> = raw.split(':').collect();
            if bits.len() != 3 {
                return Err(fail());
            }
            let size = bits[0].parse::<i64>().map_err(|_| fail())?;
            let current = bits[1].parse::<i64>().map_err(|_| fail())?;
            bits[2].parse::<i64>().map_err(|_| fail())?;
            (size, current)
        }
    };
    if size == 0 {
        // `ceil(total / 0)` raises in Python → generic 500.
        return Err(fail());
    }
    let page_size = size.min(MAX_LIMIT);
    // Both operands are non-negative here (`size == 0` returned above),
    // so this is exactly `math.ceil`.
    let total_pages =
        total_results.div_euclid(page_size) + i64::from(total_results.rem_euclid(page_size) != 0);
    let start = if current > 0 { current * page_size } else { 0 };
    let end = (start + page_size).min(total_results);
    let next_cursor = if end < total_results {
        Some(format!("{page_size}:{}:0", current + 1))
    } else {
        None
    };
    Ok(V2Page {
        page_size,
        start,
        end,
        prev_cursor: format!("{page_size}:{}:0", current - 1),
        cursor: format!("{page_size}:{current}:0"),
        next_cursor: next_cursor.clone(),
        prev_page_results: current > 0,
        next_page_results: next_cursor.is_some(),
        total_pages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_first_page_cursors() {
        let page = v2_page(None, 2500).expect("page");
        assert_eq!(page.page_size, 1000);
        assert_eq!((page.start, page.end), (0, 1000));
        assert_eq!(page.prev_cursor, "1000:-1:0");
        assert_eq!(page.cursor, "1000:0:0");
        assert_eq!(page.next_cursor, Some("1000:1:0".to_owned()));
        assert!(!page.prev_page_results);
        assert!(page.next_page_results);
        assert_eq!(page.total_pages, 3);
    }

    #[test]
    fn v2_last_page_has_no_next() {
        let page = v2_page(Some("1000:2:0"), 2500).expect("page");
        assert_eq!((page.start, page.end), (2000, 2500));
        assert_eq!(page.next_cursor, None);
        assert!(page.prev_page_results);
        assert!(!page.next_page_results);
    }

    #[test]
    fn v2_bad_cursor_and_zero_page_are_500s() {
        assert!(v2_page(Some("nope"), 10).is_err());
        assert!(v2_page(Some("0:0:0"), 10).is_err());
    }
}
