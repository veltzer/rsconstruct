# Icheckpatch Processor

## Purpose

Checks C source files with the Linux kernel's [`checkpatch.pl`](https://docs.kernel.org/dev-tools/checkpatch.html) rules, in-process (no Perl script to download, no Perl). The Rust alternative to the [checkpatch](checkpatch.md) processor: a port of `checkpatch.pl` at kernel commit `e5f92606156a`, as `checkpatch.pl --no-tree -f FILE` runs it, so a file gets the same report, message for message, and passes or fails the way checkpatch would exit.

## How It Works

Each file goes through what `--no-tree -f` does to it: the file becomes the new-file diff checkpatch reads (`diff -u /dev/null FILE`, including its handling of a missing final newline and of binary files), its lines are sanitised as checkpatch sanitises them (comments and the insides of strings blanked), and every check that file mode reaches runs in checkpatch's order, with checkpatch's patterns. Those patterns are Perl regular expressions built from each other (`$Type`, `$Declare`, `$FuncArg`, ...) and rebuilt while a file is read, as checkpatch learns the file's own type names: the engine carries a Perl-compatible matcher and interpolates the patterns the way Perl does, so they match what they match in Perl.

Any ERROR, WARNING or CHECK fails the product, with checkpatch's report as the message:

```text
ERROR: open brace '{' following function definitions go on the next line
#2: FILE: src/bad.c:2:
+int f(void) {

WARNING: space prohibited before semicolon
#3: FILE: src/bad.c:3:
+    return 0 ;

total: 3 errors, 2 warnings, 4 lines checked
```

As in checkpatch, CHECKs (its `--strict` messages) are reported only for files under `net/`, `drivers/net/` and `drivers/staging/`, and long lines are a CHECK in file mode.

```toml
[processor.checker.icheckpatch]
src_dirs = ["src"]
```

### Fidelity

Checked against `checkpatch.pl` (the pinned commit) file by file over the fleet's 5666 C files, a 12005-file sample of the Linux 7.1.4 tree (C, headers, assembler, device trees, Kconfig, shell, `MAINTAINERS`, run from the tree's root so the path-dependent checks apply) and 17117 mutated files: identical output, byte for byte, and the same verdict. That includes checkpatch dying: a macro argument that makes one of its interpolated patterns invalid kills checkpatch, and fails the file here. It also includes checkpatch's slow corners: one mutant (`static ##inline int f(void)`) sends checkpatch's patterns into exponential backtracking, and the port, matching the same patterns the same way, takes as long (that is, forever).

The engine's unit tests (`src/engines/checkpatch/tests.rs`) keep it that way: they run `checkpatch.pl` itself (a missing one fails them) over the files in `src/engines/checkpatch/testdata/` and deterministic mutants of them, check the pattern variables against the ones checkpatch builds, and check the matcher against Perl.

Faster than checkpatch.pl: about twice as fast on ordinary sources (the fleet in half the time), as fast on machine-generated register headers, where both spend their time trying type patterns at every identifier.

What is not ported, because `--no-tree -f` cannot reach it or the processor does not use it: patch and commit-message checks, `--fix`, the options (each file is checked with checkpatch's defaults), `.checkpatch.conf`, spelling (`spelling.txt` is not installed with the script) and the checks that read a kernel tree. checkpatch's `CamelCase` CHECK seeds its word list from a git checkout's `include/*.h`; here it starts empty, as checkpatch does outside a git checkout. A file name the shell would split or reinterpret (a space, a quote, `$`, `;`, ...) makes checkpatch's `diff` fail and the file pass unchecked; the port does the same.

## Source Files

- Input: `.c` and `.h` files under `src_dirs`, or the files in `src_files`
- Output: none (checker)

## Configuration

No fields beyond the standard ones.

Switching a repo from checkpatch: `[processor.checker.checkpatch]` → `[processor.checker.icheckpatch]`.

## Batch support

Not used: each file is checked on its own, as `checkpatch.pl --no-tree -f FILE` checks it, in parallel as separate products. (checkpatch given several files carries state from one to the next, and stops the whole run at an empty one.)

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
