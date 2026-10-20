#!/usr/bin/env python3
"""Import enchantment scrolls, materials and target gear into the widget catalog.

The modeled catalog stores enchanted item prefixes, but not the scroll that
applies them. Those rows live in the client archive:

- Online/Item/RecipeItem.txt — one scroll, its success rate, level and targets
- Online/Item/TableRecipe.txt — materials for each recipe key
- Online/Item/TargetItemType.txt — gear vocabulary and its description text id
- Online/Item/EnchantItem.txt — explicit scroll, base, result and effect links
- Online/Item/TableAdvBasicParm.txt — additive effects and inheritance flags
- Online/Map/EnchantNPC.txt — fortune fee, enchant fees by grade, success weight
- Online/Map/EnchantNPCBonus.txt — fortune-telling weights for each NPC

Commented backup rows are ignored. Scroll success rates are the whole-number
percents written in the file. The NPC success column is stored as the client's
integer weight, not converted into a percent. Material names in the source are
checked against the catalog so a shifted column cannot be stored under the
wrong item code. The fee column marked unread by the client is not imported.
"""

import argparse
import hashlib
import json
import re
import sqlite3
import sys
from contextlib import closing
from pathlib import Path

import enchant_data

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_TARGET = ROOT / "data" / "gersang.db"
DEFAULT_CLIENT = ROOT / "data" / "gcs"
LEVEL = re.compile(r"Level([0-9]+)\Z")

# These tables belong to this importer. Keep explicit columns for writes and
# validate an existing schema before replacing any rows.
TABLE_COLUMNS = {
    "enchant_recipes": ("item_code", "success_rate", "level", "recipe_key"),
    "enchant_materials": ("recipe_key", "slot", "item_code", "qty"),
    "enchant_target_types": ("token", "label_gts", "item_code"),
    "enchant_recipe_targets": ("item_code", "slot", "token"),
    "enchant_npcs": ("npc_code", "grade", "fortune_fee", "success_rate", "param_code",
                     "fee_1", "fee_2", "fee_3", "fee_4", "fee_5", "fee_6", "fee_7"),
    "enchant_npc_bonus": ("param_code", "share_rate", "per_level", "per_credit",
                          "weight_z", "weight_o", "weight_i", "weight_y", "weight_l",
                          "weight_t", "weight_c", "weight_u", "weight_belt", "weight_ring",
                          "weight_shoes", "weight_armor", "weight_glove", "weight_helmet",
                          "weight_element", "weight_food", "weight_weapon"),
}
TABLE_KEYS = {
    "enchant_recipes": ("item_code",),
    "enchant_materials": ("recipe_key", "slot"),
    "enchant_target_types": ("token",),
    "enchant_recipe_targets": ("item_code", "slot"),
    "enchant_npcs": ("npc_code",),
    "enchant_npc_bonus": ("param_code",),
}
TEXT_COLUMNS = {"recipe_key", "token", "param_code"}


def create_tables(target):
    for table, columns in TABLE_COLUMNS.items():
        keys = TABLE_KEYS[table]
        fields = []
        for column in columns:
            kind = "TEXT" if column in TEXT_COLUMNS else "INTEGER"
            constraint = "PRIMARY KEY" if keys == (column,) else "NOT NULL"
            fields.append(f"{column} {kind} {constraint}")
        if len(keys) > 1:
            fields.append(f"PRIMARY KEY ({', '.join(keys)})")
        # executescript would commit BEGIN IMMEDIATE before executing the DDL.
        target.execute(f"CREATE TABLE IF NOT EXISTS {table} ({', '.join(fields)})")
        found = {row[1]: row for row in target.execute(f"PRAGMA table_info({table})")}
        if set(found) != set(columns):
            raise ValueError(f"Incompatible existing {table} columns")
        for column in columns:
            row = found[column]
            kind = "TEXT" if column in TEXT_COLUMNS else "INTEGER"
            primary_key = keys.index(column) + 1 if column in keys else 0
            requires_not_null = keys != (column,)
            if row[2].upper() != kind or row[5] != primary_key or (requires_not_null and not row[3]):
                raise ValueError(f"Incompatible existing {table} schema at {column}")
    target.execute("CREATE INDEX IF NOT EXISTS idx_enchant_materials_item ON enchant_materials(item_code)")
    target.execute("CREATE INDEX IF NOT EXISTS idx_enchant_materials_key ON enchant_materials(recipe_key)")


def replace_rows(target, table, rows):
    columns = TABLE_COLUMNS.get(table) or enchant_data.TABLE_COLUMNS[table]
    # Avoid rewriting pages on an identical refresh; compare complete owned
    # rows rather than trusting metadata left by an earlier run.
    existing = target.execute(f"SELECT {', '.join(columns)} FROM {table}").fetchall()
    if len(existing) == len(rows) and set(existing) == set(rows):
        return
    target.execute(f"DELETE FROM {table}")
    target.executemany(
        f"INSERT INTO {table} ({', '.join(columns)}) VALUES ({', '.join('?' for _ in columns)})",
        rows,
    )


def create_views(target):
    """Expose explicit equipment links and their effects without multiplying materials."""
    effects = ", ".join(
        f"f.{column} AS effect_{column}"
        for column in enchant_data.EFFECT_SOURCE_COLUMNS if column != "effect_key"
    )
    definitions = {
        "v_enchant_equipment": (
            "SELECT e.*, r.success_rate AS scroll_success_rate, "
            "r.level AS scroll_required_level, r.recipe_key, " + effects +
            " FROM enchant_equipment e "
            "LEFT JOIN enchant_recipes r ON r.item_code = e.scroll_item "
            "JOIN enchant_effects f ON f.effect_key = e.effect_key"
        ),
        "v_enchant_materials": (
            "SELECT r.item_code AS scroll_item, r.recipe_key, m.slot, "
            "m.item_code AS material_item, m.qty "
            "FROM enchant_recipes r JOIN enchant_materials m ON m.recipe_key = r.recipe_key"
        ),
    }
    for name, query in definitions.items():
        sql = f"CREATE VIEW {name} AS {query}"
        previous = target.execute("SELECT sql FROM sqlite_master WHERE type='view' AND name=?", (name,)).fetchone()
        if previous and previous[0] == sql:
            continue
        target.execute(f"DROP VIEW IF EXISTS {name}")
        target.execute(sql)


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def connect(path, readonly=False):
    return sqlite3.connect(path.resolve().as_uri() + ("?mode=ro" if readonly else "?mode=rw"), uri=True)


def cells(line):
    parts = [part.strip().replace("\x00", "") for part in line.split("\t")]
    while parts and not parts[-1]:
        parts.pop()
    return parts


def table_rows(text, signature):
    header_at = None
    for index, line in enumerate(text.splitlines()):
        if line.startswith("#") and signature in line:
            if header_at is not None:
                raise ValueError(f"Repeated header for {signature}")
            header_at = index
    if header_at is None:
        raise ValueError(f"Missing header: {signature}")
    rows = []
    for line in text.splitlines()[header_at + 1 :]:
        stripped = line.strip()
        if not stripped or stripped.startswith(";"):
            continue
        if stripped.startswith("#"):
            raise ValueError(f"Unexpected extra header after {signature}")
        row = cells(line)
        if row:
            rows.append(row)
    if not rows:
        raise ValueError(f"No data rows for {signature}")
    return rows


def integer(value, label, *, minimum=None, maximum=None):
    if not isinstance(value, str) or not value.isascii() or not value.isdecimal():
        raise ValueError(f"Invalid integer for {label}: {value!r}")
    number = int(value)
    if minimum is not None and number < minimum or maximum is not None and number > maximum:
        raise ValueError(f"Out of range for {label}: {number}")
    return number


def letters(value):
    return "".join(ch for ch in value if ch.isalpha())


def same_item(written, catalog):
    """The recipe file keeps an older short name; the catalog may add a note or a digit."""
    left, right = letters(written), letters(catalog)
    if left == right:
        return True
    if len(left) < 2 or len(right) < 2:
        return False
    return left in right or right in left


def find_text(texts, *needles):
    found = [
        text for text in texts
        if any(line.startswith("#") and all(needle in line for needle in needles) for line in text.splitlines())
    ]
    if len(found) != 1:
        raise ValueError(f"Expected one client table containing {needles!r}, found {len(found)}")
    return found[0]


def parser_package_root(parser_root=None):
    candidates = [Path(parser_root)] if parser_root is not None else [
        ROOT, ROOT.parent / "gs-parser", ROOT.parent.parent / "gs-parser",
    ]
    for candidate in candidates:
        if all((candidate / "gersang" / name).is_file() for name in ("archive.py", "extra.py")):
            return candidate.resolve()
    searched = ", ".join(str(path) for path in candidates)
    raise ValueError(
        "Cannot find the gs-parser package (gersang/archive.py and gersang/extra.py). "
        f"Pass --parser-root /path/to/gs-parser. Searched: {searched}"
    )


def client_texts(client, *, parser_root=None):
    path = Path(client) / "gersang.gcs"
    if not path.is_file():
        raise ValueError(f"Client archive does not exist: {path}. Pass --client /path/to/client/static")
    if sys.version_info < (3, 10):
        raise ValueError("Reading client archives with gs-parser requires Python 3.10 or newer")
    package_root = parser_package_root(parser_root)
    sys.path.insert(0, str(package_root))
    try:
        from gersang.archive import read_archive
        from gersang.extra import _decode
    except ImportError as error:
        raise ValueError(f"Cannot load gs-parser from {package_root}: {error}") from error
    finally:
        sys.path.remove(str(package_root))
    texts = []
    for member in read_archive(str(path)):
        text = _decode(member.data)
        if text:
            texts.append(text)
    if not texts:
        raise ValueError(f"No text tables in {path}")
    return texts


def parse_enchant(recipe_text, material_text, target_text, items, strings):
    recipes = []
    targets = []
    seen_items = set()
    recipe_keys = set()
    for row in table_rows(recipe_text, "재료 아이템 테이블 인덱스"):
        if len(row) != 27:
            raise ValueError(f"Enchant scroll row has {len(row)} columns, expected 27: {row[0] if row else ''}")
        code = integer(row[0], "scroll item", minimum=1)
        if code in seen_items:
            raise ValueError(f"Duplicate enchant scroll: {code}")
        seen_items.add(code)
        if code not in items:
            raise ValueError(f"Enchant scroll is not in the catalog: {code}")
        if not same_item(row[1], items[code]):
            raise ValueError(f"Enchant scroll name does not match item {code}: {row[1]!r} != {items[code]!r}")
        rate = integer(row[23], f"{code} success", minimum=0, maximum=100)
        level_match = LEVEL.fullmatch(row[24])
        if not level_match:
            raise ValueError(f"Unparsed enchant level for {code}: {row[24]!r}")
        key = row[25]
        if not key or key == "0":
            raise ValueError(f"Missing material table for scroll {code}")
        tokens = [token.strip() for token in row[26].split(",") if token.strip()]
        if not tokens or any("," in token or " " in token for token in tokens):
            raise ValueError(f"Invalid enchant targets for {code}: {row[26]!r}")
        recipes.append((code, rate, int(level_match.group(1)), key))
        recipe_keys.add(key)
        for slot, token in enumerate(tokens, start=1):
            targets.append((code, slot, token))

    materials = []
    seen_keys = set()
    for row in table_rows(material_text, "재료1 코드"):
        if len(row) != 31:
            raise ValueError(f"Material table {row[0] if row else ''} has {len(row)} columns, expected 31")
        key = row[0]
        if not key or key in seen_keys:
            raise ValueError(f"Invalid or duplicate material table: {key!r}")
        seen_keys.add(key)
        filled = 0
        for slot in range(1, 11):
            base = 1 + (slot - 1) * 3
            name, code_text, qty_text = row[base], row[base + 1], row[base + 2]
            if code_text in ("", "0") and qty_text in ("", "0"):
                continue
            code = integer(code_text, f"{key} material {slot}", minimum=1)
            qty = integer(qty_text, f"{key} quantity {slot}", minimum=1)
            if code not in items:
                raise ValueError(f"Material {code} for {key} is not in the catalog")
            if not same_item(name, items[code]):
                raise ValueError(f"Material name does not match item {code}: {name!r} != {items[code]!r}")
            materials.append((key, slot, code, qty))
            filled += 1
        if filled == 0:
            raise ValueError(f"Material table {key} has no materials")
    missing_keys = sorted(recipe_keys - seen_keys)
    if missing_keys:
        raise ValueError(f"Scrolls reference missing material tables: {', '.join(missing_keys)}")

    types = []
    seen_tokens = set()
    for row in table_rows(target_text, "아이템 종류 어휘"):
        if len(row) not in (3, 4):
            raise ValueError(f"Target type has {len(row)} columns: {row[:2]!r}")
        if len(row) == 4 and not row[3].startswith(";"):
            raise ValueError(f"Unexpected target columns: {row!r}")
        token = row[0]
        label = integer(row[2], f"{token} text", minimum=1)
        if not token or token in seen_tokens:
            raise ValueError(f"Invalid or duplicate target token: {token!r}")
        seen_tokens.add(token)
        # Most rows point at a description string. A few point at the gear item
        # itself when that string was never given its own text id.
        if label in strings:
            types.append((token, label, 0))
        elif label in items and same_item(row[1], items[label]):
            types.append((token, 0, label))
        else:
            raise ValueError(f"Target {token} points at unknown text or item {label}")
    unknown = sorted({token for _, _, token in targets} - seen_tokens)
    if unknown:
        raise ValueError(f"Scrolls use unknown target tokens: {', '.join(unknown[:12])}")
    return recipes, materials, types, targets


def parse_npcs(npc_text, bonus_text, names):
    npcs = []
    seen = set()
    for row in table_rows(npc_text, "주술연마 비용(1등급)"):
        if len(row) != 24:
            raise ValueError(f"Enchant NPC row has {len(row)} columns, expected 24")
        code = integer(row[0], "enchant NPC", minimum=1)
        if code in seen:
            raise ValueError(f"Duplicate enchant NPC: {code}")
        seen.add(code)
        if code not in names:
            raise ValueError(f"Enchant NPC is not in the catalog: {code}")
        if not same_item(row[1], names[code]):
            raise ValueError(f"Enchant NPC name does not match {code}: {row[1]!r} != {names[code]!r}")
        grade = integer(row[2], f"{code} grade", minimum=1)
        fortune_fee = integer(row[3], f"{code} fortune fee", minimum=0)
        integer(row[4], f"{code} unread fee", minimum=0)
        success_rate = integer(row[5], f"{code} success weight", minimum=0)
        param = row[6]
        if not param:
            raise ValueError(f"Missing fortune parameter for NPC {code}")
        integer(row[7], f"{code} portrait", minimum=0)
        for index in range(8, 17):
            integer(row[index], f"{code} line {index}", minimum=1)
        fees = [integer(row[17 + grade_index], f"{code} fee {grade_index + 1}", minimum=0) for grade_index in range(7)]
        npcs.append((code, grade, fortune_fee, success_rate, param, *fees))
    bonuses = []
    seen_params = set()
    for row in table_rows(bonus_text, "TarotPrb. Share Rate"):
        if len(row) != 21:
            raise ValueError(f"Enchant NPC bonus {row[0] if row else ''} has {len(row)} columns, expected 21")
        param = row[0]
        if not param or param in seen_params:
            raise ValueError(f"Invalid or duplicate fortune parameter: {param!r}")
        seen_params.add(param)
        weights = [integer(row[index], f"{param} weight {index}", minimum=0) for index in range(1, 21)]
        bonuses.append((param, *weights))
    missing = sorted({row[4] for row in npcs} - seen_params)
    if missing:
        raise ValueError(f"Enchant NPCs reference missing fortune parameters: {', '.join(missing)}")
    return npcs, bonuses


def import_enchant(client, target_path=DEFAULT_TARGET, *, parser_root=None):
    client, target_path = Path(client), Path(target_path)
    if not target_path.is_file():
        raise ValueError(f"Target catalog does not exist: {target_path}")
    texts = client_texts(client, parser_root=parser_root)
    recipe_text = find_text(texts, "재료 아이템 테이블 인덱스", "연마되는 아이템 종류")
    material_text = find_text(texts, "재료1 코드", "재료10 수량")
    target_text = find_text(texts, "아이템 종류 어휘", "설명 텍스트 인덱스")
    equipment_text = find_text(texts, "비법 아이템 코드", "기본 성능 향상 테이블")
    effect_text = find_text(texts, "테이블 인덱스", "주술 연마 성공확률 주인공")
    npc_text = find_text(texts, "점괘 비용", "주술연마 비용(1등급)")
    bonus_text = find_text(texts, "Parm Code", "TarotPrb. Share Rate")
    with closing(connect(target_path, readonly=True)) as source:
        items = dict(source.execute("SELECT code, name_ko FROM items"))
        names = dict(source.execute("SELECT code, name_ko FROM npcs"))
        strings = {int(row[0]) for row in source.execute(
            "SELECT id FROM strings WHERE object = 'OnlineText.dat' AND lang IN ('en', 'zh')"
        )}
    recipes, materials, types, targets = parse_enchant(recipe_text, material_text, target_text, items, strings)
    npcs, bonuses = parse_npcs(npc_text, bonus_text, names)
    equipment = enchant_data.parse_enchant_data(equipment_text, effect_text, items, {row[0] for row in recipes})
    digest = hashlib.sha256()
    source_tables = {
        "RecipeItem": recipe_text, "TableRecipe": material_text, "TargetItemType": target_text,
        "EnchantNPC": npc_text, "EnchantNPCBonus": bonus_text,
        "EnchantItem": equipment_text, "TableAdvBasicParm": effect_text,
    }
    for text in source_tables.values():
        digest.update(text.encode("utf-8"))
    result = {
        "schema_version": 2,
        "source": "gersang.gcs",
        "archive_sha256": file_sha256(client / "gersang.gcs"),
        "sha256": digest.hexdigest(),
        "tables": {name: {"decoded_text_sha256": hashlib.sha256(text.encode("utf-8")).hexdigest()}
                   for name, text in source_tables.items()},
        "scroll_success_unit": "percent",
        "npc_success_unit": "client integer weight",
        "notes": {
            "links": "EnchantItem explicit base, scroll, result and effect codes; no name matching",
            "zero_scroll": "Source has no scroll code; preserved without an inferred link",
            "missing_items": "All source equipment retained; availability flags refer to this catalog",
            "effects": "Source additive values; requirement fields and inheritance flags kept separately",
            "decimals": "Original numeric text; enchant modifiers are not converted to percentages",
            "revert_scroll": "Restoration scroll item, not a restoration result",
            "jackpot": "Separate source result; no jackpot probability inferred",
        },
        "counts": {
            "scrolls": len(recipes),
            "materials": len(materials),
            "material_tables": len({key for key, _, _, _ in materials}),
            "target_types": len(types),
            "scroll_targets": len(targets),
            "npcs": len(npcs),
            "npc_bonuses": len(bonuses),
            **equipment["counts"],
        },
    }
    with closing(connect(target_path)) as target:
        target.execute("BEGIN IMMEDIATE")
        try:
            create_tables(target)
            enchant_data.create_tables(target)
            for table, rows in (
                ("enchant_recipe_targets", targets), ("enchant_materials", materials),
                ("enchant_recipes", recipes), ("enchant_target_types", types),
                ("enchant_npcs", npcs), ("enchant_npc_bonus", bonuses),
                ("enchant_effects", equipment["enchant_effects"]),
                ("enchant_equipment", equipment["enchant_equipment"]),
            ):
                replace_rows(target, table, rows)
            create_views(target)
            target.execute(
                "INSERT INTO meta(key, value) VALUES ('enchant_source', ?) "
                "ON CONFLICT(key) DO UPDATE SET value = excluded.value WHERE value IS NOT excluded.value",
                (json.dumps(result, sort_keys=True),),
            )
            if target.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
                raise ValueError("Target integrity check failed")
            target.commit()
        except BaseException:
            target.rollback()
            raise
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", type=Path, default=DEFAULT_CLIENT)
    parser.add_argument("--target", type=Path, default=DEFAULT_TARGET)
    parser.add_argument("--parser-root", type=Path,
                        help="gs-parser checkout containing gersang/; defaults to a sibling gs-parser checkout")
    args = parser.parse_args()
    try:
        result = import_enchant(args.client, args.target, parser_root=args.parser_root)
    except (OSError, sqlite3.Error, ValueError) as error:
        parser.exit(1, f"Import failed: {error}\n")
    counts = result["counts"]
    print(
        f"Enchant scrolls {counts['scrolls']}, materials {counts['materials']}, "
        f"tables {counts['material_tables']}, targets {counts['scroll_targets']}, "
        f"NPCs {counts['npcs']}, equipment {counts['equipment']}, effects {counts['effect_profiles']}; "
        f"linked equipment {counts['linked_equipment']}, source zero-scroll {counts['zero_scroll_equipment']}, "
        f"missing scroll references {counts['missing_scroll_equipment']}"
    )


if __name__ == "__main__":
    main()
