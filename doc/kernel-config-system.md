# Feature design: kernel-style config system

## Status

Draft — awaiting review. The user flagged this as "a big item" in
`problems.txt`. This doc unpacks what they likely mean and proposes a
phased shape.

## Origin

`problems.txt`:

> want a config system like the kernels config system.
> This is a big item.

## What the kernel's config system *is*

The Linux kernel has a layered config story most rsconstruct users
will know in outline:

1. **Kconfig files**: distributed across the source tree
   (`drivers/net/Kconfig`, `arch/x86/Kconfig`, etc.). Each declares
   *symbols* (`CONFIG_NETFILTER`), their type (bool, tristate, int,
   string), their default values, and **dependency expressions**
   (`depends on NET && X86`).
2. **`make menuconfig` / `make defconfig` / `make oldconfig`**: tools
   that read the Kconfig graph and let the user select symbols
   interactively, from a base file, or by reconciling an existing
   `.config` against a newer Kconfig graph.
3. **`.config`**: the resolved config — a flat `CONFIG_FOO=y` /
   `CONFIG_BAR=m` / `# CONFIG_BAZ is not set` file.
4. **`include/generated/autoconf.h`**: derived from `.config` and
   `#include`d everywhere; turns `CONFIG_FOO=y` into `#define
   CONFIG_FOO 1`.
5. **The build system** (`make`): reads `.config`, conditionally
   includes Kbuild fragments, conditionally compiles files based on
   `obj-$(CONFIG_FOO) += foo.o`.

The pieces that make this *the* kernel-style config system, as
opposed to "just a config file":

- **Distributed declarations** (each subsystem owns its symbols).
- **Typed symbols** with declared dependencies between them.
- **Dependency-aware UI** (menuconfig grays out unavailable options).
- **Reconciliation** (oldconfig keeps your choices when symbols
  appear / disappear).
- **Multiple stored configs** (`arch/x86/configs/x86_64_defconfig`).
- **Config drives compilation** (builds use the resolved config).

## What rsconstruct has today

A flat `rsconstruct.toml`:

```toml
[processor.ruff]
enabled = true
args = ["--target-version=py313"]

[processor.cc]
cflags = ["-O2", "-Wall"]
```

That is, today rsconstruct's config is shaped much more like
`Cargo.toml` or `pyproject.toml` than like a kernel `.config`. There
is no symbol declaration with types and dependencies; there is no
menuconfig-style UI; there is no notion of a "saved config preset"
(though see the [variants design doc](variants.md) for an adjacent
proposal).

## What "kernel-style" might mean for rsconstruct

The user's request is broad. There are at least four distinct things
they might want, and each has different implications:

### Reading 1 — `defconfig` presets

Save a complete `rsconstruct.toml` shape under a name; switch between
named presets:

```bash
rsconstruct defconfig minimal      # writes a minimal toml
rsconstruct defconfig debug        # writes a debug toml
rsconstruct defconfig release      # writes a release toml
```

Today the closest analogues are `rsconstruct smart enable-detected`
and the `[[profile]]` proposal in `doc/variants.md`. A real defconfig
system would let users name and version their full configs and
distribute them as files.

Cost: medium. Mostly a CLI feature plus a `configs/` directory
convention. No deep architecture change.

### Reading 2 — `menuconfig`-style interactive editor

A TUI that walks the user through every option:

```
[*] Enable ruff processor
    └── Target Python version: py313
[*] Enable cc processor
    └── Compiler flags: ...
[ ] Enable mypy processor
```

Cost: high. We'd need a TUI dependency (ratatui, cursive), a
declarative description of every config field (we already have
`KnownFields` and `field_descriptions`), and dependency rules that
say "ruff requires Python being configured".

Real value if there are >50 config knobs and users get lost. With
~20 processors at one section each, our "knob count" is small enough
that menuconfig is overkill. `processors config <name>` already shows
each processor's effective config.

### Reading 3 — `oldconfig`-style reconciliation

When rsconstruct adds a new processor with config fields, projects
that pin to an older version should be told "here are the new fields,
which value do you want?". Today `deny_unknown_fields` rejects
forward-compat fields; new fields don't surface in old projects until
the user updates.

Cost: low to medium. We have provenance tracking already (which fields
are defaults, which are user-set). A `rsconstruct config sync`
command that walks the rsconstruct.toml, lists newly-added fields with
their defaults, and asks the user to accept/edit each is a real
feature.

### Reading 4 — symbol dependency graph

The deepest interpretation: declare that `[processor.cpplint]`
*requires* `[processor.cc]` to be enabled, and let the config system
enforce this. Today such cross-processor dependencies live in code
(processors silently noop if their inputs don't exist).

Cost: high. Touches every processor's configuration story. Probably
overkill for rsconstruct's scope — projects that need cross-processor
coupling typically express it through directory layout and src_dirs,
not through symbol expressions.

## Recommendation

I don't think "fully kernel-style" is the right target. Three of the
four readings above are heavy features for relatively small gains.

What I'd actually ship, in priority order:

1. **Defconfig presets** (Reading 1). Small, useful, matches the
   variants-doc proposal of named build profiles.
   Concrete shape: `configs/<name>.toml` files in the project, plus
   `rsconstruct defconfig <name>` to copy one to `rsconstruct.toml`,
   plus `rsconstruct defconfig --save <name>` to write the current
   one out. Makes "we have a CI config, a debug config, and a
   release config" easy to manage.

2. **Field reconciliation** (Reading 3). When rsconstruct adds new
   config fields, `rsconstruct config sync` should report them and
   offer to add them with defaults. Right now the user has to read
   release notes.

Skip **menuconfig** (Reading 2) and **symbol dependencies** (Reading
4). They're real-kernel features but the cost/benefit isn't there for
a build tool with our knob count.

## Defconfig — concrete proposal

### Layout

A `configs/` directory at project root, gitignored or not at the
user's discretion:

```
configs/
├── ci.toml
├── dev.toml
└── release.toml
```

Each file is a complete `rsconstruct.toml` (same schema, same
validation).

### Commands

```bash
rsconstruct defconfig list                    # show available defconfigs
rsconstruct defconfig load <name>             # copy configs/<name>.toml to rsconstruct.toml
rsconstruct defconfig save <name>             # write current rsconstruct.toml to configs/<name>.toml
rsconstruct defconfig diff <name>             # diff current vs configs/<name>.toml
rsconstruct defconfig diff <a> <b>            # diff two named configs
```

`load` warns / errors if `rsconstruct.toml` has uncommitted changes
(use `git status` heuristic); add `--force` to override.

`save` warns if a `configs/<name>.toml` exists, requires `--force` to
overwrite.

### What this is NOT

- It is not a layered/inherited config (`extends = "ci"`). That's a
  more invasive feature.
- It is not a typed config system with menus.
- It is not a dependency graph between symbols.

It is the simplest thing that gives users *named, versioned configs*
they can switch between with one command.

### Estimated cost

~250 lines for the four subcommands plus integration tests. No new
deps. Schema reuse is 100% — `configs/X.toml` is just a regular
`rsconstruct.toml` parked elsewhere.

## Field reconciliation — concrete proposal

### Command

```bash
rsconstruct config sync
```

Walks the rsconstruct.toml, compares against the schema, identifies:

- New fields (rsconstruct knows about them, user doesn't have them).
  Emit each with its default value, ask "add to config? [y/N/edit]".
- Removed fields (user has them, rsconstruct no longer knows them).
  Emit each, ask "remove? [y/N]".
- Changed defaults (rsconstruct's default has changed; user is using
  the old default). Emit each with old → new, ask "update? [y/N]".

### Mechanism

The provenance system already tracks which fields are user-set vs
default-derived. We add a new query: "which fields exist in the
schema but are missing from the user's config?". Compare against the
processor's `KnownFields::known_fields()` list.

### Estimated cost

~150 lines. Reuses `KnownFields`, provenance tracking, and the
existing config-validation pipeline.

## Open questions

1. **Is the user actually asking for menuconfig-style?** "Like the
   kernel's" is broad. If they specifically want `make menuconfig`
   (Reading 2), that's a much bigger ship and I'd push back on it
   given our knob count. Tell me which reading matches your intent.

2. **Defconfig naming**: `configs/<name>.toml` vs
   `.rsconstruct/configs/<name>.toml`. The former is project-visible
   (likely the right answer — these are intended to be checked in);
   the latter hides them. Recommend the former.

3. **Defconfig vs variants**: the [variants doc](variants.md) proposes
   a `--profile` flag that selects per-processor config sections at
   runtime. Defconfig is "swap whole files". They're complementary
   but if we ship variants first, defconfig becomes "make defconfig
   that sets a default profile". If we ship defconfig first, variants
   becomes "swap fields without rewriting the file".

   I'd actually ship defconfig first — it's smaller, more useful
   immediately, and doesn't require the variant infrastructure
   decision.

4. **Reconciliation interactivity**: blocking prompts vs a non-
   interactive `--apply-defaults` mode? The kernel `oldconfig`
   blocks; that's annoying in CI. Default to non-interactive (just
   list the diffs and exit), with `--interactive` for the prompt
   flow. CI users can pipe to a parser.

## What I want from you before writing code

- Which reading (1, 2, 3, 4) actually matches what you want?
- If 1 or 3 (or both): ship in the order I proposed (defconfig
  first, reconciliation second), or different order?
- If 2 or 4: confirm the cost is worth it for our scope. I'll
  push back unless I'm convinced.
