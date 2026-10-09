#!/usr/bin/env python3
"""Generate src/tidy/tables.json from the HTML Tidy sources.

itidy (processor.checker.itidy) is a port of HTML Tidy 5.8.0; its tag,
attribute, attribute-version and entity tables are data, not code, so they
are extracted from tidy's C sources rather than retyped. The C tables are
built from nested macros (version bit sets, INCLUDE_ARIA, ...), so the
sources are run through the C preprocessor first and the expanded
initializers are parsed.

Usage: gen-itidy-tables.py <tidy-html5 checkout> > src/tidy/tables.json
"""

import json
import re
import subprocess
import sys
from pathlib import Path


def preprocess(root: Path, name: str) -> str:
    cmd = [
        "gcc", "-E", "-P",
        f"-I{root / 'include'}", f"-I{root / 'src'}",
        str(root / "src" / f"{name}.c"),
    ]
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def bits(expr: str) -> int:
    """Evaluate a preprocessed C bit expression such as (1u|2u|(1 << 4))."""
    cleaned = re.sub(r"(\d)u\b", r"\1", expr)
    if not re.fullmatch(r"[\d\s()|<]+", cleaned):
        raise ValueError(f"unexpected bit expression: {expr!r}")
    return eval(cleaned)  # noqa: S307 - digits and operators only, checked above


def initializer_rows(text: str, name: str) -> list[str]:
    """The `{ ... }` rows of the C array initializer called `name`."""
    start = text.index(f"{name}[] =")
    body = text[text.index("{", start) + 1:]
    depth, rows, row, started = 0, [], [], False
    for ch in body:
        if ch == "{":
            depth += 1
            started = True
            if depth == 1:
                row = []
                continue
        if ch == "}":
            depth -= 1
            if depth == 0:
                rows.append("".join(row))
                started = False
                continue
            if depth < 0:
                break
        if started and depth >= 1:
            row.append(ch)
    return rows


def split_fields(row: str) -> list[str]:
    fields, depth, cur = [], 0, []
    for ch in row:
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        if ch == "," and depth == 0:
            fields.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
    tail = "".join(cur).strip()
    if tail:
        fields.append(tail)
    return fields


def strip_prefix(name: str, prefix: str) -> str:
    assert name.startswith(prefix), name
    return name[len(prefix):]


def main() -> None:
    root = Path(sys.argv[1])

    attr_rows = initializer_rows(preprocess(root, "attrs"), "attribute_defs ")
    attributes = []
    for row in attr_rows:
        ident, name, checker = split_fields(row)
        if name == "((void *)0)" or ident == "N_TIDY_ATTRIBS":
            continue
        checker = None if checker == "((void *)0)" else strip_prefix(checker, "prvTidy") if checker.startswith("prvTidy") else checker
        attributes.append({"id": strip_prefix(ident, "TidyAttr_"), "name": name.strip('"'), "check": checker})

    attrdict = preprocess(root, "attrdict")
    attrvers = {}
    for match in re.finditer(r"const AttrVersion prvTidyW3CAttrsFor_(\w+)\[\] =", attrdict):
        tag = match.group(1)
        rows = initializer_rows(attrdict[match.start():], f"prvTidyW3CAttrsFor_{tag}")
        entries = []
        for row in rows:
            ident, vers = split_fields(row)
            if ident == "TidyAttr_UNKNOWN":
                continue
            entries.append([strip_prefix(ident, "TidyAttr_"), bits(vers)])
        attrvers[tag] = entries

    tags = []
    for row in initializer_rows(preprocess(root, "tags"), "tag_defs"):
        fields = split_fields(row)
        ident, name, vers, attrs, model, parser, chk = fields
        if name == "((void *)0)":
            continue
        tags.append({
            "id": strip_prefix(ident, "TidyTag_"),
            "name": name.strip('"'),
            "versions": bits(vers),
            "attrvers": None if attrs == "((void *)0)" else re.fullmatch(r"&prvTidyW3CAttrsFor_(\w+)\[0\]", attrs).group(1),
            "model": bits(model),
            "parser": None if parser == "((void *)0)" else strip_prefix(parser, "prvTidy"),
            "check": None if chk == "((void *)0)" else chk,
        })

    entities = []
    for row in initializer_rows(preprocess(root, "entities"), "entities"):
        name, vers, code = split_fields(row)
        if name == "((void *)0)":
            continue
        entities.append([name.strip('"'), bits(vers), int(code)])

    json.dump(
        {"attributes": attributes, "attrvers": attrvers, "tags": tags, "entities": entities},
        sys.stdout,
        separators=(",", ":"),
    )
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
