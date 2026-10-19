#!/usr/bin/env python3
"""Import character base stats, spirits, and mounts from a gs-parser full DB.

Only the matching character and companion records are updated, in one transaction.
The source is opened read-only. No website data or derived player-build stats are
used. HP/MP are the character sheet's Life/Mana values, already in health points;
attack speed and ground/air type are unknown and remain NULL. Mount bonuses are
facts, never invented character base stats. Raw-sheet members are resolved by
column signatures so a changed archive ordering does not silently join wrong data.
"""

import argparse
import hashlib
import json
import re
import sqlite3
from collections import Counter
from contextlib import closing
from pathlib import Path

DEFAULT_TARGET = Path(__file__).resolve().parents[1] / "data" / "gersang.db"
DEFAULT_CLIENT_ROOT = DEFAULT_TARGET.parent / "client"
MIN_ATTACK = "MinApower = TotalEquipMinDamage + IP.GetLevel()*6/10 + TotalStr*4/10"
MAX_ATTACK = "MaxApower = TotalEquipMaxDamage + IP.GetLevel()*6/10 + TotalStr*4/10"
ATTRIBUTE = "Attribute(1 : 바람, 2 : 불, 3 : 물)"
GRADE = "grade(초급 : 1 중급 : 2 상급 : 3 최상급 : 4)"
INTIMACY = "Intimacy( 진화 시 필요한 친밀도 )"
STATS = {
    "strength": "Str", "dexterity": "Dex", "vitality": "Vit", "intelligence": "Int",
    "hp": "Life", "mp": "Mana", "hit_resist": "DamageResist", "magic_resist": "MagicResist",
    "atk_min": MIN_ATTACK, "atk_max": MAX_ATTACK, "move_speed": "MoveSpeed",
    "attack_speed": None, "armor": "AC", "unit_type": None,
    "element": "Type", "element_value": "ElementDamage",
}
ELEMENTS = {"0": 0, "F": 1, "W": 2, "w": 2, "A": 3, "T": 4, "E": 5}
# PetInfo's family attribute is a different enum from the combat Type. Early
# water spirits, for example, have combat Type=0 but Attribute=3 throughout.
FAMILY_ELEMENTS = {1: 3, 2: 1, 3: 2, 4: 5, 5: 4, 6: 0}
COMPANION_COLUMNS = [
    "code", "kind", "riding_level", "field_move_speed", "movement_bonus",
    "movement_scope", "family_element", "spirit_attribute", "spirit_grade",
    "spirit_group", "evolution_root", "evolution_order",
]
SHEETS = {
    "stats": {"ID", "Name", "Name Code", "Nation", "Desc Text", *[v for v in STATS.values() if v]},
    "spirits": {"CharacterID", "CharacterName", ATTRIBUTE, GRADE, "Group", "SkillGTS", "Upper CharacterID", INTIMACY,
                *[f"{p}{i}" for p in ("Material", "Quentity") for i in range(1, 6)]},
    "mounts": {"말 번호", "이름", "이름 GTS", "필요 승마술 GTS", "필드 이동속도", "탈것 정보 GTS", "습득 정보 GTS",
               *[f"옵션 {i}" for i in range(1, 9)], *[f"옵션 수치{i}" for i in range(1, 9)]},
    "skins": {"스킨 번호", "초상화 번호", "요구 승마술 레벨", "말 이름 GTS", "말 설명 GTS", "이동속도",
              "특성 레벨1", "특성 범위1", *[f"특성 {p}{i}" for p in ("종류", "레벨", "범위") for i in range(2, 7)]},
    "traits": {"특성 번호", "주인공 옵션 GTS", "아군 옵션 GTS"},
    "bonuses": {"번호", "힘 증가", "민첩 증가", "생명력 증가", "지력 증가", "이동속도 증가", "타격저항 증가", "마법저항 증가", "속성공격력 증가", "스킬 사거리 증가", "공격력 증가"},
    "purchases": {"판매 말 번호", "말종류 (0: 개량말, 1: 기본말)", "필요 승마술 레벨", "재료 말", "재료 코드1", "재료2", "재료3", "재료4", "재료 코드5", "수수료", "구매조건", *[f"수량{i}" for i in range(1, 6)], *[f"조건{i}" for i in range(1, 6)]},
    "riding": {"승마술 레벨", "승마술 정보 GTS", "수수료", *[f"승마술 아이템 코드{i}" for i in range(1, 6)], *[f"승마술 아이템 개수{i}" for i in range(1, 6)]},
    "vehicles": {"Name", "ID", "Vehicle", "layer1", "layer2"},
}


def quoted(identifier):
    return '"' + identifier.replace('"', '""') + '"'


def integer(value, label, *, missing=None):
    text = str(value).strip() if value is not None else ""
    if not text:
        return missing
    if not re.fullmatch(r"-?[0-9]+", text):
        raise ValueError(f"Invalid integer for {label}: {value!r}")
    return int(text)


def connect(path, readonly=False):
    conn = sqlite3.connect(Path(path).resolve().as_uri() + ("?mode=ro" if readonly else "?mode=rw"), uri=True)
    conn.row_factory = sqlite3.Row
    return conn


def resolve_sheets(source):
    available = []
    for row in source.execute("SELECT member, title, table_name FROM raw_sheet WHERE archive='gersang.gcs'"):
        columns = {r[1] for r in source.execute(f"PRAGMA table_info({quoted(row['table_name'])})")}
        available.append((dict(row), columns))
    resolved = {}
    for key, needed in SHEETS.items():
        matches = [row for row, columns in available if needed <= columns]
        if len(matches) != 1:
            raise ValueError(f"Expected one {key} source sheet, found {len(matches)}")
        resolved[key] = matches[0]
    return resolved


def unique_rows(source, table, key):
    rows = {}
    for row in source.execute(f"SELECT * FROM {quoted(table)}"):
        code = str(row[key]).strip()
        if not code or code in rows:
            raise ValueError(f"Missing or duplicate {key} in {table}: {code!r}")
        rows[code] = dict(row)
    if not rows:
        raise ValueError(f"Empty source sheet: {table}")
    return rows


def canonical(code):
    match = re.fullmatch(r"([A-Z]+)([0-9]+)", code)
    return f"{match[1]}{int(match[2])}" if match else code


def portrait_calls(text):
    """Same quote/nesting-aware call parser used by gs-parser's portraits.py."""
    marker = "SetCharacterBasisData("
    start = 0
    while (at := text.find(marker, start)) >= 0:
        index, depth, quote, chars = at + len(marker), 1, False, []
        while index < len(text) and depth:
            char = text[index]
            if char == '"':
                quote = not quote
            elif not quote:
                if char == "(":
                    depth += 1
                elif char == ")":
                    depth -= 1
                    if not depth:
                        break
            chars.append(char)
            index += 1
        if depth:
            raise ValueError("Unterminated SetCharacterBasisData call")
        args, buf, quote = [], [], False
        for char in "".join(chars):
            if char == '"':
                quote = not quote
            if char == "," and not quote:
                args.append("".join(buf).strip())
                buf = []
            else:
                buf.append(char)
        args.append("".join(buf).strip())
        yield args
        start = index + 1


def portrait_index(source):
    result = {}
    for row in source.execute("SELECT body FROM raw_doc WHERE archive='gersang.gcs' AND body LIKE '%SetCharacterBasisData(%'"):
        for args in portrait_calls(row[0]):
            if len(args) < 4 or not args[3].isdigit():
                continue
            face = int(args[3])
            if not 0 <= face < 1500:
                continue
            match = next((m for token in reversed(args) if (m := re.fullmatch(r'"([A-Z]+[0-9]+)"', token))), None)
            if not match:
                continue
            sheet, frame = divmod(face, 300)
            suffix = f"_{sheet + 1}" if sheet else ""
            result.setdefault(canonical(match[1]), (f"Assets/Textures/UI/Portraits/PORTRAIT_INVEN{suffix}.AGF", frame))
    return result


def mount_portrait_path(source):
    """Use the dedicated horse atlas only when present in a source manifest."""
    for (table,) in source.execute("SELECT table_name FROM raw_sheet WHERE archive='gersang.gcs'"):
        columns = {r[1] for r in source.execute(f"PRAGMA table_info({quoted(table)})")}
        if {"crc32", "name", "path"} <= columns:
            row = source.execute(
                f"SELECT path FROM {quoted(table)} WHERE lower(path)=? LIMIT 1",
                ("assets/textures/ui/portraits/horseportrait.agf",),
            ).fetchone()
            if row:
                return row[0]
    return ""


def mount_sprite_paths(vehicles, client_root):
    """Join source vehicle tokens to actual bundled base mount sheets.

    The source's vehicle table maps mount ID to HOxx; it must not be inferred
    from the mount ID or translated name. VE<Vehicle>N11HSU files are complete
    base horse artwork. VH/VT sheets are optional glow layers and are not a
    substitute for the base image. N11 is selected only as a representative
    animation; the source does not provide its native action timing here.
    """
    directory = Path(client_root)
    for component in ("Assets", "Textures", "HeroCharacter"):
        if not directory.is_dir():
            return {}
        matches = [p for p in directory.iterdir() if p.name.casefold() == component.casefold() and p.is_dir()]
        if len(matches) > 1:
            raise ValueError(f"Ambiguous client directory: {component}")
        if not matches:
            return {}
        directory = matches[0]
    files = {}
    for path in directory.iterdir():
        if path.is_file() and path.suffix.casefold() == ".agf":
            files.setdefault(path.name.casefold(), []).append(path)
    result = {}
    for mount_id, row in vehicles.items():
        token = row["Vehicle"].strip()
        if not re.fullmatch(r"[A-Za-z]+[0-9]+", token):
            raise ValueError(f"Invalid source vehicle asset token: {mount_id}/{token!r}")
        matches = files.get(f"ve{token}n11hsu.agf".casefold(), [])
        if len(matches) > 1:
            raise ValueError(f"Ambiguous mount base sheet: {mount_id}/{token}")
        if matches:
            result[mount_id] = matches[0].relative_to(client_root).as_posix()
    return result


def clean_text(text):
    return re.sub(r"\s+", " ", text.replace("\\n", " ").replace("\\r", " ")).strip()


def evolution_positions(spirits):
    """Root and one-based order of each observed, linear evolution chain.

    Order is not a grade: a chain may begin at grade 2. Validate the graph before
    writing so malformed/cross-family links cannot silently misgroup the list.
    """
    previous = {}
    for code, row in spirits.items():
        next_code = row["Upper CharacterID"].strip()
        if next_code in ("", "0"):
            continue
        if next_code not in spirits:
            raise ValueError(f"Unknown evolution target: {code} -> {next_code}")
        if next_code in previous:
            raise ValueError(f"Ambiguous evolution parents for {next_code}")
        other = spirits[next_code]
        if (row[ATTRIBUTE], row["Group"]) != (other[ATTRIBUTE], other["Group"]):
            raise ValueError(f"Evolution crosses spirit family: {code} -> {next_code}")
        previous[next_code] = code
    result = {}
    for code in spirits:
        seen, root, order = set(), code, 1
        while root in previous:
            if root in seen:
                raise ValueError(f"Cycle in spirit evolution chain: {code}")
            seen.add(root)
            root = previous[root]
            order += 1
        result[code] = (root, order)
    return result


def upsert(target, table, columns, keys, rows):
    rows = list(rows)
    if not rows:
        return
    update = [c for c in columns if c not in keys]
    assignments = ", ".join(f"{quoted(c)}=excluded.{quoted(c)}" for c in update)
    changed = " OR ".join(f"{quoted(c)} IS NOT excluded.{quoted(c)}" for c in update)
    target.executemany(
        f"INSERT INTO {quoted(table)} ({','.join(map(quoted, columns))}) VALUES ({','.join('?' for _ in columns)}) "
        f"ON CONFLICT ({','.join(map(quoted, keys))}) DO UPDATE SET {assignments} WHERE {changed}", rows,
    )


def create_tables(target):
    # Do not use executescript: it implicitly commits an active transaction.
    target.execute("CREATE TABLE IF NOT EXISTS character_stats (code TEXT PRIMARY KEY, " + ", ".join(f"{name} INTEGER" for name in STATS) + ")")
    for ddl in [
        "CREATE TABLE IF NOT EXISTS character_companions (code TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('mount','spirit')), riding_level INTEGER, field_move_speed INTEGER, movement_bonus INTEGER, movement_scope INTEGER CHECK(movement_scope IN (0,1)), family_element INTEGER CHECK(family_element BETWEEN 0 AND 5), spirit_attribute INTEGER, spirit_grade INTEGER, spirit_group INTEGER, evolution_root TEXT, evolution_order INTEGER)",
        "CREATE TABLE IF NOT EXISTS character_facts (code TEXT NOT NULL, slot INTEGER NOT NULL, label_en TEXT NOT NULL, label_zh TEXT NOT NULL, value_en TEXT NOT NULL, value_zh TEXT NOT NULL, PRIMARY KEY(code,slot))",
        "CREATE TABLE IF NOT EXISTS character_materials (code TEXT NOT NULL, kind TEXT NOT NULL, slot INTEGER NOT NULL, item_code INTEGER NOT NULL, qty INTEGER NOT NULL CHECK(qty>0), PRIMARY KEY(code,kind,slot))",
        "CREATE TABLE IF NOT EXISTS character_upgrades (code TEXT PRIMARY KEY, target_code TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS character_requirements (code TEXT NOT NULL, kind TEXT NOT NULL, target_code TEXT NOT NULL, PRIMARY KEY(code,kind,target_code))",
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    ]:
        target.execute(ddl)
    expected = {"code": "TEXT", **{k: "INTEGER" for k in STATS}}
    found = {r[1]: r[2].upper() for r in target.execute("PRAGMA table_info(character_stats)")}
    if any(found.get(k) != v for k, v in expected.items()):
        raise ValueError("Incompatible existing character_stats schema")
    expected = {column: ("TEXT" if column in ("code", "kind", "evolution_root") else "INTEGER") for column in COMPANION_COLUMNS}
    found = {r[1]: r[2].upper() for r in target.execute("PRAGMA table_info(character_companions)")}
    if any(found.get(k) != v for k, v in expected.items()):
        raise ValueError("Incompatible existing character_companions schema")


def import_character_data(source_path, target_path=DEFAULT_TARGET, *, client_root=DEFAULT_CLIENT_ROOT):
    source_path, target_path = Path(source_path), Path(target_path)
    if source_path.resolve() == target_path.resolve():
        raise ValueError("Source and target databases must be different files")
    with closing(connect(source_path, True)) as source, closing(connect(target_path)) as target:
        sheets = resolve_sheets(source)
        data = {key: unique_rows(source, sheets[key]["table_name"], field) for key, field in {
            "stats": "ID", "spirits": "CharacterID", "mounts": "말 번호", "skins": "스킨 번호",
            "traits": "특성 번호", "bonuses": "번호", "purchases": "판매 말 번호", "riding": "승마술 레벨",
            "vehicles": "ID",
        }.items()}
        strings = {(int(r["id"]), r["lang"]): r["text"] for r in source.execute("SELECT id,lang,text FROM strings WHERE object='OnlineText.dat' AND lang IN ('en','zh')") if str(r["id"]).isdigit()}
        used_strings = set()

        def text(gts, lang):
            gts = integer(gts, "GTS", missing=0)
            if not gts:
                return ""
            used_strings.add(gts)
            return clean_text(strings.get((gts, lang), ""))

        portraits = portrait_index(source)
        horse_portrait = mount_portrait_path(source)
        mount_sprites = mount_sprite_paths(data["vehicles"], Path(client_root))
        sprites = {canonical(r[0]): r[1] for r in source.execute("SELECT code,path FROM monster_sprites")}
        existing = {r["code"]: dict(r) for r in target.execute("SELECT * FROM characters")}
        if not existing:
            raise ValueError("Target characters is empty")
        new_chars, facts, materials, upgrades, requirements, companions = [], [], [], [], [], []
        facts_per_code = Counter()
        item_codes = {r[0] for r in target.execute("SELECT code FROM items")}

        def fact(code, label_en, label_zh, value_en, value_zh=None, *, bonus=False):
            if value_zh is None:
                value_zh = value_en
            if not str(value_en).strip() and not str(value_zh).strip():
                return
            key = (code, bonus)
            facts_per_code[key] += 1
            slot = facts_per_code[key] + (0 if bonus else 100)
            if bonus and slot >= 100:
                raise ValueError("Too many mount bonus facts")
            facts.append((code, slot, label_en, label_zh, str(value_en), str(value_zh)))

        def material(code, kind, slot, item, qty):
            item, qty = integer(item, "material", missing=0), integer(qty, "quantity", missing=0)
            if not item and not qty:
                return
            if not item or qty <= 0 or item not in item_codes:
                raise ValueError(f"Invalid or missing material item for {code}: {item} x {qty}")
            materials.append((code, kind, slot, item, qty))

        def character(row):
            code, rank = row[0], row[2]
            if code in existing and existing[code]["rank"] != rank:
                raise ValueError(f"New companion collides with existing character: {code}")
            if not row[4] or not text(row[4], "zh") or not text(row[4], "en"):
                raise ValueError(f"Missing bilingual companion name: {code}")
            for gts in row[5:7]:
                text(gts, "en"); text(gts, "zh")
            new_chars.append(row)

        # These grade names follow the source header; grade 5 extends it in the
        # live table. Keep the number, rather than guess its unlisted title.
        grades = {1: ("Beginner", "下級"), 2: ("Intermediate", "中級"), 3: ("Advanced", "上級"), 4: ("Highest", "最高級")}
        attributes = {1: ("Wind", "風"), 2: ("Fire", "火"), 3: ("Water", "水"), 4: ("Earth", "地"), 5: ("Thunder", "雷"), 6: ("Non-elemental", "無屬性")}
        positions = evolution_positions(data["spirits"])
        for code, row in data["spirits"].items():
            if code not in data["stats"]:
                raise ValueError(f"Missing character stats for spirit: {code}")
            stats = data["stats"][code]
            face = portraits.get(canonical(code), ("", -1))
            name_gts = integer(stats["Name Code"], code)
            desc_gts = integer(stats["Desc Text"], code, missing=0)
            character((code, 0, 14, stats["Name"], name_gts, desc_gts, 0, *face, sprites.get(canonical(code), "")))
            grade, attribute = integer(row[GRADE], code), integer(row[ATTRIBUTE], code)
            grade_names = grades.get(grade, (f"Grade {grade}", f"{grade} 階"))
            fact(code, "Grade", "階級", *grade_names)
            if attribute not in attributes:
                raise ValueError(f"Unknown spirit attribute: {attribute}")
            group = integer(row["Group"], code)
            if grade is None or group is None or grade < 1 or group < 1:
                raise ValueError(f"Invalid spirit grade/group: {code}")
            companions.append((code, "spirit", None, None, None, None,
                               FAMILY_ELEMENTS[attribute], attribute, grade, group,
                               *positions[code]))
            fact(code, "Spirit element", "精靈屬性", *attributes[attribute])
            fact(code, "Skill effect", "技能效果", text(row["SkillGTS"], "en"), text(row["SkillGTS"], "zh"))
            next_code = row["Upper CharacterID"].strip()
            if next_code not in ("", "0"):
                if next_code not in data["spirits"]:
                    raise ValueError(f"Unknown evolution target: {code} -> {next_code}")
                upgrades.append((code, next_code))
                fact(code, "Evolution intimacy", "進化所需親密度", f"{integer(row[INTIMACY], code):,}")
                for i in range(1, 6):
                    material(code, "evolution", i, row[f"Material{i}"], row[f"Quentity{i}"])
            # A terminal spirit's intimacy=100000000 is a sentinel, not an
            # attainable evolution requirement; omit it with no next form.

        # 0759 row 3 says "current maximum rating", and 4..10 contain temporary
        # text plus copied recipes. Only the localized, actionable row 2 is a
        # real upgrade; never publish those placeholders as attainable levels.
        riding = data["riding"].get("2")
        if not riding or not text(riding["승마술 정보 GTS"], "en") or not text(riding["승마술 정보 GTS"], "zh"):
            raise ValueError("Missing real level-2 riding progression")
        for mount_id, row in data["mounts"].items():
            code = f"mount:{integer(mount_id, 'mount ID')}"
            skin = data["skins"].get(mount_id)
            if not skin:
                raise ValueError(f"Missing mount skin: {mount_id}")
            if mount_id not in data["vehicles"]:
                raise ValueError(f"Missing vehicle asset mapping: {mount_id}")
            # Dedicated atlas is present in the client asset manifest. Optional
            # local extraction is independent; absence must use UI fallback.
            face = integer(skin["초상화 번호"], code)
            character((code, 0, 13, row["이름"], integer(row["이름 GTS"], code), integer(skin["말 설명 GTS"], code), integer(row["습득 정보 GTS"], code), horse_portrait, face if horse_portrait else -1, mount_sprites.get(mount_id, "")))
            required_level = integer(skin["요구 승마술 레벨"], code)
            field_move_speed = integer(row["필드 이동속도"], code)
            movement_bonus, movement_scope = None, None
            fact(code, "Required riding level", "所需馬術等級", required_level)
            fact(code, "Field movement speed", "地圖移動速度", field_move_speed, bonus=True)
            for i in range(1, 7):
                # The first kind column is named 'movement speed', followed by
                # level1/scope1. It is a trait ID, NOT a second speed value.
                kind = integer(skin["이동속도" if i == 1 else f"특성 종류{i}"], code, missing=0)
                if not kind:
                    continue
                level = integer(skin[f"특성 레벨{i}"], code)
                scope = integer(skin[f"특성 범위{i}"], code)
                trait, values = data["traits"].get(str(kind)), data["bonuses"].get(str(level))
                if not trait or not values or scope not in (0, 1):
                    raise ValueError(f"Invalid mount trait: {code}/{i}")
                if kind == 1:
                    if movement_bonus is not None:
                        raise ValueError(f"Ambiguous movement traits for {code}")
                    movement_bonus = integer(values["이동속도 증가"], code)
                    movement_scope = scope
                # Translate the scope explicitly. Some EN option labels in 0777
                # mistakenly call physical resistance 'Wisdom'. The trait table
                # plus numeric level table gives the unambiguous meaning.
                scope_en, scope_zh = ("Main character", "主角") if scope == 0 else ("Main character and mercenaries", "主角與傭兵")
                labels = {1: [("Movement bonus", "移速加成", "이동속도 증가")], 2: [("Strength bonus", "力量加成", "힘 증가"), ("Agility bonus", "敏捷加成", "민첩 증가"), ("Vitality bonus", "體力加成", "생명력 증가"), ("Intelligence bonus", "智力加成", "지력 증가")], 3: [("Physical resistance bonus", "打擊抵抗加成", "타격저항 증가")], 4: [("Magic resistance bonus", "魔法抵抗加成", "마법저항 증가")], 5: [("Elemental attack bonus", "屬性攻擊加成", "속성공격력 증가")], 7: [("Skill range bonus", "技能射程加成", "스킬 사거리 증가")], 8: [("Attack bonus", "攻擊力加成", "공격력 증가")]}
                if kind in labels:
                    for label_en, label_zh, column in labels[kind]:
                        amount = integer(values[column], code)
                        fact(code, label_en, label_zh, f"+{amount} · {scope_en}", f"+{amount} · {scope_zh}", bonus=True)
                elif kind == 6:
                    fact(code, "Mounted combat", "騎乘戰鬥", text(trait["주인공 옵션 GTS"], "en"), text(trait["주인공 옵션 GTS"], "zh"))
                else:
                    raise ValueError(f"Unmapped mount trait kind: {kind}")
            companions.append((code, "mount", required_level, field_move_speed,
                               movement_bonus, movement_scope, None, None, None,
                               None, None, None))
            if required_level >= 2:
                fact(code, "Riding level 2 training", "馬術 2 級修煉", text(riding["승마술 정보 GTS"], "en"), text(riding["승마술 정보 GTS"], "zh"))
                fact(code, "Riding level 2 fee", "馬術 2 級費用", f"{integer(riding['수수료'], code):,}")
                for i in range(1, 6):
                    material(code, "riding:2", i, riding[f"승마술 아이템 코드{i}"], riding[f"승마술 아이템 개수{i}"])
            purchase = data["purchases"].get(mount_id)
            if purchase:
                purchase_kind = integer(purchase["말종류 (0: 개량말, 1: 기본말)"], code)
                fact(code, "Acquisition", "取得方式", "Purchase" if purchase_kind else "Improve a mount", "購買" if purchase_kind else "坐騎改良")
                fact(code, "Acquisition fee", "購買／改良費用", f"{integer(purchase['수수료'], code):,}")
                base = integer(purchase["재료 말"], code, missing=0)
                if base:
                    if str(base) not in data["mounts"]:
                        raise ValueError(f"Missing required mount: {base}")
                    requirements.append((code, "acquisition", f"mount:{base}"))
                for i, column in enumerate(["재료 코드1", "재료2", "재료3", "재료4", "재료 코드5"], 1):
                    material(code, "acquisition", i, purchase[column], purchase[f"수량{i}"])
            # Import only text referenced by these companions; preserve target
            # translations when present rather than refreshing unrelated data.
            for gts in (row["이름 GTS"], skin["말 설명 GTS"], row["습득 정보 GTS"]):
                text(gts, "en"); text(gts, "zh")

        companion_codes = {row[0] for row in new_chars}
        stat_rows = []
        for code in sorted(set(existing) | companion_codes):
            row = data["stats"].get(code)
            if not row:
                continue
            values = []
            for name, column in STATS.items():
                if column is None:
                    values.append(None)
                elif name == "element":
                    if row[column] not in ELEMENTS:
                        raise ValueError(f"Unknown character element: {code}/{row[column]}")
                    values.append(ELEMENTS[row[column]])
                else:
                    values.append(integer(row[column], f"{code}/{column}"))
            stat_rows.append((code, *values))
        if not stat_rows:
            raise ValueError("No character stats matched the target")
        digest = hashlib.sha256()
        with source_path.open("rb") as source_file:
            for chunk in iter(lambda: source_file.read(1024 * 1024), b""):
                digest.update(chunk)
        result = {
            "source_file": source_path.name, "source_sha256": digest.hexdigest(), "sheets": sheets,
            "counts": {"characters_with_stats": len(stat_rows), "companions": len(companions), "spirits": len(data["spirits"]), "mounts": len(data["mounts"]), "mount_sprite_assets": len(set(mount_sprites) & set(data["mounts"])), "facts": len(facts), "materials": len(materials), "evolutions": len(upgrades), "required_mounts": len(requirements)},
            "notes": {"hp_mp": "Life and Mana, no multiplier", "attack": "source MinApower/MaxApower base values", "unavailable": ["attack_speed", "unit_type", "mount base stats", "mount native action timing"], "riding": "level 2 only; maximum sentinel and temporary entries excluded", "mount_speed": "0777 field speed; 0779 trait kind/level resolved through 0778 and 0780", "family_element": "PetInfo Attribute mapped to app enum: 0 none, 1 fire, 2 water, 3 wind, 4 thunder, 5 earth; independent from combat Type", "evolution_order": "1-based position in observed chain; not the source grade", "mount_sprites": "vehicle table ID -> Vehicle token -> existing VE<Vehicle>N11HSU.agf base sheet; optional VH/VT glow layers are not composited"},
        }
        target.execute("BEGIN IMMEDIATE")
        try:
            create_tables(target)
            upsert(target, "characters", ["code", "nation", "rank", "name_ko", "name_gts", "desc_gts", "hire_gts", "portrait_path", "portrait_frame", "sprite_path"], ["code"], new_chars)
            upsert(target, "character_stats", ["code", *STATS], ["code"], stat_rows)
            upsert(target, "character_companions", COMPANION_COLUMNS, ["code"], companions)
            # Only replace dependent rows for companions owned by this import.
            for table in ("character_facts", "character_materials", "character_upgrades", "character_requirements"):
                target.executemany(f"DELETE FROM {table} WHERE code=?", [(code,) for code in companion_codes])
            for table, rows in [("character_facts", facts), ("character_materials", materials), ("character_upgrades", upgrades), ("character_requirements", requirements)]:
                if rows:
                    target.executemany(f"INSERT INTO {table} VALUES ({','.join('?' for _ in rows[0])})", rows)
            target.executemany("INSERT OR IGNORE INTO strings(object,id,lang,text) VALUES ('OnlineText.dat',?,?,?)", [(f"{gts:08d}", lang, strings[(gts, lang)]) for gts in sorted(used_strings) for lang in ("en", "zh") if (gts, lang) in strings])
            upsert(target, "meta", ["key", "value"], ["key"], [("character_data_source", json.dumps(result, sort_keys=True))])
            if target.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
                raise ValueError("Target integrity check failed")
            target.commit()
        except BaseException:
            target.rollback()
            raise
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--target", type=Path, default=DEFAULT_TARGET)
    parser.add_argument("--client-root", type=Path, default=DEFAULT_CLIENT_ROOT, help="client asset root used to verify optional mount animation files")
    args = parser.parse_args()
    try:
        result = import_character_data(args.source, args.target, client_root=args.client_root)
    except (OSError, sqlite3.Error, ValueError) as error:
        parser.exit(1, f"Import failed: {error}\n")
    print(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
