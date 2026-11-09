# Sandboxing Chromium

Four processors render through Chromium: `chromium` (google-chrome, HTML to
PDF), `marp` (marp-cli, slides), `mermaid` (mmdc, diagrams, via puppeteer)
and `drawio` (draw.io desktop, an Electron app). Until 2026-11-08 all four
were marked `sandboxable: false`, and a build that would run one under
`execution_model = "sandbox"` failed at the start with a config error. They
now run sandboxed. This page records why they failed, what was measured,
and what the processors do about it. For the sandbox itself, see
[Sandboxed Execution](sandbox.md).

## Why Chromium fails inside bwrap

Chromium sandboxes its own renderer processes. On Linux it prefers to do that
with an unprivileged user namespace, and falls back to a setuid helper
(`chrome-sandbox`) when it cannot create one.

Inside bwrap it cannot create one. Ubuntu restricts unprivileged user
namespaces through AppArmor (`kernel.apparmor_restrict_unprivileged_userns =
1`) and lets bwrap through with its own profile, `bwrap-userns-restrict`.
That profile stops processes inside bwrap from creating a user namespace of
their own. Chromium then tries the setuid helper, which cannot work inside
the sandbox either, and aborts:

```text
FATAL:sandbox/linux/suid/client/setuid_sandbox_host.cc:172] The SUID sandbox
helper binary was found, but is not configured correctly.
```

The tools built on Chromium report the same failure their own way: marp
prints "Failed converting Markdown. (Failed to launch the browser process",
mmdc prints "Error: Failed to launch the browser process", and drawio exits
with an error.

This is a property of the environment (Ubuntu's AppArmor setup), not of the
build: rsconstruct cannot change it. What it can do is tell Chromium not to
start its own sandbox.

## What was measured

On 2026-11-08, on Ubuntu with bubblewrap, each tool was run inside a bwrap
with the same shape as rsconstruct's (`--ro-bind / /`, writable `$HOME` and
`/tmp`, `--dev /dev`, `--proc /proc`, `--unshare-net`), once as is and once
with Chromium's own sandbox turned off:

| Tool | As is | With Chromium's sandbox off | How it is turned off |
|---|---|---|---|
| google-chrome | aborts (SUID sandbox helper) | works | `--no-sandbox` |
| marp | fails to launch the browser | works | env `CHROME_NO_SANDBOX=1` (read by marp-cli) |
| mmdc | fails to launch the browser | works | puppeteer config `{"args":["--no-sandbox"]}` passed with `-p` |
| drawio | fails | works | `--no-sandbox` |

Every "works" run produced a real output (a PDF or an SVG). Natively all
four work as is, with one exception noted below.

Then the processors themselves, built with the change below, ran in a
scratch project with one input each and `execution_model = "sandbox"`, through
rsconstruct's real sandbox (overlay included):

- All four built and their outputs reached the tree, with `-j 1` and `-j 4`
  and with each one left out in turn.
- Eight repeated forced builds with `-j 4`: all passed, about 1–2 s each.
- The very first build in that project had marp time out three times (20 s
  each, its `timeout_secs`) and fail; it did not happen again in the
  twelve builds that followed, nor when marp's exact bwrap command was run by
  hand. Not explained; noted here in case it returns.
- The same project with rsconstruct 0.9.158, where they were still marked
  `sandboxable: false`, failed at the start with the config error, as it
  should have.

## Is turning Chromium's sandbox off safe here?

Chromium's sandbox protects the machine from hostile web content. Here
Chromium renders the repository's own files, not the web. And while
sandboxed it runs with no network, a read-only filesystem except `$HOME` and
`/tmp`, and the project behind an overlay where anything not declared is
thrown away. bwrap covers the part that matters for a build. Natively nothing
changes: Chromium keeps its own sandbox, except in the `chromium` processor,
which has always passed `--no-sandbox`.

## What the processors do

`crate::sandbox::active()` tells a processor whether the product running on
this thread runs inside bwrap. That is true when the product runs sandboxed
(the executor sets the sandbox spec around each execute), and also when the
whole process is inside an outer sandbox: every sandboxed run sets
`RSCONSTRUCT_IN_SANDBOX=1`, which everything it starts inherits. The second
case was found on 2026-11-09, the first time rsconstruct's own gate ran
sandboxed. A sandboxed `cargo nextest` runs the integration tests. A test
starts rsconstruct, which builds a marp product natively, but still inside
the outer bwrap, and Chromium aborted exactly as in "Why Chromium fails
inside bwrap". Each processor turns Chromium's sandbox off only when
`active()` is true:

- **chromium** (`src/processor/generator/chromium.rs`): already passes
  `--no-sandbox` on every run, natively too, so it needed no code change, only
  `sandboxable: true`.
- **drawio** (`drawio.rs`): appends `--no-sandbox` when sandboxed.
- **marp** (`marp.rs`): sets `CHROME_NO_SANDBOX=1` in the tool's environment
  when sandboxed.
- **mermaid** (`mermaid.rs`): when sandboxed, writes
  `{"args":["--no-sandbox"]}` to a fresh file in the temporary directory
  (writable inside the sandbox, outside the project), passes it with `-p`,
  and removes it after the run. If the user's `args` already pass a puppeteer
  config (`-p`, `--puppeteerConfigFile`), that one is used as is and
  nothing is added: the user owns that choice.

All four are now `sandboxable: true`. The `sandboxable` flag and the up-front
config error (`sandbox::check_sandboxable`) stay for any future processor
that genuinely cannot run sandboxed; today none is marked that way.

## Found along the way: mmdc natively

Natively, mmdc fails on the same machine with:

```text
FATAL:content/browser/zygote_host/zygote_host_impl_linux.cc:128] No usable
sandbox! If you are running on Ubuntu 23.10+ or another Linux distro that has
disabled unprivileged user namespaces with AppArmor, ...
```

puppeteer's bundled Chrome is not the system google-chrome, so Ubuntu's
AppArmor does not let it create a user namespace even outside bwrap. This is
not caused by the sandbox and was not changed here: a project that runs
mermaid natively on Ubuntu 23.10 or newer has to pass a no-sandbox puppeteer
config through `args` itself, as web-assets did with its `.mmdc.config.json`
until it dropped mermaid.

## Fleet

No fleet repository configures any of the four processors today, so they
never blocked the sandbox for the fleet in practice. The change matters for
a project that adds one of them.
