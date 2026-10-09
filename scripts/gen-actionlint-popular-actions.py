#!/usr/bin/env python3
"""Convert actionlint's popular_actions.go into src/engines/actionlint/popular_actions.json.

iactionlint (src/engines/actionlint/) is a port of actionlint and checks `uses:` of
well-known actions against the same data set actionlint ships: the inputs
and outputs of each popular action, and the specs whose runner is too old.
actionlint generates that data into a Go source file; this script turns the
Go file into the JSON iactionlint embeds, so the two tools agree.

usage: gen-actionlint-popular-actions.py path/to/actionlint/popular_actions.go

Writes src/engines/actionlint/popular_actions.json next to this repository's src/.
Re-run it when moving iactionlint to a newer actionlint release.
"""
import json
import re
import sys
from pathlib import Path

ENTRY = re.compile(r'^\t"(?P<spec>[^"]+)": \{$')
NAME = re.compile(r'^\t\tName:\s+"(?P<name>[^"]*)",$')
INPUT = re.compile(
    r'^\t\t\t"(?P<id>[^"]+)":\s+\{"(?P<name>[^"]*)", (?P<req>true|false), (?P<dep>true|false), "(?P<msg>(?:[^"\\]|\\.)*)"\},$'
)
OUTPUT = re.compile(r'^\t\t\t"(?P<id>[^"]+)":\s+\{"(?P<name>[^"]*)"\},$')
OUTDATED = re.compile(r'^\t"(?P<spec>[^"]+)":\s+\{\},$')


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    lines = Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
    actions: dict[str, dict] = {}
    outdated: list[str] = []
    cur = None
    section = None  # "inputs" / "outputs"
    in_outdated = False
    for line in lines:
        if line.startswith("var OutdatedPopularActionSpecs"):
            in_outdated = True
            continue
        if in_outdated:
            m = OUTDATED.match(line)
            if m:
                outdated.append(m["spec"])
            elif line.startswith("}"):
                in_outdated = False
            continue
        m = ENTRY.match(line)
        if m:
            cur = {"name": "", "inputs": {}, "outputs": {}, "skip_inputs": False, "skip_outputs": False}
            actions[m["spec"]] = cur
            section = None
            continue
        if cur is None:
            continue
        m = NAME.match(line)
        if m:
            cur["name"] = m["name"]
            continue
        if line.startswith("\t\tInputs:"):
            section = "inputs"
            continue
        if line.startswith("\t\tOutputs:"):
            section = "outputs"
            continue
        if line.startswith("\t\tSkipInputs:"):
            cur["skip_inputs"] = "true" in line
            continue
        if line.startswith("\t\tSkipOutputs:"):
            cur["skip_outputs"] = "true" in line
            continue
        if section == "inputs":
            m = INPUT.match(line)
            if m:
                cur["inputs"][m["id"]] = {
                    "name": m["name"],
                    "required": m["req"] == "true",
                    "deprecated": m["dep"] == "true",
                    "deprecation_message": json.loads('"' + m["msg"] + '"'),
                }
                continue
        if section == "outputs":
            m = OUTPUT.match(line)
            if m:
                cur["outputs"][m["id"]] = m["name"]
                continue
        if line == "\t\t},":
            section = None
        if line == "\t},":
            cur = None
    out = Path(__file__).resolve().parent.parent / "src" / "engines" / "actionlint" / "popular_actions.json"
    data = {"actions": actions, "outdated": sorted(outdated)}
    out.write_text(json.dumps(data, separators=(",", ":"), sort_keys=True) + "\n")
    print(f"{len(actions)} actions, {len(outdated)} outdated specs -> {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
