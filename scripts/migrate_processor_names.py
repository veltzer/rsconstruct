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
* rewrites references to a *default* output directory that moved. A
  processor whose default ``output_dir`` is now ``out/processor.<type>.<name>``
  used to default to ``out/<name>`` (``out/generator`` for the generic
  generator), and a named instance to ``out/<name>.<instance>``; a few
  processors carry such a path in code (``out/cc``, ``out/linux-module``,
  ``out/gem``, ``out/tags``). Only those paths are rewritten, wherever they
  occur in the file -- a downstream ``src_dirs = ["out/marp"]`` follows the
  output it points at. A path under ``out/`` that a config chose for itself
  (``out/tera/books`` when tera writes in place) is left alone;
* lists, without changing them, the other tracked files of that repository
  that still mention a moved path, because a Makefile or a CI step pointing
  into ``out/`` has to be fixed by hand.

Names, types and default output directories are all asked of the installed
rsconstruct (``processor list --json`` and ``processor defconfig --json``),
so the binary on PATH must already speak full names. A name the binary does
not know is left as it is and reported; the build then fails on it with a
hint, which is the honest outcome for a plugin or a typo.

The script is idempotent: a header that already carries a type segment is
left alone and a moved path is never matched twice, so running it again
changes nothing.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

GENERIC_TYPES = {"creator", "generator", "explicit", "mass_generator"}
# Output paths built in code rather than declared as an output_dir default,
# old path -> new path. `processor defconfig` cannot report these.
CODE_DEFAULTS = {
    "out/cc": "out/processor.creator.cc",
    "out/linux-module": "out/processor.creator.linux_module",
    "out/gem": "out/processor.creator.gem",
    "out/tags": "out/processor.generator.tags",
}


def rsconstruct_json(*args):
    try:
        out = subprocess.run(
            ["rsconstruct", "--json", *args],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError) as exc:
        sys.exit(f"cannot run `rsconstruct --json {' '.join(args)}`: {exc}")
    return json.loads(out)


def load_registry():
    """({short name: type}, {old out path: new out path}) from the binary."""
    types = {}
    moved = dict(CODE_DEFAULTS)
    for entry in rsconstruct_json("processor", "list"):
        full = entry["name"]
        if not full.startswith("processor."):
            sys.exit(
                f"this rsconstruct still reports short names ({full!r}); "
                "install one that speaks full names first"
            )
        _, ptype, short = full.split(".", 2)
        if short != "generic":
            types[short] = ptype
        default_dir = rsconstruct_json("processor", "defconfig", full).get("output_dir") or ""
        if default_dir == f"out/{full}":
            # The old default was out/<old name>, and the old name of a
            # generic processor was its type.
            old_name = ptype if short == "generic" else short
            moved[f"out/{old_name}"] = default_dir
    return types, moved


class Migrator:
    def __init__(self, types, moved):
        self.types = types
        self.moved = moved
        self.unknown = set()
        # out/<old>, optionally followed by .<instance> (the old named-
        # instance default), ending at a path boundary. A new-style path
        # starts with out/processor. and can never match an old name.
        alternatives = "|".join(
            re.escape(old[len("out/"):]) for old in sorted(moved, key=len, reverse=True)
        )
        self.out_pattern = re.compile(
            r"out/(" + alternatives + r")((?:\.[A-Za-z_]\w*)?)(?![\w.-])"
        )

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
            return self.moved["out/" + match.group(1)] + match.group(2)

        return self.out_pattern.sub(sub, text)

    def migrate_file(self, path):
        old = path.read_text(encoding="utf-8")
        new = self.rewrite_out_dirs(self.rewrite_headers(old))
        if new != old:
            path.write_text(new, encoding="utf-8")
            return True
        return False

    def stray_out_dirs(self, repo):
        """Other tracked files in ``repo`` that mention a moved out/ path."""
        try:
            files = subprocess.run(
                ["git", "-C", str(repo), "ls-files"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.split()
        except (OSError, subprocess.CalledProcessError):
            return []
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
                if self.out_pattern.search(line):
                    hits.append(f"{p}:{lineno}: {line.strip()}")
        return hits


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    migrator = Migrator(*load_registry())
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
