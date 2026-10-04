// `mass_generator::mass_generator` mirrors the layout of the sibling
// `checker/`, `generator/` and `creator/` directories, where each processor
// lives in a file named after itself. There is exactly one mass generator
// processor today; user tools plug into it by config rather than by adding a
// file here.
#[allow(clippy::module_inception)]
mod mass_generator;
