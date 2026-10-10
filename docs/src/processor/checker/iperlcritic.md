# Iperlcritic Processor

## Purpose

Checks Perl code with [Perl::Critic](https://metacpan.org/pod/Perl::Critic)'s policies, in-process (no `perlcritic` program, no Perl to install). The Rust alternative to the [perlcritic](perlcritic.md) processor: a port of Perl::Critic 1.156 and of PPI 1.284, the Perl parser under it, so a file gets the same violations, at the same lines and columns, and passes or fails the way `perlcritic` would exit.

## How It Works

Each file goes through what Perl::Critic does to it: PPI's tokenizer and lexer (with the same guesses PPI makes where Perl's grammar needs a runtime to decide — regex or division, hash or block, prototype or signature — the same tree, and the same parse errors), the `## no critic` annotations, and the policies the profile enables, each over the elements it applies to. Violations are printed in the profile's `verbose` format, sorted by position as perlcritic sorts them, and any violation fails the product:

```text
src/bad.pl:1:1: [TestingAndDebugging::RequireUseStrict] Code before strictures are enabled
src/bad.pl:3:1: [InputOutput::ProhibitTwoArgOpen] Two-argument "open" used
src/bad.pl:4:9: [Subroutines::ProhibitExplicitReturnUndef] "return" statement with explicit "undef"
```

A file PPI cannot parse fails with perlcritic's message:

```text
Problem while critiquing "src/broken.pl": Can't parse code: Encountered unexpected character '195'
```

The policies are configured as Perl::Critic's are, by a profile (`.perlcriticrc`):

```toml
[processor.checker.iperlcritic]
src_dirs = [""]
```

### Implemented policies

The 22 policies of severity 5 (`gentle`), which are what Perl::Critic's default profile and the fleet's shared `.perlcriticrc` enable: `BuiltinFunctions::ProhibitSleepViaSelect`, `ProhibitStringyEval`, `RequireGlobFunction`, `ClassHierarchies::ProhibitOneArgBless`, `ControlStructures::ProhibitMutatingListFunctions`, `InputOutput::ProhibitBarewordDirHandles`, `ProhibitBarewordFileHandles`, `ProhibitInteractiveTest`, `ProhibitTwoArgOpen`, `RequireEncodingWithUTF8Layer`, `Modules::ProhibitEvilModules`, `RequireBarewordIncludes`, `RequireFilenameMatchesPackage`, `Subroutines::ProhibitExplicitReturnUndef`, `ProhibitNestedSubs`, `ProhibitReturnSort`, `ProhibitSubroutinePrototypes`, `TestingAndDebugging::ProhibitNoStrict`, `RequireUseStrict`, `ValuesAndExpressions::ProhibitLeadingZeros`, `Variables::ProhibitConditionalDeclarations` and `RequireLexicalLoopIterators`, each with all its parameters, plus `severity` and `maximum_violations_per_document`.

The profile's global options `severity`, `verbose` (a number or a format, every `%` field except `%d`), `force`, `only`, `include`, `exclude`, `top` and `program-extensions` work as in perlcritic, as do `[Policy]` and `[-Policy]` sections. A profile that enables a policy this port does not implement (say `severity = 4`, or `[CodeLayout::ProhibitHardTabs]` raised to `severity = 5`) is an error naming it, as are `theme` and `single-policy`, so a profile either runs as perlcritic runs it or does not run. Without a profile file, Perl::Critic's defaults apply (severity 5); perlcritic's fallback to `~/.perlcriticrc` is not taken, so the build does not depend on the home directory.

### Fidelity

Checked against Perl::Critic 1.156 on PPI 1.284 over the fleet's 1313 Perl files, the 3950 modules of a Debian Perl installation (`/usr/share/perl5` and the architecture-specific tree), 207 hand-written edge cases (heredocs, formats, `__END__`/`__DATA__`, POD, prototypes and signatures under every way of enabling them, `#line` directives, byte order marks, CR and CRLF line ends, every policy's exemptions) and 12300 mutated copies: the same PPI tree for every file (every element, every token's content and location) and the same report, violation for violation. That includes PPI's failures: a character it cannot tokenize, and the cases where a `use` statement's version or arguments make version.pm or PPI's own feature detection die.

The engine's unit tests (`src/engines/perlcritic/tests.rs`) keep it that way: they run PPI and Perl::Critic themselves (a missing or different version fails them) and the engine over the files in `src/engines/perlcritic/testdata/` and deterministic mutants of them, comparing trees and reports under five profiles.

Known differences: when a policy dies on a file (a few malformed argument lists make Perl::Critic call a method on a list), perlcritic stops the whole run with Perl's message, which names a line of its own source; here only that file fails, with the same first words. Where PPI's feature detection dies on a string argument that names a Perl package already loaded in perlcritic's process (`use feature 'strict', (...)`), Perl's message lacks the "perhaps you forgot to load" hint that the port always gives. The default verbosity (4) prints as verbosity 5, with the file name, as perlcritic does when it is given several files, which a build always does.

## Source Files

- Input: `.pl` and `.pm` files under `src_dirs`, or the files in `src_files`
- Output: none (checker)

The default `src_dirs` is empty, so a stanza with no `src_dirs` or `src_files` checks nothing: name the directories (`[""]` for the whole project) or files to check.

## Configuration

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `profile` | string | `".perlcriticrc"` | The Perl::Critic profile, as `perlcritic --profile`; relative to the project root. A missing file means Perl::Critic's defaults |
| `dep_auto` | string[] | `[".perlcriticrc"]` | Config files that are inputs when they exist |

A `profile` other than `.perlcriticrc` should also be listed in `dep_inputs`, so that changing it rebuilds.

Switching a repo from perlcritic: `[processor.checker.perlcritic]` → `[processor.checker.iperlcritic]`; the fleet's `.perlcriticrc` needs no change.

## Batch support

Not used: checking in-process has no start-up cost for a batch to share, so files are checked in parallel as separate products.

## Clean behavior

This processor is a Checker — `rsconstruct clean outputs` is a no-op for it (checkers produce no outputs). See [Clean behavior](../../processors.md#clean-behavior) and [`rsconstruct clean`](../../commands.md#rsconstruct-clean).
