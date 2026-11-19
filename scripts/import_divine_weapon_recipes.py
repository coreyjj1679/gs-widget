#!/usr/bin/env python3
"""Materialize Trainer Hwan's four basic divine weapon recipes from client dialogue.

QuestText.dat lists the four eligible fey weapons and requires three different
ones. OnlineText.dat names the NPC and the fixed materials. Each recipe stores
its choice of three distinct weapons separately from its two fixed ingredients.
Kirin and advanced weapons have their own unified recipes.
"""

import argparse
import json
import sqlite3
from contextlib import closing
from itertools import combinations
from pathlib import Path

from import_facility_recipes import DEFAULT_TARGET, MATERIAL_COLUMNS, RECIPE_COLUMNS, connect

SOURCE = "divine_weapon"
FEY_WEAPONS = (3433, 3434, 3435, 3436)
CHOICE_COLUMNS = ("recipe_id", "item_code", "choice_count", "qty")
# Result, quest dialogue, item guide, crystal and trophy.
RECIPES = (
    (3993, 6907, 33076, 3905, 3915),
    (3994, 6908, 33077, 3908, 3922),
    (3995, 6909, 33078, 3906, 3916),
    (3996, 6910, 33079, 3907, 3917),
)
ITEM_NAMES = {
    3433: "두신의 팔찌", 3434: "원공의 팔찌", 3435: "누에의 목걸이", 3436: "기의 목걸이",
    3905: "바람의결정", 3906: "불의결정", 3907: "물의결정", 3908: "뇌전의결정",
    3915: "광호의발톱", 3916: "홍작의부리", 3917: "백귀의등껍질", 3922: "여의주",
    3993: "백호의 발찌", 3994: "청룡의 여의주", 3995: "주작의 부리", 3996: "현무의 강침",
}


def client_text(db, filename, text_id):
    row = db.execute("SELECT text FROM strings WHERE object=? AND id=? AND lang='zh'",
                     (filename, f"{text_id:08d}")).fetchone()
    if not row or not row[0]:
        raise ValueError(f"Missing {filename} text {text_id}")
    return row[0]


def verify_sources(db):
    names = {}
    for code, korean in ITEM_NAMES.items():
        row = db.execute("SELECT name_gts FROM items WHERE code=? AND name_ko=?", (code, korean)).fetchone()
        if not row:
            raise ValueError(f"Unexpected item identity for {code}")
        names[code] = client_text(db, "OnlineText.dat", row[0])
    npc = db.execute("SELECT name_gts FROM npcs WHERE code=164 AND name_ko='조련사_환'").fetchone()
    if not npc:
        raise ValueError("Missing Trainer Hwan (NPC 164)")
    npc_name = client_text(db, "OnlineText.dat", npc[0])
    choices = client_text(db, "QuestText.dat", 6912)
    if choices != "* 幻獸的武器有" + ", ".join(names[code] for code in FEY_WEAPONS) + "。":
        raise ValueError("Fey weapon choices differ from the reviewed NPC dialogue")
    for result, dialogue, guide, crystal, trophy in RECIPES:
        text = client_text(db, "QuestText.dat", dialogue)
        # Older dialogue calls the White Tiger Anklet 白虎的聖輪.
        title = "白虎的聖輪" if result == 3993 else names[result]
        if not text.startswith(f"<{title}>") or "任三個相異的幻獸武器" not in text:
            raise ValueError(f"Unexpected weapon or choice rule in dialogue {dialogue}")
        expected = f"找NPC{npc_name}製作 (幻獸武器 3個、{names[crystal]} 30個、{names[trophy]} 30個)"
        if client_text(db, "OnlineText.dat", guide) != expected:
            raise ValueError(f"Unexpected recipe for {result} in item guide {guide}")


def import_divine_weapon_recipes(target=DEFAULT_TARGET):
    with closing(connect(target)) as db, db:
        db.execute("BEGIN IMMEDIATE")
        verify_sources(db)
        db.execute("CREATE TABLE IF NOT EXISTS recipe_material_choices ("
                   "recipe_id INTEGER NOT NULL, item_code INTEGER NOT NULL, "
                   "choice_count INTEGER NOT NULL CHECK(choice_count > 0), qty INTEGER NOT NULL CHECK(qty > 0), "
                   "PRIMARY KEY(recipe_id, item_code))")
        existing = db.execute("SELECT source_key, id FROM recipes WHERE source=?", (SOURCE,)).fetchall()
        ids = dict(existing)
        if len(ids) != len(existing):
            raise ValueError("Duplicate divine weapon recipe keys")
        next_id = db.execute("SELECT COALESCE(MAX(id),0)+1 FROM recipes").fetchone()[0]
        recipes, materials, choices = [], [], []
        for result, _, _, crystal, trophy in RECIPES:
            key = f"divine-weapon:{result}"
            rid = ids.pop(key, None)
            # The original import stored each possible trio as its own recipe.
            # Preserve the first existing ID when converting those rows.
            for trio in combinations(FEY_WEAPONS, 3):
                legacy_id = ids.pop(key + ":" + "-".join(map(str, trio)), None)
                if rid is None:
                    rid = legacy_id
            if rid is None:
                rid, next_id = next_id, next_id + 1
            recipes.append((rid, SOURCE, key, result, 1, -1, None, None, None, None, None, "PRODUCTION", "164"))
            materials.extend([(rid, 1, crystal, 30, 0), (rid, 2, trophy, 30, 0)])
            choices.extend((rid, code, 3, 1) for code in FEY_WEAPONS)
        if ids:
            raise ValueError(f"Unrecognized divine weapon recipe keys: {sorted(ids)}")
        owned = "recipe_id IN (SELECT id FROM recipes WHERE source=?)"
        tables = (
            ("recipes", RECIPE_COLUMNS, recipes, "source=?"),
            ("recipe_materials", MATERIAL_COLUMNS, materials, owned),
            ("recipe_material_choices", CHOICE_COLUMNS, choices, owned),
        )
        if any(set(db.execute(f"SELECT {', '.join(columns)} FROM {table} WHERE {where}", (SOURCE,))) != set(rows)
               for table, columns, rows, where in tables):
            for table, _, _, where in reversed(tables):
                db.execute(f"DELETE FROM {table} WHERE {where}", (SOURCE,))
            for table, columns, rows, _ in tables:
                placeholders = ", ".join("?" for _ in columns)
                db.executemany(f"INSERT INTO {table} ({', '.join(columns)}) VALUES ({placeholders})", rows)
        metadata = {
            "source": "Bundled client strings and item identities",
            "dialogue": "QuestText.dat 6907-6912",
            "item_guide": "OnlineText.dat 33076-33079",
            "fee": "No amount in the source dialogue (-1 = unrecorded)",
            "alternatives": "Three distinct fey weapons from the four named in dialogue 6912",
        }
        db.execute("INSERT INTO meta(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value "
                   "WHERE value != excluded.value", ("divine_weapon_recipes_source", json.dumps(metadata, sort_keys=True)))
    return len(recipes)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", type=Path, default=DEFAULT_TARGET)
    args = parser.parse_args()
    try:
        print(f"{import_divine_weapon_recipes(args.target)} divine weapon recipes")
    except (ValueError, OSError, sqlite3.Error) as error:
        parser.exit(1, f"Import failed: {error}\n")
