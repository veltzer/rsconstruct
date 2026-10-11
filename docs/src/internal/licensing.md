# Licensing of Ported Engines

rsconstruct is MIT-licensed. Several of its native engines are ports of
other tools, written by translating the original's source function by
function so that the output matches exactly. A translation is a
derivative work of what it translates, so each engine carries the license
obligations of its original, not only rsconstruct's.

## Engines whose originals are not MIT-compatible

| Engine | Ported from | Original's license |
|---|---|---|
| `src/engines/shellcheck/` (`ishellcheck`) | ShellCheck 0.11.0 | GPL-3.0-or-later |
| `src/engines/hadolint/` (`idockerfile`) | hadolint 2.15.1 | GPL-3.0 |
| `src/engines/yamllint/` (`iyamllint`) | yamllint 1.38.0 | GPL-3.0-or-later |
| `src/engines/checkpatch/` (`icheckpatch`) | the kernel's checkpatch.pl | GPL-2.0-only |
| `src/engines/checkstyle/` (`icheckstyle`) | checkstyle 8.36.1 | LGPL-2.1 |
| `src/engines/perlcritic/` (`iperlcritic`) | Perl::Critic 1.156, PPI 1.284 | Artistic-1.0 or GPL-1.0-or-later |

Distributing these under MIT conflicts with the GPL and LGPL terms. Note
that the GPL engines cannot all be fixed by relicensing rsconstruct to a
single GPL version: checkpatch.pl is GPL-2.0-only, which is incompatible
with the GPL-3.0 of ShellCheck, hadolint and yamllint.

## Engines whose originals are permissive

`icpplint` (cpplint, BSD-3-Clause), `iluacheck` (luacheck, MIT),
`iactionlint` (actionlint, MIT), `isvglint` (svglint, MIT, with
htmlparser2 MIT, css-select BSD-2-Clause and fast-xml-parser MIT),
`ixmllint` (libxml2's behaviour, MIT) and `itidy` (HTML Tidy, W3C license)
are compatible with MIT; their originals' copyright notices still have to be
kept with the code.

## Status

Recorded on 2026-10-11; no decision taken yet. Options include moving the
GPL-derived engines into separately licensed crates or tools that
rsconstruct runs, relicensing per engine, or replacing them with
independent (clean-room) implementations. Until then, new engines should
not be ported from GPL or LGPL source.
