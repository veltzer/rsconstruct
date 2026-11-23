//! vnu checker: the Nu Html Checker, the validator behind validator.w3.org.
//!
//! It checks HTML against the HTML standard's rules, content model
//! included (a `<main>` inside a `<main>`, a `<style>` in the body), which
//! `tidy` does not know.
//!
//! The checker is a Java program published as one jar, so this is not a
//! {`SimpleChecker`}: the command is `java`, and the jar it runs is the tool
//! `vnu.jar`, found on PATH like any other tool and handed to `java -jar`
//! by its full path. Upstream also ships a self-contained Linux build, but
//! only under a release tag (`latest`) whose assets are replaced in place,
//! which a lock file cannot name. The jar comes from the npm registry
//! instead, where every version keeps its own URL.

use anyhow::{Context, Result};
use std::path::Path;

use crate::config::CheckerConfig;
use crate::graph::Product;

/// The checker's jar, installed as an executable so PATH finds it.
const VNU_JAR: &str = "vnu.jar";
const DESCRIPTION: &str = "Check HTML files using the Nu Html Checker (vnu)";

pub struct VnuProcessor {
    config: CheckerConfig,
}

impl VnuProcessor {
    pub const fn new(config: CheckerConfig) -> Self {
        Self { config }
    }

    fn check_files(&self, ctx: &crate::build_context::BuildContext, files: &[&Path]) -> Result<()> {
        let java = self.config.standard.require_command(DESCRIPTION)?;
        let jar =
            which::which(VNU_JAR).with_context(|| format!("Failed to find {VNU_JAR} on PATH"))?;
        let mut args = vec!["-jar".to_string(), jar.to_string_lossy().into_owned()];
        args.extend_from_slice(&self.config.standard.args);
        crate::processor::run_checker(ctx, java, None, &args, files, ctx.max_arg_len())
    }
}

impl crate::processor::Processor for VnuProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    fn auto_detect(&self, file_index: &crate::file_index::FileIndex) -> bool {
        !file_index.scan(&self.config.standard, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        crate::processor::with_declared_tools(
            vec![self.config.standard.command.clone(), VNU_JAR.to_string()],
            &self.config.standard,
        )
    }

    /// One product per scanned file (the default `discover`).
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.check_files(ctx, &[product.primary_input()])
    }

    fn execute_batch(
        &self,
        ctx: &crate::build_context::BuildContext,
        products: &[&Product],
    ) -> Vec<Result<()>> {
        crate::processor::execute_checker_batch(ctx, products, |ctx, files| {
            self.check_files(ctx, files)
        })
    }
}

fn create_vnu(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(VnuProcessor::new(cfg)))
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "vnu", processor_type: crate::processor::ProcessorType::Checker, create: create_vnu,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_extensions: &[".html", ".htm"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "java", ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::CheckerConfig>,
    keywords: &["html", "html5", "validator", "linter", "web", "frontend", "w3c", "java"],
    description: DESCRIPTION,
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    sandboxable: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "vnu.jar", runtime: "jvm",
    project: Some(crate::tools::InstallMethod::new("binary", "vnu.jar")),
    machine: Some(crate::tools::InstallMethod::new("binary", "vnu.jar")),
    alternatives: &[], prefer: None, sandbox_writable: &[], needs_network: false,
} }
// The headless runtime: the checker draws nothing.
inventory::submit! { crate::tools::ToolInfo {
    name: "java", runtime: "jvm",
    project: None,
    machine: Some(crate::tools::InstallMethod::new("apt", "default-jre-headless")),
    alternatives: &[], prefer: None, sandbox_writable: &[], needs_network: false,
} }
