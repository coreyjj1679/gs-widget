import sqlite3
import sys
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from preserve_catalog_ids import preserve


class CatalogIdsTests(unittest.TestCase):
    def make(self, path, quests, recipes):
        with closing(sqlite3.connect(path)) as db, db:
            db.executescript("""
                CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
                CREATE TABLE quests(id INTEGER PRIMARY KEY,source TEXT,code TEXT);
                CREATE TABLE quest_lines(quest_id INTEGER REFERENCES quests, text TEXT);
                CREATE TABLE recipes(id INTEGER PRIMARY KEY,source TEXT,source_key TEXT,result_item INTEGER,npc TEXT,
                  result_qty INTEGER DEFAULT 1,fee INTEGER DEFAULT 0,learn_fee,level_min,level_max,success_rate,max_qty,menu_type);
                CREATE TABLE recipe_materials(recipe_id INTEGER REFERENCES recipes,slot INTEGER,item_code INTEGER,PRIMARY KEY(recipe_id,slot));
            """)
            for rid, code in quests:
                db.execute("INSERT INTO quests VALUES(?,'main',?)", (rid, code))
                db.execute("INSERT INTO quest_lines VALUES(?,?)", (rid, code))
            for rid, key, item, material in recipes:
                db.execute("INSERT INTO recipes(id,source,source_key,result_item,npc) VALUES(?,'npc',?,?,'A')", (rid, key, item))
                db.execute("INSERT INTO recipe_materials VALUES(?,1,?)", (rid, material))

    def test_reordered_ids_and_variants_follow_their_contents_and_retired_ids_stay_unused(self):
        with tempfile.TemporaryDirectory() as folder:
            old, new, next_version = [Path(folder) / (name + ".db") for name in ("old", "new", "next")]
            self.make(old, [(1, "q1"), (2, "q2"), (3, "retired")],
                      [(1, "npc-1", 70, 12), (2, "npc-2", 70, 13), (3, "npc-3", 71, 14)])
            self.make(new, [(1, "q2"), (2, "q3"), (3, "q1")],
                      [(1, "npc-1", 70, 13), (2, "npc-2", 72, 20), (3, "npc-3", 70, 12)])
            preserve(old, new)
            with closing(sqlite3.connect(new)) as db:
                self.assertEqual(db.execute("SELECT quest_id,text FROM quest_lines ORDER BY quest_id").fetchall(),
                                 [(1, "q1"), (2, "q2"), (4, "q3")])
                self.assertEqual(db.execute("SELECT recipe_id,item_code FROM recipe_materials ORDER BY recipe_id").fetchall(),
                                 [(1, 12), (2, 13), (4, 20)])
                self.assertEqual(db.execute("PRAGMA foreign_key_check").fetchall(), [])
            first = new.read_bytes()
            self.assertEqual(preserve(old, new), {"quests": 0, "recipes": 0})
            self.assertEqual(new.read_bytes(), first)
            self.make(next_version, [(1, "q1")], [])
            preserve(new, next_version)
            with closing(sqlite3.connect(next_version)) as db:
                self.assertEqual(dict(db.execute("SELECT key,value FROM meta")),
                                 {"stable_quests_max_id": "4", "stable_recipes_max_id": "4"})
