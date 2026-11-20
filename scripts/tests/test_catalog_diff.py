import sqlite3
import sys
import tempfile
import unittest
from contextlib import ExitStack, closing
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from catalog_diff import Snapshot, attach_caption_changes, compare, display_key, name_index


class CatalogDiffTests(unittest.TestCase):
    def snapshot(self, quest_id, recipe_id, ordinal, old):
        conn = sqlite3.connect(":memory:")
        self.addCleanup(conn.close)
        conn.executescript("""
            CREATE TABLE quests(id INTEGER PRIMARY KEY, source, code, title, title_gts);
            CREATE TABLE quest_lines(quest_id, role, slot, gts_id, text_object,
                                     PRIMARY KEY(quest_id,role,slot));
            CREATE TABLE recipes(id INTEGER PRIMARY KEY, source, source_key, result_item, npc, fee);
            CREATE TABLE recipe_materials(recipe_id, slot, item_code, qty, PRIMARY KEY(recipe_id,slot));
            CREATE TABLE items(code INTEGER PRIMARY KEY, name_ko, name_gts);
        """)
        conn.execute("INSERT INTO quests VALUES(?, 'main', '10', '제목', NULL)", (quest_id,))
        conn.execute("INSERT INTO quest_lines VALUES(?, 'title', 1, 123, 'QuestText.dat')", (quest_id,))
        # Both variants have the same source, NPC and result. Reverse the
        # generated IDs and shift the source ordinals between the imports.
        variants = [(recipe_id + 1, 25), (recipe_id, 26)] if old else [(recipe_id, 25), (recipe_id + 1, 26)]
        for index, (rid, material) in enumerate(variants):
            conn.execute("INSERT INTO recipes VALUES(?, 'npc', ?, 90, '17', 100)",
                         (rid, f"npc-{ordinal + index}",))
            conn.execute("INSERT INTO recipe_materials VALUES(?, 1, ?, 1)", (rid, material))
        if old:
            conn.execute("INSERT INTO items VALUES (70, '삭제', 0)")
            conn.execute("INSERT INTO quest_lines VALUES(?, 'mission', 1, 12, 'QuestText.dat')", (quest_id,))
        else:
            conn.execute("INSERT INTO quest_lines VALUES(?, 'mission', 2, 14, 'QuestText.dat')", (quest_id,))
        return Snapshot(conn)

    def test_reimport_preserves_identity_and_removed_records_retain_old_values(self):
        old = self.snapshot(41, 105, 8, True)
        new = self.snapshot(42, 210, 119, False)
        entries = compare(old, new, "A", "B")["entries"]
        self.assertEqual([(e["table"], e["kind"], e["key"]) for e in entries],
                         [("items", "removed", "70"), ("quest_lines", "modified", "main / 10")])
        self.assertEqual(entries[0]["before"], {"name_ko": "삭제", "name_gts": 0})
        self.assertEqual(entries[0]["after"], {})
        self.assertEqual(entries[1]["before"], {"mission[1]": "QuestText.dat #12"})
        self.assertEqual(entries[1]["after"], {"mission[2]": "QuestText.dat #14"})
        self.assertEqual(compare(new, new, "B", "B")["entries"], [])

    def test_translated_rename_keeps_entity_identity_and_pure_translation_addition_is_an_edit(self):
        old = self.snapshot(1, 1, 1, False)
        new = self.snapshot(1, 1, 1, False)
        for snap, english in ((old, "Iron Staff"), (new, "Steel Staff")):
            snap.conn.execute("CREATE TABLE strings(object, id, lang, text, PRIMARY KEY(object,id,lang))")
            snap.conn.executemany("INSERT INTO items VALUES(?,?,?)", [(3, "강철봉", 2068), (4, "물", 10)])
            snap.conn.execute("INSERT INTO strings VALUES('OnlineText.dat','00002068','en',?)", (english,))
        new.conn.execute("INSERT INTO strings VALUES('OnlineText.dat','00000010','en','Water')")
        old, new = Snapshot(old.conn), Snapshot(new.conn)
        entries = [e for e in compare(old, new, "A", "B")["entries"] if e["table"] == "items"]
        self.assertEqual([(e["key"], e["kind"]) for e in entries], [("3", "renamed"), ("4", "modified")])
        self.assertEqual(entries[0]["before"], {"name_en": "Iron Staff"})
        self.assertEqual(entries[0]["previous_name"]["en"], "Iron Staff")
        self.assertEqual(entries[0]["after"], {"name_en": "Steel Staff"})

    def test_name_reference_changes_do_not_invent_translations_or_renames(self):
        old = self.snapshot(1, 1, 1, False)
        new = self.snapshot(1, 1, 1, False)
        for index, snap in enumerate((old, new)):
            snap.conn.execute("CREATE TABLE strings(object, id, lang, text, PRIMARY KEY(object,id,lang))")
            for item in (11, 12, 13):
                ref_id = item * 10 + index
                snap.conn.execute("INSERT INTO items VALUES(?, '원문', ?)", (item, ref_id))
                english = "Revised" if item == 13 and index else "Original"
                snap.conn.execute("INSERT INTO strings VALUES('OnlineText.dat',?,'en',?)", (f"{ref_id:08d}", english))
                if item == 11 or not index:
                    snap.conn.execute("INSERT INTO strings VALUES('OnlineText.dat',?,'zh','繁中名稱')", (f"{ref_id:08d}",))
        rows = {r["key"]:r for r in compare(Snapshot(old.conn), Snapshot(new.conn), "A", "B")["entries"]
                if r["table"] == "items"}
        self.assertEqual(rows["11"]["kind"], "modified")
        self.assertEqual(rows["11"]["after"], {"name_gts":111})
        self.assertEqual(rows["12"]["kind"], "modified")
        self.assertEqual(rows["12"]["after"], {"name_gts":121, "name_zh":None})
        self.assertEqual(rows["13"]["kind"], "renamed")
        self.assertEqual(rows["13"]["after"], {"name_gts":131, "name_en":"Revised", "name_zh":None})
        self.assertNotIn("zh", rows["13"]["name"])

    def test_composite_identity_keeps_empty_columns_and_literal_separators(self):
        labels = {display_key(parts) for parts in (("npc", "", 1), ("npc", 1), ("npc", "∅", 1),
                                                     ("npc / 1",), ("npc", "1 / "), ("npc", 1, ""))}
        self.assertEqual(len(labels), 6)

    def test_name_index_keeps_reused_codes_in_separate_lifetimes(self):
        def row(kind, before=None, after=None):
            return {"table":"items","key":"9","kind":kind,"before":before or {},"after":after or {}}
        releases = [
            {"entries":[row("renamed", {"name_en":"Old", "name_zh":"舊"}, {"name_en":"New", "name_zh":None})]},
            {"entries":[row("removed")]}, {"entries":[row("added")]},
            {"entries":[row("modified", {"name_zh":None}, {"name_zh":"新"})]},
            {"entries":[row("renamed", {"name_ko":"강"}, {"name_ko":"물"})]},
        ]
        index = name_index(list("ABCDEF"), releases)
        lifetimes = index["entities"]["item:9"]
        self.assertEqual([(v["since"],v.get("until")) for v in lifetimes], [(0,2),(3,None)])
        self.assertEqual([e["version"] for v in lifetimes for e in v["events"]], [1,5])
        self.assertEqual(lifetimes[0]["events"][0]["changes"][0], {"lang":"zh","before":"舊","after":""})

    def test_location_replacement_and_drop_rates_keep_both_snapshot_names(self):
        pair = []
        for i, region in enumerate((7, 8)):
            snapshot = self.snapshot(i + 1, 5, 1, False)
            snapshot.conn.executescript("""
                CREATE TABLE monsters(code TEXT PRIMARY KEY, name_ko, name_gts, location_gts);
                CREATE TABLE monster_locations(monster_code, location_gts, PRIMARY KEY(monster_code,location_gts));
                CREATE TABLE monster_categories(monster_code, category_gts, PRIMARY KEY(monster_code,category_gts));
                CREATE TABLE monster_drops(monster_code, slot, item_code, drop_rate_per_100000, PRIMARY KEY(monster_code,slot));
                CREATE TABLE maps(code INTEGER PRIMARY KEY, name_gts, name_ko);
            """)
            snapshot.conn.execute("INSERT INTO monsters VALUES('M1','여우',0,?)", (region,))
            snapshot.conn.executemany("INSERT INTO monster_locations VALUES('M1',?)", [(region,), (9,)])
            snapshot.conn.execute("INSERT INTO monster_categories VALUES('M1',?)", (region,))
            snapshot.conn.executemany("INSERT INTO maps VALUES(?,?,?)", [(1,7,"산"),(2,8,"산 2층"),(3,9,"길"),(4,9,"성")])
            snapshot.conn.execute("INSERT INTO items VALUES(50,?,0)", (["잎","약초"][i],))
            snapshot.conn.execute("INSERT INTO monster_drops VALUES('M1',2,50,?)", ([None, 0][i],))
            pair.append(Snapshot(snapshot.conn))
        rows = compare(*pair, "A", "B")["entries"]
        for table, field in (("monster_locations", "location_gts"), ("monster_categories", "category_gts")):
            changed, = [r for r in rows if r["table"] == table]
            self.assertEqual((changed["kind"], changed["key"]), ("modified", "M1"))
            self.assertEqual(changed["before"][field][0], 7)
            self.assertEqual(changed["after"][field][0], 8)
            self.assertEqual(changed["previous_regions"][0]["name"], {"ko":"산"})
            self.assertEqual(changed["regions"], [{"id":"8","name":{"ko":"산 2층"}},{"id":"9","name":{}}])
        drop, = [r for r in rows if r["table"] == "monster_drops"]
        self.assertEqual(drop["drop_before"], {"item_code":50, "rate":None, "item":{"ko":"잎"}, "monster":{"ko":"여우"}})
        self.assertEqual(drop["drop_after"], {"item_code":50, "rate":0, "item":{"ko":"약초"}, "monster":{"ko":"여우"}})

    def test_removing_a_reward_does_not_remove_the_following_level_requirement(self):
        pair = []
        for old in (True, False):
            snap = self.snapshot(10, 5, 1, False)
            snap.conn.execute("CREATE TABLE quest_steps(quest_id, role, slot, kind, code, qty, label, PRIMARY KEY(quest_id,role,slot))")
            if old:
                snap.conn.execute("INSERT INTO quest_steps VALUES(10,'reward',1,'Item','5246','1','가루')")
            snap.conn.execute("INSERT INTO quest_steps VALUES(10,'requirement',?,'Level','0','250','')", (2 if old else 1,))
            pair.append(Snapshot(snap.conn))
        row, = compare(*pair, "A", "B")["entries"]
        self.assertEqual((row["table"], row["kind"], row["before"]["code"]), ("quest_steps", "removed", "5246"))

    def test_added_monster_loot_includes_preexisting_slots_and_future_name_only_updates(self):
        releases, snapshots = [], []
        with tempfile.TemporaryDirectory() as directory, ExitStack() as connections:
            for i, version in enumerate("ABCD"):
                path = Path(directory) / f"{version}.db"
                conn = connections.enter_context(closing(sqlite3.connect(path)))
                conn.executescript("""
                    CREATE TABLE monsters(code TEXT PRIMARY KEY, name_ko, name_gts, location_gts);
                    CREATE TABLE items(code INTEGER PRIMARY KEY, name_ko, name_gts);
                    CREATE TABLE monster_drops(monster_code,slot,item_code,drop_rate_per_100000,PRIMARY KEY(monster_code,slot));
                    INSERT INTO monster_drops VALUES('M1',3,50,1),('M1',7,50,NULL);
                """)
                if i:
                    conn.execute("INSERT INTO monsters VALUES('M1','여우',0,0)")
                item_name = "잎" if i < 2 else "약초"
                conn.execute("INSERT INTO items VALUES(50,?,0)", (item_name,))
                conn.commit()
                snap = Snapshot(conn)
                if i:
                    releases.append(compare(previous, snap, snapshots[-1][1], version))
                previous = snap
                snapshots.append((path, version))
            attach_caption_changes(snapshots, releases)
        added, = releases[0]["entries"]
        self.assertEqual((added["table"], added["kind"]), ("monsters", "added"))
        self.assertEqual([(x["slot"],x["item_code"],x["rate"],x["item"]["ko"]) for x in added["loot"]],
                         [(3,50,1,"잎"), (7,50,None,"잎")])
        self.assertEqual([r["table"] for r in releases[1]["entries"]], ["items"])
        self.assertEqual([s["item"]["ko"] for s in releases[1]["loot_updates"]["M1"]], ["약초","약초"])
        self.assertNotIn("loot_updates", releases[2])

    def test_child_captions_follow_parent_renames_without_creating_phantom_changes(self):
        releases = []
        snapshots = []
        with tempfile.TemporaryDirectory() as directory:
            for index, (version, material, qty) in enumerate((("A", "잎", 1), ("B", "잎", 2), ("C", "약초", 2))):
                snapshot = self.snapshot(1, 20, 10, False)
                snapshot.conn.executemany("INSERT INTO items VALUES(?,?,0)", [(25, material), (90, "약")])
                snapshot.conn.execute("UPDATE recipe_materials SET qty=? WHERE item_code=25", (qty,))
                snapshot.conn.executescript("""
                    CREATE TABLE monsters(code TEXT PRIMARY KEY, name_ko, name_gts, location_gts);
                    INSERT INTO monsters VALUES('M1','여우',0,0);
                    CREATE TABLE monster_drops(monster_code,slot,item_code,drop_rate_per_100000,PRIMARY KEY(monster_code,slot));
                """)
                snapshot.conn.execute("INSERT INTO monster_drops VALUES('M1',1,25,?)", (qty,))
                snapshot = Snapshot(snapshot.conn)
                snapshot.conn.commit()
                path = Path(directory) / f"{index}.db"
                dest = sqlite3.connect(path)
                try:
                    snapshot.conn.backup(dest)
                finally:
                    dest.close()
                snapshots.append((path, version))
                if index:
                    releases.append(compare(previous, snapshot, snapshots[index-1][1], version))
                previous = snapshot
            attach_caption_changes(snapshots, releases)
        self.assertNotIn("caption_changes", releases[0])
        self.assertEqual([(e["table"], e["kind"]) for e in releases[1]["entries"]], [("items", "renamed")])
        self.assertEqual(releases[1]["caption_changes"], [
            {"table":"monster_drops", "key":"M1 / 1",
             "before":{"ko":"여우 · 잎"}, "after":{"ko":"여우 · 약초"},
             "drop_before":{"item_code":25,"rate":2,"item":{"ko":"잎"},"monster":{"ko":"여우"}},
             "drop_after":{"item_code":25,"rate":2,"item":{"ko":"약초"},"monster":{"ko":"여우"}}},
            {"table": "recipe_materials", "key": "npc / 90 / 17 / 1",
             "before": {"ko": "약 · 잎"}, "after": {"ko": "약 · 약초"}}
        ])
