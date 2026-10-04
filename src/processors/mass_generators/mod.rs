// `mass_generators::mass_generator` mirrors the layout of the sibling
// `checkers/`, `generators/` and `creators/` directories, where each
// processor lives in a file named after itself. There is exactly one mass
// generator processor today; user tools plug into it by config rather than
// by adding a file here.
mod mass_generator;
