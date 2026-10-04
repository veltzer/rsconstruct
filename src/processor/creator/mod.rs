mod cargo;
mod cc;
// The generic, config-driven creator is named after its category, so the
// module path reads `creator::creator`; see `generator/mod.rs` for why
// neither name changes.
#[allow(clippy::module_inception)]
mod creator;
mod gem;
mod jekyll;
mod mdbook;
mod npm;
mod pip;
mod sphinx;
