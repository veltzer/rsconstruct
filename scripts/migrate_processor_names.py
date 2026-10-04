#!/usr/bin/env python3
"""Rewrite rsconstruct.toml files from short processor names to full names.

Until rsconstruct 0.9.x a processor was named by its bare name:
``[processor.ruff]``, ``[processor.pylint.core]``, ``[processor.creator.venv]``.
A processor is now named by its full path, ``processor.<type>.<name>``:
``[processor.checker.ruff]``, ``[processor.checker.pylint.core]``,
``[processor.creator.generic.venv]``. This script performs that rewrite on
the files it is given, which is every ``rsconstruct.toml`` of a fleet:

    python3 scripts/migrate_processor_names.py ~/git/*/rsconstruct.toml

For each file it

* rewrites every ``[processor.X...]`` and ``[[processor.X...]]`` header;
* rewrites ``out/<name>`` references to processors whose default output
  directory moved from ``out/<name>`` to ``out/processor.<type>.<name>``
  (the generators, plus cc, linux_module, gem and tags), wherever they occur
  in the file -- a downstream ``src_dirs = ["out/tera"]`` follows the output
  it points at;
* lists, without changing them, the other tracked files of that repository
  that still mention such an ``out/<name>`` path, because a Makefile or a CI
  step pointing into ``out/`` has to be fixed by hand.

Which type each name has is asked of the installed rsconstruct
(``rsconstruct processor list --json``), so the binary on PATH must already
speak full names. A name the binary does not know is left as it is and
reported; the build then fails on it with a hint, which is the honest
outcome for a plugin or a typo.

The script is idempotent: a header that already carries a type segment is
left alone, so running it twice changes nothing.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

GENERIC_TYPES = {"creator", "generator", "explicit", "mass_generator"}
# Processors whose default output_dir moved from out/<name> to
# out/processor.<type>.<name>. Every generator has such a default; these
# creators carry one in code.
OUT_DIR_CREATORS = {"cc", "linux_module", "gem", "tags"}


def load_name_map():
    """{short name: type} for every built-in processor, from the binary."""
    try:
        out = subprocess.run(
            ["rsconstruct", "--json", "processor", "list"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError) as exc:
        sys.exit(f"cannot run `rsconstruct --json processor list`: {exc}")
    types = {}
    for entry in json.loads(out):
        full = entry["name"]
        if not full.startswith("processor."):
            sys.exit(
                f"this rsconstruct still reports short names ({full!r}); "
                "install one that speaks full names first"
            )
        _, ptype, short = full.split(".", 2)
        if short != "generic":
            types[short] = ptype
    return types


class Migrator:
    def __init__(self, types):
        self.types = types
        self.out_moved = {
            name for name, t in types.items() if t == "generator"
        } | {name for name in OUT_DIR_CREATORS if name in types}
        self.unknown = set()

    def full_name(self, dotted):
        """Old header path after ``processor.`` -> new one, or None if unknown."""
        parts = dotted.split(".")
        head = parts[0]
        if head in GENERIC_TYPES:
            # `[processor.generator.X]` is ambiguous: old-style it is an
            # instance X of the generic generator, new-style X is a
            # generator's own name (`[processor.generator.tera]`). The
            # registry decides: a known name (or `generic`) means the header
            # is already new-style and must be left alone, which is what
            # makes a second run a no-op.
            second = parts[1] if len(parts) > 1 else ""
            if second == "generic" or self.types.get(second) == head:
                return None
            return ".".join([head, "generic"] + parts[1:])
        if head in ("checker", "lua"):
            # Already a type segment: the header is new-style.
            return None
        if head in self.types:
            return ".".join([self.types[head], head] + parts[1:])
        self.unknown.add(head)
        return None

    def rewrite_headers(self, text):
        def sub(match):
            new = self.full_name(match.group(2))
            if new is None:
                return match.group(0)
            close = "]]" if match.group(1) == "[[" else "]"
            return f"{match.group(1)}processor.{new}{close}"

        return re.sub(r"(\[\[?)processor\.([A-Za-z_][\w.-]*)\]\]?", sub, text)

    def rewrite_out_dirs(self, text):
        def sub(match):
            name = match.group(1)
            if name in self.out_moved:
                return f"out/processor.{self.types[name]}.{name}"
            return match.group(0)

        return re.sub(r"out/([A-Za-z_][\w]*)(?![\w.])", sub, text)

    def migrate_file(self, path):
        old = path.read_text(encoding="utf-8")
        new = self.rewrite_out_dirs(self.rewrite_headers(old))
        if new != old:
            path.write_text(new, encoding="utf-8")
            return True
        return False

    def stray_out_dirs(self, repo):
        """Other tracked files in ``repo`` that mention a moved out/<name>."""
        try:
            files = subprocess.run(
                ["git", "-C", str(repo), "ls-files"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.split()
        except (OSError, subprocess.CalledProcessError):
            return []
        pattern = re.compile(
            r"out/(" + "|".join(map(re.escape, sorted(self.out_moved))) + r")(?![\w.])"
        )
        hits = []
        for rel in files:
            if rel == "rsconstruct.toml":
                continue
            p = repo / rel
            try:
                text = p.read_text(encoding="utf-8")
            except (OSError, UnicodeDecodeError):
                continue
            for lineno, line in enumerate(text.splitlines(), 1):
                if pattern.search(line):
                    hits.append(f"{p}:{lineno}: {line.strip()}")
        return hits


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    migrator = Migrator(load_name_map())
    changed = 0
    for arg in argv[1:]:
        path = Path(arg)
        if migrator.migrate_file(path):
            changed += 1
            print(f"rewrote {path}")
        for hit in migrator.stray_out_dirs(path.parent):
            print(f"  review: {hit}")
    print(f"{changed} file(s) rewritten")
    if migrator.unknown:
        names = ", ".join(sorted(migrator.unknown))
        print(f"left unchanged (not a built-in processor): {names}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
