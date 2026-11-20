import sqlite3
import sys
import unittest
from contextlib import closing
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from import_main_quest_text import DIALOGUE, parse_main, refresh


class MainQuestImportTests(unittest.TestCase):
    def test_reordered_client_columns_preserve_repeat_rules_and_dialogue(self):
        def source(grouped):
            headers = ["#Number", "End Type1", "Need Quest", "RepeatType(0, 1, 2)", "퀘스트타입(0)"]
            columns = [names[0] for names in DIALOGUE.values()]
            if grouped:
                columns = ["Additional group", "소분류(GTS)", *columns[1:]]
                columns.reverse()
            header = headers + columns
            values = {"#Number": "50000", "RepeatType(0, 1, 2)": "1", "퀘스트타입(0)": "2",
                      "Subject": "24114", "소분류(GTS)": "24114", "Start Text 1": "305", "임무 내용": "24119"}
            return "\n".join(["\t".join(header), ";50000\tcomment", "\t".join(values.get(c, "0") for c in header)])

        before = parse_main(source(False))
        after = parse_main(source(True))
        self.assertEqual(before, after)
        self.assertEqual(after["50000"], ("2", "1", {"subject": 24114, "start-1": 305, "mission": 24119}))

    def test_refresh_is_idempotent_and_checks_source_before_writing(self):
        with closing(sqlite3.connect(":memory:")) as conn:
            conn.executescript("""
                CREATE TABLE sources(revision_id, role, sha256);
                INSERT INTO sources VALUES(28,'gersang.gcs','source');
                CREATE TABLE quests(id,source,code,kind,repeat_count);
                INSERT INTO quests VALUES(733,'main','50000','24123','24122'),(4,'letter','10','2','0');
                CREATE TABLE quest_lines(quest_id,role,slot,gts_id,text_object);
                INSERT INTO quest_lines VALUES(733,'middle-2',1,24114,'QuestText.dat'),
                                              (4,'title',1,500,'Letters');
            """)
            values = {"50000": ("0", "0", {"subject": 24114})}
            with self.assertRaisesRegex(ValueError, "hash"):
                refresh(conn, values, "other version")
            with self.assertRaisesRegex(ValueError, "different main quest"):
                refresh(conn, {"10": values["50000"]}, "source")
            self.assertEqual(conn.execute("SELECT kind FROM quests WHERE id=733").fetchone()[0], "24123")
            refresh(conn, values, "source")
            modified = conn.total_changes
            refresh(conn, values, "source")
            self.assertEqual(modified, conn.total_changes)
            self.assertEqual(conn.execute("SELECT kind,repeat_count FROM quests WHERE id=733").fetchone(), ("0", "0"))
            self.assertEqual(conn.execute("SELECT * FROM quest_lines ORDER BY quest_id").fetchall(), [
                (4, "title", 1, 500, "Letters"), (733, "subject", 1, 24114, "QuestText.dat"),
            ])
