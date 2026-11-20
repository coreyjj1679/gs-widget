#!/usr/bin/env python3
"""Refresh main quest dialogue, repeat rules and quest types from the matching client archive."""
import argparse
from contextlib import closing
from pathlib import Path

from import_enchant import ROOT, client_texts, connect, file_sha256


# V3.4411 expanded the group/category fields. Its lowest-level category text ID
# takes the place of the former Subject, immediately before the opening dialogue.
DIALOGUE = {
    "subject": ("Subject", "소분류(GTS)"),
    **{f"start-{i}": (f"Start Text {i}",) for i in range(1, 7)},
    **{f"middle-{i}": (f"Middle Text {i}",) for i in range(1, 3)},
    "mission": ("임무 내용",),
    **{f"end-{i}": (f"End Text {i}",) for i in range(1, 7)},
    "explain": ("Explanation",),
}


def one_column(columns, label):
    if len(columns) != 1:
        raise ValueError(f"Expected one main quest column for {label}, got {len(columns)}")
    return columns[0]


def parse_main(text):
    lines = text.splitlines()
    start = one_column([i for i, line in enumerate(lines) if line.startswith("#Number\t")
                        and "End Type1" in line and "Need Quest" in line], "main quest header")
    header = [cell.strip() for cell in lines[start].split("\t")]
    def prefixed(prefix):
        return one_column([i for i, col in enumerate(header) if col.startswith(prefix)], prefix)
    repeat, kind = prefixed("RepeatType("), prefixed("퀘스트타입(")
    dialogue = {role: one_column([i for i, col in enumerate(header) if col in names], role)
                for role, names in DIALOGUE.items()}
    quests = {}
    for line in lines[start + 1:]:
        if not line.strip() or line.lstrip().startswith((";", "#")):
            continue
        cells = [cell.strip() for cell in line.split("\t")]
        code = cells[0]
        if not code.isdecimal() or code in quests or len(cells) != len(header):
            raise ValueError(f"Invalid or duplicate row in main quest table: {code!r}")
        if not cells[kind].isdecimal() or not cells[repeat].isdecimal():
            raise ValueError(f"Invalid repeat/type for main quest {code}")
        refs = {}
        for role, column in dialogue.items():
            value = cells[column]
            if value and value != "0":
                if not value.isdecimal():
                    raise ValueError(f"Invalid {role} text ID for main quest {code}: {value!r}")
                refs[role] = int(value)
        quests[code] = cells[kind], cells[repeat], refs
    return quests


def refresh(conn, quests, archive_hash):
    with conn:
        conn.execute("BEGIN IMMEDIATE")
        source = conn.execute("SELECT sha256 FROM sources WHERE role='gersang.gcs' "
                              "ORDER BY revision_id DESC LIMIT 1").fetchone()
        if source != (archive_hash,):
            raise ValueError("The client archive hash differs from the catalog's recorded source")
        target = {code: (key, kind, repeat) for key, code, kind, repeat in conn.execute(
            "SELECT id, code, kind, repeat_count FROM quests WHERE source='main'")}
        if not quests or quests.keys() != target.keys():
            raise ValueError("Client and catalog have different main quest codes")
        for code, (kind, repeat, refs) in quests.items():
            key, old_kind, old_repeat = target[code]
            if (kind, repeat) != (old_kind, old_repeat):
                conn.execute("UPDATE quests SET kind=?, repeat_count=? WHERE id=?", (kind, repeat, key))
            current = set(conn.execute(
                "SELECT role,slot,gts_id,text_object FROM quest_lines WHERE quest_id=?", (key,)))
            desired = {(role, 1, ref, "QuestText.dat") for role, ref in refs.items()}
            if current != desired:
                conn.execute("DELETE FROM quest_lines WHERE quest_id=?", (key,))
                conn.executemany("INSERT INTO quest_lines(quest_id,role,slot,gts_id,text_object) "
                                 "VALUES(?,?,?,?,?)", ((key, *row) for row in sorted(desired)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", type=Path, default=ROOT / "data/gcs")
    parser.add_argument("--target", type=Path, default=ROOT / "data/gersang.db")
    parser.add_argument("--parser-root", type=Path)
    args = parser.parse_args()
    texts = [text for text in client_texts(args.client, parser_root=args.parser_root)
             if any(line.startswith("#Number\t") and "End Type1" in line and "Need Quest" in line
                    for line in text.splitlines())]
    quests = parse_main(one_column(texts, "main quest table"))
    with closing(connect(args.target)) as conn:
        refresh(conn, quests, file_sha256(args.client / "gersang.gcs"))
    print(f"{len(quests)} main quests refreshed from {args.client / 'gersang.gcs'}")


if __name__ == "__main__":
    main()
