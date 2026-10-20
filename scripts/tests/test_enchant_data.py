import sqlite3
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import enchant_data as data


# Independent source headers, including the client's inheritance descriptions.
ENCHANT_HEADER = "#아이템 코드\t아이템 이름\t아이템등급\t수수께끼\t강화 가능 여부\t공명 가능 여부\t아이템 이름 코드(기본 이름을 바꿀 경우에만 사용, 0:이면 자동으로 원본 아이템 이름을 가져다 쓴다)\t기저 아이템 코드\t기저 아이템 이름\t비법 아이템 코드\t비법 아이템 이름\t원복 아이템 코드\t원복 아이템 이름\t대박 아이템 코드\t대박 아이템 이름\t아이템 접두사 코드\t아이템 접두사\t아이템 접미사 코드\t아이템 접미사\t주술연마 아이템 등급(1~4)\t그룹\t조합 캐릭터 이미지 인덱스\t유닛 단위\t유닛 단위 코드\t아이템 설명\t아이템 설명 코드\t가격\t무게\t인벤토리 스프라이트 이름(기저용을 사용한다면, 0)\t인벤토리 스프라이트 폰트 번호(기저 아이템것을 사용한다면, -1)\t필드 스프라이트 이름(기저용을 사용한다면, 0)\t필드 스프라이트 폰트 번호(기저 아이템것을 사용한다면, -1)\t착용가능한가?(기저 아이템 것을 사용한다면, -1)\t사용 가능한가?(기저 아이템 것을 이용한다면, -1)\t무기 타입(기저 아이템 것을 사용한다면, 0)\t사운드\t기본 성능 향상 테이블"
EFFECT_HEADER = "#테이블 인덱스\t힘\t민첩\t지력\t생명\t체력\t마법력\t물리저항력\t마법저항력\t방어력\t최소 데미지\t최대 데미지\t연사속도\t사정거리\t마법 테이블 인덱스(기저 아이템의 값을 사용한다면, 0)\t마법명(Comment)\t마법 최소 데미지\t마법 최대 데미지\t용병 방어력 증가\t용병 공격력 증가\t보너스 경험치 퍼센트(실수형, %)\t이동속도\t주술 연마 성공확률 주인공(실수형)\t주술 연마 성공률 용병(실수형)\t속성값 증가\t공격 데미지 증가\t피해 데미지 감소\t치명타 확률 증가\t치명타 피해 증가\t필요 힘\t필요 민첩\t필요 지력\t필요 체력\t필요 레벨\t필요 신용등급\t시간제 아이템인가?(기저 아이템의 값을 사용한다면, -1)\t몇 일용인가?(기저 아이템의 값을 사용한다면, -1)\t아이템몰 아이템인가?(기저 아이템의 값을 사용한다면, -1)"
ENCHANT_FIELDS = [
    "1001", "fixture result", "0", "0", "0", "0", "0", "1000", "fixture base",
    "1129", "[고구려의] 주술 비법", "8165", "주술해제 비법서", "0", "없음", "2506",
    "fixture prefix", "0", "없음", "4", "NONE", "0", "개", "48", "fixture description",
    "2726", "2450", "0", "NONE", "-1", "NONE", "-1", "-1", "-1", "NONE", "ETC", "fixture_effect",
]
EFFECT_FIELDS = [
    "fixture_effect", "20", "-5", "0", "7", "0", "0", "5", "10", "15", "2", "3", "0", "1",
    "0", "0", "0", "0", "50", "30", "0.05", "2", "5", "0.5", "1", "10", "20", "5", "40",
    "0", "0", "0", "0", "150", "0", "-1", "-1", "-1",
]
ENCHANT_TEXT = ENCHANT_HEADER + "\n" + "\t".join(ENCHANT_FIELDS) + "\n"
EFFECT_TEXT = EFFECT_HEADER + "\n" + "\t".join(EFFECT_FIELDS) + "\n"
CATALOG_ITEMS = {1000: "fixture base", 1001: "fixture result", 1129: "[고구려의] 주술 비법", 8165: "주술해제 비법서"}


def change_row(header, fields, index, value):
    fields = list(fields)
    fields[index] = value
    return header + "\n" + "\t".join(fields) + "\n"


class EnchantDataTests(unittest.TestCase):
    def parse(self, equipment=ENCHANT_TEXT, effects=EFFECT_TEXT, catalog=None, scrolls=(1129,)):
        return data.parse_enchant_data(equipment, effects, CATALOG_ITEMS if catalog is None else catalog, scrolls)

    def row(self, result, table):
        return dict(zip(data.TABLE_COLUMNS[table], result[table][0]))

    def test_explicit_codes_link_scroll_base_result_and_all_effect_values(self):
        parsed = self.parse()
        equipment = self.row(parsed, "enchant_equipment")
        effect = self.row(parsed, "enchant_effects")
        self.assertEqual((equipment["scroll_item"], equipment["base_item"], equipment["result_item"]), (1129, 1000, 1001))
        self.assertEqual(equipment["effect_key"], effect["effect_key"])
        self.assertEqual(equipment["revert_scroll_item"], 8165)
        self.assertEqual(equipment["scroll_status"], "linked")
        self.assertEqual((effect["strength"], effect["dexterity"], effect["vitality"]), (20, -5, 7))
        self.assertEqual((effect["damage_min"], effect["damage_max"]), (2, 3))
        self.assertEqual((effect["required_level"], equipment["enchant_grade"]), (150, 4))
        self.assertEqual(parsed["counts"]["linked_scrolls"], 1)
        self.assertEqual(parsed["counts"]["unavailable_results"], 0)

    def test_preserves_decimal_literals_inheritance_sentinels_and_complete_raw_rows(self):
        parsed = self.parse()
        equipment = self.row(parsed, "enchant_equipment")
        effect = self.row(parsed, "enchant_effects")
        self.assertEqual(effect["bonus_experience_raw"], "0.05")
        self.assertEqual(effect["main_enchant_success_raw"], "5")
        self.assertEqual(effect["mercenary_enchant_success_raw"], "0.5")
        self.assertEqual(effect["magic_table_key"], "0")
        self.assertEqual([effect[key] for key in ("timed_item", "days", "mall_item")], [-1, -1, -1])
        self.assertEqual([equipment[key] for key in ("inventory_frame", "field_frame", "wearable", "usable")], [-1] * 4)
        self.assertEqual(equipment["raw_source_row"], "\t".join(ENCHANT_FIELDS))
        self.assertEqual(effect["raw_source_row"], "\t".join(EFFECT_FIELDS))
        self.assertEqual(equipment["source_line"], 2)

    def test_zero_scroll_is_not_guessed_from_identical_name_or_prefix(self):
        parsed = self.parse(equipment=change_row(ENCHANT_HEADER, ENCHANT_FIELDS, 9, "0"))
        equipment = self.row(parsed, "enchant_equipment")
        self.assertEqual(equipment["scroll_item"], 0)
        self.assertEqual(equipment["scroll_name_ko"], CATALOG_ITEMS[1129])
        self.assertEqual(equipment["scroll_status"], "no_scroll")
        self.assertEqual(equipment["scroll_available"], 0)
        self.assertEqual(parsed["counts"]["zero_scroll_equipment"], 1)
        self.assertEqual(parsed["counts"]["scrolls_without_equipment"], 1)

    def test_missing_nonzero_scroll_and_unavailable_items_remain_explicit(self):
        parsed = self.parse(equipment=change_row(ENCHANT_HEADER, ENCHANT_FIELDS, 9, "11642"), catalog={8165})
        equipment = self.row(parsed, "enchant_equipment")
        self.assertEqual(equipment["scroll_item"], 11642)
        self.assertEqual(equipment["scroll_status"], "missing_scroll")
        self.assertEqual([equipment[key] for key in data.AVAILABILITY_COLUMNS], [0, 0, 0, 1, 0])
        self.assertEqual(parsed["counts"]["missing_scroll_equipment"], 1)
        self.assertEqual(parsed["counts"]["unavailable_results"], 1)
        self.assertEqual(parsed["counts"]["unavailable_bases"], 1)

    def test_unused_profiles_are_preserved_and_separate_from_equipment_coverage(self):
        unused = list(EFFECT_FIELDS)
        unused[0] = "unused_effect"
        parsed = self.parse(effects=EFFECT_TEXT + "\t".join(unused) + "\n", scrolls=(1129, 1130))
        self.assertEqual(parsed["counts"]["effect_profiles"], 2)
        self.assertEqual(parsed["counts"]["used_effect_profiles"], 1)
        self.assertEqual(parsed["counts"]["scrolls_without_equipment"], 1)

    def test_missing_effect_reference_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "references missing effect"):
            self.parse(equipment=change_row(ENCHANT_HEADER, ENCHANT_FIELDS, 36, "missing"))

    def test_duplicate_results_and_profiles_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "Duplicate enchant result item"):
            self.parse(equipment=ENCHANT_TEXT + "\t".join(ENCHANT_FIELDS) + "\n")
        with self.assertRaisesRegex(ValueError, "duplicate enchant effect key"):
            self.parse(effects=EFFECT_TEXT + "\t".join(EFFECT_FIELDS) + "\n")

    def test_swapped_strength_dexterity_headers_are_rejected(self):
        header = EFFECT_HEADER.split("\t")
        header[1], header[2] = header[2], header[1]
        with self.assertRaisesRegex(ValueError, "Unexpected column order"):
            self.parse(effects="\t".join(header) + "\n" + "\t".join(EFFECT_FIELDS))

    def test_swapped_base_and_scroll_headers_are_rejected(self):
        header = ENCHANT_HEADER.split("\t")
        header[7], header[9] = header[9], header[7]
        with self.assertRaisesRegex(ValueError, "Unexpected column order"):
            self.parse(equipment="\t".join(header) + "\n" + "\t".join(ENCHANT_FIELDS))

    def test_shifted_row_invalid_integer_and_nonfinite_rate_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "columns, expected 37"):
            self.parse(equipment=ENCHANT_HEADER + "\n" + "\t".join(ENCHANT_FIELDS[:-1]))
        for index, value in ((1, "1.5"), (1, str(2 ** 63)), (20, "NaN"), (23, "Infinity")):
            with self.subTest(index=index, value=value), self.assertRaises(ValueError):
                self.parse(effects=change_row(EFFECT_HEADER, EFFECT_FIELDS, index, value))

    def test_backup_rows_and_trailing_blank_header_columns_are_ignored(self):
        text = "; ignored backup\n" + ENCHANT_HEADER + "\t\t\n;" + "\t".join(ENCHANT_FIELDS) + "\n" + "\t".join(ENCHANT_FIELDS)
        parsed = self.parse(equipment=text)
        self.assertEqual(parsed["counts"]["equipment"], 1)
        self.assertEqual(self.row(parsed, "enchant_equipment")["source_line"], 4)

    def test_schema_supports_exact_typed_roundtrip_and_stays_in_caller_transaction(self):
        parsed = self.parse()
        with sqlite3.connect(":memory:") as conn:
            conn.execute("BEGIN IMMEDIATE")
            data.create_tables(conn)
            self.assertTrue(conn.in_transaction)
            for table, columns in data.TABLE_COLUMNS.items():
                conn.executemany(f"INSERT INTO {table} ({', '.join(columns)}) VALUES ({', '.join('?' for _ in columns)})", parsed[table])
                self.assertEqual(conn.execute(f"SELECT {', '.join(columns)} FROM {table}").fetchall(), parsed[table])
            data.create_tables(conn)
            conn.rollback()
            self.assertEqual(conn.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall(), [])

    def test_incompatible_schema_is_rejected(self):
        with sqlite3.connect(":memory:") as conn:
            conn.execute("CREATE TABLE enchant_effects (effect_key TEXT PRIMARY KEY)")
            with self.assertRaisesRegex(ValueError, "Incompatible existing enchant_effects columns"):
                data.create_tables(conn)


if __name__ == "__main__":
    unittest.main()
