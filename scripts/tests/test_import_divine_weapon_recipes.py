import shutil
import sqlite3
import sys
import tempfile
import unittest
from contextlib import closing
from itertools import combinations
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import import_divine_weapon_recipes as importer


class DivineWeaponRecipesTest(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.catalog = Path(temp.name) / "test.db"
        shutil.copyfile(importer.DEFAULT_TARGET, self.catalog)
        with closing(sqlite3.connect(self.catalog)) as db, db:
            db.execute("DROP TABLE IF EXISTS recipe_material_choices")
            db.execute("DELETE FROM recipe_materials WHERE recipe_id IN (SELECT id FROM recipes WHERE source=?)",
                       (importer.SOURCE,))
            db.execute("DELETE FROM recipes WHERE source=?", (importer.SOURCE,))

    def test_fixed_materials_choice_group_and_repeat_import(self):
        with closing(sqlite3.connect(self.catalog)) as db:
            other_recipes = db.execute("SELECT * FROM recipes ORDER BY id").fetchall()
            other_parts = db.execute("SELECT * FROM recipe_materials ORDER BY recipe_id, slot").fetchall()
        self.assertEqual(importer.import_divine_weapon_recipes(self.catalog), 4)
        with closing(sqlite3.connect(self.catalog)) as db:
            self.assertEqual(db.execute("SELECT * FROM recipes WHERE source!=? ORDER BY id",
                                        (importer.SOURCE,)).fetchall(), other_recipes)
            self.assertEqual(db.execute("SELECT * FROM recipe_materials WHERE recipe_id NOT IN "
                                        "(SELECT id FROM recipes WHERE source=?) ORDER BY recipe_id,slot",
                                        (importer.SOURCE,)).fetchall(), other_parts)
            for result, crystal, trophy in [(3993, 3905, 3915), (3994, 3908, 3922),
                                            (3995, 3906, 3916), (3996, 3907, 3917)]:
                routes = db.execute("SELECT id,fee,npc,result_qty FROM recipes WHERE source=? AND result_item=?",
                                    (importer.SOURCE, result)).fetchall()
                self.assertEqual(len(routes), 1)
                rid, fee, npc, quantity = routes[0]
                self.assertEqual((fee, npc, quantity), (-1, "164", 1))
                parts = db.execute("SELECT item_code,qty FROM recipe_materials WHERE recipe_id=? ORDER BY slot",
                                   (rid,)).fetchall()
                self.assertEqual(parts, [(crystal, 30), (trophy, 30)])
                options = db.execute("SELECT item_code,choice_count,qty FROM recipe_material_choices "
                                     "WHERE recipe_id=? ORDER BY item_code", (rid,)).fetchall()
                self.assertEqual(options, [(3433, 3, 1), (3434, 3, 1), (3435, 3, 1), (3436, 3, 1)])
        before = self.catalog.read_bytes()
        importer.import_divine_weapon_recipes(self.catalog)
        self.assertEqual(self.catalog.read_bytes(), before)

    def test_migrates_old_combinations_to_one_recipe_with_first_id(self):
        first_id = 50000
        with closing(sqlite3.connect(self.catalog)) as db, db:
            for rid, trio in enumerate(combinations(importer.FEY_WEAPONS, 3), first_id):
                key = "divine-weapon:3995:" + "-".join(map(str, trio))
                db.execute("INSERT INTO recipes VALUES (?,?,?,3995,1,-1,NULL,NULL,NULL,NULL,NULL,'PRODUCTION','164')",
                           (rid, importer.SOURCE, key))
                for slot, item in enumerate(trio, 1):
                    db.execute("INSERT INTO recipe_materials VALUES (?,?,?,1,0)", (rid, slot, item))
        importer.import_divine_weapon_recipes(self.catalog)
        with closing(sqlite3.connect(self.catalog)) as db:
            self.assertEqual(db.execute("SELECT id FROM recipes WHERE result_item=3995 AND source=?",
                                        (importer.SOURCE,)).fetchall(), [(first_id,)])
            self.assertEqual(db.execute("SELECT item_code FROM recipe_materials WHERE recipe_id=? ORDER BY slot",
                                        (first_id,)).fetchall(), [(3906,), (3916,)])
            self.assertEqual(db.execute("SELECT count(*) FROM recipe_materials WHERE recipe_id BETWEEN ? AND ?",
                                        (first_id + 1, first_id + 3)).fetchone()[0], 0)

    def test_changed_client_requirements_abort_before_writes(self):
        with closing(sqlite3.connect(self.catalog)) as db, db:
            db.execute("UPDATE strings SET text=replace(text,'30個','40個') WHERE object='OnlineText.dat' "
                       "AND id='00033078' AND lang='zh'")
        before = self.catalog.read_bytes()
        with self.assertRaisesRegex(ValueError, "3995"):
            importer.import_divine_weapon_recipes(self.catalog)
        self.assertEqual(self.catalog.read_bytes(), before)
