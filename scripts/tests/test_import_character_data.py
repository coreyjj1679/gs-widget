"""End-to-end tests for a small, independent character/companion fixture."""

import importlib.util
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / "import_character_data.py"
spec = importlib.util.spec_from_file_location("import_character_data", MODULE_PATH)
importer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(importer)

CHARACTERS = """CREATE TABLE characters (
    code TEXT PRIMARY KEY, nation INTEGER NOT NULL, rank INTEGER NOT NULL,
    name_ko TEXT NOT NULL, name_gts INTEGER NOT NULL, desc_gts INTEGER NOT NULL,
    hire_gts INTEGER NOT NULL, portrait_path TEXT NOT NULL,
    portrait_frame INTEGER NOT NULL, sprite_path TEXT NOT NULL)"""


class CharacterImportTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.source = Path(self.temp.name) / "source.db"
        self.target = Path(self.temp.name) / "target.db"
        self.client = Path(self.temp.name) / "client"
        with closing(sqlite3.connect(self.source)) as source, source:
            source.execute("CREATE TABLE raw_sheet(archive,member,title,table_name)")
            source.execute("CREATE TABLE raw_doc(archive,member,body)")
            source.execute("CREATE TABLE strings(object,id,lang,text,PRIMARY KEY(object,id,lang))")
            source.execute("CREATE TABLE monster_sprites(code,path)")
            # Deliberately use unrelated member/table numbers: discovery must
            # match the column signature, not a current archive position.
            self.tables = {}
            for i, (kind, columns) in enumerate(importer.SHEETS.items()):
                table = f"fixture_{i + 2000}"
                self.tables[kind] = table
                source.execute(f"CREATE TABLE {table} (" + ",".join(importer.quoted(c) + " TEXT" for c in sorted(columns)) + ")")
                source.execute("INSERT INTO raw_sheet VALUES ('gersang.gcs', ?, ?, ?)", (i + 2000, kind, table))
            self.row(source, "stats", {"ID": "U00", "Name": "Porter", "Name Code": "10", "Str": "15", "Dex": "10", "Vit": "20", "Int": "5", "Life": "80", "Mana": "20", "Type": "0", importer.MIN_ATTACK: "12", importer.MAX_ATTACK: "14"})
            self.row(source, "stats", {"ID": "F16", "Name": "Spirit", "Name Code": "11", "Str": "100", "Life": "2000", "Mana": "200", "Type": "0"})
            self.row(source, "stats", {"ID": "H29", "Name": "Dragon", "Name Code": "12", "Str": "3000", "Dex": "3000", "Vit": "3750", "Int": "500", "Life": "15000", "Mana": "2000", "DamageResist": "220", "MagicResist": "220", "Type": "W", "ElementDamage": "20"})
            self.row(source, "spirits", {"CharacterID": "F16", "CharacterName": "Spirit", importer.ATTRIBUTE: "3", importer.GRADE: "1", "Group": "3", "SkillGTS": "20", "Upper CharacterID": "H29", importer.INTIMACY: "3000", "Material1": "100", "Quentity1": "2"})
            self.row(source, "spirits", {"CharacterID": "H29", "CharacterName": "Dragon", importer.ATTRIBUTE: "3", importer.GRADE: "4", "Group": "3", "SkillGTS": "21", "Upper CharacterID": "0", importer.INTIMACY: "100000000"})
            for mount_id in (1, 3):
                self.row(source, "vehicles", {"Name": f"Mount {mount_id}", "ID": str(mount_id), "Vehicle": f"HO{mount_id:02d}"})
                self.row(source, "mounts", {"말 번호": str(mount_id), "이름": f"Mount {mount_id}", "이름 GTS": str(30 + mount_id), "필요 승마술 GTS": "2", "필드 이동속도": "3", "습득 정보 GTS": "40"})
                skin = {"스킨 번호": str(mount_id), "초상화 번호": str(mount_id - 1), "요구 승마술 레벨": "2", "말 이름 GTS": str(30 + mount_id), "말 설명 GTS": "41", "이동속도": "1", "특성 레벨1": "1"}
                if mount_id == 3:
                    skin.update({"특성 종류2": "2", "특성 레벨2": "0", "특성 종류3": "3", "특성 레벨3": "0", "특성 범위3": "1", "특성 종류4": "8", "특성 레벨4": "2", "특성 범위4": "1"})
                self.row(source, "skins", skin)
            for kind in (1, 2, 3, 8):
                self.row(source, "traits", {"특성 번호": str(kind), "주인공 옵션 GTS": "42", "아군 옵션 GTS": "43"})
            self.row(source, "bonuses", {"번호": "0", "힘 증가": "25", "민첩 증가": "25", "생명력 증가": "25", "지력 증가": "100", "이동속도 증가": "2", "타격저항 증가": "20"})
            self.row(source, "bonuses", {"번호": "1", "이동속도 증가": "3"})
            self.row(source, "bonuses", {"번호": "2", "공격력 증가": "300"})
            self.row(source, "purchases", {"판매 말 번호": "3", "말종류 (0: 개량말, 1: 기본말)": "0", "필요 승마술 레벨": "2", "재료 말": "1", "재료 코드1": "101", "수량1": "40", "수수료": "10000000"})
            self.row(source, "riding", {"승마술 레벨": "2", "승마술 정보 GTS": "50", "승마술 아이템 코드1": "102", "승마술 아이템 개수1": "200", "수수료": "10000000"})
            self.row(source, "riding", {"승마술 레벨": "3", "승마술 정보 GTS": "51", "승마술 아이템 코드1": "102", "승마술 아이템 개수1": "200", "수수료": "10000000"})
            self.row(source, "riding", {"승마술 레벨": "4", "승마술 정보 GTS": "52", "승마술 아이템 코드1": "102", "승마술 아이템 개수1": "200", "수수료": "10000000"})
            for gts in (10, 11, 12, 20, 21, 31, 33, 40, 41, 42, 43, 50, 51, 52):
                for lang in ("en", "zh"):
                    value = {50: "Need 200 whips", 51: "This is the current maximum rating.", 52: "Temporary Text"}.get(gts, f"{lang} Name {gts}")
                    source.execute("INSERT INTO strings VALUES ('OnlineText.dat',?,?,?)", (f"{gts:08d}", lang, value))
            source.execute("INSERT INTO raw_doc VALUES ('gersang.gcs', 2500, ?)", ('SetCharacterBasisData(x, "name, with comma", 11, 583, 0, "F16")\nSetCharacterBasisData(x, "dragon", 12, 1009, 0, "H29")',))
            source.executemany("INSERT INTO monster_sprites VALUES (?,?)", [("F16", "water.agf"), ("H29", "fire.agf")])
        with closing(sqlite3.connect(self.target)) as target, target:
            target.execute(CHARACTERS)
            target.execute("INSERT INTO characters VALUES ('U00',1,0,'Porter',10,0,0,'original.agf',1,'original-sprite.agf')")
            target.execute("CREATE TABLE strings(object,id,lang,text,PRIMARY KEY(object,id,lang))")
            target.execute("INSERT INTO strings VALUES ('OnlineText.dat','00000031','en','Preferred horse name')")
            target.execute("CREATE TABLE items(code INTEGER PRIMARY KEY)")
            target.executemany("INSERT INTO items VALUES (?)", [(100,), (101,), (102,)])
            target.execute("CREATE TABLE monster_drops(monster_code,slot,item_code,drop_rate_per_100000)")
            target.execute("INSERT INTO monster_drops VALUES ('M01',1,100,5500)")

    def row(self, source, kind, overrides):
        columns = sorted(importer.SHEETS[kind])
        values = {c: "0" for c in columns}
        values.update(overrides)
        source.execute(f"INSERT INTO {self.tables[kind]} ({','.join(map(importer.quoted, columns))}) VALUES ({','.join('?' for _ in columns)})", [values[c] for c in columns])

    def run_import(self):
        return importer.import_character_data(self.source, self.target, client_root=self.client)

    def snapshot(self):
        with closing(sqlite3.connect(self.target)) as target, target:
            return list(target.iterdump())

    def test_import_preserves_base_units_companion_relations_and_existing_data(self):
        original_source = self.source.read_bytes()
        result = self.run_import()
        self.assertEqual(self.source.read_bytes(), original_source)
        self.assertEqual(result["counts"]["characters_with_stats"], 3)
        with closing(sqlite3.connect(self.target)) as db, db:
            self.assertEqual(db.execute("SELECT strength,hp,mp,attack_speed,unit_type,element,element_value FROM character_stats WHERE code='H29'").fetchone(), (3000, 15000, 2000, None, None, 2, 20))
            self.assertEqual(db.execute("SELECT count(*) FROM character_stats WHERE code LIKE 'mount:%'").fetchone()[0], 0)
            self.assertEqual(db.execute("SELECT rank,portrait_path,portrait_frame,sprite_path FROM characters WHERE code='F16'").fetchone(), (14, "Assets/Textures/UI/Portraits/PORTRAIT_INVEN_2.AGF", 283, "water.agf"))
            self.assertEqual(db.execute("SELECT target_code FROM character_upgrades WHERE code='F16'").fetchone()[0], "H29")
            self.assertEqual(db.execute("SELECT target_code FROM character_requirements WHERE code='mount:3'").fetchone()[0], "mount:1")
            self.assertEqual(db.execute("SELECT item_code,qty FROM character_materials WHERE code='mount:3' AND kind='acquisition'").fetchone(), (101, 40))
            self.assertEqual(db.execute("SELECT value_en,value_zh FROM character_facts WHERE code='mount:3' AND label_en='Attack bonus'").fetchone(), ("+300 · Main character and mercenaries", "+300 · 主角與傭兵"))
            self.assertEqual(db.execute("SELECT value_en FROM character_facts WHERE code='mount:3' AND label_en='Field movement speed'").fetchone()[0], "3")
            self.assertEqual(db.execute("SELECT value_en FROM character_facts WHERE code='mount:3' AND label_en='Movement bonus'").fetchone()[0], "+3 · Main character")
            self.assertEqual(db.execute("SELECT text FROM strings WHERE id='00000031' AND lang='en'").fetchone()[0], "Preferred horse name")
            self.assertEqual(db.execute("SELECT portrait_path FROM characters WHERE code='U00'").fetchone()[0], "original.agf")
            self.assertEqual(db.execute("SELECT * FROM monster_drops").fetchone(), ("M01", 1, 100, 5500))

    def test_terminal_evolution_and_placeholder_riding_requirements_are_excluded(self):
        self.run_import()
        with closing(sqlite3.connect(self.target)) as db, db:
            self.assertEqual(db.execute("SELECT count(*) FROM character_facts WHERE code='H29' AND label_en='Evolution intimacy'").fetchone()[0], 0)
            self.assertEqual(db.execute("SELECT DISTINCT kind FROM character_materials WHERE kind LIKE 'riding:%'").fetchall(), [("riding:2",)])
            self.assertEqual(db.execute("SELECT count(*) FROM character_facts WHERE value_en LIKE '%Temporary%' OR label_en LIKE '%level 3%'").fetchone()[0], 0)

    def test_family_and_grade_do_not_depend_on_combat_type_or_chain_position(self):
        self.run_import()
        with closing(sqlite3.connect(self.target)) as db, db:
            self.assertEqual(db.execute("SELECT code,family_element,spirit_attribute,spirit_grade,spirit_group,evolution_root,evolution_order FROM character_companions WHERE kind='spirit' ORDER BY evolution_order").fetchall(), [
                ("F16", 2, 3, 1, 3, "F16", 1),
                ("H29", 2, 3, 4, 3, "F16", 2),
            ])
            # The early spirit is water-family even though its base combat
            # attribute is neutral; the later form has intrinsic water attack.
            self.assertEqual(db.execute("SELECT code,element FROM character_stats WHERE code IN ('F16','H29') ORDER BY code").fetchall(), [("F16", 0), ("H29", 2)])
            self.assertEqual(db.execute("SELECT riding_level,field_move_speed,movement_bonus,movement_scope,family_element,spirit_grade FROM character_companions WHERE code='mount:3'").fetchone(), (2, 3, 3, 0, None, None))

    def test_non_elemental_pet_groups_stay_separate_and_do_not_invent_earlier_stages(self):
        with closing(sqlite3.connect(self.source)) as db, db:
            for code, group, grade in [("F38", 6, 2), ("T109", 7, 2), ("K08", 8, 1)]:
                self.row(db, "stats", {"ID": code, "Name": code, "Name Code": "11", "Type": "0"})
                self.row(db, "spirits", {"CharacterID": code, "CharacterName": code, importer.ATTRIBUTE: "6", importer.GRADE: str(grade), "Group": str(group), "SkillGTS": "20"})
        self.run_import()
        with closing(sqlite3.connect(self.target)) as db, db:
            self.assertEqual(db.execute("SELECT code,family_element,spirit_group,spirit_grade,evolution_root,evolution_order FROM character_companions WHERE family_element=0 ORDER BY spirit_group").fetchall(), [
                ("F38", 0, 6, 2, "F38", 1),
                ("T109", 0, 7, 2, "T109", 1),
                ("K08", 0, 8, 1, "K08", 1),
            ])

    def test_invalid_evolution_graph_rolls_back_instead_of_misgrouping(self):
        before = self.target.read_bytes()
        with closing(sqlite3.connect(self.source)) as db, db:
            db.execute(f'UPDATE {self.tables["spirits"]} SET "Upper CharacterID"=? WHERE CharacterID=?', ("F16", "H29"))
        with self.assertRaisesRegex(ValueError, "Cycle"):
            self.run_import()
        self.assertEqual(self.target.read_bytes(), before)
        with closing(sqlite3.connect(self.source)) as db, db:
            db.execute(f'UPDATE {self.tables["spirits"]} SET "Upper CharacterID"=? WHERE CharacterID=?', ("0", "H29"))
            db.execute(f'UPDATE {self.tables["spirits"]} SET {importer.quoted(importer.ATTRIBUTE)}=? WHERE CharacterID=?', ("2", "H29"))
        with self.assertRaisesRegex(ValueError, "crosses spirit family"):
            self.run_import()
        self.assertEqual(self.target.read_bytes(), before)

    def test_absent_movement_bonus_stays_null_and_duplicate_traits_are_rejected(self):
        with closing(sqlite3.connect(self.source)) as db, db:
            db.execute(f'UPDATE {self.tables["skins"]} SET "이동속도"=0 WHERE "스킨 번호"=1')
        self.run_import()
        with closing(sqlite3.connect(self.target)) as db, db:
            self.assertEqual(db.execute("SELECT movement_bonus,movement_scope FROM character_companions WHERE code='mount:1'").fetchone(), (None, None))
        before = self.snapshot()
        with closing(sqlite3.connect(self.source)) as db, db:
            db.execute(f'UPDATE {self.tables["skins"]} SET "특성 종류5"=1 WHERE "스킨 번호"=3')
        with self.assertRaisesRegex(ValueError, "Ambiguous movement traits"):
            self.run_import()
        self.assertEqual(self.snapshot(), before)

    def test_mount_sprite_uses_source_vehicle_token_and_preserves_real_filename_case(self):
        directory = self.client / "Assets" / "Textures" / "HeroCharacter"
        directory.mkdir(parents=True)
        for filename in ("veho01n11hsu.AGF", "VEHO14N11HSU.AGF", "veho03n11hsu.AGF", "VHHO14N11HSU.AGF"):
            (directory / filename).write_bytes(b"asset fixture")
        with closing(sqlite3.connect(self.source)) as db, db:
            # The vehicle token is the join, not the numerical mount ID. An
            # ID-based filename guess would select the wrong HO03 sheet here.
            db.execute(f'UPDATE {self.tables["vehicles"]} SET Vehicle=? WHERE ID=?', ("HO14", "3"))
        result = self.run_import()
        self.assertEqual(result["counts"]["mount_sprite_assets"], 2)
        with closing(sqlite3.connect(self.target)) as db, db:
            self.assertEqual(db.execute("SELECT sprite_path FROM characters WHERE code='mount:3'").fetchone()[0], "Assets/Textures/HeroCharacter/VEHO14N11HSU.AGF")
        before = self.snapshot()
        self.run_import()
        self.assertEqual(self.snapshot(), before)

    def test_missing_mount_base_does_not_use_glow_layer_or_other_mount(self):
        directory = self.client / "Assets" / "Textures" / "HeroCharacter"
        directory.mkdir(parents=True)
        for filename in ("VHHO03N11HSU.AGF", "VTHO03N11HSU.AGF", "veho02n11hsu.AGF"):
            (directory / filename).write_bytes(b"asset fixture")
        result = self.run_import()
        self.assertEqual(result["counts"]["mount_sprite_assets"], 0)
        with closing(sqlite3.connect(self.target)) as db, db:
            self.assertEqual(db.execute("SELECT sprite_path FROM characters WHERE code='mount:3'").fetchone()[0], "")
            self.assertEqual(db.execute("SELECT count(*) FROM character_companions").fetchone()[0], 4)

    def test_missing_vehicle_join_rejects_import_without_rewriting_target(self):
        with closing(sqlite3.connect(self.source)) as db, db:
            db.execute(f'DELETE FROM {self.tables["vehicles"]} WHERE ID=3')
        before = self.snapshot()
        with self.assertRaisesRegex(ValueError, "Missing vehicle asset mapping"):
            self.run_import()
        self.assertEqual(self.snapshot(), before)

    def test_repeat_import_is_semantically_idempotent(self):
        self.run_import()
        before = self.snapshot()
        self.run_import()
        self.assertEqual(self.snapshot(), before)

    def test_invalid_source_leaves_every_target_table_unchanged(self):
        before = self.target.read_bytes()
        with closing(sqlite3.connect(self.source)) as db, db:
            db.execute(f'UPDATE {self.tables["stats"]} SET Life=? WHERE ID=?', ("not a number", "H29"))
        with self.assertRaisesRegex(ValueError, "Invalid integer"):
            self.run_import()
        self.assertEqual(self.target.read_bytes(), before)

    def test_write_failure_rolls_back_new_tables_and_companion_rows(self):
        with closing(sqlite3.connect(self.target)) as db, db:
            db.execute("CREATE TRIGGER reject_companion BEFORE INSERT ON characters WHEN NEW.rank=14 BEGIN SELECT RAISE(ABORT,'test rejection'); END")
        before = self.snapshot()
        with self.assertRaisesRegex(sqlite3.IntegrityError, "test rejection"):
            self.run_import()
        self.assertEqual(self.snapshot(), before)

    def test_companion_code_collision_is_not_overwritten(self):
        with closing(sqlite3.connect(self.target)) as db, db:
            db.execute("INSERT INTO characters VALUES ('F16',2,1,'Unrelated hero',99,0,0,'user.agf',1,'user-sprite.agf')")
        before = self.snapshot()
        with self.assertRaisesRegex(ValueError, "collides"):
            self.run_import()
        self.assertEqual(self.snapshot(), before)

    def test_missing_material_and_ambiguous_sheet_are_rejected(self):
        with closing(sqlite3.connect(self.target)) as db, db:
            db.execute("DELETE FROM items WHERE code=101")
        with self.assertRaisesRegex(ValueError, "missing material"):
            self.run_import()
        with closing(sqlite3.connect(self.target)) as db, db:
            db.execute("INSERT INTO items VALUES (101)")
        with closing(sqlite3.connect(self.source)) as db, db:
            db.execute(f'CREATE TABLE duplicate_stats AS SELECT * FROM {self.tables["stats"]}')
            db.execute("INSERT INTO raw_sheet VALUES ('gersang.gcs',9999,'duplicate','duplicate_stats')")
        with self.assertRaisesRegex(ValueError, "Expected one stats source sheet"):
            self.run_import()

    def test_source_and_target_must_differ(self):
        with self.assertRaisesRegex(ValueError, "different files"):
            importer.import_character_data(self.source, self.source)


if __name__ == "__main__":
    unittest.main()
