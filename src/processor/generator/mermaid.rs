//! mermaid generator — registered as a `SimpleGenerator` with a custom execute fn.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::config::StandardConfig;
use crate::graph::Product;
use crate::processor::{check_command_output, ensure_output_dir, run_command};

use crate::processor::{DiscoverMode, SimpleGenerator, SimpleGeneratorParams};

fn execute_mermaid(
    ctx: &crate::build_context::BuildContext,
    config: &StandardConfig,
    product: &Product,
) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    ensure_output_dir(output)?;
    let command = config.require_command("mermaid")?;
    let mut cmd = Command::new(command);
    cmd.arg("-i").arg(input);
    cmd.arg("-o").arg(output);
    for arg in &config.args {
        cmd.arg(arg);
    }
    // mmdc renders through Chromium (puppeteer). Inside bwrap Chromium's
    // own sandbox cannot start and the browser fails to launch, so a
    // sandboxed run hands mmdc a puppeteer config that turns it off; bwrap
    // confines the run instead. Natively it stays on. A user who passes
    // their own config in `args` owns that choice.
    let no_sandbox_config = if crate::sandbox::active() && !passes_puppeteer_config(&config.args) {
        let path = no_sandbox_config_path();
        fs::write(&path, NO_SANDBOX_PUPPETEER_CONFIG)
            .with_context(|| format!("Failed to write puppeteer config {}", path.display()))?;
        cmd.arg("-p").arg(&path);
        Some(path)
    } else {
        None
    };
    let out = run_command(ctx, &cmd);
    if let Some(path) = no_sandbox_config {
        fs::remove_file(&path)
            .with_context(|| format!("Failed to remove puppeteer config {}", path.display()))?;
    }
    check_command_output(&out?, format_args!("mmdc {}", input.display()))
}

/// The puppeteer config a sandboxed run passes with `-p`.
const NO_SANDBOX_PUPPETEER_CONFIG: &str = r#"{"args":["--no-sandbox"]}"#;

/// Whether the user's `args` already pass a puppeteer config file.
fn passes_puppeteer_config(args: &[String]) -> bool {
    args.iter().any(|a| {
        a == "-p" || a == "--puppeteerConfigFile" || a.starts_with("--puppeteerConfigFile=")
    })
}

/// A fresh path in the temporary directory (writable inside the sandbox,
/// outside the project) for one invocation's puppeteer config.
fn no_sandbox_config_path() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "rsconstruct-mmdc-{}-{}.json",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn create_mermaid(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleGenerator::new(
            cfg,
            SimpleGeneratorParams {
                extra_tools: &["node"],
                extra_tools_fn: None,
                source_filter: None,
                discover_mode: DiscoverMode::MultiFormat,
                execute_fn: execute_mermaid,
                is_builtin: false,
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "mermaid", processor_type: crate::processor::ProcessorType::Generator, create: create_mermaid,
    fields: &[],
    omit_standard_fields: &["batch"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".mmd"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { output_dir: "out/processor.generator.mermaid", formats: &["png"], command: "mmdc", ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["diagram", "mermaid", "svg", "png", "generator", "node", "npm"],
    description: "Render Mermaid diagrams to images",
    is_builtin: false,
    is_rust: false,
    supports_batch: false,
    caches_outputs: true,
    // Chromium-based; sandboxed runs pass a no-sandbox puppeteer config
    // (docs/src/internal/sandbox-chromium.md).
    sandboxable: true,
    max_jobs_cap: None,
} }

#[cfg(test)]
mod tests {
    use super::*;

    /// A user-supplied puppeteer config, in any of mmdc's spellings, is
    /// left alone; other args are not mistaken for one.
    #[test]
    fn detects_a_user_puppeteer_config() {
        let cases: [(&[&str], bool); 4] = [
            (&["-p", "pp.json"], true),
            (&["--puppeteerConfigFile", "pp.json"], true),
            (&["--puppeteerConfigFile=pp.json"], true),
            (&["-t", "dark", "-b", "transparent"], false),
        ];
        for (args, expected) in cases {
            let args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
            assert_eq!(passes_puppeteer_config(&args), expected, "{args:?}");
        }
    }
}
inventory::submit! { crate::tools::ToolInfo {
    name: "mmdc", runtime: "node",
    project: Some(crate::tools::InstallMethod::new("npm", "@mermaid-js/mermaid-cli")),
    machine: Some(crate::tools::InstallMethod::new("npm", "@mermaid-js/mermaid-cli")),
    alternatives: &[], prefer: None, sandbox_writable: &[], needs_network: false,
} }
