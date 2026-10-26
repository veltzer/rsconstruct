# Licensing of Ported Engines

Each module under `src/engines/` is a Rust port of a specific release of an existing tool (see `src/engines/mod.rs`). This page records, for each engine, which upstream project it ports and the licence that project is distributed under, so the question of what the ports owe their sources can be answered from facts rather than memory.

## This crate's licence

rsconstruct is distributed under the MIT licence: `license = "MIT"` in `Cargo.toml` and the `LICENSE` file at the repository root.

## Upstream licences

The version is the one the engine's module documentation names. The licence was checked against the upstream project itself (its licence file, its package metadata, or the `SPDX-License-Identifier` in its source) on 2026-10-26.

| Engine | Ports | Upstream licence |
|---|---|---|
| `actionlint` | actionlint 1.7.12 (rhysd/actionlint) | MIT |
| `checkpatch` | the Linux kernel's `scripts/checkpatch.pl` | GPL-2.0 (`SPDX-License-Identifier: GPL-2.0` in the script) |
| `checkstyle` | checkstyle 8.36.1 | LGPL-2.1 |
| `cpplint` | cpplint 2.0.2 | BSD-3-Clause |
| `hadolint` | hadolint 2.15.1, with the Dockerfile grammar of language-docker 16.0.0 | GPL-3.0 (hadolint); GPL-3.0-or-later (language-docker) |
| `luacheck` | luacheck 1.2.0 (lunarmodules/luacheck) | MIT |
| `perlcritic` | `Perl::Critic` 1.156 on PPI 1.284 | Perl's terms: Artistic licence or GPL-1.0-or-later, for both |
| `shellcheck` | ShellCheck 0.11.0 | GPL-3.0 |
| `svglint` | svglint 4.2.1, reproducing the behaviour of htmlparser2, fast-xml-parser and css-select | MIT (svglint, htmlparser2, fast-xml-parser); BSD-2-Clause (css-select) |
| `tidy` | HTML Tidy 5.8.0 (tidy-html5) | the HTML Tidy licence (SPDX `HTMLTIDY`), a permissive W3C-style licence |
| `xmllint` | the behaviour of libxml2's `xmllint --noout [--schema]` | MIT |
| `yamllint` | yamllint (adrienverge/yamllint); the module does not name a release | GPL-3.0-or-later |

## How this interacts with the crate's licence

A port that follows its upstream source file by file, as these engines do, is a translation of that source, and a translation is generally treated as a derivative work. Where the upstream licence is permissive (MIT, BSD, the HTML Tidy licence), distributing the port under MIT is compatible, provided the upstream copyright and permission notices are kept with it. Where the upstream licence is copyleft (GPL-2.0 for checkpatch, GPL-3.0 for hadolint, ShellCheck and yamllint, LGPL-2.1 for checkstyle, the Artistic/GPL choice for `Perl::Critic` and PPI), the upstream licence's terms apply to the port, and the crate's single `license = "MIT"` does not describe those engines accurately.

Neither the upstream copyright notices nor a per-engine licence statement are currently recorded in the source tree. Until they are, this mismatch is an open item, not a settled decision; this page states the facts and is not legal advice.
