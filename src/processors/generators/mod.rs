mod cc;
mod gem;
mod npm;
pub mod tags;
mod tera;

pub use cc::CcProcessor;
pub use gem::GemProcessor;
pub use npm::NpmProcessor;
pub use tags::TagsProcessor;
pub use tera::TeraProcessor;
