import json
import sqlite3
import sys
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from import_drop_rates import import_drop_rates


class DropRateImportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "full.db"
        self.target = self.root / "widget.db"
        with closing(sqlite3.connect(self.source)) as conn, conn:
            conn.execute("CREATE TABLE raw_sheet (archive TEXT, member INTEGER, table_name TEXT)")
            # Member numbering may change in a future archive.
            conn.execute("INSERT INTO raw_sheet VALUES ('gersang.gcs', 42, 'characters')")
            columns = ["ID", "Name", "Villagecode", "Name Code"]
            columns.extend(f"{prefix}{slot}" for slot in range(1, 11) for prefix in ("Item Code", "Percent"))
            conn.execute("CREATE TABLE characters (" + ", ".join(f'"{name}" TEXT' for name in columns) + ")")
        with closing(sqlite3.connect(self.target)) as conn, conn:
            conn.execute("CREATE TABLE monster_drops (monster_code TEXT, slot INTEGER, item_code INTEGER, PRIMARY KEY(monster_code, slot))")

    def add_source(self, code, drops):
        values = [code, "Monster", "0", "0"]
        for slot in range(1, 11):
            item, rate = drops.get(slot, (0, 0))
            values.extend([str(item), str(rate)])
        with closing(sqlite3.connect(self.source)) as conn, conn:
            conn.execute("INSERT INTO characters VALUES (" + ", ".join("?" for _ in values) + ")", values)

    def add_target(self, *rows):
        with closing(sqlite3.connect(self.target)) as conn, conn:
            conn.executemany("INSERT INTO monster_drops VALUES (?, ?, ?)", rows)

    def rows(self):
        with closing(sqlite3.connect(self.target)) as conn, conn:
            return conn.execute("SELECT * FROM monster_drops ORDER BY monster_code, slot").fetchall()

    def test_matches_items_after_reordering_and_keeps_solo_rates_separate(self):
        self.add_source("M24", {1: (143, 5500), 2: (741, 2750), 9: (143, 1000), 10: (99, 100)})
        self.add_target(("M24", 1, 741), ("M24", 2, 143), ("M24", 3, 99), ("M24", 9, 143))
        result = import_drop_rates(self.source, self.target)
        self.assertEqual(self.rows(), [("M24", 1, 741, 2750), ("M24", 2, 143, 5500), ("M24", 3, 99, None), ("M24", 9, 143, 1000)])
        self.assertEqual(result["counts"]["reordered"], 2)
        self.assertEqual(result["denominator"], 100000)
        self.assertEqual(result["table"], "characters")

    def test_unknown_is_distinct_from_explicit_zero(self):
        self.add_source("M24", {1: (143, 0), 2: (741, ""), 3: (99, 100000)})
        self.add_target(("M24", 1, 143), ("M24", 2, 741), ("M24", 3, 99), ("M24", 4, 666), ("M404", 1, 143))
        result = import_drop_rates(self.source, self.target)
        self.assertEqual([row[3] for row in self.rows()], [0, None, 100000, None, None])
        self.assertEqual(result["counts"]["imported"], 2)
        self.assertEqual(result["counts"]["unknown"], 3)

    def test_duplicate_item_within_mode_is_not_guessed(self):
        self.add_source("M24", {1: (143, 5500), 2: (143, 4000), 9: (143, 1000)})
        self.add_target(("M24", 1, 143), ("M24", 9, 143))
        result = import_drop_rates(self.source, self.target)
        self.assertEqual([row[3] for row in self.rows()], [None, 1000])
        self.assertEqual(result["counts"]["ambiguous"], 1)

    def test_rerun_is_idempotent_and_clears_stale_rates(self):
        self.add_source("M24", {1: (143, 5500), 2: (741, 2750)})
        self.add_target(("M24", 1, 143), ("M24", 2, 741))
        first = import_drop_rates(self.source, self.target)
        before = self.target.read_bytes()
        self.assertEqual(import_drop_rates(self.source, self.target), first)
        self.assertEqual(self.target.read_bytes(), before)
        with closing(sqlite3.connect(self.source)) as conn, conn:
            conn.execute('UPDATE characters SET "Item Code2" = 0')
        import_drop_rates(self.source, self.target)
        self.assertEqual(self.rows(), [("M24", 1, 143, 5500), ("M24", 2, 741, None)])
        with closing(sqlite3.connect(self.target)) as conn, conn:
            provenance = json.loads(conn.execute("SELECT value FROM meta WHERE key = 'drop_rates_source'").fetchone()[0])
        self.assertEqual(provenance["counts"]["unknown"], 1)

    def test_invalid_rate_does_not_partially_migrate_target(self):
        for bad_rate in (100001, -1, "garbage"):
            with self.subTest(rate=bad_rate):
                with closing(sqlite3.connect(self.source)) as conn, conn:
                    conn.execute("DELETE FROM characters")
                with closing(sqlite3.connect(self.target)) as conn, conn:
                    conn.execute("DELETE FROM monster_drops")
                self.add_source("M24", {1: (143, 5500), 2: (741, bad_rate)})
                self.add_target(("M24", 1, 143), ("M24", 2, 741))
                before = self.rows()
                with self.assertRaises(ValueError):
                    import_drop_rates(self.source, self.target)
                self.assertEqual(self.rows(), before)

    def test_duplicate_source_monsters_and_no_matches_leave_target_untouched(self):
        self.add_target(("M24", 1, 143))
        self.add_source("M24", {1: (999, 5500)})
        before = self.rows()
        with self.assertRaisesRegex(ValueError, "No drop rates matched"):
            import_drop_rates(self.source, self.target)
        self.assertEqual(self.rows(), before)
        self.add_source("M24", {1: (143, 5500)})
        with self.assertRaisesRegex(ValueError, "Duplicate monster ID"):
            import_drop_rates(self.source, self.target)
        self.assertEqual(self.rows(), before)

    def test_sqlite_failure_rolls_back_column_and_values(self):
        self.add_source("M24", {1: (143, 5500)})
        self.add_target(("M24", 1, 143))
        with closing(sqlite3.connect(self.target)) as conn, conn:
            conn.execute("CREATE TRIGGER reject_updates BEFORE UPDATE ON monster_drops BEGIN SELECT RAISE(ABORT, 'fixture rejection'); END")
        before = self.rows()
        with self.assertRaisesRegex(sqlite3.IntegrityError, "fixture rejection"):
            import_drop_rates(self.source, self.target)
        self.assertEqual(self.rows(), before)


if __name__ == "__main__":
    unittest.main()
