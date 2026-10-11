//! iperlcritic checker: `Perl::Critic`'s severity-5 policies, in-process.
//!
//! The engine is `crate::engines::perlcritic`, a port of `Perl::Critic`
//! 1.156 and the PPI 1.284 parser under it. This file is the processor
//! around it: the profile the run uses (`.perlcriticrc`, read once per
//! build; without one, `Perl::Critic`'s defaults), and each file's
//! violations printed as `perlcritic` prints them. A file fails when it has
//! a violation, when PPI cannot parse it, or when a policy dies on it, as
//! `perlcritic` exits non-zero on each.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::engines::perlcritic::{Outcome, Profile};
use crate::graph::Product;

fn default_profile() -> String {
    ".perlcriticrc".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IperlcriticConfig {
    /// The `Perl::Critic` profile, as `perlcritic --profile`; relative to
    /// the project root. A missing file means `Perl::Critic`'s defaults.
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for IperlcriticConfig {
    fn default() -> Self {
        Self {
            profile: default_profile(),
            standard: StandardConfig::default(),
        }
    }
}

pub struct IperlcriticProcessor {
    config: IperlcriticConfig,
    /// The profile, read once per build. The error is kept as text so
    /// every product reports the same failure.
    profile: OnceLock<Result<Profile, String>>,
}

impl IperlcriticProcessor {
    pub const fn new(config: IperlcriticConfig) -> Self {
        Self {
            config,
            profile: OnceLock::new(),
        }
    }

    fn load_profile(&self) -> Result<Profile> {
        let path = Path::new(&self.config.profile);
        let text = if path.exists() {
            std::fs::read_to_string(path).with_context(|| {
                format!("Failed to read Perl::Critic profile {}", path.display())
            })?
        } else {
            String::new()
        };
        crate::engines::perlcritic::profile::load(&text)
            .with_context(|| format!("iperlcritic: {}", path.display()))
    }

    fn profile(&self) -> Result<&Profile> {
        let loaded = self
            .profile
            .get_or_init(|| self.load_profile().map_err(|e| format!("{e:#}")));
        match loaded {
            Ok(profile) => Ok(profile),
            Err(e) => bail!("{e}"),
        }
    }

    fn check_file(&self, file: &Path) -> Result<()> {
        let profile = self.profile()?;
        let bytes =
            std::fs::read(file).with_context(|| format!("Failed to read {}", file.display()))?;
        let name = file.display().to_string();
        match crate::engines::perlcritic::check_file(&name, &bytes, profile) {
            Outcome::Violations(lines) if lines.is_empty() => Ok(()),
            Outcome::Violations(lines) => {
                bail!(
                    "{}",
                    String::from_utf8_lossy(&lines.concat()).trim_end_matches('\n')
                )
            }
            Outcome::ParseError(message) | Outcome::Fatal(message) => bail!("{message}"),
        }
    }
}

impl crate::processor::Processor for IperlcriticProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    // Serialize the FULL config (the trait default covers StandardConfig
    // only), so the extra fields reach config-change detection.
    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    /// One product per scanned file (the default `discover`).
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    fn execute(&self, _ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.check_file(product.primary_input())
    }

    fn execute_batch(
        &self,
        _ctx: &crate::build_context::BuildContext,
        products: &[&Product],
    ) -> Vec<Result<()>> {
        crate::processor::execute_checker_batch_per_file(products, |file| self.check_file(file))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IperlcriticProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::RustAlternative { replaces: "processor.checker.perlcritic", by: "processor.checker.iperlcritic" }
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "iperlcritic",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<IperlcriticConfig>,
        fields: &[
            crate::config::FieldSpec { name: "profile", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "The Perl::Critic profile, as perlcritic --profile; relative to the project root (.perlcriticrc; a missing file means Perl::Critic's defaults)" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".pl", ".pm"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { dep_auto: &[".perlcriticrc"], ..crate::config::ProcessorDefaults::EMPTY }),
        keywords: &["perl", "linter", "checker", "pl", "pm", "perlcritic", "rust"],
        description: "Analyze Perl code with Perl::Critic's severity-5 policies (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: true,
        max_jobs_cap: None,
    }
}
