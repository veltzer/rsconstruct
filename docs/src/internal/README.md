# Internal Documentation

This section collects documentation aimed at rsconstruct's **contributors and maintainers** — people who modify the codebase itself, not end users who configure rsconstruct for their projects.

If you are using rsconstruct to build a project, you can stop reading now. Everything below is about how rsconstruct works internally: data structures, design decisions, invariants, coding style, and the reasoning behind non-obvious choices.

## What belongs here

A chapter belongs in "For Maintainers" if it answers **at least one** of these questions:

- How is rsconstruct implemented? (Architecture, cache layout, execution model)
- Why did we make this design choice? (Design notes, rejected alternatives, tradeoffs)
- What contract must my code uphold? (Processor contract, invariants, coding standards)
- What's the right way to extend rsconstruct? (Adding processors, adding analyzers)
- What's the non-obvious implementation detail I need to know? (Checksum cache layers, descriptor keys, shared-output-directory semantics)

A chapter does NOT belong here if it answers:

- How do I install rsconstruct?
- How do I configure a processor for my project?
- How do I use processor X on file type Y?

Those are user-facing and live in the main section above.

## How to use this section

Read in roughly this order if you're new to the codebase:

1. **[Architecture](architecture.md)** — 10-minute tour of the major modules and their responsibilities.
2. **[Coding Standards](coding-standards.md)** — conventions you'll be held to in code review.
3. **[Strictness](strictness.md)** — how the compiler is configured to reject lax code, and the rules for opting out.
4. **[Processor Contract](processor-contract.md)** — the interface every processor must satisfy. Read before adding a new processor.
5. **[Testing](testing.md)** — how the test suite is structured and how to add new tests.
6. **[Cache System](cache.md)** and **[Checksum Cache](checksum-cache.md)** — how incremental builds actually work.

After that, read topic-specific chapters as the work demands:

- Building cache features → [Cache System](cache.md), [Processor Versioning](processor-versioning.md)
- Adding a processor that writes into a shared directory → [Shared Output Directory](shared-output-directory.md)
- Adding cross-processor dependencies → [Cross-Processor Dependencies](cross-processor-dependencies.md)
- Thinking about ordering and enumeration → [Processor Ordering](processor-ordering.md), [Output Prediction](output-prediction.md)

## Links to individual chapters

See the table of contents in the sidebar. Brief one-line summaries:

- **Architecture** — module map and major data flows.
- **Design Notes** — collected rationale for design decisions.
- **Coding Standards** — naming, file layout, error handling conventions.
- **Strictness** — crate-level `#![deny(warnings)]`, rules for `#[allow]`.
- **Testing** — integration test structure and philosophy.
- **Parameter Naming** — canonical names for the same concept in different places.
- **Processor Contract** — what every processor must implement and uphold.
- **Cache System** — content-addressed object store, descriptor keys.
- **Checksum Cache** — mtime-based content hash caching.
- **Dependency Caching** — caching of source-file dependency scans (e.g. C/C++ headers).
- **Processor Versioning** — how processors invalidate caches when their behavior changes.
- **Cross-Processor Dependencies** — how one processor's outputs become another's inputs.
- **Shared Output Directory** — handling multiple processors that write into the same folder.
- **Processor Ordering** — why rsconstruct does NOT have explicit ordering primitives.
- **Output Prediction** — the MassGenerator design: tools that enumerate their outputs in advance.
- **Per-Processor Statistics** — why cache stats can't group by processor today, options for fixing it.
- **Unreferenced Files** — detecting files on disk that no product references.
- **Internal Processors** — pure-Rust processors that do not shell out.
- **Missing Processors** — tools we don't yet wrap but should.
- **Crates.io Publishing** — release process.
- **Per-Processor max_jobs** — design note for per-processor parallelism limits.
- **Rejected Audit Findings** — audit issues deliberately rejected, kept to prevent re-flagging.
- **Suggestions** — ideas for future work.
- **Suggestions Done** — archive of completed suggestions.
- **TODO** — ongoing and completed task list.
