#!/usr/bin/env python3
"""Import the client's village manufacturing recipes from a gs-parser full DB.

Makingfacility.txt joins the recipe book and result by explicit item codes. Its
five Element groups are required materials; MakeItemQuantity is the batch yield.
OnlineText.dat 00033780 independently records Dragon Kukri's book quantity (1),
five materials, fee, five-day duration and India Friendship requirement. Books
are recorded as required inputs, without claiming permanent unlocks or reuse.

Only source='facility' recipes and this importer's metadata tables are replaced.
Other recipes, their IDs and the source database are preserved. Existing IDs are
reused by a content key, so source row order and added recipes do not renumber
earlier imports. Identical refreshes do not rewrite the target database.
"""

import argparse
import hashlib
import json
import re
import sqlite3
from contextlib import closing
from pathlib import Path

DEFAULT_TARGET = Path(__file__).resolve().parents[1] / "data" / "gersang.db"
META_KEY = "facility_recipes_source"
SOURCE_KEY = "facility-v1:"
COLUMNS = (
    "VillageCode", "VillageName", "Recipe Item Code", "Recipe Item Name",
    "MakeItemCode", "MakeItemName", "MakeItemQuantity",
    *[f"Element{i}{part}" for i in range(5) for part in ("Name", "Code", "Num")],
    "SpendMoney", "Time", "InfluenceType", "InfluenceValue",
)
RECIPE_COLUMNS = (
    "id", "source", "source_key", "result_item", "result_qty", "fee",
    "learn_fee", "level_min", "level_max", "success_rate", "max_qty", "menu_type", "npc",
)
MATERIAL_COLUMNS = ("recipe_id", "slot", "item_code", "qty", "enhance")
FACILITY_COLUMNS = ("recipe_id", "book_item", "duration_days", "influence_type", "influence_value")
LOCATION_COLUMNS = ("recipe_id", "village_code", "village_name_ko", "town_code")


def quoted(identifier):
    return '"' + identifier.replace('"', '""') + '"'


def connect(path, readonly=False):
    return sqlite3.connect(Path(path).resolve().as_uri() + ("?mode=ro" if readonly else "?mode=rw"), uri=True)


def file_sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def integer(value, label, minimum=0):
    text = str(value).strip() if value is not None else ""
    if not re.fullmatch(r"[0-9]+", text):
        raise ValueError(f"Invalid integer for {label}: {value!r}")
    number = int(text)
    if not minimum <= number <= (1 << 63) - 1:
        raise ValueError(f"Out of range for {label}: {number}")
    return number


def source_sheet(source):
    matches = []
    for member, title, table, row_count in source.execute(
        "SELECT member, title, table_name, row_count FROM raw_sheet WHERE archive='gersang.gcs'"
    ):
        columns = {row[1] for row in source.execute(f"PRAGMA table_info({quoted(table)})")}
        if {"VillageCode", "Recipe Item Code", "MakeItemCode"} <= columns:
            if columns != set(COLUMNS):
                raise ValueError(f"Unsupported manufacturing columns in {table}")
            matches.append({"member": member, "title": title, "table_name": table, "row_count": row_count})
    if len(matches) != 1:
        raise ValueError(f"Expected one village manufacturing sheet, found {len(matches)}")
    return matches[0]


def read_recipes(source, sheet):
    cursor = source.execute(f"SELECT {', '.join(map(quoted, COLUMNS))} FROM {quoted(sheet['table_name'])}")
    groups, villages, locations = {}, {}, set()
    count = 0
    for count, values in enumerate(cursor, start=1):
        row = dict(zip(COLUMNS, values))
        label = f"{sheet['table_name']} row {count}"
        village = str(integer(row["VillageCode"], f"{label} village", 1))
        name = row["VillageName"].strip()
        if not name or name == "0":
            raise ValueError(f"Missing village name in {label}")
        if village in villages and villages[village] != name:
            raise ValueError(f"Conflicting names for village {village}")
        villages[village] = name
        materials = []
        for slot in range(5):
            code = integer(row[f"Element{slot}Code"], f"{label} material {slot}")
            qty = integer(row[f"Element{slot}Num"], f"{label} quantity {slot}")
            if bool(code) != bool(qty):
                raise ValueError(f"Incomplete material {slot} in {label}")
            if code:
                materials.append((code, qty))
        if not materials:
            raise ValueError(f"No required materials in {label}")
        recipe = {
            "book_item": integer(row["Recipe Item Code"], f"{label} book", 1),
            "result_item": integer(row["MakeItemCode"], f"{label} result", 1),
            "result_qty": integer(row["MakeItemQuantity"], f"{label} yield", 1),
            "fee": integer(row["SpendMoney"], f"{label} fee"),
            "duration_days": integer(row["Time"], f"{label} duration", 1),
            "influence_type": integer(row["InfluenceType"], f"{label} influence type"),
            "influence_value": integer(row["InfluenceValue"], f"{label} influence value"),
            "materials": materials,
        }
        if recipe["book_item"] in {code for code, _ in materials}:
            raise ValueError(f"Recipe book also listed as a material in {label}")
        payload = json.dumps(recipe, sort_keys=True, separators=(",", ":"))
        key = SOURCE_KEY + hashlib.sha256(payload.encode()).hexdigest()
        if (key, village) in locations:
            raise ValueError(f"Duplicate manufacturing recipe/location in {label}")
        locations.add((key, village))
        group = groups.setdefault(key, {**recipe, "locations": []})
        group["locations"].append((village, name, ""))
    if not count or count != sheet["row_count"]:
        raise ValueError(f"Source row count mismatch: read {count}, expected {sheet['row_count']}")
    return groups


def validate_schema(target, table, columns, keys, text=(), nullable=()):
    kind = target.execute("SELECT type FROM sqlite_master WHERE name=?", (table,)).fetchone()
    if kind != ("table",):
        raise ValueError(f"Expected target table {table}")
    found = {row[1]: row for row in target.execute(f"PRAGMA table_info({quoted(table)})")}
    if set(found) != set(columns):
        raise ValueError(f"Incompatible existing {table} columns")
    for column in columns:
        row = found[column]
        expected_type = "TEXT" if column in text else "INTEGER"
        primary = keys.index(column) + 1 if column in keys else 0
        required = column not in nullable and keys != (column,)
        if row[2].upper() != expected_type or row[5] != primary or (required and not row[3]):
            raise ValueError(f"Incompatible existing {table} schema at {column}")


def prepare_target(target):
    validate_schema(target, "recipes", RECIPE_COLUMNS, ("id",),
                    ("source", "source_key", "menu_type", "npc"),
                    ("learn_fee", "level_min", "level_max", "success_rate", "max_qty"))
    validate_schema(target, "recipe_materials", MATERIAL_COLUMNS, ("recipe_id", "slot"))
    target.execute("CREATE TABLE IF NOT EXISTS recipe_facility ("
                   "recipe_id INTEGER PRIMARY KEY, book_item INTEGER NOT NULL, "
                   "duration_days INTEGER NOT NULL, influence_type INTEGER NOT NULL, influence_value INTEGER NOT NULL)")
    target.execute("CREATE TABLE IF NOT EXISTS recipe_facility_locations ("
                   "recipe_id INTEGER NOT NULL, village_code TEXT NOT NULL, village_name_ko TEXT NOT NULL, "
                   "town_code TEXT NOT NULL DEFAULT '', PRIMARY KEY(recipe_id, village_code))")
    validate_schema(target, "recipe_facility", FACILITY_COLUMNS, ("recipe_id",))
    validate_schema(target, "recipe_facility_locations", LOCATION_COLUMNS, ("recipe_id", "village_code"),
                    ("village_code", "village_name_ko", "town_code"))
    target.execute("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
    validate_schema(target, "meta", ("key", "value"), ("key",), ("key", "value"))


def validate_owned_rows(target):
    previous = target.execute("SELECT value FROM meta WHERE key=?", (META_KEY,)).fetchone()
    existing = target.execute("SELECT id, source_key FROM recipes WHERE source='facility'").fetchall()
    ids = {row[0] for row in existing}
    metadata_ids = {row[0] for row in target.execute("SELECT recipe_id FROM recipe_facility")}
    location_ids = {row[0] for row in target.execute("SELECT DISTINCT recipe_id FROM recipe_facility_locations")}
    if existing or metadata_ids or location_ids:
        if not previous or json.loads(previous[0]).get("importer_version") != 1:
            raise ValueError("Unowned existing facility data")
        if metadata_ids != ids or location_ids != ids:
            raise ValueError("Facility metadata does not match owned recipes")
    mapping = {}
    for recipe_id, key in existing:
        if not re.fullmatch(re.escape(SOURCE_KEY) + "[0-9a-f]{64}", key) or key in mapping:
            raise ValueError("Conflicting existing facility source keys")
        mapping[key] = recipe_id
    return mapping


def import_facility_recipes(full_db, target_db=DEFAULT_TARGET):
    source_path, target_path = Path(full_db).resolve(), Path(target_db).resolve()
    if source_path == target_path or source_path.samefile(target_path):
        raise ValueError("Source and target databases must be different")
    digest = file_sha256(source_path)
    with closing(connect(source_path, readonly=True)) as source:
        sheet = source_sheet(source)
        groups = read_recipes(source, sheet)
    if file_sha256(source_path) != digest:
        raise ValueError("Source database changed during the import")
    with closing(connect(target_path)) as target, target:
        target.execute("BEGIN IMMEDIATE")
        prepare_target(target)
        existing = validate_owned_rows(target)
        known = {row[0] for row in target.execute("SELECT code FROM items")}
        referenced = {code for recipe in groups.values()
                      for code in (recipe["result_item"], recipe["book_item"], *[m[0] for m in recipe["materials"]])}
        if referenced - known:
            raise ValueError(f"Unknown item references: {sorted(referenced - known)}")
        next_id = target.execute("SELECT coalesce(max(id), 0) + 1 FROM recipes").fetchone()[0]
        recipes, materials, facilities, locations = [], [], [], []
        for key, row in sorted(groups.items()):
            recipe_id = existing.get(key)
            if recipe_id is None:
                recipe_id, next_id = next_id, next_id + 1
            recipes.append((recipe_id, "facility", key, row["result_item"], row["result_qty"], row["fee"],
                            None, None, None, None, None, "facility", ""))
            materials.extend((recipe_id, slot, code, qty, 0)
                             for slot, (code, qty) in enumerate([(row["book_item"], 1), *row["materials"]], 1))
            facilities.append((recipe_id, row["book_item"], row["duration_days"], row["influence_type"], row["influence_value"]))
            locations.extend((recipe_id, *place) for place in sorted(row["locations"]))
        desired = {"recipes": (RECIPE_COLUMNS, recipes), "recipe_materials": (MATERIAL_COLUMNS, materials),
                   "recipe_facility": (FACILITY_COLUMNS, facilities),
                   "recipe_facility_locations": (LOCATION_COLUMNS, locations)}
        changed = False
        for table, (columns, rows) in desired.items():
            where = " WHERE source='facility'" if table == "recipes" else (
                " WHERE recipe_id IN (SELECT id FROM recipes WHERE source='facility')" if table == "recipe_materials" else "")
            previous = target.execute(f"SELECT {', '.join(columns)} FROM {table}{where}").fetchall()
            changed |= set(previous) != set(rows) or len(previous) != len(rows)
        if changed:
            target.execute("DELETE FROM recipe_facility_locations")
            target.execute("DELETE FROM recipe_facility")
            target.execute("DELETE FROM recipe_materials WHERE recipe_id IN (SELECT id FROM recipes WHERE source='facility')")
            target.execute("DELETE FROM recipes WHERE source='facility'")
            for table, (columns, rows) in desired.items():
                target.executemany(f"INSERT INTO {table} ({', '.join(columns)}) VALUES ({', '.join('?' for _ in columns)})", rows)
        metadata = {
            "importer_version": 1, "source_file": source_path.name, "source_sha256": digest,
            "archive": "gersang.gcs", "sheet": sheet,
            "fields": {"book_item": "Recipe Item Code", "result_item": "MakeItemCode", "result_qty": "MakeItemQuantity",
                       "fee": "SpendMoney", "duration_days": "Time", "influence_type": "InfluenceType",
                       "influence_value": "InfluenceValue", "materials": "Element0..4Code / Element0..4Num",
                       "locations": "VillageCode / VillageName"},
            "counts": {"source_rows": sheet["row_count"], "recipes": len(recipes), "materials": len(materials),
                       "locations": len(locations), "books": len({row[1] for row in facilities}), "linked_towns": 0},
            "influence_types": sorted({row[3] for row in facilities}),
            "notes": {"book_quantity": "1 required book; no claim of permanent unlock or reuse",
                      "units_evidence": "OnlineText.dat 00033780: book quantity 1, production days, India Friendship",
                      "influence_evidence": "InfluenceStoreBuyList.txt: 우호도=2 -> 우호도GTS=00014709 (India Friendship)",
                      "town_codes": "Unresolved: no source numeric VillageCode -> towns.code join; names are not used to infer it",
                      "identity": "SHA-256 of complete numeric recipe terms excluding village and display names"},
        }
        encoded = json.dumps(metadata, ensure_ascii=False, sort_keys=True)
        previous = target.execute("SELECT value FROM meta WHERE key=?", (META_KEY,)).fetchone()
        if previous != (encoded,):
            target.execute("INSERT INTO meta(key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", (META_KEY, encoded))
    return metadata


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--full-db", type=Path, required=True, help="gs-parser full database (read-only)")
    parser.add_argument("--target", type=Path, default=DEFAULT_TARGET)
    args = parser.parse_args()
    print(json.dumps(import_facility_recipes(args.full_db, args.target), ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
