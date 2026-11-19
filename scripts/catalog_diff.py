"""Build the widget's offline game-version history from read-only SQLite snapshots."""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from contextlib import ExitStack, closing
from functools import lru_cache
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
IMPORT_TABLES = {"meta", "changes", "sources", "revisions"}
CATEGORIES = {
    "items": "items item_sets item_set_effects item_set_members item_set_magic "
             "item_bases item_affix item_gear enhance",
    "monsters": "monsters monster_drops monster_locations monster_categories monster_sprites monster_portraits",
    "recipes": "recipes recipe_materials recipe_facility recipe_facility_locations recipe_material_choices "
               "item_formulas item_formula_parts",
    "quests": "quests quest_steps quest_lines",
    "places": "maps npcs towns town_goods",
    "characters": "characters character_skills character_stats character_companions character_facts "
                  "character_materials character_upgrades character_requirements job_steps job_materials",
    "production": "work_buildings work_slots work_facilities work_products work_upgrades work_facility_upgrades price_brackets",
    "enchant": "enchant_equipment enchant_effects enchant_recipes enchant_materials enchant_target_types "
              "enchant_recipe_targets enchant_npcs enchant_npc_bonus",
    "strings": "strings",
    "patches": "patches patch_lines",
}
TABLE_ORDER = [table for tables in CATEGORIES.values() for table in tables.split()]
CATEGORY_OF = {table: category for category, tables in CATEGORIES.items() for table in tables.split()}
NAME_ROUTES = {
    "items": "item", "monsters": "monster", "characters": "hero", "npcs": "npc", "maps": "map",
    "item_sets": "set", "towns": "town", "work_buildings": "yard", "work_facilities": "bay", "quests": "quest",
}


def quote(name):
    return '"' + name.replace('"', '""') + '"'


def display_key(parts):
    # Preserve positions in composite keys, including an empty NPC in recipes.
    # Ordinary game codes stay readable while embedded separators cannot collide.
    def component(value):
        if value == "":
            return "∅"
        text = str(value).replace("\\", "\\\\").replace("/", "\\/")
        return "\\∅" if text == "∅" else text
    return " / ".join(component(part) for part in parts)


def localized(database, obj, text_id):
    if not text_id or not obj:
        return {}
    return dict(database.execute(
        "SELECT lang, text FROM strings WHERE object=? AND id=? AND text<>''",
        (obj, text_id),
    ))


class Snapshot:
    def __init__(self, conn):
        self.conn = conn
        conn.row_factory = sqlite3.Row
        self.tables = {row[0] for row in conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT GLOB 'sqlite_*'"
        )} - IMPORT_TABLES
        self.quest_keys = {}
        self.quest_ids = {}
        if "quests" in self.tables:
            for quest_id, source, code in conn.execute("SELECT id, source, code FROM quests"):
                self.quest_keys[quest_id] = (source, code)
                self.quest_ids[source, code] = quest_id
        self.recipe_keys = {}
        if "recipes" in self.tables:
            # Numeric IDs and source_key ordinals are reassigned during a full
            # import. A few NPCs offer two variants for the same output.
            variants = Counter()
            for recipe_id, source, result, npc in conn.execute(
                "SELECT id, source, result_item, npc FROM recipes ORDER BY length(source_key), source_key, id"
            ):
                key = (source, result, npc)
                variants[key] += 1
                if variants[key] > 1:
                    key += (f"#{variants[key]}",)
                self.recipe_keys[recipe_id] = key

    def rows(self, table):
        if table not in self.tables:
            return
        if table in ("monster_locations", "monster_categories"):
            # The lookup ID is the value of a location/category membership.
            # Comparing empty composite-PK rows would report moving the same
            # monster to a different floor as a removal and an addition.
            field = "location_gts" if table == "monster_locations" else "category_gts"
            memberships = defaultdict(list)
            for monster, text_id in self.conn.execute(
                f"SELECT monster_code,{field} FROM {table} ORDER BY monster_code,{field}"
            ):
                memberships[monster].append(text_id)
            for monster, ids in memberships.items():
                yield (monster,), {field: ids}
            return
        if table == "quest_steps":
            # Importer slots are one sequence across objectives, rewards and
            # requirements. Dropping a reward shifts following requirements'
            # raw ordinals even when the prerequisite and amount are identical.
            ordinals = Counter()
            for row in self.conn.execute("SELECT * FROM quest_steps ORDER BY quest_id,slot"):
                values = dict(row)
                owner = self.quest_keys[values.pop("quest_id")]
                role = values.pop("role")
                del values["slot"]
                # Requirements carry a typed target (e.g. Level/0 or Quest/72201).
                # Number the other role's slots within their own type so a change
                # to what an item reward grants remains one comparable slot.
                key = (*owner, role, values["kind"])
                if role == "requirement":
                    key += (values["code"],)
                ordinals[key] += 1
                yield (*key, ordinals[key]), values
            return
        if table == "quest_lines":
            # A main quest can change many dialogue slots in one import. Keep
            # every old/new reference together under its stable quest identity.
            grouped = {}
            for quest_id, role, slot, gts_id, obj in self.conn.execute(
                "SELECT quest_id,role,slot,gts_id,text_object FROM quest_lines ORDER BY quest_id,role,slot"
            ):
                links = grouped.setdefault(self.quest_keys[quest_id], {})
                links[f"{role}[{slot}]"] = f"{obj} #{gts_id}"
            yield from grouped.items()
            return

        schema = list(self.conn.execute(f"PRAGMA table_info({quote(table)})"))
        pk = [r["name"] for r in sorted(schema, key=lambda r: r["pk"]) if r["pk"]]
        if not pk:
            raise ValueError(f"{table} needs a stable key before it can be compared")
        # A translation change can rename an existing entity without modifying
        # its local row. Join only its specific name pointer, never free text.
        columns = {r["name"] for r in schema}
        query = f"SELECT * FROM {quote(table)}"
        if "name_gts" in columns and "name_ko" in columns and "strings" in self.tables:
            query = (f"SELECT t.*,en.text AS name_en,zh.text AS name_zh FROM {quote(table)} t "
                     "LEFT JOIN strings en ON en.object='OnlineText.dat' AND en.lang='en' "
                     "AND t.name_gts>0 AND en.id=printf('%08d',t.name_gts) "
                     "LEFT JOIN strings zh ON zh.object='OnlineText.dat' AND zh.lang='zh' "
                     "AND t.name_gts>0 AND zh.id=printf('%08d',t.name_gts)")
        for row in self.conn.execute(query):
            values = dict(row)
            key = tuple(values.pop(col) for col in pk)
            if table == "quests":
                key = self.quest_keys[key[0]]
                for col in ("source", "code"):
                    del values[col]
            elif table == "recipes":
                key = self.recipe_keys[key[0]]
                for col in ("source", "source_key", "result_item", "npc"):
                    del values[col]
            elif pk[0] == "quest_id":
                key = self.quest_keys[key[0]] + key[1:]
            elif pk[0] == "recipe_id":
                key = self.recipe_keys[key[0]] + key[1:]
            if table in ("enchant_effects", "enchant_equipment"):
                # Provenance duplicates the parsed fields, including trailing tabs.
                values.pop("source_line", None)
                values.pop("raw_source_row", None)
            yield key, values

    @lru_cache(maxsize=1024)
    def monster_regions(self, code):
        ids = set()
        if "monster_locations" in self.tables:
            ids.update(row[0] for row in self.conn.execute(
                "SELECT location_gts FROM monster_locations WHERE monster_code=? AND location_gts>0", (code,)
            ))
        if not ids and "monsters" in self.tables:
            ids.update(row[0] for row in self.conn.execute(
                "SELECT location_gts FROM monsters WHERE code=? AND location_gts>0", (code,)
            ))
        return [{"id": str(region), "name": self.region_name(region)} for region in sorted(ids)]

    @lru_cache(maxsize=1024)
    def region_name(self, text_id):
        names = self.texts("OnlineText.dat", f"{text_id:08d}")
        if "ko" not in names and "maps" in self.tables:
            # Multiple maps can share a location string. Only use their local
            # name when all populated source names for this GTS actually agree.
            options = [row[0] for row in self.conn.execute(
                "SELECT DISTINCT name_ko FROM maps WHERE name_gts=? AND trim(name_ko) NOT IN ('','0') LIMIT 2",
                (text_id,),
            )]
            if len(options) == 1:
                names["ko"] = options[0]
        return names

    def drop_snapshot(self, monster, values):
        return {"item_code": values["item_code"], "rate": values.get("drop_rate_per_100000"),
                "item": self.entity_name("items", values["item_code"]),
                "monster": self.entity_name("monsters", monster)}

    def loot(self, code):
        if "monster_drops" not in self.tables:
            return []
        return [{"slot": slot, "item_code": item, "rate": rate, "item": self.entity_name("items", item)}
                for slot, item, rate in self.conn.execute(
                    "SELECT slot,item_code,drop_rate_per_100000 FROM monster_drops WHERE monster_code=? ORDER BY slot",
                    (code,),
                )]

    @lru_cache(maxsize=4096)
    def entity_name(self, table, code):
        if table not in self.tables:
            return {}
        row = self.conn.execute(f"SELECT * FROM {quote(table)} WHERE code=?", (code,)).fetchone()
        if row is None:
            return {}
        fields = dict(row)
        names = self.texts("OnlineText.dat", f"{fields['name_gts']:08d}") if fields.get("name_gts") else {}
        if fields.get("name_ko"):
            names["ko"] = fields["name_ko"]
        return names

    def with_ingredient(self, owner_names, owner_code, item_code):
        item_names = self.entity_name("items", item_code)

        def fallback(names, code):
            return names.get("en") or names.get("ko") or names.get("zh") or str(code)

        return {
            lang: (owner_names.get(lang) or fallback(owner_names, owner_code)) + " · " +
                  (item_names.get(lang) or fallback(item_names, item_code))
            for lang in ("zh", "en", "ko") if lang in item_names or lang in owner_names
        }

    def texts(self, obj, text_id):
        return localized(self.conn, obj, text_id) if "strings" in self.tables else {}

    @lru_cache(maxsize=2048)
    def quest_name(self, key):
        quest_id = self.quest_ids[key]
        quest = dict(self.conn.execute("SELECT * FROM quests WHERE id=?", (quest_id,)).fetchone())
        names = self.texts("OnlineText.dat", f"{quest['title_gts']:08d}") if quest.get("title_gts") else {}
        if "quest_lines" in self.tables and "strings" in self.tables:
            for lang, value in self.conn.execute(
                "SELECT s.lang,s.text FROM quest_lines l JOIN strings s "
                "ON s.object=l.text_object AND s.id=printf('%08d',l.gts_id) "
                "WHERE l.quest_id=? AND l.role IN ('title','subject','start-subject') AND s.text<>'' "
                "ORDER BY CASE l.role WHEN 'title' THEN 0 WHEN 'subject' THEN 1 ELSE 2 END,l.slot",
                (quest_id,),
            ):
                names.setdefault(lang, value)
        if quest.get("title") and quest["title"] != "0":
            names["ko"] = quest["title"]
        return names

    def name(self, table, key, values):
        if table == "strings":
            names = self.texts(key[0], key[1])
        elif table in ("quests", "quest_lines", "quest_steps"):
            names = self.quest_name(key[:2])
        elif table in ("recipes", "recipe_materials", "recipe_facility",
                       "recipe_facility_locations", "recipe_material_choices"):
            names = self.entity_name("items", key[1])
            if table == "recipe_materials":
                names = self.with_ingredient(names, key[1], values["item_code"])
        elif table.startswith("monster"):
            names = self.entity_name("monsters", key[0])
            if table == "monster_drops":
                names = self.with_ingredient(names, key[0], values["item_code"])
        elif table.startswith("character"):
            names = self.entity_name("characters", key[0])
        elif table in ("item_sets", "item_set_effects"):
            names = self.entity_name("item_sets", key[0])
        elif table in ("items", "enhance", "enchant_equipment", "enchant_recipes", "enchant_recipe_targets") or (
            table.startswith("item_") and table != "item_set_magic"
        ):
            names = self.entity_name("items", key[0])
        elif table in ("maps", "npcs", "towns", "work_buildings", "work_facilities", "work_upgrades"):
            names = self.entity_name(table, key[0])
        elif table == "work_products":
            names = self.entity_name("items", key[1])
        elif table == "enchant_materials" and "item_code" in values:
            names = self.entity_name("items", values["item_code"])
        else:
            names = {}
        # Keep a compact row title. The complete changed text is in the values.
        return {lang: " ".join(value.split())[:96] for lang, value in names.items() if value.strip()}


def entries_for(old, new, table):
    previous = {}
    for key, values in old.rows(table):
        if key in previous:
            raise ValueError(f"Duplicate key in old {table}: {key}")
        previous[key] = values

    entries = []
    def entry(key, previous_values, current_values, before, after):
        exists_before, exists_after = previous_values is not None, current_values is not None
        snapshot = new if exists_after else old
        name = snapshot.name(table, key, current_values if exists_after else previous_values)
        if not exists_before:
            kind = "added"
        elif not exists_after:
            kind = "removed"
        else:
            name_fields = {"name_ko", "name_en", "name_zh", "result_name_ko"}
            if table == "quests":
                name_fields.add("title")
            renamed = any(isinstance(before.get(field), str) and before[field].strip() not in ("", "0")
                          and isinstance(after.get(field), str) and after[field].strip() not in ("", "0")
                          and before[field] != after[field] for field in name_fields)
            kind = "renamed" if renamed else "modified"
        record = {
            "category": CATEGORY_OF.get(table, "other"),
            "table": table,
            "key": display_key(key),
            "kind": kind, "name": name, "before": before, "after": after,
        }
        if table == "monsters" and kind == "added":
            # A recorded drop slot can predate its monster. The list must
            # describe the new monster, including slots the import left alone.
            record["loot"] = new.loot(key[0])
        if exists_before and exists_after:
            old_name = old.name(table, key, previous_values)
            if old_name != name:
                record["previous_name"] = old_name
        if record["category"] == "monsters":
            regions = snapshot.monster_regions(key[0])
            if regions:
                record["regions"] = regions
            if exists_before and exists_after:
                old_regions = old.monster_regions(key[0])
                if old_regions != regions:
                    record["previous_regions"] = old_regions
        if table == "monster_drops":
            if exists_before:
                record["drop_before"] = old.drop_snapshot(key[0], previous_values)
            if exists_after:
                record["drop_after"] = new.drop_snapshot(key[0], current_values)
        entries.append(record)

    seen = set()
    for key, values in new.rows(table):
        if key in seen:
            raise ValueError(f"Duplicate key in new {table}: {key}")
        seen.add(key)
        before = previous.pop(key, None)
        if before is None:
            entry(key, None, values, {}, values)
        elif before != values:
            fields = {k for k in before.keys() | values.keys()
                      if k not in before or k not in values or before[k] != values[k]}
            entry(key, before, values,
                  {k: v for k, v in before.items() if k in fields},
                  {k: v for k, v in values.items() if k in fields})
    for key, values in previous.items():
        entry(key, values, None, values, {})
    entries.sort(key=lambda e: e["key"])
    return entries


def compare(old, new, from_version, to_version):
    tables = old.tables | new.tables
    ordered = [t for t in TABLE_ORDER if t in tables] + sorted(tables - set(TABLE_ORDER))
    # A drop can change in V2, then its monster can move in V3 while that
    # drop remains unchanged. Keep the sparse region transitions separately
    # so a V1/V3 comparison can group it at the correct endpoint.
    monster_codes = set()
    for snapshot in (old, new):
        if "monster_locations" in snapshot.tables:
            monster_codes.update(r[0] for r in snapshot.conn.execute("SELECT DISTINCT monster_code FROM monster_locations"))
        if "monsters" in snapshot.tables:
            monster_codes.update(r[0] for r in snapshot.conn.execute("SELECT code FROM monsters"))
    region_changes = {}
    for code in sorted(monster_codes):
        before, after = old.monster_regions(code), new.monster_regions(code)
        if before != after:
            region_changes[code] = {"before": before, "after": after}
    return {"from_version": from_version, "to_version": to_version, "region_changes": region_changes,
            "entries": [entry for table in ordered for entry in entries_for(old, new, table)]}


def read_version(path, conn, override):
    if override:
        return override
    if conn.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta'").fetchone():
        row = conn.execute("SELECT value FROM meta WHERE key='game_version'").fetchone()
        if row and row[0].strip():
            return row[0].strip()
    sidecar = path.with_name("client-version.txt")
    if sidecar.is_file():
        return sidecar.read_text(encoding="utf-8").strip()
    raise ValueError(f"Missing game version for {path}. Pass the version explicitly.")


def git_text(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def git_db(ref, directory, index):
    path = directory / f"snapshot-{index}.db"
    with path.open("wb") as dest:
        subprocess.run(["git", "show", f"{ref}:data/gersang.db"], cwd=ROOT, stdout=dest, check=True)
    return path


def comparison(left, right):
    with ExitStack() as stack:
        snapshots = [Snapshot(stack.enter_context(closing(sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True))))
                     for path, _ in (left, right)]
        versions = [read_version(path, snap.conn, version)
                    for (path, version), snap in zip((left, right), snapshots)]
        return compare(*snapshots, *versions)


def history_snapshots(directory, current):
    # Keep the last catalog before each client's version changes. Importer
    # fixes within one game version belong to that version's final snapshot.
    if git_text("rev-parse", "--is-shallow-repository") == "true":
        raise ValueError("Complete Git history is required to rebuild the packaged game versions")
    revisions = git_text("log", "--first-parent", "--reverse", "--format=%H", "--", "data/client-version.txt").splitlines()
    last_version = None
    snapshots = []
    for revision in revisions:
        version = git_text("show", f"{revision}:data/client-version.txt")
        if last_version is not None and version != last_version:
            snapshots.append((git_db(revision + "^1", directory, len(snapshots)), last_version))
        last_version = version
    with closing(sqlite3.connect(current[0].resolve().as_uri() + "?mode=ro", uri=True)) as conn:
        current_version = read_version(current[0], conn, current[1])
    if last_version is not None and current_version != last_version:
        snapshots.append((git_db("HEAD", directory, len(snapshots)), last_version))
    snapshots.append((current[0], current_version))
    return snapshots


def attach_caption_changes(snapshots, releases):
    # A relation's label can come from unrelated rows (for example a monster
    # and the item in its drop slot). Keep only adjacent caption transitions
    # for rows that can actually occur in a comparison, and only if the row's
    # normal change record does not already carry the transition.
    if len(releases) < 2:
        return
    touched = defaultdict(set)
    for release in releases:
        for entry in release["entries"]:
            touched[entry["table"]].add(entry["key"])

    previous = {}
    loot_by_monster = {}
    for index, (path, _) in enumerate(snapshots):
        current = {}
        with closing(sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True)) as conn:
            snapshot = Snapshot(conn)
            for table, wanted in touched.items():
                for key, values in snapshot.rows(table):
                    label = display_key(key)
                    if label in wanted:
                        drop = snapshot.drop_snapshot(key[0], values) if table == "monster_drops" else None
                        current[table, label] = (snapshot.name(table, key, values), drop)
            if index:
                release = releases[index - 1]
                introduced = {r["key"]: r["loot"] for r in release["entries"]
                              if r["table"] == "monsters" and r["kind"] == "added"}
                retired = {r["key"] for r in release["entries"] if r["table"] == "monsters" and r["kind"] == "removed"}
                # This list is shown for an added monster even if the chosen
                # target is a later release. Only archive changes to rosters
                # of monsters introduced in recorded history, including names.
                updates = {}
                for code in sorted(loot_by_monster.keys() - retired - introduced.keys()):
                    slots = snapshot.loot(code)
                    if slots != loot_by_monster[code]:
                        updates[code] = slots
                if updates:
                    release["loot_updates"] = updates
                for code in retired:
                    loot_by_monster.pop(code, None)
                loot_by_monster.update(updates)
                loot_by_monster.update(introduced)
        if index:
            release = releases[index - 1]
            data_changes = {(e["table"], e["key"]) for e in release["entries"]}
            changes = []
            for k in sorted((previous.keys() & current.keys()) - data_changes):
                if previous[k] == current[k]:
                    continue
                (old_name, old_drop), (new_name, new_drop) = previous[k], current[k]
                change = {"table": k[0], "key": k[1], "before": old_name, "after": new_name}
                if new_drop is not None:
                    change.update(drop_before=old_drop, drop_after=new_drop)
                changes.append(change)
            if changes:
                release["caption_changes"] = changes
        previous = current


def write_history(output_path, versions, releases):
    # Keep each changed record on one line for Git review; replace only after
    # successful generation. Existing packaged history survives a failed run.
    compact = dict(ensure_ascii=False, separators=(",", ":"))
    with tempfile.TemporaryDirectory(dir=output_path.parent) as directory:
        pending = Path(directory) / "catalog-diff.json"
        with pending.open("w", encoding="utf-8", newline="\n") as output:
            output.write('{\n  "versions": ' + json.dumps(versions, **compact) + ',\n  "releases": [')
            for i, release in enumerate(releases):
                output.write((',' if i else '') + '\n    {"from_version":' + json.dumps(release["from_version"]) +
                             ',"to_version":' + json.dumps(release["to_version"]) +
                             ',"region_changes":' + json.dumps(release["region_changes"], **compact))
                if "caption_changes" in release:
                    output.write(',"caption_changes":' + json.dumps(release["caption_changes"], **compact))
                if "loot_updates" in release:
                    output.write(',"loot_updates":' + json.dumps(release["loot_updates"], **compact))
                output.write(',"entries":[\n')
                for j, row in enumerate(release["entries"]):
                    output.write((',\n' if j else '') + "      " + json.dumps(row, **compact))
                output.write("\n    ]}")
            output.write("\n  ]\n}\n")
        pending.replace(output_path)


def name_index(versions, releases):
    entities = defaultdict(list)
    introduced, active = {}, {}
    for version, release in enumerate(releases, start=1):
        for row in release["entries"]:
            table = row["table"]
            if table not in NAME_ROUTES:
                continue
            key = row["key"].replace(" / ", ":", 1) if table == "quests" else row["key"]
            route = f"{NAME_ROUTES[table]}:{key}"
            if row["kind"] == "added":
                introduced[route] = version
                continue
            if row["kind"] == "removed":
                if route in active:
                    active.pop(route)["until"] = version
                introduced.pop(route, None)
                continue
            changes = []
            for lang in ("zh", "en", "ko"):
                field = "title" if table == "quests" and lang == "ko" else f"name_{lang}"
                if field not in row["before"] and field not in row["after"]:
                    continue
                names = [side.get(field) for side in (row["before"], row["after"])]
                before, after = (s if isinstance(s, str) and s.strip() not in ("", "0") else "" for s in names)
                if before != after:
                    changes.append({"lang": lang, "before": before, "after": after})
            # A translation being filled for the first time has no old name
            # to find. When another name changes, retain all language changes
            # on that record, including a translation becoming unavailable.
            if not any(c["before"] for c in changes):
                continue
            if route not in active:
                span = {"since": introduced.get(route, 0), "events": []}
                active[route] = span
                entities[route].append(span)
            active[route]["events"].append({"version": version, "changes": changes})
    return {"versions": versions, "entities": dict(sorted(entities.items()))}


def write_name_index(path, versions, releases):
    with tempfile.TemporaryDirectory(dir=path.parent) as directory:
        pending = Path(directory) / "names.json"
        pending.write_text(json.dumps(name_index(versions, releases), ensure_ascii=False,
                                      separators=(",", ":")) + "\n", encoding="utf-8")
        pending.replace(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    baseline = parser.add_mutually_exclusive_group()
    baseline.add_argument("--from-ref", help="Export a single comparison with this Git revision")
    baseline.add_argument("--from-db", type=Path, help="Export a single comparison with this SQLite file")
    parser.add_argument("--from-version")
    parser.add_argument("--to-db", type=Path, default=ROOT / "data/gersang.db")
    parser.add_argument("--to-version")
    parser.add_argument("--output", type=Path, default=ROOT / "data/catalog-diff.json")
    parser.add_argument("--names-output", type=Path, help="Also export the compact catalog name history")
    args = parser.parse_args()
    if args.names_output is None and args.output.resolve() == ROOT / "data/catalog-diff.json":
        args.names_output = ROOT / "data/catalog-names.json"
    outputs = {p.resolve() for p in (args.output, args.names_output) if p is not None}
    if args.names_output is not None and len(outputs) != 2:
        parser.error("--output and --names-output must be distinct files")
    if any(path is not None and path.resolve() in outputs for path in (args.from_db, args.to_db)):
        parser.error("--output must be different from both input databases")
    with tempfile.TemporaryDirectory() as temp:
        directory = Path(temp)
        current = args.to_db, args.to_version
        if args.from_ref:
            version = args.from_version or git_text("show", f"{args.from_ref}:data/client-version.txt")
            snapshots = [(git_db(args.from_ref, directory, 0), version), current]
        elif args.from_db:
            snapshots = [(args.from_db, args.from_version), current]
        else:
            snapshots = history_snapshots(directory, current)
        releases = [comparison(left, right) for left, right in zip(snapshots, snapshots[1:])]
        versions = [r["from_version"] for r in releases] + [releases[-1]["to_version"] if releases else snapshots[-1][1]]
        if len(versions) == 2 and versions[0] == versions[1] and not releases[0]["entries"]:
            versions, releases = versions[:1], []
        if len(set(versions)) != len(versions):
            parser.error("Changed snapshots share a version label. Use --from-version and --to-version for a development comparison.")
        attach_caption_changes(snapshots, releases)
        write_history(args.output, versions, releases)
        if args.names_output:
            write_name_index(args.names_output, versions, releases)
    print(f"{len(versions)} versions, {sum(len(r['entries']) for r in releases)} changes across adjacent versions")


if __name__ == "__main__":
    main()
