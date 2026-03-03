mod cargo;
mod cc;
mod gem;
mod mdbook;
mod npm;
mod pip;
mod sphinx;

pub use cargo::CargoProcessor;
pub use cc::CcProcessor;
pub use gem::GemProcessor;
pub use mdbook::MdbookProcessor;
pub use npm::NpmProcessor;
pub use pip::PipProcessor;
pub use sphinx::SphinxProcessor;

