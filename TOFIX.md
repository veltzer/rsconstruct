# TOFIX

Findings from writing the missing processor docs pages on 2026-10-06. Each
was reproduced in a scratch project; the pages document the current
behavior, so fixing an item means updating its page too.

## Medium

- `src/processor/checker/iyamllint.rs:27` - parses with `serde_yaml_ng::from_str`, which rejects valid multi-document YAML (`---` separators) with "more than one document is not supported". Parse with a multi-document loader instead. This also matters for plan.md Stage 1, which counts iyamllint as yamllint's Rust alternative: it honours none of `.yamllint.yaml`'s rules (syntax and duplicate keys only), so that row's "confirm every option is honoured" is really "implement the rules".
- `src/processor/generator/isass.rs` - partials (`_*.scss`) are compiled to their own `.css`, unlike the `sass` CLI. Skip files whose name starts with `_` by default.
- `plan.md` Stage 3 `iyq` row says "reuse ijq's filter evaluation", but `ijq` evaluates no filters: it is a JSON parse check identical to `ijsonlint`. The row needs a real jq engine (e.g. the `jaq` crate).
