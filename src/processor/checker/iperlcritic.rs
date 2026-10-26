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

    /// The profile's text. Only the default `.perlcriticrc` may be absent
    /// (then `Perl::Critic`'s defaults apply, as for `perlcritic` without
    /// one); a profile the user named must exist — discovery checks that
    /// first, so this is the error for a file that vanished mid-build.
    fn load_profile(&self) -> Result<Profile> {
        let path = Path::new(&self.config.profile);
        let text = if path.is_file() {
            std::fs::read_to_string(path).with_context(|| {
                format!("Failed to read Perl::Critic profile {}", path.display())
            })?
        } else if self.config.profile == default_profile() {
            String::new()
        } else {
            bail!("iperlcritic profile not found: {}", self.config.profile);
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

    /// A `profile` other than the default is an input of every product (it
    /// must exist); the default `.perlcriticrc` is a `dep_auto` input.
    fn extra_dep_inputs(&self) -> Result<Vec<String>> {
        let named = crate::processor::named_config_file(
            "iperlcritic",
            "profile",
            &self.config.profile,
            &default_profile(),
        )?;
        Ok(named.into_iter().collect())
    }

    /// One product per scanned file (the default `discover`), plus the
    /// profile read from disk.
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    /// No `execute_batch`: checking in-process has no start-up cost for a
    /// batch to share, and a batch runs its files one after another on a
    /// single worker, so files are checked in parallel as separate
    /// products instead (`supports_batch: false`).
    fn execute(&self, _ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.check_file(product.primary_input())
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
        omit_standard_fields: &[
        "command",
        "formats",
        "args",
        "output_dir",
        "required_tools",
        "batch",
    ],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".pl", ".pm"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { dep_auto: &[".perlcriticrc"], ..crate::config::ProcessorDefaults::EMPTY }),
        keywords: &["perl", "linter", "checker", "pl", "pm", "perlcritic", "rust"],
        description: "Analyze Perl code with Perl::Critic's severity-5 policies (in-process)",
        is_builtin: true,
        is_rust: true,
        supports_batch: false,
        caches_outputs: true,
        max_jobs_cap: None,
    }
}
