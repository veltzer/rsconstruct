use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{LinuxModuleConfig, LinuxModuleManifest, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, run_command, check_command_output, anchor_display_dir};

pub struct LinuxModuleProcessor {
    base: ProcessorBase,
    config: LinuxModuleConfig,
}

impl LinuxModuleProcessor {
    pub fn new(config: LinuxModuleConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::LINUX_MODULE,
                "Build Linux kernel modules from linux-module.yaml manifests",
            ),
            config,
        }
    }

    /// Parse a linux-module.yaml file.
    fn parse_manifest(yaml_path: &Path) -> Result<LinuxModuleManifest> {
        let content = fs::read_to_string(yaml_path)
            .with_context(|| format!("Failed to read {}", yaml_path.display()))?;
        let manifest: LinuxModuleManifest = serde_yml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", yaml_path.display()))?;
        Ok(manifest)
    }

    /// Compute the output directory for a linux-module.yaml file.
    fn output_dir_for(yaml_path: &Path) -> PathBuf {
        let anchor_dir = yaml_path.parent().unwrap_or(Path::new(""));
        if anchor_dir.as_os_str().is_empty() {
            PathBuf::from("out/linux-module")
        } else {
            Path::new("out/linux-module").join(anchor_dir)
        }
    }

    /// Get the default KDIR path from the running kernel.
    fn default_kdir() -> String {
        let uname = Command::new("uname").arg("-r").output();
        match uname {
            Ok(output) if output.status.success() => {
                let release = String::from_utf8_lossy(&output.stdout).trim().to_string();
                format!("/lib/modules/{release}/build")
            }
            _ => "/lib/modules/$(uname -r)/build".into(),
        }
    }

    /// Generate a Kbuild file for building a kernel module.
    fn write_kbuild(module_dir: &Path, module: &crate::config::LinuxModuleModuleDef) -> Result<()> {
        let mut content = format!("obj-m := {}.o\n", module.name);

        let objs: Vec<String> = module.sources.iter()
            .map(|s| {
                let p = Path::new(s);
                let stem = p.file_stem().unwrap_or_default().to_string_lossy();
                format!("{stem}.o")
            })
            .collect();
        content.push_str(&format!("{}-objs := {}\n", module.name, objs.join(" ")));

        if !module.extra_cflags.is_empty() {
            content.push_str(&format!("ccflags-y := {}\n", module.extra_cflags.join(" ")));
        }

        fs::write(module_dir.join("Kbuild"), &content)
            .with_context(|| format!("Failed to write Kbuild in {}", module_dir.display()))?;
        Ok(())
    }

    /// Build a single kernel module. Runs make in the module's source directory.
    fn build_module(manifest: &LinuxModuleManifest, anchor_dir: &Path, module: &crate::config::LinuxModuleModuleDef, output_dir: &Path) -> Result<()> {
        let module_dir = if anchor_dir.as_os_str().is_empty() {
            std::env::current_dir()?
        } else {
            std::env::current_dir()?.join(anchor_dir)
        };

        let kdir = manifest.kdir.clone().unwrap_or_else(Self::default_kdir);

        Self::write_kbuild(&module_dir, module)?;

        let mut cmd = Command::new(&manifest.make);
        cmd.arg("-C").arg(&kdir);
        if let Some(ref arch) = manifest.arch {
            cmd.arg(format!("ARCH={arch}"));
        }
        if let Some(ref cross) = manifest.cross_compile {
            cmd.arg(format!("CROSS_COMPILE={cross}"));
        }
        cmd.arg(format!("M={}", module_dir.display()));
        cmd.arg(format!("V={}", manifest.v));
        cmd.arg(format!("W={}", manifest.w));
        cmd.arg("modules");
        cmd.current_dir(&module_dir);

        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("make modules for {}", module.name))?;

        // Copy the .ko file to the output directory
        let ko_name = format!("{}.ko", module.name);
        let ko_src = module_dir.join(&ko_name);
        if ko_src.exists() {
            ctx!(fs::create_dir_all(output_dir), format!("Failed to create output dir: {}", output_dir.display()))?;
            fs::copy(&ko_src, output_dir.join(&ko_name))
                .with_context(|| format!("Failed to copy {} to output", ko_name))?;
        }

        // Clean up build artifacts from source directory
        let mut clean_cmd = Command::new(&manifest.make);
        clean_cmd.arg("-C").arg(&kdir);
        if let Some(ref arch) = manifest.arch {
            clean_cmd.arg(format!("ARCH={arch}"));
        }
        if let Some(ref cross) = manifest.cross_compile {
            clean_cmd.arg(format!("CROSS_COMPILE={cross}"));
        }
        clean_cmd.arg(format!("M={}", module_dir.display()));
        clean_cmd.arg("clean");
        clean_cmd.current_dir(&module_dir);
        let _ = run_command(&mut clean_cmd);

        // Remove the Kbuild we generated
        let _ = fs::remove_file(module_dir.join("Kbuild"));

        Ok(())
    }

    /// Execute a full linux-module.yaml build.
    fn execute_build(&self, yaml_path: &Path) -> Result<()> {
        let manifest = Self::parse_manifest(yaml_path)?;
        let anchor_dir = yaml_path.parent().unwrap_or(Path::new(""));
        let output_dir = Self::output_dir_for(yaml_path);

        for module in &manifest.modules {
            Self::build_module(&manifest, anchor_dir, module, &output_dir)?;
        }

        Ok(())
    }
}

impl Processor for LinuxModuleProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }


    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        self.base.processor_type()
    }


    fn config_json(&self) -> Option<String> {
        crate::processors::ProcessorBase::config_json(&self.config)
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.standard.max_jobs
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["make".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let Some(files) = crate::processors::scan_or_skip(&self.config.standard.scan, file_index) else {
            return Ok(());
        };
        let hash = Some(output_config_hash(&self.config, &[]));
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        for yaml_path in files {
            let manifest = match Self::parse_manifest(&yaml_path) {
                Ok(m) => m,
                Err(e) => {
                    anyhow::bail!("Failed to parse {}: {}", yaml_path.display(), e);
                }
            };

            let anchor_dir = yaml_path.parent().unwrap_or(Path::new(""));
            let output_dir = Self::output_dir_for(&yaml_path);

            let mut inputs: Vec<PathBuf> = Vec::new();
            inputs.push(yaml_path.clone());

            let mut outputs: Vec<PathBuf> = Vec::new();

            for module in &manifest.modules {
                for source in &module.sources {
                    inputs.push(crate::processors::resolve_anchor_path(anchor_dir, source));
                }
                outputs.push(output_dir.join(format!("{}.ko", module.name)));
            }

            inputs.extend_from_slice(&extra);

            graph.add_product(inputs, outputs, instance_name, hash.clone())?;
        }
        Ok(())
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        let yaml_path = product.primary_input();
        let display_dir = anchor_display_dir(yaml_path);
        self.execute_build(yaml_path)
            .with_context(|| format!("linux_module build failed in {}", display_dir))
    }

}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(LinuxModuleProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "linux_module",
        processor_type: crate::processors::ProcessorType::Creator,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::LinuxModuleConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::LinuxModuleConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::LinuxModuleConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::LinuxModuleConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::LinuxModuleConfig>,
    }
}
