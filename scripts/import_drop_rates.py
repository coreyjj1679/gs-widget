#!/usr/bin/env python3
"""Enrich the widget catalog with character-table drop rates from gs-parser.

Usage: python3 scripts/import_drop_rates.py /path/to/gs-parser/data/gersang-full.db

Rates are integer parts per 100,000 (5500 = 5.5%). Slots 9 and 10 are
solo-only. Match by monster, item, and solo mode because the catalog and raw
character sheet may order their items differently. Preserve all catalog rows;
missing or ambiguous source matches remain NULL, never an assumed zero.
Reward pools and special map/boss tables are deliberately not imported.
"""

import argparse
import hashlib
import json
import sqlite3
from collections import Counter, defaultdict
from contextlib import closing
from pathlib import Path


RATE_COLUMN = "drop_rate_per_100000"
DENOMINATOR = 100_000
DEFAULT_TARGET = Path(__file__).resolve().parents[1] / "data" / "gersang.db"


def quoted(identifier):
    return '"' + identifier.replace('"', '""') + '"'


def connect_existing(path, *, readonly=False):
    return sqlite3.connect(path.resolve().as_uri() + ("?mode=ro" if readonly else "?mode=rw"), uri=True)


def character_sheet(source):
    """Resolve the table via raw_sheet rather than relying on member numbering."""
    required = {"ID", "Name", "Villagecode", "Name Code"}
    required.update(f"{prefix}{slot}" for prefix in ("Item Code", "Percent") for slot in range(1, 11))
    candidates = []
    for archive, member, table in source.execute(
        "SELECT archive, member, table_name FROM raw_sheet WHERE archive = 'gersang.gcs'"
    ):
        columns = {row[1] for row in source.execute(f"PRAGMA table_info({quoted(table)})")}
        if required <= columns:
            candidates.append((archive, member, table))
    if len(candidates) != 1:
        raise ValueError(f"Expected one character drop sheet, found {len(candidates)}")
    return candidates[0]


def integer(value, label, *, missing=None):
    text = str(value).strip() if value is not None else ""
    if not text:
        return missing
    if not text.isascii() or not text.isdecimal():
        raise ValueError(f"Invalid integer for {label}: {value!r}")
    return int(text)


def source_drops(source, table, monster_codes):
    fields = ["ID"] + [f"{prefix}{slot}" for slot in range(1, 11) for prefix in ("Item Code", "Percent")]
    index = defaultdict(list)
    seen = set()
    for row in source.execute(f"SELECT {', '.join(map(quoted, fields))} FROM {quoted(table)}"):
        code = row[0]
        if code not in monster_codes:
            continue
        if code in seen:
            raise ValueError(f"Duplicate monster ID in character sheet: {code}")
        seen.add(code)
        for slot in range(1, 11):
            item = integer(row[slot * 2 - 1], f"{code} item slot {slot}", missing=0)
            if item:
                index[(code, item, slot >= 9)].append((slot, row[slot * 2]))
    return index, seen


def import_drop_rates(source_path, target_path=DEFAULT_TARGET):
    source_path, target_path = Path(source_path), Path(target_path)
    if source_path.resolve() == target_path.resolve():
        raise ValueError("Source and target databases must be different files")
    with closing(connect_existing(source_path, readonly=True)) as source, closing(connect_existing(target_path)) as target:
        archive, member, table = character_sheet(source)
        # An explicit transaction also makes ALTER TABLE roll back on validation errors.
        target.execute("BEGIN IMMEDIATE")
        try:
            rows = target.execute("SELECT monster_code, slot, item_code FROM monster_drops ORDER BY monster_code, slot").fetchall()
            if not rows:
                raise ValueError("Target monster_drops is empty")
            index, seen = source_drops(source, table, {row[0] for row in rows})
            counts = Counter(total=len(rows), imported=0, unknown=0, reordered=0)
            updates = []
            for code, slot, item in rows:
                if not 1 <= slot <= 10:
                    raise ValueError(f"Invalid target drop slot: {code}/{slot}")
                candidates = index[(code, item, slot >= 9)]
                rate = None
                if code not in seen:
                    counts["missing_monster"] += 1
                elif not candidates:
                    counts["missing_item"] += 1
                elif len(candidates) != 1:
                    counts["ambiguous"] += 1
                else:
                    source_slot, raw_rate = candidates[0]
                    rate = integer(raw_rate, f"{code} rate slot {source_slot}")
                    if rate is None:
                        counts["missing_rate"] += 1
                    elif not 0 <= rate <= DENOMINATOR:
                        raise ValueError(f"Rate outside 0..{DENOMINATOR}: {code}/{source_slot} = {rate}")
                    else:
                        counts["reordered"] += source_slot != slot
                counts["unknown" if rate is None else "imported"] += 1
                updates.append((rate, code, slot, rate))
            if not counts["imported"]:
                raise ValueError("No drop rates matched; refusing to overwrite the target")

            columns = {row[1]: row[2] for row in target.execute("PRAGMA table_info(monster_drops)")}
            if RATE_COLUMN not in columns:
                target.execute(f"ALTER TABLE monster_drops ADD COLUMN {RATE_COLUMN} INTEGER CHECK ({RATE_COLUMN} BETWEEN 0 AND {DENOMINATOR})")
            elif columns[RATE_COLUMN].upper() != "INTEGER":
                raise ValueError(f"Existing {RATE_COLUMN} column is not INTEGER")
            target.executemany(
                f"UPDATE monster_drops SET {RATE_COLUMN} = ? WHERE monster_code = ? AND slot = ? AND {RATE_COLUMN} IS NOT ?",
                updates,
            )
            digest = hashlib.sha256()
            with source_path.open("rb") as source_file:
                for chunk in iter(lambda: source_file.read(1024 * 1024), b""):
                    digest.update(chunk)
            provenance = dict(
                source_file=source_path.name, source_sha256=digest.hexdigest(),
                archive=archive, member=member, table=table, denominator=DENOMINATOR,
                matching="monster_code + item_code + solo_mode; unique source slot",
                counts=dict(counts),
            )
            target.execute("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            target.execute(
                "INSERT INTO meta(key, value) VALUES ('drop_rates_source', ?) "
                "ON CONFLICT(key) DO UPDATE SET value = excluded.value WHERE value != excluded.value",
                (json.dumps(provenance, sort_keys=True),),
            )
            integrity = target.execute("PRAGMA integrity_check").fetchone()[0]
            if integrity != "ok":
                raise ValueError(f"Target integrity check failed: {integrity}")
            target.commit()
            return provenance
        except BaseException:
            target.rollback()
            raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="gs-parser data/gersang-full.db")
    parser.add_argument("--target", type=Path, default=DEFAULT_TARGET, help="widget catalog to update (must exist)")
    args = parser.parse_args()
    try:
        result = import_drop_rates(args.source, args.target)
    except (OSError, sqlite3.Error, ValueError) as error:
        parser.exit(1, f"Import failed: {error}\n")
    print(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
