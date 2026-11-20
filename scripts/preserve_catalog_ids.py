#!/usr/bin/env python3
"""Keep published recipe and quest routes stable after a full game catalog import.

Pass the previous widget database and the freshly imported target. Only the local
IDs and their relation keys change. Game-provided codes, costs and materials stay
in the target. The saved high-water marks keep retired IDs out of future imports.
"""
import argparse
import sqlite3
from collections import defaultdict
from contextlib import closing
from pathlib import Path


def quote(name):
    return '"' + name.replace('"', '""') + '"'


def reference_tables(db, column):
    tables = [name for (name,) in db.execute("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")]
    return [name for name in tables if any(row[1] == column for row in db.execute(f"PRAGMA table_info({quote(name)})"))]


def quest_ids(db):
    ids = {}
    for rid, source, code in db.execute("SELECT id, source, code FROM quests"):
        key = source, code
        if key in ids:
            raise ValueError(f"Duplicate quest {key}")
        ids[key] = rid
    return ids


def recipe_groups(db):
    groups = defaultdict(list)
    for rid, source, item, npc, source_key in db.execute(
        "SELECT id,source,result_item,npc,source_key FROM recipes ORDER BY source_key,id"
    ):
        groups[source, item, npc].append((rid, source_key))
    return groups


def variant_signature(db, rid, children):
    # A few vendors sell multiple variants of the same item. Source ordinals
    # drift after insertions, so use their full terms to distinguish variants.
    terms = db.execute("SELECT result_qty,fee,learn_fee,level_min,level_max,success_rate,max_qty,menu_type "
                       "FROM recipes WHERE id=?", (rid,)).fetchone()
    details = []
    for table in children:
        cols = [r[1] for r in db.execute(f"PRAGMA table_info({quote(table)})") if r[1] != "recipe_id"]
        names = ",".join(map(quote, cols))
        rows = tuple(db.execute(f"SELECT {names} FROM {quote(table)} WHERE recipe_id=? ORDER BY {names}", (rid,)))
        if rows:
            details.append((table, tuple(cols), rows))
    return terms, tuple(details)


def recipe_matches(old, new):
    previous, current = recipe_groups(old), recipe_groups(new)
    old_refs, new_refs = reference_tables(old, "recipe_id"), reference_tables(new, "recipe_id")
    matches = {}
    for group in previous.keys() & current.keys():
        before, after = previous[group], current[group]
        if len(before) == len(after) == 1:
            matches[after[0][0]] = before[0][0]
            continue
        variants = defaultdict(list)
        for rid, _ in reversed(before):
            variants[variant_signature(old, rid, old_refs)].append(rid)
        for rid, _ in after:
            candidates = variants.get(variant_signature(new, rid, new_refs))
            if candidates:
                matches[rid] = candidates.pop()
    return matches


def remap(old, new, table, reference, matches):
    meta_key = f"stable_{table}_max_id"
    last = old.execute("SELECT value FROM meta WHERE key=?", (meta_key,)).fetchone()
    ceiling = max(int(last[0]) if last else 0, old.execute(f"SELECT coalesce(max(id),0) FROM {table}").fetchone()[0])
    for (rid,) in new.execute(f"SELECT id FROM {table} ORDER BY id"):
        if rid <= 0:
            raise ValueError(f"Unexpected nonpositive ID in {table}: {rid}")
        if rid not in matches:
            ceiling += 1
            matches[rid] = ceiling
    if len(set(matches.values())) != len(matches):
        raise ValueError(f"Duplicate historical IDs in {table}")
    changes = [(rid, dest) for rid, dest in matches.items() if rid != dest]
    if changes:
        # Reserve negative IDs inside the transaction so swaps cannot collide.
        new.execute("CREATE TEMP TABLE id_map(original INTEGER PRIMARY KEY, final INTEGER NOT NULL UNIQUE)")
        new.executemany("INSERT INTO id_map VALUES (?,?)", changes)
        keys = [(child, reference) for child in reference_tables(new, reference)] + [(table, "id")]
        for child, column in keys:
            new.execute(f"UPDATE {quote(child)} SET {column}=-{column} WHERE {column} IN (SELECT original FROM id_map)")
        for child, column in keys:
            new.execute(f"UPDATE {quote(child)} SET {column}=(SELECT final FROM id_map WHERE original=-{column}) "
                        f"WHERE {column} IN (SELECT -original FROM id_map)")
        new.execute("DROP TABLE id_map")
    new.execute("INSERT INTO meta(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value "
                "WHERE value<>excluded.value", (meta_key, str(ceiling)))
    return len(changes)


def preserve(previous, target):
    previous, target = Path(previous).resolve(), Path(target).resolve()
    if previous.samefile(target):
        raise ValueError("Previous and target catalogs must be different files")
    with closing(sqlite3.connect(previous.as_uri() + "?mode=ro", uri=True)) as old, \
         closing(sqlite3.connect(target.as_uri() + "?mode=rw", uri=True)) as new, new:
        new.execute("PRAGMA foreign_keys=ON")
        new.execute("BEGIN IMMEDIATE")
        new.execute("PRAGMA defer_foreign_keys=ON")
        before, after = quest_ids(old), quest_ids(new)
        quests = {after[key]: before[key] for key in before.keys() & after.keys()}
        return {"quests": remap(old, new, "quests", "quest_id", quests),
                "recipes": remap(old, new, "recipes", "recipe_id", recipe_matches(old, new))}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("previous", type=Path, help="Last widget catalog with stable IDs")
    parser.add_argument("--target", type=Path, default=Path(__file__).resolve().parents[1] / "data/gersang.db")
    args = parser.parse_args()
    print("Updated local IDs:", preserve(args.previous, args.target))
