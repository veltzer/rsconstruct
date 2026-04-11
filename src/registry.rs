use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::config::{self, KnownFields};
use crate::processors::ProductDiscovery;

/// Operations that each processor type provides to the registry.
/// Implemented generically via `TypedEntry<C>`.
pub(crate) trait RegistryOps: Send + Sync {
    fn name(&self) -> &'static str;
    fn create(&self, config_toml: &toml::Value) -> Result<Box<dyn ProductDiscovery>>;
    fn create_default(&self) -> Box<dyn ProductDiscovery>;
    fn resolve_defaults(&self, value: &mut toml::Value) -> Result<()>;
    fn known_fields(&self) -> &'static [&'static str];
    fn output_fields(&self) -> &'static [&'static str];
    fn must_fields(&self) -> &'static [&'static str];
    fn field_descriptions(&self) -> &'static [(&'static str, &'static str)];
    fn defconfig_json(&self) -> Option<String>;
}

/// Generic implementation of RegistryOps for a (Config, Processor) pair.
struct TypedEntry<C> {
    name: &'static str,
    ctor: fn(C) -> Box<dyn ProductDiscovery>,
}

/// Apply both processor defaults and scan defaults to a TOML value.
fn apply_all_defaults(name: &str, value: &mut toml::Value) {
    config::apply_processor_defaults(name, value);
    config::apply_scan_defaults(name, value);
}

impl<C> RegistryOps for TypedEntry<C>
where
    C: Default + DeserializeOwned + Serialize + Clone + KnownFields + Send + Sync + 'static,
{
    fn name(&self) -> &'static str { self.name }

    fn create(&self, config_toml: &toml::Value) -> Result<Box<dyn ProductDiscovery>> {
        let mut config_val = config_toml.clone();
        apply_all_defaults(self.name, &mut config_val);
        let cfg: C = toml::from_str(&toml::to_string(&config_val)?)?;
        Ok((self.ctor)(cfg))
    }

    fn create_default(&self) -> Box<dyn ProductDiscovery> {
        let config_val = toml::Value::Table(toml::map::Map::new());
        self.create(&config_val).unwrap()
    }

    fn resolve_defaults(&self, value: &mut toml::Value) -> Result<()> {
        apply_all_defaults(self.name, value);
        let cfg: C = toml::from_str(&toml::to_string(value)?)?;
        *value = toml::Value::try_from(&cfg)?;
        Ok(())
    }

    fn known_fields(&self) -> &'static [&'static str] { C::known_fields() }
    fn output_fields(&self) -> &'static [&'static str] { C::output_fields() }
    fn must_fields(&self) -> &'static [&'static str] { C::must_fields() }
    fn field_descriptions(&self) -> &'static [(&'static str, &'static str)] { C::field_descriptions() }

    fn defconfig_json(&self) -> Option<String> {
        let mut config_val = toml::Value::Table(toml::map::Map::new());
        apply_all_defaults(self.name, &mut config_val);
        let cfg: C = toml::from_str(&toml::to_string(&config_val).ok()?).ok()?;
        let json = serde_json::to_value(cfg).ok()?;
        serde_json::to_string_pretty(&json).ok()
    }
}

/// Create a registry entry. `ctor` takes a deserialized config and returns a boxed processor.
pub(crate) fn entry<C>(
    name: &'static str,
    ctor: fn(C) -> Box<dyn ProductDiscovery>,
) -> Box<dyn RegistryOps>
where
    C: Default + DeserializeOwned + Serialize + Clone + KnownFields + Send + Sync + 'static,
{
    Box::new(TypedEntry { name, ctor })
}

/// Build the processor registry — one entry per builtin processor type.
pub(crate) fn build_registry() -> Vec<Box<dyn RegistryOps>> {
    use crate::config::*;
    use crate::processors::*;
    vec![
        entry::<TeraConfig>("tera", |cfg| Box::new(TeraProcessor::new(cfg))),
        entry::<CcSingleFileConfig>("cc_single_file", |cfg| Box::new(CcSingleFileProcessor::new(cfg))),
        entry::<CcConfig>("cc", |cfg| Box::new(CcProcessor::new(cfg))),
        entry::<CppcheckConfig>("cppcheck", |cfg| Box::new(CppcheckProcessor::new(cfg))),
        entry::<ClangTidyConfig>("clang_tidy", |cfg| Box::new(ClangTidyProcessor::new(cfg))),
        entry::<ZspellConfig>("zspell", |cfg| Box::new(ZspellProcessor::new(cfg))),
        entry::<ShellcheckConfig>("shellcheck", |cfg| Box::new(ShellcheckProcessor::new(cfg))),
        entry::<LuacheckConfig>("luacheck", |cfg| Box::new(LuacheckProcessor::new(cfg))),
        entry::<MakeConfig>("make", |cfg| Box::new(MakeProcessor::new(cfg))),
        entry::<CargoConfig>("cargo", |cfg| Box::new(CargoProcessor::new(cfg))),
        entry::<ClippyConfig>("clippy", |cfg| Box::new(ClippyProcessor::new(cfg))),
        entry::<JsonSchemaConfig>("json_schema", |cfg| Box::new(JsonSchemaProcessor::new(cfg))),
        entry::<TagsConfig>("tags", |cfg| Box::new(TagsProcessor::new(cfg))),
        entry::<PipConfig>("pip", |cfg| Box::new(PipProcessor::new(cfg))),
        entry::<SphinxConfig>("sphinx", |cfg| Box::new(SphinxProcessor::new(cfg))),
        entry::<MdbookConfig>("mdbook", |cfg| Box::new(MdbookProcessor::new(cfg))),
        entry::<NpmConfig>("npm", |cfg| Box::new(NpmProcessor::new(cfg))),
        entry::<GemConfig>("gem", |cfg| Box::new(GemProcessor::new(cfg))),
        entry::<MdlConfig>("mdl", |cfg| Box::new(MdlProcessor::new(cfg))),
        entry::<MarkdownlintConfig>("markdownlint", |cfg| Box::new(MarkdownlintProcessor::new(cfg))),
        entry::<AspellConfig>("aspell", |cfg| Box::new(AspellProcessor::new(cfg))),
        entry::<PdflatexConfig>("pdflatex", |cfg| Box::new(PdflatexProcessor::new(cfg))),
        entry::<AsciiConfig>("ascii", |cfg| Box::new(AsciiProcessor::new(cfg))),
        entry::<TermsConfig>("terms", |cfg| Box::new(TermsProcessor::new(cfg))),
        entry::<MakoConfig>("mako", |cfg| Box::new(MakoProcessor::new(cfg))),
        entry::<Jinja2Config>("jinja2", |cfg| Box::new(Jinja2Processor::new(cfg))),
        entry::<PdfuniteConfig>("pdfunite", |cfg| Box::new(PdfuniteProcessor::new(cfg))),
        entry::<IpdfuniteConfig>("ipdfunite", |cfg| Box::new(IpdfuniteProcessor::new(cfg))),
        entry::<CreatorConfig>("creator", |cfg| Box::new(CreatorProcessor::new(cfg))),
        entry::<ScriptConfig>("script", |cfg| Box::new(ScriptProcessor::new(cfg))),
        entry::<GeneratorConfig>("generator", |cfg| Box::new(GeneratorProcessor::new(cfg))),
        entry::<ExplicitConfig>("explicit", |cfg| Box::new(ExplicitProcessor::new(cfg))),
        entry::<LinuxModuleConfig>("linux_module", |cfg| Box::new(LinuxModuleProcessor::new(cfg))),
        entry::<CpplintConfig>("cpplint", |cfg| Box::new(CpplintProcessor::new(cfg))),
        entry::<CheckpatchConfig>("checkpatch", |cfg| Box::new(CheckpatchProcessor::new(cfg))),
        entry::<JekyllConfig>("jekyll", |cfg| Box::new(JekyllProcessor::new(cfg))),
        entry::<IjqConfig>("ijq", |cfg| Box::new(IjqProcessor::new(cfg))),
        entry::<IjsonlintConfig>("ijsonlint", |cfg| Box::new(IjsonlintProcessor::new(cfg))),
        entry::<IyamllintConfig>("iyamllint", |cfg| Box::new(IyamllintProcessor::new(cfg))),
        entry::<IyamlschemaConfig>("iyamlschema", |cfg| Box::new(IyamlschemaProcessor::new(cfg))),
        entry::<ItaploConfig>("itaplo", |cfg| Box::new(ItaploProcessor::new(cfg))),
        entry::<RustSingleFileConfig>("rust_single_file", |cfg| Box::new(RustSingleFileProcessor::new(cfg))),
        entry::<EncodingConfig>("encoding", |cfg| Box::new(EncodingProcessor::new(cfg))),
        entry::<DuplicateFilesConfig>("duplicate_files", |cfg| Box::new(DuplicateFilesProcessor::new(cfg))),
        entry::<MarpImagesConfig>("marp_images", |cfg| Box::new(MarpImagesProcessor::new(cfg))),
        entry::<LicenseHeaderConfig>("license_header", |cfg| Box::new(LicenseHeaderProcessor::new(cfg))),
    ]
}
