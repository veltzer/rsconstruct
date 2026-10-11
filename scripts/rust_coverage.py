#!/usr/bin/env python
"""How much of the fleet's toolchain is Rust.

Reads `rsconstruct --json processor list` and every rsconstruct.toml under
the fleet directory (default ~/git), and prints the two tables plan.md's
Stage 0 asks for:

1. the coverage line: Rust processors out of all, and processor
   declarations in the fleet served by a Rust processor;
2. every non-Rust processor the fleet still declares, by how many repos
   declare it, with the Rust processors that can replace it.

Usage: rust_coverage.py [FLEET_DIR]
"""

import glob
import json
import os
import re
import subprocess
import sys


def main() -> None:
    fleet = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~/git")
    entries = json.loads(
        subprocess.check_output(["rsconstruct", "--json", "processor", "list"])
    )
    by_name = {e["name"]: e for e in entries}

    # Headers are processor.<type>.<name>[.<instance>]; the processor is the
    # first three segments.
    usage: dict[str, set[str]] = {}
    for path in sorted(glob.glob(os.path.join(fleet, "*", "rsconstruct.toml"))):
        repo = os.path.basename(os.path.dirname(path))
        with open(path, encoding="utf-8") as f:
            text = f.read()
        for m in re.finditer(r"^\[processor\.([a-z_]+\.[a-z0-9_]+)", text, re.M):
            usage.setdefault("processor." + m.group(1), set()).add(repo)

    rust = {name for name, e in by_name.items() if e["rust"]}
    total = sum(len(repos) for repos in usage.values())
    served = sum(len(repos) for name, repos in usage.items() if name in rust)
    percent = 100 * served // total if total else 0
    print(
        f"processors: {len(rust)}/{len(entries)} rust; "
        f"declarations: {served}/{total} ({percent} %) served by rust"
    )
    print()

    rows = sorted(
        (
            (len(repos), name)
            for name, repos in usage.items()
            if name in by_name and name not in rust
        ),
        key=lambda r: (-r[0], r[1]),
    )
    width = max((len(name) for _, name in rows), default=0)
    for count, name in rows:
        alternatives = ", ".join(by_name[name].get("rust_alternatives", []))
        print(f"{name:{width}} {count:4} repos  {alternatives}")

    unknown = sorted(name for name in usage if name not in by_name)
    if unknown:
        print()
        print("declared but not built in (lua plugins or typos): " + ", ".join(unknown))


if __name__ == "__main__":
    main()
