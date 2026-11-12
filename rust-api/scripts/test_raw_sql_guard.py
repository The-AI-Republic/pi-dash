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


class OutputLists(unittest.TestCase):
    def test_escaped_quotes_in_ordinary_strings(self):
        self.assertEqual(dup_names(r'SELECT \"issues\".\"id\", \"states\".\"id\" FROM t'), ["id"])
        self.assertEqual(dup_names(r'SELECT a.x AS \"id\", s.id FROM t'), ["id"])

    def test_escaped_newlines_and_leading_whitespace(self):
        self.assertEqual(dup_names(r"SELECT a.id, b.id\nFROM t"), ["id"])
        self.assertEqual(dup_names(r"\n  SELECT a.id, b.id FROM t"), ["id"])

    def test_cte_main_select_and_returning(self):
        sql = "WITH x AS (SELECT 1 AS id, 2 AS id) SELECT a.id, b.id FROM x"
        self.assertEqual(dup_names(sql), ["id"])
        self.assertEqual(dup_names("UPDATE t SET a = 1 RETURNING t.id, s.id"), ["id"])
        self.assertEqual(dup_names("INSERT INTO t (a) VALUES (1) RETURNING id"), [])

    def test_is_distinct_from_does_not_end_the_list(self):
        sql = "SELECT a.x IS DISTINCT FROM b.x AS d, c.id, e.id FROM t"
        self.assertEqual(dup_names(sql), ["id"])
        self.assertEqual(dup_names("SELECT EXTRACT(year FROM a.t) AS y, b.id FROM t"), [])

    def test_operator_words_are_not_aliases(self):
        self.assertEqual(dup_names("SELECT a.flag AND b.flag, c.flag FROM t"), [])
        self.assertEqual(dup_names("SELECT a.x = b.x, c.x FROM t"), [])

    def test_quoted_names_keep_their_case(self):
        self.assertEqual(dup_names('SELECT "Foo", foo FROM t'), [])
        self.assertEqual(dup_names('SELECT a."Foo", b."Foo" FROM t'), ["Foo"])


class CountSites(unittest.TestCase):
    def test_sqlx_entry_points(self):
        self.assertEqual(sites('fn f() { sqlx::query("SELECT 1"); }'), 1)
        self.assertEqual(sites('fn f() { sqlx::query_as::<_, (i32,)>("SELECT 1"); }'), 1)
        self.assertEqual(sites('fn f() { sqlx::query!("SELECT 1"); sqlx::raw_sql("x"); }'), 2)
        self.assertEqual(sites('fn f() { let q = sqlx::QueryBuilder::new("UPDATE t SET "); }'), 1)

    def test_any_path_prefix_counts(self):
        self.assertEqual(sites('fn f() { ::sqlx::query("a"); crate::sqlx::query("b"); }'), 2)

    def test_aliased_import(self):
        self.assertEqual(sites('use sqlx::query as q;\nfn f() { q("a"); }'), 1)

    def test_methods_and_definitions_named_query_do_not_count(self):
        src = 'use sqlx::query_as;\nfn query(x: u8) {}\nfn f(u: Uri) { let _ = u.query(); }'
        self.assertEqual(sites(src), 0)

    def test_reason_text_inside_a_string_is_not_a_reason(self):
        self.assertEqual(sites('fn f() { let s = "// raw-sql: y"; sqlx::query("a"); }'), 1)

    def test_block_comment_reason(self):
        self.assertEqual(sites('fn f() { sqlx::query("a"); /* raw-sql: advisory lock */ }'), 0)

    def test_positional_get_turbofish_and_unchecked(self):
        src = """
        fn f(r: &PgRow) {
            let _ = r.get::<String, _>(1);
            let _ = r.try_get_unchecked(0);
            let _ = items.get(0);
        }
        """
        self.assertEqual(sites(src), 2)

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

    def test_cfg_test_mod_in_a_non_mod_file_resolves_beside_it_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "crates" / "c" / "src"
            (src / "handlers").mkdir(parents=True)
            (src / "lib.rs").write_text("mod handlers;\nmod db;\n")
            (src / "handlers.rs").write_text("#[cfg(test)]\nmod db;\n")
            (src / "handlers" / "db.rs").write_text('fn t() { sqlx::query("t"); }\n')
            (src / "db.rs").write_text('pub fn p() { sqlx::query("p"); }\n')
            counts, _ = guard.collect(root)
            self.assertEqual(counts, {"crates/c/src/db.rs": 1})

    def test_a_second_copy_of_a_duplicate_statement_is_a_new_entry(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "crates" / "c" / "src"
            src.mkdir(parents=True)
            (src / "lib.rs").write_text(
                'pub const A: &str = "SELECT a.id, b.id FROM t";\n'
                'pub const B: &str = "SELECT a.id, b.id FROM t";\n')
            _, dups = guard.collect(root)
            self.assertEqual(len(dups), 2)

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
