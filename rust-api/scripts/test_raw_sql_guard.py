#!/usr/bin/env python3
"""Unit tests for raw_sql_guard.py (stdlib unittest, no DB).

    python3 scripts/test_raw_sql_guard.py
"""

import sys
import tempfile
import textwrap
import unittest
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import raw_sql_guard as guard  # noqa: E402


def dup_names(sql):
    names = guard.select_output_names(sql) or []
    return sorted(name for name, n in Counter(names).items() if n > 1)


def sites(src):
    return guard.count_sites(textwrap.dedent(src))[0]


class SelectOutputNames(unittest.TestCase):
    def test_duplicates_survive_unresolvable_columns(self):
        self.assertEqual(dup_names("SELECT i.id, s.id, COALESCE(x,0) FROM t"), ["id"])
        self.assertEqual(dup_names("SELECT a.id, b.*, s.id FROM t"), ["id"])

    def test_alias_only_at_top_level(self):
        self.assertEqual(dup_names("SELECT CAST(a AS text), CAST(b AS text) FROM t"), [])

    def test_implicit_alias(self):
        self.assertEqual(dup_names("SELECT i.name n1, s.name n2 FROM t"), [])
        self.assertEqual(dup_names("SELECT i.name n1, s.name n1 FROM t"), ["n1"])
        self.assertEqual(dup_names("SELECT COUNT(*) n, MAX(z) n FROM t"), ["n"])

    def test_keywords_are_not_aliases(self):
        sql = "SELECT CASE WHEN a THEN 1 ELSE 0 END, CASE WHEN b THEN 1 ELSE 0 END FROM t"
        self.assertEqual(dup_names(sql), [])

    def test_commas_inside_literals(self):
        self.assertEqual(dup_names("SELECT 'a,b' AS x, y.x FROM t"), ["x"])


class CountSites(unittest.TestCase):
    def test_sqlx_entry_points(self):
        self.assertEqual(sites('fn f() { sqlx::query("SELECT 1"); }'), 1)
        self.assertEqual(sites('fn f() { sqlx::query_as::<_, (i32,)>("SELECT 1"); }'), 1)
        self.assertEqual(sites('fn f() { sqlx::query!("SELECT 1"); sqlx::raw_sql("x"); }'), 2)
        self.assertEqual(sites('fn f() { let q = sqlx::QueryBuilder::new("UPDATE t SET "); }'), 1)

    def test_bare_query_needs_a_sqlx_import(self):
        self.assertEqual(sites('fn f() { query("x"); }'), 0)
        self.assertEqual(sites('use sqlx::query;\nfn f() { query("x"); }'), 1)

    def test_positional_reads_in_every_form(self):
        src = """
        fn f(r: &PgRow) {
            let _ = r.try_get(0);
            let _ = r.try_get::<String, _>(2);
            let _ = sqlx::Row::try_get(r, 3);
            let _ = r.try_get("name");
        }
        """
        self.assertEqual(sites(src), 3)

    def test_seaorm_escape_hatches_but_not_any_raw_method(self):
        src = """
        fn f() {
            Statement::from_string(b, "x");
            Expr::cust("x");
            stack.pop_raw();
        }
        """
        self.assertEqual(sites(src), 2)

    def test_reason_covers_its_line_or_the_next_one_when_alone(self):
        same_line = 'fn f() {\n    sqlx::query("a"); // raw-sql: lock\n    sqlx::query("b");\n}'
        self.assertEqual(sites(same_line), 1)
        alone = 'fn f() {\n    // raw-sql: advisory lock\n    sqlx::query("a");\n    sqlx::query("b");\n}'
        self.assertEqual(sites(alone), 1)

    def test_test_code_is_not_counted(self):
        src = """
        fn f() { sqlx::query("a"); }
        #[cfg(test)]
        mod tests { fn t() { sqlx::query("b"); } }
        """
        self.assertEqual(sites(src), 1)


class Collect(unittest.TestCase):
    def test_cfg_test_sibling_files_and_test_literals_are_skipped(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "crates" / "c" / "src"
            src.mkdir(parents=True)
            (src / "lib.rs").write_text(
                "#[cfg(test)]\nmod support;\n"
                'pub fn f() { sqlx::query("SELECT 1"); }\n'
                '#[cfg(test)]\nmod tests { const Q: &str = r#"\n SELECT a.id, b.id FROM t"#; }\n'
            )
            (src / "support.rs").write_text('pub fn s() { sqlx::query("SELECT 2"); }\n')
            counts, dups = guard.collect(root)
            self.assertEqual(counts, {"crates/c/src/lib.rs": 1})
            self.assertEqual(dups, {})

    def test_production_literal_with_leading_space_is_checked(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "crates" / "c" / "src"
            src.mkdir(parents=True)
            (src / "lib.rs").write_text('pub const Q: &str = " SELECT i.id, s.id FROM t";\n')
            _, dups = guard.collect(root)
            self.assertEqual(list(dups.values()), [["id"]])


if __name__ == "__main__":
    unittest.main()
