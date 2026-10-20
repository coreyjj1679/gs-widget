import json
import sqlite3
import sys
import tempfile
import unittest
from contextlib import closing
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import import_enchant as importer
from import_enchant import parse_enchant, parse_npcs
from test_enchant_data import ENCHANT_TEXT, EFFECT_TEXT, ENCHANT_FIELDS, EFFECT_FIELDS


RECIPE = """
; backup
#아이템 코드	아이템 이름	Grade	Riddle	Reinforcement	Resonance	이름 코드	그룹	단위	단위 코드	설명문	설명문 코드	가격	무게	최대	그림	폰트	필드	필드폰트	Cost	사운드	신용	유료	연마 성공 확률(실수형)	사용 조건	재료 아이템 테이블 인덱스	연마되는 아이템 종류
1129	[고구려의] 주술 비법	0	0	0	0	2398	E	개	48	설명	2618	1	1	100	a.agf	0	b.agf	0	10	ETC	1	0	15	Level150	4Cls_4	TAEHWANG_ARMOR, TAEHWANG_HELMET
;1128	옛 행	0	0	0	0	0	E	개	48	설명	0	1	1	100	a.agf	0	b.agf	0	10	ETC	1	0	99	Level1	Basic	ALL
"""

MATERIALS = """
#테이블 인덱스	재료1	재료1 코드	재료1 수량	재료2	재료2 코드	재료2 수량	재료3	재료3 코드	재료3 수량	재료4	재료4 코드	재료4 수량	재료5	재료5 코드	재료5 수량	재료6	재료6 코드	재료6 수량	재료7	재료7 코드	재료7 수량	재료8	재료8 코드	재료8 수량	재료9	재료9 코드	재료9 수량	재료10	재료10 코드	재료10 수량
4Cls_4	적색 정기의 돌	2206	20	흑철괴	2287	10	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0
"""

TARGETS = """
#아이템 종류 어휘	설명(사용안함)	설명 텍스트 인덱스
TAEHWANG_ARMOR	태황갑옷	6889	;주석
TAEHWANG_HELMET	태황투구	6892
"""

ITEMS = {
    1129: "[고구려의] 주술 비법",
    2206: "적색 정기의 돌",
    2287: "흑철괴",
    1000: "fixture base",
    1001: "fixture result",
    8165: "주술해제 비법서",
}
STRINGS = {6889, 6892}


class EnchantParseTests(unittest.TestCase):
    def parse(self, recipe=RECIPE, materials=MATERIALS, targets=TARGETS):
        return parse_enchant(recipe, materials, targets, ITEMS, STRINGS)

    def test_skips_comments_and_keeps_codes_quantities_and_targets(self):
        recipes, materials, types, links = self.parse()
        self.assertEqual(recipes, [(1129, 15, 150, "4Cls_4")])
        self.assertEqual(materials, [("4Cls_4", 1, 2206, 20), ("4Cls_4", 2, 2287, 10)])
        self.assertEqual(types, [("TAEHWANG_ARMOR", 6889, 0), ("TAEHWANG_HELMET", 6892, 0)])
        self.assertEqual(links, [(1129, 1, "TAEHWANG_ARMOR"), (1129, 2, "TAEHWANG_HELMET")])

    def test_rejects_a_material_whose_name_does_not_match_its_code(self):
        broken = MATERIALS.replace("흑철괴", "다른물건")
        with self.assertRaises(ValueError):
            self.parse(materials=broken)

    def test_rejects_an_unknown_target_token(self):
        with self.assertRaises(ValueError):
            self.parse(recipe=RECIPE.replace("TAEHWANG_HELMET", "MISSING_HELMET"))


NPC = """
#NPC 코드	NPC 이름	NPC 등급	점괘 비용	주술연마 비용(읽지않음)	주술 연마 성공확률(실수형)	tarot param code	NPC 초상화 인덱스	NPC 인사말1	NPC 인사말2	NPC 인사말3	NPC 점괘 대사1	NPC 점괘 대사2	NPC 점괘 대사3	NPC 주술연마 대사1	NPC 주술연마 대사2	NPC 주술연마 대사3	주술연마 비용(1등급)	주술연마 비용(2등급)	주술연마 비용(3등급)	주술연마 비용(4등급)	주술연마 비용(5등급)	주술연마 비용(6등급)	주술연마 비용(7등급)
37	아무개 산신령	1	10000	5200	2500000	1Cls_EnchantChosun	58	2913	2914	2915	2916	2917	2918	2919	2920	2921	10000	50000	100000	300000	1000000	1000000	1000000
"""

BONUS = """
#Parm Code	TarotPrb. Share Rate	Inc.TarotPrb. Per Req.Level	Inc.TarotPrb. Per Req.Credit	fTarotPerIsZ	fTarotPerIsO	fTarotPerIsI	fTarotPerIsY	fTarotPerIsL	fTarotPerIsT	fTarotPerIsC	fTarotPerIsU	fTarotPerIsBelt	fTarotPerIsRing	fTarotPerIsShoes	fTarotPerIsArmor	fTarotPerIsGlove	fTarotPerIsHelmet	fTarotPerIsElement	fTarotPerIsFood	fTarotPerIsWeapon
1Cls_EnchantChosun	98	10000	10000	20000	20000	20000	10000	10000	40000	20000	25000	10000	10000	10000	10000	10000	10000	40000	35000	10000
"""


class EnchantNpcTests(unittest.TestCase):
    def test_keeps_fees_and_fortune_weights_without_the_unread_fee(self):
        npcs, bonuses = parse_npcs(NPC, BONUS, {37: "[전체:주술연마] 조선 아무개 산신령"})
        self.assertEqual(npcs, [(37, 1, 10000, 2500000, "1Cls_EnchantChosun", 10000, 50000, 100000, 300000, 1000000, 1000000, 1000000)])
        self.assertEqual(bonuses[0][0], "1Cls_EnchantChosun")
        self.assertEqual(bonuses[0][1], 98)
        self.assertNotIn(5200, npcs[0])


class EnchantImportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.target = self.root / "widget.db"
        self.client = self.root / "client"
        self.client.mkdir()
        (self.client / "gersang.gcs").write_bytes(b"mock archive; decoded tables supplied by fixture")
        with closing(sqlite3.connect(self.target)) as conn, conn:
            conn.execute("CREATE TABLE items (code INTEGER PRIMARY KEY, name_ko TEXT NOT NULL)")
            conn.executemany("INSERT INTO items VALUES (?, ?)", ITEMS.items())
            conn.execute("CREATE TABLE npcs (code INTEGER PRIMARY KEY, name_ko TEXT NOT NULL)")
            conn.execute("INSERT INTO npcs VALUES (37, '[전체:주술연마] 조선 아무개 산신령')")
            conn.execute("CREATE TABLE strings (object TEXT, id TEXT, lang TEXT, text TEXT)")
            conn.executemany("INSERT INTO strings VALUES ('OnlineText.dat', ?, 'zh', 'fixture')",
                             [(f"{code:08d}",) for code in STRINGS])
            conn.execute("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            conn.execute("INSERT INTO meta VALUES ('catalog_version', 'fixture-version')")
            conn.execute("CREATE TABLE recipes (id INTEGER PRIMARY KEY, source TEXT, result_item INTEGER)")
            conn.execute("INSERT INTO recipes VALUES (1, 'reinforce', 1129)")
            conn.execute("CREATE TABLE recipe_materials (recipe_id INTEGER, item_code INTEGER, qty INTEGER)")
            conn.execute("INSERT INTO recipe_materials VALUES (1, 2287, 7)")

    def import_fixture(self, recipe=RECIPE, materials=MATERIALS, targets=TARGETS,
                       equipment=ENCHANT_TEXT, effects=EFFECT_TEXT):
        with mock.patch.object(importer, "client_texts",
                               return_value=[recipe, materials, targets, NPC, BONUS, equipment, effects]):
            return importer.import_enchant(self.client, self.target)

    def rows(self, table):
        with closing(sqlite3.connect(self.target)) as conn:
            return conn.execute(f"SELECT * FROM {table} ORDER BY 1, 2").fetchall()

    def snapshot(self):
        with closing(sqlite3.connect(self.target)) as conn:
            return tuple(conn.iterdump())

    def test_imports_all_tables_preserves_catalog_and_reruns_without_changes(self):
        before = {table: self.rows(table) for table in (
            "items", "npcs", "strings", "recipes", "recipe_materials",
        )}
        result = self.import_fixture()
        self.assertEqual(result["counts"], {
            "scrolls": 1, "materials": 2, "material_tables": 1,
            "target_types": 2, "scroll_targets": 2, "npcs": 1, "npc_bonuses": 1,
            "equipment": 1, "effect_profiles": 1, "used_effect_profiles": 1,
            "linked_equipment": 1, "zero_scroll_equipment": 0, "missing_scroll_equipment": 0,
            "linked_scrolls": 1, "scrolls_without_equipment": 0,
            "unavailable_results": 0, "unavailable_bases": 0,
        })
        self.assertEqual(self.rows("enchant_recipes"), [(1129, 15, 150, "4Cls_4")])
        self.assertEqual(self.rows("enchant_materials"), [("4Cls_4", 1, 2206, 20), ("4Cls_4", 2, 2287, 10)])
        self.assertEqual(self.rows("enchant_target_types"), [("TAEHWANG_ARMOR", 6889, 0), ("TAEHWANG_HELMET", 6892, 0)])
        self.assertEqual(self.rows("enchant_recipe_targets"), [(1129, 1, "TAEHWANG_ARMOR"), (1129, 2, "TAEHWANG_HELMET")])
        self.assertEqual(self.rows("enchant_npcs")[0][3], 2500000)
        self.assertEqual(self.rows("enchant_npc_bonus")[0][1], 98)
        for table, rows in before.items():
            self.assertEqual(self.rows(table), rows, table)
        metadata = dict(self.rows("meta"))
        self.assertEqual(metadata["catalog_version"], "fixture-version")
        self.assertEqual(json.loads(metadata["enchant_source"]), result)
        self.assertEqual(result["archive_sha256"], importer.file_sha256(self.client / "gersang.gcs"))
        self.assertEqual(len(result["tables"]), 7)
        with closing(sqlite3.connect(self.target)) as conn:
            self.assertEqual(conn.execute(
                "SELECT scroll_item, base_item, result_item, effect_strength, effect_dexterity, "
                "effect_bonus_experience_raw FROM v_enchant_equipment"
            ).fetchall(), [(1129, 1000, 1001, 20, -5, "0.05")])
            self.assertEqual(conn.execute(
                "SELECT e.result_item, m.material_item, m.qty FROM v_enchant_equipment e "
                "JOIN v_enchant_materials m ON m.scroll_item=e.scroll_item ORDER BY m.slot"
            ).fetchall(), [(1001, 2206, 20), (1001, 2287, 10)])
        initial_bytes, initial_rows = self.target.read_bytes(), self.snapshot()
        self.assertEqual(self.import_fixture(), result)
        self.assertEqual(self.snapshot(), initial_rows)
        self.assertEqual(self.target.read_bytes(), initial_bytes)

    def test_refresh_removes_stale_materials_and_targets_and_updates_values(self):
        first = self.import_fixture()
        changed = self.import_fixture(
            recipe=RECIPE.replace("15\tLevel150", "20\tLevel150").replace(", TAEHWANG_HELMET", ""),
            materials=MATERIALS.replace("적색 정기의 돌\t2206\t20", "적색 정기의 돌\t2206\t25")
                               .replace("흑철괴\t2287\t10", "0\t0\t0"),
            targets=TARGETS.replace("TAEHWANG_HELMET\t태황투구\t6892\n", ""),
        )
        self.assertNotEqual(first["sha256"], changed["sha256"])
        self.assertEqual(self.rows("enchant_recipes"), [(1129, 20, 150, "4Cls_4")])
        self.assertEqual(self.rows("enchant_materials"), [("4Cls_4", 1, 2206, 25)])
        self.assertEqual(self.rows("enchant_recipe_targets"), [(1129, 1, "TAEHWANG_ARMOR")])
        self.assertEqual(self.rows("enchant_target_types"), [("TAEHWANG_ARMOR", 6889, 0)])

    def test_refresh_removes_stale_equipment_and_effect_profiles(self):
        equipment, effect = list(ENCHANT_FIELDS), list(EFFECT_FIELDS)
        equipment[0], equipment[36], effect[0] = "1002", "second_effect", "second_effect"
        self.import_fixture(equipment=ENCHANT_TEXT + "\t".join(equipment) + "\n",
                            effects=EFFECT_TEXT + "\t".join(effect) + "\n")
        self.assertEqual(len(self.rows("enchant_equipment")), 2)
        self.assertEqual(len(self.rows("enchant_effects")), 2)
        self.import_fixture()
        self.assertEqual([row[0] for row in self.rows("enchant_equipment")], [1001])
        self.assertEqual([row[0] for row in self.rows("enchant_effects")], ["fixture_effect"])

    def test_first_import_rolls_back_schema_and_rows_after_late_write_failure(self):
        with closing(sqlite3.connect(self.target)) as conn, conn:
            conn.execute("CREATE TRIGGER reject_enchant_meta BEFORE INSERT ON meta "
                         "WHEN NEW.key = 'enchant_source' BEGIN SELECT RAISE(ABORT, 'fixture metadata failure'); END")
        before, before_bytes = self.snapshot(), self.target.read_bytes()
        with self.assertRaisesRegex(sqlite3.IntegrityError, "fixture metadata failure"):
            self.import_fixture()
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(self.target.read_bytes(), before_bytes)

    def test_refresh_rolls_back_deleted_and_updated_rows_after_late_failure(self):
        self.import_fixture()
        with closing(sqlite3.connect(self.target)) as conn, conn:
            conn.execute("CREATE TRIGGER reject_enchant_meta BEFORE UPDATE ON meta "
                         "WHEN NEW.key = 'enchant_source' BEGIN SELECT RAISE(ABORT, 'fixture refresh failure'); END")
        before, before_bytes = self.snapshot(), self.target.read_bytes()
        with self.assertRaisesRegex(sqlite3.IntegrityError, "fixture refresh failure"):
            self.import_fixture(materials=MATERIALS.replace("흑철괴\t2287\t10", "0\t0\t0"))
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(self.target.read_bytes(), before_bytes)

    def test_incompatible_existing_schema_does_not_create_or_replace_tables(self):
        with closing(sqlite3.connect(self.target)) as conn, conn:
            conn.execute("CREATE TABLE enchant_npcs (npc_code TEXT PRIMARY KEY)")
            conn.execute("INSERT INTO enchant_npcs VALUES ('preserve-me')")
        before = self.snapshot()
        with self.assertRaisesRegex(ValueError, "Incompatible existing enchant_npcs columns"):
            self.import_fixture()
        self.assertEqual(self.snapshot(), before)

    def test_matching_columns_with_wrong_type_are_rejected_before_deletion(self):
        with closing(sqlite3.connect(self.target)) as conn, conn:
            conn.execute("CREATE TABLE enchant_recipes (item_code INTEGER PRIMARY KEY, "
                         "success_rate TEXT NOT NULL, level INTEGER NOT NULL, recipe_key TEXT NOT NULL)")
            conn.execute("INSERT INTO enchant_recipes VALUES (999, 'preserve-me', 1, 'fixture')")
        before = self.snapshot()
        with self.assertRaisesRegex(ValueError, "schema at success_rate"):
            self.import_fixture()
        self.assertEqual(self.snapshot(), before)

    def test_invalid_source_leaves_existing_enchant_data_untouched(self):
        self.import_fixture()
        before = self.snapshot()
        with self.assertRaisesRegex(ValueError, "missing material tables"):
            self.import_fixture(recipe=RECIPE.replace("4Cls_4", "missing"))
        self.assertEqual(self.snapshot(), before)

    def test_missing_target_is_not_created(self):
        missing = self.root / "absent.db"
        with self.assertRaisesRegex(ValueError, "Target catalog does not exist"):
            importer.import_enchant(self.client, missing)
        self.assertFalse(missing.exists())


class EnchantParserLocationTests(unittest.TestCase):
    def test_finds_sibling_parser_from_nested_widget_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / "gs-parser" / "gersang"
            package.mkdir(parents=True)
            for name in ("archive.py", "extra.py"):
                (package / name).touch()
            with mock.patch.object(importer, "ROOT", root / "gs-widget" / "gs-widget"):
                self.assertEqual(importer.parser_package_root(), package.parent.resolve())

    def test_explicit_parser_location_is_used_and_invalid_location_explains_flag(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / "custom-parser" / "gersang"
            package.mkdir(parents=True)
            for name in ("archive.py", "extra.py"):
                (package / name).touch()
            self.assertEqual(importer.parser_package_root(package.parent), package.parent.resolve())
            with self.assertRaisesRegex(ValueError, "Pass --parser-root"):
                importer.parser_package_root(root / "missing")

    def test_missing_client_archive_explains_client_flag(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(ValueError, "Pass --client"):
                importer.client_texts(Path(temporary))


if __name__ == "__main__":
    unittest.main()
