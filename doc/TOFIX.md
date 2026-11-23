# To fix

## `rsconstruct clean all` fails on sandbox runs left by a killed build

Seen 2026-11-23 with rsconstruct 0.9.200, in book-openbook and
demos-ai-coding, after a build was killed mid-run (the session running it ran
out of memory):

```text
Removing build directories...
Error [EXIT_IO_ERROR]: Failed to remove .rsconstruct/sandbox: Permission denied (os error 13)
Exited with EXIT_IO_ERROR (5)
```

What was left: `.rsconstruct/sandbox/<pid>-<n>/work/work`, owned by the user
with mode `d---------`, one per run that was in flight. overlayfs creates its
`work/work` directory with mode 000, and a plain recursive delete cannot get
into it.

### Why `build` copes and `clean` does not

The sandbox already knows about this case:

- `src/sandbox.rs` `remove_run_dir` sets `work/work` to 0700 before
  `remove_dir_all`.
- `src/sandbox.rs` `remove_leftover_runs` uses it to clear runs that killed
  builds left behind, and `src/builder/build.rs` calls it when a build
  starts.

`rsconstruct clean all` goes another way: `src/builder/clean.rs` hands
`.rsconstruct` to `src/tool_store.rs` `clear_state_dir`, which calls plain
`fs::remove_dir_all` on every entry except the tool store, `sandbox/`
included. This also happens on the early-return path when there is no tools
directory, where it calls `remove_dir_all` on `.rsconstruct` as a whole. So
the cleanup that exists for exactly this situation is skipped, and the one
command a user reaches for to start over fails.

### Fix

Have `clear_state_dir` (both paths) call `crate::sandbox::remove_leftover_runs`
for `.rsconstruct/sandbox` before deleting the rest. The alternative is to
make the sandbox opening-up a general "remove a tree that may hold
overlayfs work dirs" helper that both use. Add a test like the existing
`remove_leftover_runs` tests in `src/sandbox.rs`: a fake run dir whose
`work/work` is chmod 000, then `clean all` must succeed and leave nothing
behind.

### Workaround until then

```bash
chmod -R u+rwx .rsconstruct/sandbox && rsconstruct clean all
```

Running `rsconstruct build` first also works, because it removes the
leftovers on start.
