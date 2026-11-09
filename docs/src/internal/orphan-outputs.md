# Orphan Outputs

An orphan output is a file an earlier build produced that no product in the
current build declares any more. A full build removes them. This is on by
default (`[build] remove_orphan_outputs = true`) and can be turned off; see
[Turning it off and on](#turning-it-off-and-on).

## Turning it off and on

It is one `[build]` key, set in any layer of the config chain (see
[User config](../configuration.md#user-config-configrsconstructconfigtoml)).
Later layers win, key by key:

1. built-in default: `true`;
2. `~/.config/rsconstruct/config.toml`: for every project you build on this
   machine;
3. `rsconstruct.toml`: for one project, everywhere it is built, CI included;
4. `rsconstruct.local.toml`: for one project, on this machine only.

To turn it off, keeping every output as earlier versions did:

```toml
[build]
remove_orphan_outputs = false
```

To turn it on again, set it to `true` or delete the line (the default is
`true`). A user-config `false` is overridden by a project that sets `true`, and
the other way round.

While it is off, builds neither remove nor record anything, and the record in
`.rsconstruct/outputs.json` stays as the last build with it on left it. The
first build after turning it back on compares against that record, so outputs
that stopped being produced while it was off are removed then (if unchanged
and not tracked by git). Delete `.rsconstruct/outputs.json` to start a new
record instead; the next build then removes nothing.

There is no command-line flag for it. To keep one build from removing
anything without changing the config, narrow it (`-p`, `-x`, a target): a
narrowed build never removes or records.

The first build with a version of rsconstruct that has this feature, or in a
fresh checkout (CI), has no record yet: it removes nothing and only records.

## The problem

A build only knows the products it has now. The files a product wrote stay on
disk after the product disappears, and nothing looks at them again. A product
disappears when:

- its source file is deleted or renamed;
- its processor is disabled or removed from `rsconstruct.toml`;
- a config change moves its outputs (`output_dir`, `formats`);
- a mass generator stops planning a file.

`rsconstruct clean outputs` does not help: it cleans the outputs of the products
in the current graph, and an orphan belongs to none of them. The case that
prompted this: when veltzer.github.io disabled two data types, the old JSON for
them stayed in `_site`, ready to be published to GitHub Pages, until it was
deleted by hand.

## How it works

The code is `src/orphans.rs`, called at the end of `Builder::build`.

1. At the end of every full build, rsconstruct records the output files the
   build is responsible for in `.rsconstruct/outputs.json`. For each output it
   stores the size and modification time the file has at that point.
2. The next full build compares the record with its own outputs. A recorded
   path that no product declares now is an orphan.
3. An orphan is removed only if it still has the recorded size and
   modification time and git does not track it. Directories the removals leave
   empty are removed too, up to the project root.
4. An orphan that changed since that build (edited by hand, or replaced by a
   hand-written file) or that git tracks (a generator was stopped, but its last
   output was kept as a source and committed) is kept. The build warns about it
   once, and it is dropped from the record.
5. The build reports every removal:
   `[build] removed 2 output(s) no longer produced: gen/a/one.txt, gen/b.txt`.

It runs after the products have executed, so a file this build reads stays in
place until the build is done. It does not run when the build stops on an
internal error or is interrupted. The record is written even when products
failed, because a failed product's outputs are simply not on disk to be
recorded.

### Guards

- **Narrowed builds.** A build narrowed with `-p`, `-x`, targets, `--iset` or
  `--pset` does not see every product, so to it everything else would look
  orphaned. Such a build neither removes nor records anything.
- **The first build** with no record (a fresh checkout, or the first build
  with this version) removes nothing and only writes the record.
- **Paths outside the project** (absolute, or containing `..`) are never
  recorded, so nothing outside the project can be removed.
- **git** is only asked (`git ls-files`) when there is an orphan to remove,
  so a normal build runs no extra process. Outside a git work tree nothing
  counts as tracked.

### Files inside `output_dirs`

An explicit stanza with `output_dirs` declares directories, not files. Its
files are the tree the object store recorded the last time the stanza was
built or restored, the same list the executor uses to clear a tree product's
stale files before it reruns (`last_tree_key`, `previous_tree_paths`). Those
files go into the record with the declared outputs. The tree leaves out files
other products put in the same directory, so a shared directory works too.
When the stanza goes away, its files are orphans like any others. A file
someone put into the directory by hand was never in the tree, so it stays, and
so does the directory holding it.

While the stanza stays, a file its tool stopped writing is not an orphan
here: the executor already removes the previous tree's files before each run.

Reading the trees costs one object-store lookup per such stanza. On
veltzer.github.io (120 files from `_site/logos`, 3160 recorded in all) the
orphans phase measured 14 ms steady state on 2026-11-09, in line with the
figure in "Cost" below.

### Not covered

- Outputs that were never in a record: those from before the first build with
  this version, or produced only while it was off.

## Why it is on by default

It was built off by default and measured first (below). The cost turned out
to be a few milliseconds even on the repositories with the most output files,
so it was switched on: a stale output is a correctness problem (it can be
published, or picked up by a later build), and the guards keep the removal to
files rsconstruct itself wrote and nobody has touched since.

## Cost

Measured on 2026-11-08 on a 16-core Linux machine with no-op builds (nothing to
rebuild), where the extra work weighs the most. The "orphans" phase time comes
from `--timings`. For book-openbook and teaching-slides it is the steady state
(three runs each, alternating off and on): the record was already present, so
each run loads it, compares, stats every output and writes the record again.
For the other three it is the first run, which only stats and writes; on
book-openbook the first run took 7 ms against 16 ms steady, so expect roughly
double for those in steady state.

| Repository        | Products | Output files | Orphans phase | No-op build (off) | No-op build (on) |
|-------------------|---------:|-------------:|--------------:|------------------:|-----------------:|
| book-openbook     |     5106 |         5069 |         16 ms |            1.19 s |           1.20 s |
| veltzer.github.io |     6032 |         3036 |          8 ms |                 — |                — |
| teaching-syllabi  |     4791 |         2049 |          4 ms |                 — |                — |
| teaching-slides   |    17983 |         1841 |          5 ms |            0.73 s |           0.71 s |
| demos-os-linux    |     6659 |         1783 |          3 ms |                 — |                — |

The cost grows with the number of output files, not with products: checkers
produce no files. book-openbook, which has the most output files in the fleet,
pays 16 ms, about 1.3% of a 1.2 s no-op build, and under 1 ms per 300 outputs.
On teaching-slides the difference is within the noise between runs. The record
for book-openbook is 489 KB. The time is the same on a build that does real
work, so there it is a smaller share still.

## Functional check

Run in a scratch clone of demos-lang-python with three extra tera templates:

1. Building created `gen/a/one.txt`, `gen/a/two.txt` and `gen/b.txt`.
2. After deleting the templates of `one.txt` and `b.txt`, the next build
   removed both and kept `gen/a/`, which still held `two.txt`.
3. After `two.txt` was edited by hand and its template deleted, the build kept
   it with a warning ("changed since they were built").
4. After the tera stanza was removed, the build kept the git-tracked
   `README.md` with a warning ("tracked by git").
5. The following no-op build and a `-p` build reported nothing.

`orphan_outputs_are_removed_by_a_full_build_only_when_enabled` in
`tests/tests_mod/build.rs` covers the full-build, `-p` and disabled cases,
and `orphan_files_in_output_dirs_are_removed` covers `output_dirs`;
the unit tests in `src/orphans.rs` cover the record and the guards.
