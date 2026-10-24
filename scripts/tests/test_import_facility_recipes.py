"""Regression coverage for source-backed village manufacturing imports."""

import importlib.util
import json
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / "import_facility_recipes.py"
spec = importlib.util.spec_from_file_location("import_facility_recipes", MODULE_PATH)
importer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(importer)

RECIPES = """CREATE TABLE recipes (
    id INTEGER PRIMARY KEY, source TEXT NOT NULL, source_key TEXT NOT NULL,
    result_item INTEGER NOT NULL, result_qty INTEGER NOT NULL, fee INTEGER NOT NULL,
    learn_fee INTEGER, level_min INTEGER, level_max INTEGER, success_rate INTEGER,
    max_qty INTEGER, menu_type TEXT NOT NULL, npc TEXT NOT NULL)"""
MATERIALS = """CREATE TABLE recipe_materials (
    recipe_id INTEGER NOT NULL, slot INTEGER NOT NULL, item_code INTEGER NOT NULL,
    qty INTEGER NOT NULL, enhance INTEGER NOT NULL, PRIMARY KEY(recipe_id, slot))"""


class FacilityImportTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.source = Path(self.temp.name) / "source.db"
        self.target = Path(self.temp.name) / "target.db"
        with closing(sqlite3.connect(self.source)) as source, source:
            source.execute("CREATE TABLE raw_sheet(archive, member, title, table_name, row_count)")
            # Discover by fields, not the archive's current member number.
            source.execute("INSERT INTO raw_sheet VALUES ('gersang.gcs', 2050, 'Makingfacility.txt', 'fixture', 0)")
            self.create_sheet(source, "fixture")
            self.add_row(source)
            self.add_row(source, VillageCode="629", VillageName="찬디가르")
            self.add_row(source, **{"Recipe Item Code": "4662", "MakeItemCode": "4626", "MakeItemQuantity": "10"})
        with closing(sqlite3.connect(self.target)) as target, target:
            target.execute(RECIPES)
            target.execute(MATERIALS)
            target.execute("INSERT INTO recipes VALUES (742,'unified','unified-116',5626,1,0,NULL,NULL,NULL,NULL,20,'PRODUCTION','414')")
            target.execute("INSERT INTO recipe_materials VALUES (742,1,5897,1,0)")
            target.execute("CREATE TABLE items(code INTEGER PRIMARY KEY)")
            target.executemany("INSERT INTO items VALUES (?)", [(code,) for code in (4570, 4656, 2287, 4621, 4104, 4105, 4626, 4662, 5626, 5897, 999)])
            target.execute("CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            target.execute("INSERT INTO meta VALUES ('existing', 'keep me')")
            target.execute("CREATE TABLE towns(code TEXT PRIMARY KEY, name_ko TEXT NOT NULL)")
            target.execute("INSERT INTO towns VALUES ('Is004', '뱅갈로르')")

    def create_sheet(self, source, table):
        source.execute(f"CREATE TABLE {table} (" + ",".join(importer.quoted(c) + " TEXT" for c in importer.COLUMNS) + ")")

    def add_row(self, source, **overrides):
        row = {column: "0" for column in importer.COLUMNS}
        row.update({"VillageCode": "644", "VillageName": "뱅갈로르", "Recipe Item Code": "4656",
                    "Recipe Item Name": "용두쿠크리 제조서", "MakeItemCode": "4570", "MakeItemName": "용두쿠크리",
                    "MakeItemQuantity": "1", "SpendMoney": "100000000", "Time": "5", "InfluenceType": "2", "InfluenceValue": "750"})
        for slot, (code, qty) in enumerate(((2287, 200), (4621, 50), (4104, 20), (4105, 3), (4626, 20))):
            row[f"Element{slot}Code"], row[f"Element{slot}Num"] = str(code), str(qty)
        row.update(overrides)
        source.execute("INSERT INTO fixture VALUES (" + ",".join("?" for _ in importer.COLUMNS) + ")", [row[c] for c in importer.COLUMNS])
        source.execute("UPDATE raw_sheet SET row_count=(SELECT count(*) FROM fixture) WHERE table_name='fixture'")

    def run_import(self):
        return importer.import_facility_recipes(self.source, self.target)

    def recipe_ids(self):
        with closing(sqlite3.connect(self.target)) as db:
            return dict(db.execute("SELECT source_key,id FROM recipes WHERE source='facility'"))

    def test_books_materials_batch_yield_conditions_and_locations_are_preserved(self):
        source_before = self.source.read_bytes()
        metadata = self.run_import()
        self.assertEqual(source_before, self.source.read_bytes())
        self.assertEqual(metadata["counts"], {"source_rows": 3, "recipes": 2, "materials": 12, "locations": 3, "books": 2, "linked_towns": 0})
        with closing(sqlite3.connect(self.target)) as db:
            recipe_id = db.execute("SELECT id FROM recipes WHERE source='facility' AND result_item=4570").fetchone()[0]
            self.assertEqual(db.execute("SELECT result_qty,fee,max_qty,npc FROM recipes WHERE id=?", (recipe_id,)).fetchone(), (1, 100000000, None, ""))
            self.assertEqual(db.execute("SELECT item_code,qty,enhance FROM recipe_materials WHERE recipe_id=? ORDER BY slot", (recipe_id,)).fetchall(),
                             [(4656, 1, 0), (2287, 200, 0), (4621, 50, 0), (4104, 20, 0), (4105, 3, 0), (4626, 20, 0)])
            self.assertEqual(db.execute("SELECT book_item,duration_days,influence_type,influence_value FROM recipe_facility WHERE recipe_id=?", (recipe_id,)).fetchone(), (4656, 5, 2, 750))
            # An identical Korean town name is not a numeric-code join.
            self.assertEqual(db.execute("SELECT village_code,village_name_ko,town_code FROM recipe_facility_locations WHERE recipe_id=? ORDER BY village_code", (recipe_id,)).fetchall(),
                             [("629", "찬디가르", ""), ("644", "뱅갈로르", "")])
            self.assertEqual(db.execute("SELECT result_qty,max_qty FROM recipes WHERE source='facility' AND result_item=4626").fetchone(), (10, None))
            self.assertEqual(db.execute("SELECT source,source_key,result_item FROM recipes WHERE id=742").fetchone(), ("unified", "unified-116", 5626))
            self.assertEqual(db.execute("SELECT * FROM recipe_materials WHERE recipe_id=742").fetchall(), [(742, 1, 5897, 1, 0)])
            self.assertEqual(db.execute("SELECT value FROM meta WHERE key='existing'").fetchone(), ("keep me",))

    def test_repeat_import_is_byte_identical_and_source_reordering_keeps_ids(self):
        self.run_import()
        before, ids = self.target.read_bytes(), self.recipe_ids()
        self.run_import()
        self.assertEqual(self.target.read_bytes(), before)
        with closing(sqlite3.connect(self.source)) as db, db:
            rows = db.execute("SELECT * FROM fixture ORDER BY rowid DESC").fetchall()
            db.execute("DELETE FROM fixture")
            db.executemany("INSERT INTO fixture VALUES (" + ",".join("?" for _ in importer.COLUMNS) + ")", rows)
            self.add_row(db, **{"MakeItemCode": "999", "Recipe Item Code": "5897"})
        self.run_import()
        refreshed = self.recipe_ids()
        self.assertEqual({key: refreshed[key] for key in ids}, ids)
        self.assertEqual(len(refreshed), 3)

    def test_economic_and_requirement_differences_do_not_collapse(self):
        with closing(sqlite3.connect(self.source)) as db, db:
            for changes in ({"SpendMoney": "1"}, {"Time": "6"}, {"InfluenceType": "9"},
                            {"InfluenceValue": "751"}, {"MakeItemQuantity": "2"}, {"Recipe Item Code": "5897"}):
                self.add_row(db, **changes)
        metadata = self.run_import()
        self.assertEqual(metadata["counts"]["recipes"], 8)
        self.assertEqual(metadata["influence_types"], [2, 9])

    def test_unknown_item_rolls_back_schema_and_data(self):
        with closing(sqlite3.connect(self.source)) as db, db:
            db.execute('UPDATE fixture SET "Element0Code"=123456')
        before = self.target.read_bytes()
        with self.assertRaisesRegex(ValueError, "Unknown item references"):
            self.run_import()
        self.assertEqual(self.target.read_bytes(), before)

    def test_incomplete_material_or_duplicate_location_does_not_overwrite_import(self):
        self.run_import()
        before = self.target.read_bytes()
        with closing(sqlite3.connect(self.source)) as db, db:
            db.execute('UPDATE fixture SET "Element0Num"=0 WHERE rowid=1')
        with self.assertRaisesRegex(ValueError, "Incomplete material"):
            self.run_import()
        self.assertEqual(self.target.read_bytes(), before)
        with closing(sqlite3.connect(self.source)) as db, db:
            db.execute('UPDATE fixture SET "Element0Num"=200 WHERE rowid=1')
            self.add_row(db)
        with self.assertRaisesRegex(ValueError, "Duplicate manufacturing"):
            self.run_import()
        self.assertEqual(self.target.read_bytes(), before)

    def test_unknown_owned_schema_or_unowned_existing_source_is_rejected(self):
        with closing(sqlite3.connect(self.target)) as db, db:
            db.execute("CREATE TABLE recipe_facility(recipe_id INTEGER PRIMARY KEY, mystery TEXT)")
        before = self.target.read_bytes()
        with self.assertRaisesRegex(ValueError, "Incompatible existing recipe_facility"):
            self.run_import()
        self.assertEqual(self.target.read_bytes(), before)
        with closing(sqlite3.connect(self.target)) as db, db:
            db.execute("DROP TABLE recipe_facility")
            db.execute("UPDATE recipes SET source='facility'")
        before = self.target.read_bytes()
        with self.assertRaisesRegex(ValueError, "Unowned existing facility data"):
            self.run_import()
        self.assertEqual(self.target.read_bytes(), before)

    def test_ambiguous_source_and_unrecognized_columns_are_rejected(self):
        with closing(sqlite3.connect(self.source)) as db, db:
            self.create_sheet(db, "other")
            db.execute("INSERT INTO raw_sheet VALUES ('gersang.gcs',2051,'other','other',0)")
        before = self.target.read_bytes()
        with self.assertRaisesRegex(ValueError, "Expected one village manufacturing sheet"):
            self.run_import()
        self.assertEqual(self.target.read_bytes(), before)
        with closing(sqlite3.connect(self.source)) as db, db:
            db.execute("DELETE FROM raw_sheet WHERE table_name='other'")
            db.execute("ALTER TABLE fixture ADD COLUMN unknown_rule TEXT")
        with self.assertRaisesRegex(ValueError, "Unsupported manufacturing columns"):
            self.run_import()
        self.assertEqual(self.target.read_bytes(), before)

    def test_write_failure_rolls_back_replacement(self):
        self.run_import()
        with closing(sqlite3.connect(self.target)) as db, db:
            db.execute("CREATE TRIGGER reject_new_recipe BEFORE INSERT ON recipes WHEN NEW.source='facility' BEGIN SELECT RAISE(ABORT,'fixture failure'); END")
        before = self.target.read_bytes()
        with closing(sqlite3.connect(self.source)) as db, db:
            self.add_row(db, **{"MakeItemCode": "999"})
        with self.assertRaisesRegex(sqlite3.IntegrityError, "fixture failure"):
            self.run_import()
        self.assertEqual(self.target.read_bytes(), before)

    def test_same_database_is_rejected_without_modification(self):
        before = self.source.read_bytes()
        with self.assertRaisesRegex(ValueError, "must be different"):
            importer.import_facility_recipes(self.source, self.source)
        self.assertEqual(self.source.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
