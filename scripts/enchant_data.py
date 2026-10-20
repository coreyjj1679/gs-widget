"""Exact client enchant item/effect relationships, without inferred recipes.

EnchantItem explicitly connects a result item to its base item, scroll and
TableAdvBasicParm key. A zero scroll remains zero: matching its name or prefix
to another scroll would invent a relationship. Missing catalog items and
missing nonzero scroll references remain visible through availability fields.

All source columns, including inheritance sentinels, are retained. Decimal
experience/enchant modifiers remain their original numeric text; this module
does not convert them into percentages or combine them with NPC weights.
"""

import re
from collections import Counter


EQUIPMENT_SOURCE_COLUMNS = (
    "result_item", "result_name_ko", "item_grade", "riddle", "reinforcement", "resonance",
    "name_gts", "base_item", "base_name_ko", "scroll_item", "scroll_name_ko",
    "revert_scroll_item", "revert_name_ko", "jackpot_item", "jackpot_name_ko",
    "prefix_gts", "prefix_name_ko", "suffix_gts", "suffix_name_ko", "enchant_grade",
    "item_group", "character_image_index", "unit_name_ko", "unit_gts", "description_ko",
    "description_gts", "price", "weight", "inventory_sprite", "inventory_frame",
    "field_sprite", "field_frame", "wearable", "usable", "weapon_type", "sound", "effect_key",
)
EFFECT_SOURCE_COLUMNS = (
    "effect_key", "strength", "dexterity", "intelligence", "vitality", "hp", "mp",
    "hit_resist", "magic_resist", "armor", "damage_min", "damage_max", "attack_speed",
    "attack_range", "magic_table_key", "magic_name_ko", "magic_damage_min", "magic_damage_max",
    "mercenary_defense", "mercenary_attack", "bonus_experience_raw", "move_speed",
    "main_enchant_success_raw", "mercenary_enchant_success_raw", "element", "attack_damage",
    "damage_reduction", "critical_chance", "critical_damage", "required_strength",
    "required_dexterity", "required_intelligence", "required_hp", "required_level",
    "required_credit", "timed_item", "days", "mall_item",
)
EQUIPMENT_HEADER_PREFIXES = tuple((
    "아이템 코드|아이템 이름|아이템등급|수수께끼|강화 가능 여부|공명 가능 여부|아이템 이름 코드|"
    "기저 아이템 코드|기저 아이템 이름|비법 아이템 코드|비법 아이템 이름|원복 아이템 코드|"
    "원복 아이템 이름|대박 아이템 코드|대박 아이템 이름|아이템 접두사 코드|아이템 접두사|"
    "아이템 접미사 코드|아이템 접미사|주술연마 아이템 등급|그룹|조합 캐릭터 이미지 인덱스|"
    "유닛 단위|유닛 단위 코드|아이템 설명|아이템 설명 코드|가격|무게|인벤토리 스프라이트 이름|"
    "인벤토리 스프라이트 폰트 번호|필드 스프라이트 이름|필드 스프라이트 폰트 번호|"
    "착용가능한가?|사용 가능한가?|무기 타입|사운드|기본 성능 향상 테이블"
).split("|"))
EFFECT_HEADER_PREFIXES = tuple((
    "테이블 인덱스|힘|민첩|지력|생명|체력|마법력|물리저항력|마법저항력|방어력|최소 데미지|"
    "최대 데미지|연사속도|사정거리|마법 테이블 인덱스|마법명|마법 최소 데미지|마법 최대 데미지|"
    "용병 방어력 증가|용병 공격력 증가|보너스 경험치 퍼센트|이동속도|주술 연마 성공확률 주인공|"
    "주술 연마 성공률 용병|속성값 증가|공격 데미지 증가|피해 데미지 감소|치명타 확률 증가|"
    "치명타 피해 증가|필요 힘|필요 민첩|필요 지력|필요 체력|필요 레벨|필요 신용등급|"
    "시간제 아이템인가?|몇 일용인가?|아이템몰 아이템인가?"
).split("|"))
AVAILABILITY_COLUMNS = (
    "result_available", "base_available", "scroll_available", "revert_available", "jackpot_available",
)
TABLE_COLUMNS = {
    "enchant_equipment": EQUIPMENT_SOURCE_COLUMNS + AVAILABILITY_COLUMNS +
                         ("scroll_status", "source_line", "raw_source_row"),
    "enchant_effects": EFFECT_SOURCE_COLUMNS + ("source_line", "raw_source_row"),
}
TABLE_KEYS = {"enchant_equipment": ("result_item",), "enchant_effects": ("effect_key",)}
_EQUIPMENT_TEXT = {
    "result_name_ko", "base_name_ko", "scroll_name_ko", "revert_name_ko", "jackpot_name_ko",
    "prefix_name_ko", "suffix_name_ko", "item_group", "unit_name_ko", "description_ko",
    "inventory_sprite", "field_sprite", "weapon_type", "sound", "effect_key", "scroll_status",
    "raw_source_row",
}
DECIMAL_FIELDS = {"bonus_experience_raw", "main_enchant_success_raw", "mercenary_enchant_success_raw"}
_EFFECT_TEXT = {"effect_key", "magic_table_key", "magic_name_ko", "raw_source_row"} | DECIMAL_FIELDS
COLUMN_TYPES = {
    table: {column: "TEXT" if column in text_columns else "INTEGER" for column in columns}
    for table, columns, text_columns in (
        ("enchant_equipment", TABLE_COLUMNS["enchant_equipment"], _EQUIPMENT_TEXT),
        ("enchant_effects", TABLE_COLUMNS["enchant_effects"], _EFFECT_TEXT),
    )
}
INTEGER_COLUMNS = {
    table: tuple(column for column, kind in kinds.items() if kind == "INTEGER")
    for table, kinds in COLUMN_TYPES.items()
}
_SIGNED_INTEGER = re.compile(r"-?[0-9]+\Z")
_DECIMAL_LITERAL = re.compile(r"-?[0-9]+(?:\.[0-9]+)?\Z")


def _integer(value, label, *, minimum=None):
    if not _SIGNED_INTEGER.fullmatch(value):
        raise ValueError(f"Invalid integer for {label}: {value!r}")
    number = int(value)
    if not -(2 ** 63) <= number < 2 ** 63 or (minimum is not None and number < minimum):
        raise ValueError(f"Out of range for {label}: {value!r}")
    return number


def _rows(text, signatures, header_prefixes):
    """Yield source line, original row and trimmed fields after one header."""
    lines = text.splitlines()
    headers = [index for index, line in enumerate(lines)
               if line.lstrip().startswith("#") and all(value in line for value in signatures)]
    if len(headers) != 1:
        raise ValueError(f"Expected one header containing {signatures!r}, found {len(headers)}")
    header = lines[headers[0]].strip().lstrip("#").split("\t")
    width = len(header_prefixes)
    if len(header) != width or any(field.split("(", 1)[0].strip() != prefix
                                   for field, prefix in zip(header, header_prefixes)):
        raise ValueError(f"Unexpected column order for client table {signatures!r}")
    count = 0
    for index in range(headers[0] + 1, len(lines)):
        line = lines[index]
        if not line.strip() or line.lstrip().startswith(";"):
            continue
        if line.lstrip().startswith("#"):
            raise ValueError(f"Unexpected header at line {index + 1}")
        fields = [field.strip().replace("\x00", "") for field in line.split("\t")]
        while fields and not fields[-1]:
            fields.pop()
        if len(fields) != width:
            raise ValueError(f"Client row at line {index + 1} has {len(fields)} columns, expected {width}")
        count += 1
        yield index + 1, line, fields
    if not count:
        raise ValueError(f"No rows for client table {signatures!r}")


def parse_effects(text):
    """Preserve every TableAdvBasicParm profile, including unused ones."""
    result, seen = [], set()
    for line, raw, fields in _rows(text, ("테이블 인덱스", "주술 연마 성공확률 주인공", "필요 신용등급"), EFFECT_HEADER_PREFIXES):
        key = fields[0]
        if not key or key == "0" or key in seen:
            raise ValueError(f"Invalid or duplicate enchant effect key: {key!r}")
        seen.add(key)
        values = []
        for column, value in zip(EFFECT_SOURCE_COLUMNS, fields):
            if column in DECIMAL_FIELDS and not _DECIMAL_LITERAL.fullmatch(value):
                raise ValueError(f"Invalid numeric literal for {key}/{column}: {value!r}")
            if COLUMN_TYPES["enchant_effects"][column] == "INTEGER":
                value = _integer(value, f"{key}/{column}")
            values.append(value)
        result.append(tuple(values) + (line, raw))
    return result


def parse_equipment(text, catalog_items, scroll_codes, effect_keys):
    """Use only explicit client codes; retain unavailable items and zero refs."""
    item_codes, scroll_codes, effect_keys = set(catalog_items), set(scroll_codes), set(effect_keys)
    result, seen = [], set()
    for line, raw, fields in _rows(text, ("기저 아이템 코드", "비법 아이템 코드", "기본 성능 향상 테이블"), EQUIPMENT_HEADER_PREFIXES):
        values = []
        for column, value in zip(EQUIPMENT_SOURCE_COLUMNS, fields):
            if COLUMN_TYPES["enchant_equipment"][column] == "INTEGER":
                value = _integer(value, f"enchant line {line}/{column}")
                if column in {"result_item", "base_item"} and value <= 0:
                    raise ValueError(f"Invalid {column} at enchant line {line}: {value}")
                if column in {"scroll_item", "revert_scroll_item", "jackpot_item", "prefix_gts", "suffix_gts"} and value < 0:
                    raise ValueError(f"Invalid {column} at enchant line {line}: {value}")
            values.append(value)
        row = dict(zip(EQUIPMENT_SOURCE_COLUMNS, values))
        if row["result_item"] in seen:
            raise ValueError(f"Duplicate enchant result item: {row['result_item']}")
        seen.add(row["result_item"])
        if row["effect_key"] not in effect_keys:
            raise ValueError(f"Enchant result {row['result_item']} references missing effect {row['effect_key']!r}")
        availability = tuple(int(row[column] != 0 and row[column] in item_codes)
                             for column in ("result_item", "base_item", "scroll_item", "revert_scroll_item", "jackpot_item"))
        status = "no_scroll" if row["scroll_item"] == 0 else (
            "linked" if row["scroll_item"] in scroll_codes else "missing_scroll"
        )
        result.append(tuple(values) + availability + (status, line, raw))
    return result


def parse_enchant_data(enchant_text, effect_text, catalog_items, scroll_codes):
    """Return table rows and coverage counts for the enclosing importer."""
    scroll_codes = set(scroll_codes)
    effects = parse_effects(effect_text)
    equipment = parse_equipment(enchant_text, catalog_items, scroll_codes, {row[0] for row in effects})
    rows = [dict(zip(TABLE_COLUMNS["enchant_equipment"], row)) for row in equipment]
    states = Counter(row["scroll_status"] for row in rows)
    linked_scrolls = {row["scroll_item"] for row in rows if row["scroll_status"] == "linked"}
    return {
        "enchant_equipment": equipment,
        "enchant_effects": effects,
        "counts": {
            "equipment": len(equipment), "effect_profiles": len(effects),
            "used_effect_profiles": len({row["effect_key"] for row in rows}),
            "linked_equipment": states["linked"], "zero_scroll_equipment": states["no_scroll"],
            "missing_scroll_equipment": states["missing_scroll"], "linked_scrolls": len(linked_scrolls),
            "scrolls_without_equipment": len(set(scroll_codes) - linked_scrolls),
            "unavailable_results": sum(not row["result_available"] for row in rows),
            "unavailable_bases": sum(not row["base_available"] for row in rows),
        },
    }


def create_tables(conn):
    """Create/validate owned tables without committing the caller's transaction."""
    for table, columns in TABLE_COLUMNS.items():
        key = TABLE_KEYS[table][0]
        definitions = [f"{column} {COLUMN_TYPES[table][column]} NOT NULL" +
                       (" PRIMARY KEY" if column == key else "") for column in columns]
        conn.execute(f"CREATE TABLE IF NOT EXISTS {table} ({', '.join(definitions)})")
        existing = {row[1]: row for row in conn.execute(f"PRAGMA table_info({table})")}
        if set(existing) != set(columns):
            raise ValueError(f"Incompatible existing {table} columns")
        for column in columns:
            row = existing[column]
            if row[2].upper() != COLUMN_TYPES[table][column] or not row[3] or row[5] != int(column == key):
                raise ValueError(f"Incompatible existing {table} schema at {column}")
    for column in ("scroll_item", "base_item", "effect_key"):
        conn.execute(f"CREATE INDEX IF NOT EXISTS idx_enchant_equipment_{column} ON enchant_equipment({column})")
