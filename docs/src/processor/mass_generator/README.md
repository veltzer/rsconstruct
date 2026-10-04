# Mass Generators

A mass generator wraps a tool that produces many files in one invocation
*and* can say in advance which files those will be. Its `predict_command`
prints a manifest at discovery time; every entry becomes a product of its
own, with its own inputs and blob cache entry, and the tool's `command`
runs at most once per build. See
[Processor Types](../../processor-types.md#mass-generator) for how mass
generators work and how they differ from creators.

The page in this section documents the built-in `mass_generator` processor,
which is driven entirely by config: any tool that honors the manifest
contract plugs into it without a new processor file.

Run `rsconstruct processor list` for the full list with each processor's type.
