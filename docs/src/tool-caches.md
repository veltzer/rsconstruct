# Tool Caches

rsconstruct keeps two machine-wide caches for the tools it installs, both
shared by every project on the machine. Neither decides *which* version of a
tool a project gets; that is always [`rsconstruct.lock`](tool-lock.md)'s.
They only avoid fetching and unpacking the same release twice.

| Cache | Default location | Override | Holds |
|---|---|---|---|
| Tool store | `~/.cache/rsconstruct/tools` | `$RSCONSTRUCT_TOOL_STORE` | Installed tools, one directory per version |
| Download cache | `~/.cache/rsconstruct/downloads` | `$RSCONSTRUCT_DOWNLOAD_CACHE` | Release assets as downloaded, one file per URL |

When the override is unset and `$XDG_CACHE_HOME` is set, both live under
`$XDG_CACHE_HOME/rsconstruct/` instead of `~/.cache/rsconstruct/`.

Neither is touched by `rsconstruct cache clear` or `clean all`, which handle
build state, not tools.

## The tool store

**What it holds.** One install prefix per tool version, for the tools
rsconstruct installs into projects: crates (`cargo`), npm packages, gems and
the release binaries of the `binary` method. The layout is:

```text
~/.cache/rsconstruct/tools/<method>/<name>@<version>/
```

A release binary's version is the SHA-256 of its executable
(`binary/uv@sha256-2380...`), so two releases of a tool sit side by side. Each
entry is installed into a private temporary directory and renamed into place
once complete, so an interrupted install never leaves a half-written entry.

**How projects use it.** `tool install project` gives each project exactly the
versions its lock names, in `.rsconstruct/tools/pkgs/`, as hard links to the
store's files (copies when the project is on another filesystem). A version is
therefore installed once per machine, however many projects lock it. Store
files are made read-only, because a hard link shares the file: a tool writing
to its own install would change every project's copy.

**What deleting it costs.** Projects keep working: their tools are hard links,
which survive the store's copy being deleted. The next `tool install project`
in a project that lacks some version installs that version into the store
again (from the download cache when it is there). Deleting is always safe;
the directories are writable, so `rm -rf ~/.cache/rsconstruct/tools` works
despite the read-only files.

**In CI.** The Rust repositories' `ci.yml` caches
`~/.cache/rsconstruct/tools` between runs, keyed on `rsconstruct.lock`: a hit
is exactly the locked set, and after a lock change the previous store is
restored and only what moved is installed. The other repositories' `build.yml`
caches the project's `.rsconstruct` directory instead, which includes the
project's copies of its tools.

**Images.** Set `RSCONSTRUCT_TOOL_STORE` where tools are baked into a
container image and a job may run there with another `$HOME`, so that the job
still finds them.

## The download cache

**What it holds.** The release assets of the `binary` method exactly as
downloaded (the tarball, zip, raw executable or `.deb`), one file per URL,
named by a hash of the URL and the asset's own name
(`3f9c0e1a2b4d5e6f-zizmor-x86_64-unknown-linux-gnu.tar.gz`). Beside each
asset is its pin record (the same name with `.pin`): the lock entry the
asset makes, that is its SHA-256 and the installed executable's (or a
`.deb`'s package and version). `tool lock` writes the record when it
downloads an asset, and an install writes it once both hashes have
passed.

**Who uses it.** Every download of a release binary goes through it: `tool
lock` (which downloads each binary to record the hashes the lock holds),
`tool install project` (filling the tool store) and `tool install machine`.
Each looks in the cache before the network and adds what it downloads. In a
fleet of repositories, `tool lock --upgrade` downloads a new release once, in
the first repository, and reads it from the cache in every other one: on a
repository locking six binaries, a lock with a cold cache took 44 seconds and
one with a warm cache 3.7. `tool lock` reads a release's pin record first,
when there is one, and then needs neither the asset nor the network beyond
finding which release is the latest.

**What is never cached.** Only a URL that names one release is cached, which
is every URL `rsconstruct.lock` records. A `releases/latest` URL serves
whatever was released last, and a vendor URL that always serves the current
build (Chrome's `.deb`) changes under the same name, so both are always
fetched.

**Why a cached copy can be trusted.** It is hashed exactly like a fresh
download. An install compares it with the SHA-256 the lock records; a copy
that differs is deleted and the asset fetched again, and the install fails
only if the server's bytes differ too. Entries are written beside their final
name and renamed into place, so a concurrent reader (several projects
installing at once) sees a whole file or none.

**Why it is not inside the tool store.** CI caches the store; the archives the
store's installs were unpacked from would only double what that cache
carries.

**What deleting an entry costs.** Nothing but the next download of that asset.
A pin record is trusted as it is, like the asset it was made from: an
install still checks every download against the lock, so a wrong record
can only make an install fail, never install the wrong bytes.
Tests point it at a directory of their own, so a test run never fills the
user's cache.

## Size and pruning

Nothing is removed automatically: every version installed and every asset
downloaded stays until it is pruned. Each new release of a tool a project
locks adds one store entry and one download, so both caches grow with every
lock upgrade. `rsconstruct tool cache` looks after them:

```bash
rsconstruct tool cache list                     # every entry, its size and when it was last used
rsconstruct tool cache prune --older-than 60d   # remove what was not used in 60 days
rsconstruct tool cache prune --older-than 60d --dry-run   # list it, remove nothing
rsconstruct tool cache prune --unreferenced-under ~/git   # remove what no project there uses
rsconstruct tool cache prune --installed-archives   # remove archives the store holds
rsconstruct tool cache clear                    # remove everything in both caches
```

`prune` removes the entries that meet every criterion it is given, so
`--unreferenced-under ~/git --older-than 30d` removes only what no project
uses and nothing has used for a month:

- `--older-than N`d: last used more than N days ago (see below).
- `--unreferenced-under DIR` (repeatable): used by no project under DIR.
  A project is a directory with an `rsconstruct.lock`; hidden directories,
  `node_modules` and `target` are not searched. A store entry is used when
  the project's tool directory links it or its lock names it, and an asset
  or pin record when the lock names its URL. This is the criterion that
  frees disk space: an entry no project links has no other copy of its
  files.
- `--installed-archives`: a downloaded asset whose pin record names an
  executable the tool store already holds. Its record stays, so `tool
  lock` still needs no download; only an install after the store entry is
  gone downloads the asset again. An asset without a record (downloaded
  before records were kept) and a `.deb`, which is not installed into the
  store, stay.

`--json` gives each of them as JSON. A download cache entry is named after the
asset it holds (`<hash of the URL>-<asset name>`), so the list says what each
one is.

**What "last used" means.** A store entry is used when a project links it
(`tool install project` in a project that lacks that version); a download
when `tool lock` or an install is served from it. Running a tool does not
count: once a project has its copy, it never goes back to the store. So
`prune --older-than` can remove a store entry that projects still run.
That is harmless, since a project's tools are hard links that keep their
files, but the disk space of such an entry is only freed once no project
links its files any more, and a project that later needs the version again
installs it anew. The sizes `list` shows count each entry's files, whether or
not a project shares them.

Do not prune or clear while an install is running: an entry removed half-way
through being linked fails that install, which then has to be run again.
Everything else about pruning is safe at any time.
