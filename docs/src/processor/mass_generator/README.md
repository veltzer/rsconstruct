# Mass Generators

A mass generator wraps a tool that produces many files in one invocation
*and* can say in advance which files those will be. Its planner computes a
manifest at discovery time; every entry becomes a product of its own, with its
own inputs and blob cache entry, and the tool's `command` runs at most once per
build. See [Processor Types](../../processor-types.md#mass-generator) for how
mass generators work.

Built-in mass generators include `processor.mass_generator.generic` (driven
entirely by config — any tool that honors the manifest contract plugs into it
without a new processor file), static site and documentation builders
(`mdbook`, `jekyll`, `sphinx`, `zola`), project compilers (`cargo`, `cc`,
`linux_module`), and package installers (`gem`, `npm`, `pip`).

Run `rsconstruct processor list` for the full list with each processor's type.
