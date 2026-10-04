// `generic` is the config-driven mass generator (`processor.mass_generator.generic`):
// any tool that honors the manifest contract plugs into it by config rather
// than by adding a file here. The directory keeps the one-file-per-processor
// layout of its sibling categories so a built-in mass generator has a place
// to go.
mod generic;
