use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    OnceTool, PlannedOutput, add_planned_products, fresh_state_dir, install_staged,
    require_planned_output, resolve_dep_inputs_and_auto,
};
use crate::config::{StandardConfig, output_config_hash};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{Processor, anchor_display_dir, check_command_output, run_command};

/// A single kernel module definition inside linux-module.yaml.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct LinuxModuleModuleDef {
    pub name: String,
    pub sources: Vec<String>,
    #[serde(default)]
    pub extra_cflags: Vec<String>,
}

/// Parsed contents of a linux-module.yaml file.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct LinuxModuleManifest {
    #[serde(default = "default_make_tool")]
    pub make: String,
    #[serde(default)]
    pub kdir: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub cross_compile: Option<String>,
    #[serde(default = "default_linux_module_v")]
    pub v: u32,
    #[serde(default = "default_linux_module_w")]
    pub w: u32,
    pub modules: Vec<LinuxModuleModuleDef>,
}

fn default_make_tool() -> String {
    "make".into()
}

const fn default_linux_module_v() -> u32 {
    0
}

const fn default_linux_module_w() -> u32 {
    1
}

/// Linux module config. No custom fields.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct LinuxModuleConfig {
    #[serde(flatten)]
    pub standard: StandardConfig,
}

/// The tools the discovered manifests say a build runs: each manifest's
/// `make`, and `uname` when any manifest leaves `kdir` unset (the default
/// kdir comes from `uname -r`). `None` until `discover` has read a manifest.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ManifestTools {
    make: BTreeSet<String>,
    needs_uname: bool,
}

pub struct LinuxModuleProcessor {
    config: LinuxModuleConfig,
    /// Filled during `discover`, so `required_tools()` names the `make` each
    /// manifest actually runs (the build's tool check and the declared-tools
    /// guard run after discovery). Before discovery it can only know the
    /// defaults.
    manifest_tools: Mutex<Option<ManifestTools>>,
    runs: Mutex<BTreeMap<PathBuf, Arc<OnceTool>>>,
}

impl LinuxModuleProcessor {
    pub const fn new(config: LinuxModuleConfig) -> Self {
        Self {
            config,
            manifest_tools: Mutex::new(None),
            runs: Mutex::new(BTreeMap::new()),
        }
    }

    fn run_guard(&self, yaml_path: &Path) -> Arc<OnceTool> {
        let mut guard = self.runs.lock();
        Arc::clone(
            guard
                .entry(yaml_path.to_path_buf())
                .or_insert_with(|| Arc::new(OnceTool::new())),
        )
    }

    /// Parse a linux-module.yaml file.
    fn parse_manifest(yaml_path: &Path) -> Result<LinuxModuleManifest> {
        let content = fs::read_to_string(yaml_path)
            .with_context(|| format!("Failed to read {}", yaml_path.display()))?;
        let manifest: LinuxModuleManifest = serde_norway::from_str(&content)
            .with_context(|| format!("Failed to parse {}", yaml_path.display()))?;
        Ok(manifest)
    }

    /// Compute the output directory for a linux-module.yaml file.
    fn output_dir_for(yaml_path: &Path) -> PathBuf {
        let anchor_dir = crate::processor::parent_dir_or_empty(yaml_path);
        if anchor_dir.as_os_str().is_empty() {
            PathBuf::from("out/processor.mass_generator.linux_module")
        } else {
            Path::new("out/processor.mass_generator.linux_module").join(anchor_dir)
        }
    }

    /// Get the default KDIR path from the running kernel. Fails when the
    /// kernel release cannot be determined — commands run without a shell, so
    /// a literal `$(uname -r)` fallback would never expand and `make -C`
    /// would fail with a baffling path.
    fn default_kdir(ctx: &crate::build_context::BuildContext) -> Result<String> {
        let mut cmd = Command::new("uname");
        cmd.arg("-r");
        let output = crate::processor::run_command_capture(ctx, &cmd)
            .context("Failed to run 'uname -r' to locate kernel build directory (set 'kdir' in linux-module.yaml to override)")?;
        if !output.status.success() {
            anyhow::bail!(
                "'uname -r' failed while locating the kernel build directory (set 'kdir' in linux-module.yaml to override): {}",
                String::from_utf8_lossy(&output.stderr).trim_end()
            );
        }
        let release = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(format!("/lib/modules/{release}/build"))
    }

    /// Generate a Kbuild file for building a kernel module.
    fn write_kbuild(module_dir: &Path, module: &LinuxModuleModuleDef) -> Result<()> {
        let mut content = format!("obj-m := {}.o\n", module.name);

        let objs: Vec<String> = module
            .sources
            .iter()
            .map(|s| {
                let p = Path::new(s);
                let stem = p.file_stem().unwrap_or_default().to_string_lossy();
                format!("{stem}.o")
            })
            .collect();
        let _ = writeln!(content, "{}-objs := {}", module.name, objs.join(" "));

        if !module.extra_cflags.is_empty() {
            let _ = writeln!(content, "ccflags-y := {}", module.extra_cflags.join(" "));
        }

        fs::write(module_dir.join("Kbuild"), &content)
            .with_context(|| format!("Failed to write Kbuild in {}", module_dir.display()))?;
        Ok(())
    }

    /// The kbuild invocation `make -C <kdir> [ARCH=..] [CROSS_COMPILE=..]
    /// M=<module_dir>`, in the module directory, with `target` and any
    /// extra arguments to follow.
    fn kbuild_command(manifest: &LinuxModuleManifest, kdir: &str, module_dir: &Path) -> Command {
        let mut cmd = Command::new(&manifest.make);
        cmd.arg("-C").arg(kdir);
        if let Some(ref arch) = manifest.arch {
            cmd.arg(format!("ARCH={arch}"));
        }
        if let Some(ref cross) = manifest.cross_compile {
            cmd.arg(format!("CROSS_COMPILE={cross}"));
        }
        cmd.arg(format!("M={}", module_dir.display()));
        cmd.current_dir(module_dir);
        cmd
    }

    /// Run `make modules` and read the built `.ko` (bytes and mode) into
    /// memory. It has to be read before the cleanup: kbuild's `clean`
    /// recursively deletes every `.ko` under `M=<module_dir>`.
    fn make_module(
        ctx: &crate::build_context::BuildContext,
        manifest: &LinuxModuleManifest,
        kdir: &str,
        module_dir: &Path,
        module: &LinuxModuleModuleDef,
    ) -> Result<(Vec<u8>, u32)> {
        let mut cmd = Self::kbuild_command(manifest, kdir, module_dir);
        cmd.arg(format!("V={}", manifest.v));
        cmd.arg(format!("W={}", manifest.w));
        cmd.arg("modules");
        let output = run_command(ctx, &cmd)?;
        check_command_output(&output, format_args!("make modules for {}", module.name))?;

        let ko_src = module_dir.join(format!("{}.ko", module.name));
        let ko_bytes = fs::read(&ko_src).map_err(|e| anyhow::anyhow!(
            "make reported success but did not produce {} (check the module name in linux-module.yaml): {}",
            ko_src.display(), e
        ))?;
        let ko_mode = fs::metadata(&ko_src)
            .map(|m| crate::platform::get_mode(&m))
            .with_context(|| format!("Failed to stat {}", ko_src.display()))?;
        Ok((ko_bytes, ko_mode))
    }

    /// Remove what a build left in the source directory: `make clean` for
    /// kbuild's intermediates (`.o`, `.mod`, `.cmd` files, the `.ko`), then
    /// the generated `Kbuild`. Both run even when the first fails, so a
    /// failed `make clean` never leaves the `Kbuild` behind as well.
    fn clean_module_dir(
        ctx: &crate::build_context::BuildContext,
        manifest: &LinuxModuleManifest,
        kdir: &str,
        module_dir: &Path,
        module: &LinuxModuleModuleDef,
    ) -> Result<()> {
        let mut clean_cmd = Self::kbuild_command(manifest, kdir, module_dir);
        clean_cmd.arg("clean");
        let cleaned = run_command(ctx, &clean_cmd).and_then(|output| {
            check_command_output(&output, format_args!("make clean for {}", module.name))
        });
        let kbuild = module_dir.join("Kbuild");
        let removed = fs::remove_file(&kbuild)
            .with_context(|| format!("Failed to remove generated {}", kbuild.display()));
        match (cleaned, removed) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(e), Ok(())) | (Ok(()), Err(e)) => Err(e),
            (Err(clean_err), Err(remove_err)) => Err(anyhow::anyhow!(
                "{clean_err:#}\n  (removing the generated Kbuild also failed: {remove_err:#})"
            )),
        }
    }

    /// Build a single kernel module into `staging`. Runs make in the
    /// module's source directory, and cleans that directory up afterwards
    /// whether or not the build succeeded: a failed `make modules` must not
    /// leave the generated `Kbuild` and kbuild's intermediates in the source
    /// tree.
    fn build_module(
        ctx: &crate::build_context::BuildContext,
        manifest: &LinuxModuleManifest,
        anchor_dir: &Path,
        module: &LinuxModuleModuleDef,
        staging: &Path,
    ) -> Result<()> {
        let cwd = std::env::current_dir()
            .context("Failed to get current directory for linux module build")?;
        let module_dir = if anchor_dir.as_os_str().is_empty() {
            cwd
        } else {
            cwd.join(anchor_dir)
        };

        let kdir = match manifest.kdir.clone() {
            Some(kdir) => kdir,
            None => Self::default_kdir(ctx)?,
        };

        Self::write_kbuild(&module_dir, module)?;
        let built = Self::make_module(ctx, manifest, &kdir, &module_dir, module);
        let cleaned = Self::clean_module_dir(ctx, manifest, &kdir, &module_dir, module);
        let (ko_bytes, ko_mode) = match (built, cleaned) {
            (Ok(ko), Ok(())) => ko,
            (Ok(_), Err(clean_err)) => return Err(clean_err),
            (Err(build_err), Ok(())) => return Err(build_err),
            (Err(build_err), Err(clean_err)) => {
                return Err(anyhow::anyhow!(
                    "{build_err:#}\n  (cleaning the source directory afterwards also failed: {clean_err:#})"
                ));
            }
        };

        let ko_name = format!("{}.ko", module.name);
        crate::errors::ctx(
            fs::create_dir_all(staging),
            &format!("Failed to create staging dir: {}", staging.display()),
        )?;
        let ko_dst = staging.join(&ko_name);
        fs::write(&ko_dst, &ko_bytes)
            .with_context(|| format!("Failed to write {ko_name} to staging"))?;
        crate::platform::set_permissions_mode(&ko_dst, ko_mode)
            .with_context(|| format!("Failed to set mode on {}", ko_dst.display()))?;

        Ok(())
    }

    /// Execute a full linux-module.yaml build into staging, check against
    /// the plan, and move the planned `.ko` files into `output_dir`.
    fn execute_build(
        &self,
        ctx: &crate::build_context::BuildContext,
        yaml_path: &Path,
        instance_name: &str,
    ) -> Result<()> {
        let manifest = Self::parse_manifest(yaml_path)?;
        let anchor_dir = crate::processor::parent_dir_or_empty(yaml_path);
        let output_dir = Self::output_dir_for(yaml_path);
        let flat = yaml_path.to_string_lossy().replace('/', "_");
        let staging = fresh_state_dir(instance_name, &format!(".{flat}.staging"))?;

        let mut planned_relative = BTreeSet::new();
        for module in &manifest.modules {
            planned_relative.insert(format!("{}.ko", module.name));
            Self::build_module(ctx, &manifest, anchor_dir, module, &staging)?;
        }

        install_staged(
            &staging,
            &planned_relative,
            &output_dir,
            instance_name,
            "linux_module",
            false,
        )
    }
}

impl Processor for LinuxModuleProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    /// One product per module in each scanned manifest; the module sources
    /// are read from the manifest on disk, not from the index.
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
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

    /// After discovery: the `make` of every discovered manifest, `uname`
    /// when any of them leaves `kdir` unset, then the config's
    /// `required_tools`. Before discovery the manifests are unread, and the
    /// list is what the manifest defaults imply: `make` and `uname`.
    fn required_tools(&self) -> Vec<String> {
        let discovered = self
            .manifest_tools
            .lock()
            .clone()
            .unwrap_or_else(|| ManifestTools {
                make: BTreeSet::from([default_make_tool()]),
                needs_uname: true,
            });
        let mut tools: Vec<String> = discovered.make.into_iter().collect();
        if discovered.needs_uname {
            tools.push("uname".to_string());
        }
        crate::processor::with_declared_tools(tools, &self.config.standard)
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
        let config_hash = output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        );
        let extra = resolve_dep_inputs_and_auto(&self.config.standard)?;

        let mut planned = Vec::new();
        for yaml_path in files {
            let manifest = match Self::parse_manifest(&yaml_path) {
                Ok(m) => m,
                Err(e) => {
                    anyhow::bail!("Failed to parse {}: {}", yaml_path.display(), e);
                }
            };
            {
                let mut tools = self.manifest_tools.lock();
                let tools = tools.get_or_insert_with(ManifestTools::default);
                tools.make.insert(manifest.make.clone());
                tools.needs_uname |= manifest.kdir.is_none();
            }

            let anchor_dir = crate::processor::parent_dir_or_empty(&yaml_path);
            let output_dir = Self::output_dir_for(&yaml_path);

            for module in &manifest.modules {
                let mut sources = vec![yaml_path.clone()];
                for source in &module.sources {
                    sources.push(crate::processor::resolve_anchor_path(anchor_dir, source));
                }
                planned.push(PlannedOutput {
                    path: output_dir.join(format!("{}.ko", module.name)),
                    sources,
                });
            }
        }
        add_planned_products(graph, &planned, instance_name, &config_hash, &extra)
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let yaml_path = product.primary_input();
        let display_dir = anchor_display_dir(yaml_path);
        let instance_name = &product.processor;
        let guard = self.run_guard(yaml_path);
        guard.run("linux_module", || {
            self.execute_build(ctx, yaml_path, instance_name)
                .with_context(|| format!("linux_module build failed in {display_dir}"))
        })?;
        require_planned_output(product, "linux_module")
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(LinuxModuleProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 2,
        name: "linux_module",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[],
        omit_standard_fields: &["command", "formats", "args", "output_dir", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["linux-module.yaml"], src_exclude_dirs: &[] }),
        defaults: None,
        defconfig_json: crate::registries::default_config_json::<LinuxModuleConfig>,
        keywords: &["c", "linux", "kernel", "module", "builder"],
        description: "Build Linux kernel modules from linux-module.yaml manifests",
        is_builtin: false,
        is_rust: false,
        supports_batch: false,
        caches_outputs: true,
        max_jobs_cap: None,
    }
}
