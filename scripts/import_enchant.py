#!/usr/bin/env python3
"""Import enchantment scrolls, materials and target gear into the widget catalog.

The modeled catalog stores enchanted item prefixes, but not the scroll that
applies them. Those rows live in the client archive:

- Online/Item/RecipeItem.txt — one scroll, its success rate, level and targets
- Online/Item/TableRecipe.txt — materials for each recipe key
- Online/Item/TargetItemType.txt — gear vocabulary and its description text id
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
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_TARGET = ROOT / "data" / "gersang.db"
DEFAULT_CLIENT = ROOT / "data" / "gcs"
LEVEL = re.compile(r"Level([0-9]+)\Z")


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


def client_texts(client):
    if str(ROOT) not in sys.path:
        sys.path.insert(0, str(ROOT))
    from gersang.archive import read_archive
    from gersang.extra import _decode

    path = Path(client) / "gersang.gcs"
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


def import_enchant(client, target_path=DEFAULT_TARGET):
    client, target_path = Path(client), Path(target_path)
    texts = client_texts(client)
    recipe_text = find_text(texts, "재료 아이템 테이블 인덱스", "연마되는 아이템 종류")
    material_text = find_text(texts, "재료1 코드", "재료10 수량")
    target_text = find_text(texts, "아이템 종류 어휘", "설명 텍스트 인덱스")
    npc_text = find_text(texts, "점괘 비용", "주술연마 비용(1등급)")
    bonus_text = find_text(texts, "Parm Code", "TarotPrb. Share Rate")
    with connect(target_path, readonly=True) as source:
        items = dict(source.execute("SELECT code, name_ko FROM items"))
        names = dict(source.execute("SELECT code, name_ko FROM npcs"))
        strings = {int(row[0]) for row in source.execute(
            "SELECT id FROM strings WHERE object = 'OnlineText.dat' AND lang IN ('en', 'zh')"
        )}
    recipes, materials, types, targets = parse_enchant(recipe_text, material_text, target_text, items, strings)
    npcs, bonuses = parse_npcs(npc_text, bonus_text, names)
    digest = hashlib.sha256()
    for text in (recipe_text, material_text, target_text, npc_text, bonus_text):
        digest.update(text.encode("utf-8"))
    result = {
        "source": "gersang.gcs",
        "sha256": digest.hexdigest(),
        "scroll_success_unit": "percent",
        "npc_success_unit": "client integer weight",
        "counts": {
            "scrolls": len(recipes),
            "materials": len(materials),
            "material_tables": len({key for key, _, _, _ in materials}),
            "target_types": len(types),
            "scroll_targets": len(targets),
            "npcs": len(npcs),
            "npc_bonuses": len(bonuses),
        },
    }
    with connect(target_path) as target:
        target.execute("BEGIN IMMEDIATE")
        try:
            target.executescript(
                """
                CREATE TABLE IF NOT EXISTS enchant_recipes (
                    item_code INTEGER PRIMARY KEY,
                    success_rate INTEGER NOT NULL,
                    level INTEGER NOT NULL,
                    recipe_key TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS enchant_materials (
                    recipe_key TEXT NOT NULL,
                    slot INTEGER NOT NULL,
                    item_code INTEGER NOT NULL,
                    qty INTEGER NOT NULL,
                    PRIMARY KEY (recipe_key, slot)
                );
                CREATE TABLE IF NOT EXISTS enchant_target_types (
                    token TEXT PRIMARY KEY,
                    label_gts INTEGER NOT NULL,
                    item_code INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS enchant_recipe_targets (
                    item_code INTEGER NOT NULL,
                    slot INTEGER NOT NULL,
                    token TEXT NOT NULL,
                    PRIMARY KEY (item_code, slot)
                );
                CREATE INDEX IF NOT EXISTS idx_enchant_materials_item ON enchant_materials(item_code);
                CREATE INDEX IF NOT EXISTS idx_enchant_materials_key ON enchant_materials(recipe_key);
                CREATE TABLE IF NOT EXISTS enchant_npcs (
                    npc_code INTEGER PRIMARY KEY,
                    grade INTEGER NOT NULL,
                    fortune_fee INTEGER NOT NULL,
                    success_rate INTEGER NOT NULL,
                    param_code TEXT NOT NULL,
                    fee_1 INTEGER NOT NULL,
                    fee_2 INTEGER NOT NULL,
                    fee_3 INTEGER NOT NULL,
                    fee_4 INTEGER NOT NULL,
                    fee_5 INTEGER NOT NULL,
                    fee_6 INTEGER NOT NULL,
                    fee_7 INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS enchant_npc_bonus (
                    param_code TEXT PRIMARY KEY,
                    share_rate INTEGER NOT NULL,
                    per_level INTEGER NOT NULL,
                    per_credit INTEGER NOT NULL,
                    weight_z INTEGER NOT NULL,
                    weight_o INTEGER NOT NULL,
                    weight_i INTEGER NOT NULL,
                    weight_y INTEGER NOT NULL,
                    weight_l INTEGER NOT NULL,
                    weight_t INTEGER NOT NULL,
                    weight_c INTEGER NOT NULL,
                    weight_u INTEGER NOT NULL,
                    weight_belt INTEGER NOT NULL,
                    weight_ring INTEGER NOT NULL,
                    weight_shoes INTEGER NOT NULL,
                    weight_armor INTEGER NOT NULL,
                    weight_glove INTEGER NOT NULL,
                    weight_helmet INTEGER NOT NULL,
                    weight_element INTEGER NOT NULL,
                    weight_food INTEGER NOT NULL,
                    weight_weapon INTEGER NOT NULL
                );
                """
            )
            for table in (
                "enchant_recipe_targets", "enchant_materials", "enchant_recipes", "enchant_target_types",
                "enchant_npcs", "enchant_npc_bonus",
            ):
                target.execute(f"DELETE FROM {table}")
            target.executemany("INSERT INTO enchant_recipes VALUES (?, ?, ?, ?)", recipes)
            target.executemany("INSERT INTO enchant_materials VALUES (?, ?, ?, ?)", materials)
            target.executemany("INSERT INTO enchant_target_types VALUES (?, ?, ?)", types)
            target.executemany("INSERT INTO enchant_recipe_targets VALUES (?, ?, ?)", targets)
            target.executemany(
                "INSERT INTO enchant_npcs VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                npcs,
            )
            target.executemany(
                "INSERT INTO enchant_npc_bonus VALUES (" + ", ".join("?" for _ in range(21)) + ")",
                bonuses,
            )
            target.execute(
                "INSERT INTO meta(key, value) VALUES ('enchant_source', ?) "
                "ON CONFLICT(key) DO UPDATE SET value = excluded.value",
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
    args = parser.parse_args()
    try:
        result = import_enchant(args.client, args.target)
    except (OSError, sqlite3.Error, ValueError) as error:
        parser.exit(1, f"Import failed: {error}\n")
    counts = result["counts"]
    print(
        f"Enchant scrolls {counts['scrolls']}, materials {counts['materials']}, "
        f"tables {counts['material_tables']}, targets {counts['scroll_targets']}, "
        f"NPCs {counts['npcs']}"
    )


if __name__ == "__main__":
    main()
