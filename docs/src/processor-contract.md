# Processor Contract

Rules that all processors must follow.

## Fail hard, never degrade gracefully

When something fails, it must fail the entire build. Do not try-and-fallback,
do not silently substitute defaults for missing resources, do not swallow errors.
If a processor is configured to use a file and that file does not exist, that is
an error. The user must fix their configuration or their project, not the code.

Optional features must be opt-in via explicit configuration (default off).
When the user enables a feature, all resources it requires must exist.

## No work without source files

An enabled processor must not fail the build if no source files match its
file patterns. Zero matching files means zero products discovered; the
processor simply does nothing. This is not an error — it is the normal
state for a freshly initialized project.

## Single responsibility

Each processor handles one type of transformation or check.
A processor discovers its own products and knows how to execute, clean, and
report on them.

## Deterministic discovery

`discover()` must return the same products given the same filesystem state.
File discovery, processor iteration, and topological sort must all produce
sorted, deterministic output so builds are reproducible.

## Incremental correctness

Products must declare all their inputs. If any declared input changes,
the product is rebuilt. If no inputs change, the cached result is reused.
Processors must not rely on undeclared side inputs for correctness
(support files read at execution time but excluded from the input list
are acceptable only when changes to those files can never cause a
previously-passing product to fail).

## Execution isolation

A processor's `execute()` must only write to the declared output paths
(or, for mass generators, to the expected output directory).
It must not modify source files, other products' outputs, or global state.

## Error reporting

On failure, `execute()` returns an `Err` with a clear message including
the relevant file path and the nature of the problem. The executor
decides whether to abort or continue based on `--keep-going`.
