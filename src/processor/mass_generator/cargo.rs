use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{OnceTool, require_planned_output};
use crate::config::{StandardConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{
    Processor, SiblingFilter, anchor_display_dir, check_command_output, run_in_anchor_dir,
};

fn default_cargo() -> String {
    "cargo".into()
}

fn default_cargo_profiles() -> Vec<String> {
    vec!["dev".into(), "release".into()]
}

/// Cargo config. Custom: cargo, profiles.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CargoConfig {
    #[serde(default = "default_cargo")]
    pub cargo: String,
    #[serde(default = "default_cargo_profiles")]
    pub profiles: Vec<String>,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for CargoConfig {
    fn default() -> Self {
        Self {
            cargo: "cargo".into(),
            profiles: default_cargo_profiles(),
            standard: StandardConfig::default(),
        }
    }
}

/// Map a Cargo `--profile` name to the subdirectory under `target/`.
fn profile_dir_name(profile: &str) -> &str {
    match profile {
        "dev" | "test" => "debug",
        "release" | "bench" => "release",
        other => other,
    }
}

/// Enumerate the top-level artifact filenames (`<bin>`, `lib<name>.rlib`,
/// `lib<name>.so`/`.dylib`, `lib<name>.a`) that `cargo build` writes into
/// `target/<profile_dir>/` for the package or workspace at `cargo_toml`.
fn planned_cargo_artifacts(cargo_toml: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    collect_manifest_artifacts(cargo_toml, &mut out)?;
    out.sort();
    out.dedup();
    Ok(out)
}

fn collect_manifest_artifacts(cargo_toml: &Path, out: &mut Vec<String>) -> Result<()> {
    let text = fs::read_to_string(cargo_toml)
        .with_context(|| format!("Failed to read {}", cargo_toml.display()))?;
    let doc: toml::Value = toml::from_str(&text)
        .with_context(|| format!("Failed to parse {}", cargo_toml.display()))?;
    let crate_dir = crate::processor::parent_dir_or_empty(cargo_toml);

    if let Some(pkg) = doc.get("package").and_then(toml::Value::as_table) {
        let pkg_name = pkg
            .get("name")
            .and_then(toml::Value::as_str)
            .with_context(|| format!("{} [package] is missing `name`", cargo_toml.display()))?;
        let autolib = pkg
            .get("autolib")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        let autobins = pkg
            .get("autobins")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);

        let lib_table = doc.get("lib").and_then(toml::Value::as_table);
        let has_lib = lib_table.is_some()
            || (autolib
                && crate::processor::resolve_anchor_path(crate_dir, "src/lib.rs").is_file());
        if has_lib {
            let raw_name = lib_table
                .and_then(|t| t.get("name"))
                .and_then(toml::Value::as_str)
                .unwrap_or(pkg_name);
            let lib_name = raw_name.replace('-', "_");
            let proc_macro = lib_table
                .and_then(|t| t.get("proc-macro").or_else(|| t.get("proc_macro")))
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            let crate_types: Vec<&str> = lib_table
                .and_then(|t| t.get("crate-type").or_else(|| t.get("crate_type")))
                .and_then(toml::Value::as_array)
                .map_or_else(
                    || {
                        if proc_macro {
                            vec!["proc-macro"]
                        } else {
                            vec!["rlib"]
                        }
                    },
                    |arr| arr.iter().filter_map(toml::Value::as_str).collect(),
                );
            for ct in crate_types {
                match ct {
                    "lib" | "rlib" => out.push(format!("lib{lib_name}.rlib")),
                    "staticlib" => out.push(format!("lib{lib_name}.a")),
                    "dylib" | "cdylib" | "proc-macro" => out.push(format!(
                        "{}{lib_name}{}",
                        std::env::consts::DLL_PREFIX,
                        std::env::consts::DLL_SUFFIX
                    )),
                    _ => {}
                }
            }
        }

        let mut bin_names = Vec::new();
        let mut main_rs_claimed = false;
        if let Some(bins) = doc.get("bin").and_then(toml::Value::as_array) {
            for bin in bins {
                if let Some(name) = bin.get("name").and_then(toml::Value::as_str) {
                    bin_names.push(name.to_string());
                }
                if bin
                    .get("path")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|p| p == "src/main.rs")
                {
                    main_rs_claimed = true;
                }
            }
        }
        if autobins {
            if !main_rs_claimed
                && !bin_names.iter().any(|n| n == pkg_name)
                && crate::processor::resolve_anchor_path(crate_dir, "src/main.rs").is_file()
            {
                bin_names.push(pkg_name.to_string());
            }
            let src_bin = crate::processor::resolve_anchor_path(crate_dir, "src/bin");
            if src_bin.is_dir() {
                let entries = fs::read_dir(&src_bin)
                    .with_context(|| format!("Failed to read {}", src_bin.display()))?;
                for entry in entries {
                    let entry =
                        entry.with_context(|| format!("Failed to read {}", src_bin.display()))?;
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rs") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                            && !bin_names.iter().any(|n| n == stem)
                        {
                            bin_names.push(stem.to_string());
                        }
                    } else if path.is_dir()
                        && path.join("main.rs").is_file()
                        && let Some(stem) = path.file_name().and_then(|s| s.to_str())
                        && !bin_names.iter().any(|n| n == stem)
                    {
                        bin_names.push(stem.to_string());
                    }
                }
            }
        }
        out.extend(bin_names);
    }

    if let Some(members) = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
    {
        for member in members.iter().filter_map(toml::Value::as_str) {
            let member_toml =
                crate::processor::resolve_anchor_path(crate_dir, member).join("Cargo.toml");
            if member_toml.is_file() && member_toml != cargo_toml {
                collect_manifest_artifacts(&member_toml, out)?;
            }
        }
    }

    Ok(())
}

pub struct CargoProcessor {
    config: CargoConfig,
    runs: Mutex<BTreeMap<(PathBuf, String), Arc<OnceTool>>>,
}

impl CargoProcessor {
    pub const fn new(config: CargoConfig) -> Self {
        Self {
            config,
            runs: Mutex::new(BTreeMap::new()),
        }
    }

    fn run_guard(&self, cargo_toml: &Path, profile: &str) -> Arc<OnceTool> {
        let mut guard = self.runs.lock();
        Arc::clone(
            guard
                .entry((cargo_toml.to_path_buf(), profile.to_string()))
                .or_insert_with(|| Arc::new(OnceTool::new())),
        )
    }

    fn planned_outputs_for(&self, cargo_toml: &Path) -> Result<(Vec<String>, bool)> {
        let subcommand = self
            .config
            .standard
            .require_command(crate::processor::names::CARGO)?;
        if subcommand == "build" {
            let artifacts = planned_cargo_artifacts(cargo_toml)?;
            if !artifacts.is_empty() {
                return Ok((artifacts, false));
            }
        }
        Ok((vec![format!(".rsconstruct-{subcommand}.stamp")], true))
    }

    /// Run cargo build in the Cargo.toml's directory with the given profile
    fn execute_cargo(
        &self,
        ctx: &crate::build_context::BuildContext,
        cargo_toml: &Path,
        profile: &str,
    ) -> Result<()> {
        let subcommand = self
            .config
            .standard
            .require_command(crate::processor::names::CARGO)?;
        let anchor_dir = crate::processor::parent_dir_or_empty(cargo_toml);
        let target_dir = anchor_dir.join("target").join(profile_dir_name(profile));
        let (planned, is_stamp) = self.planned_outputs_for(cargo_toml)?;
        for artifact in &planned {
            let path = target_dir.join(artifact);
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(anyhow::Error::from(e)
                        .context(format!("Failed to remove stale output {}", path.display())));
                }
            }
        }

        let mut cmd = Command::new(&self.config.cargo);
        cmd.arg(subcommand);
        cmd.args(["--profile", profile]);
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(ctx, &mut cmd, cargo_toml)?;
        check_command_output(
            &output,
            format_args!(
                "cargo {} --profile {} in {}",
                subcommand,
                profile,
                anchor_display_dir(cargo_toml)
            ),
        )?;
        if is_stamp {
            for artifact in &planned {
                let path = target_dir.join(artifact);
                crate::processor::ensure_output_dir(&path)?;
                fs::write(&path, format!("{subcommand} {profile}\n"))
                    .with_context(|| format!("Failed to write {}", path.display()))?;
            }
        }
        Ok(())
    }
}

impl Processor for CargoProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processor::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn replaces_own_outputs(&self) -> bool {
        true
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.cargo.clone()]
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let Some(files) = crate::processor::scan_or_skip(&self.config.standard, file_index) else {
            return Ok(());
        };

        let siblings = SiblingFilter {
            extensions: &[".rs", ".toml", "Cargo.lock"],
            excludes: &["/.git/", "/target/", "/.rsconstruct/"],
        };
        let config_hash = output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        );
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        for anchor in files {
            let anchor_dir = anchor
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();

            let sibling_files = file_index.query(
                &anchor_dir,
                siblings.extensions,
                siblings.excludes,
                &[],
                &[],
                &[],
            );

            let base_inputs =
                crate::processor::build_anchor_inputs(&anchor, &sibling_files, &extra);
            let (artifacts, _) = self.planned_outputs_for(&anchor)?;

            for profile in &self.config.profiles {
                let target_dir = anchor_dir.join("target").join(profile_dir_name(profile));
                for artifact in &artifacts {
                    let output_path = target_dir.join(artifact);
                    let hash = crate::checksum::hash_parts(&[
                        &config_hash,
                        profile,
                        &output_path.display().to_string(),
                    ]);
                    graph.add_product_with(
                        base_inputs.clone(),
                        vec![output_path],
                        instance_name,
                        Some(hash),
                        Some(profile),
                        Vec::new(),
                    )?;
                }
            }
        }

        Ok(())
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let profile = product.variant.as_deref().unwrap_or("dev");
        let cargo_toml = product.primary_input();
        let guard = self.run_guard(cargo_toml, profile);
        guard.run(&self.config.cargo, || {
            self.execute_cargo(ctx, cargo_toml, profile)
        })?;
        require_planned_output(product, &self.config.cargo)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(CargoProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 2,
        name: "cargo",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "cargo", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Path to the cargo executable" },
            crate::config::FieldSpec { name: "profiles", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Build profiles to run (e.g. dev, release)" },
        ],
        omit_standard_fields: &["formats", "dep_auto", "output_dir"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["Cargo.toml"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "build", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<CargoConfig>,
        keywords: &["rust", "builder", "cargo", "rs", "package-manager"],
        description: "Build Rust projects using Cargo",
        is_native: false,
        is_rust: true,
        supports_batch: false,
        caches_outputs: true,
        max_jobs_cap: Some(1),
    }
}
