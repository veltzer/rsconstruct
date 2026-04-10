mod simple;

mod aspell;
mod ascii;
mod checkpatch;
mod clippy;
mod clang_tidy;
mod cppcheck;
mod cpplint;
mod duplicate_files;
mod encoding;

mod license_header;
mod make;
mod marp_images;
mod markdownlint;
mod mdl;


mod script;
mod shellcheck;
mod zspell;
mod ijq;
mod ijsonlint;
mod itaplo;
mod iyamllint;
mod iyamlschema;
mod json_schema;
mod luacheck;
pub(crate) mod terms;

pub use simple::SimpleChecker;
pub use aspell::AspellProcessor;
pub use ascii::AsciiProcessor;
pub use checkpatch::CheckpatchProcessor;
pub use clippy::ClippyProcessor;
pub use clang_tidy::ClangTidyProcessor;
pub use cppcheck::CppcheckProcessor;
pub use cpplint::CpplintProcessor;
pub use duplicate_files::DuplicateFilesProcessor;
pub use encoding::EncodingProcessor;
pub use license_header::LicenseHeaderProcessor;
pub use make::MakeProcessor;
pub use marp_images::MarpImagesProcessor;
pub use markdownlint::MarkdownlintProcessor;
pub use mdl::MdlProcessor;


pub use script::ScriptProcessor;
pub use shellcheck::ShellcheckProcessor;
pub use zspell::ZspellProcessor;
pub use ijq::IjqProcessor;
pub use ijsonlint::IjsonlintProcessor;
pub use itaplo::ItaploProcessor;
pub use iyamllint::IyamllintProcessor;
pub use iyamlschema::IyamlschemaProcessor;
pub use luacheck::LuacheckProcessor;
pub use json_schema::JsonSchemaProcessor;
pub use terms::TermsProcessor;
