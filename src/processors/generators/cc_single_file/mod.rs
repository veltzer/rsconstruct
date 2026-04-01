mod source_flags;

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{CcSingleFileConfig, CompilerProfile, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, clean_outputs, check_command_output, format_command, run_command};

use source_flags::{SourceFlags, parse_source_flags, should_exclude_for_profile};

pub struct CcSingleFileProcessor {
    config: CcSingleFileConfig,
    profiles: Vec<CompilerProfile>,
    output_dir: PathBuf,
}

impl CcSingleFileProcessor {
    pub fn new(config: CcSingleFileConfig) -> Self {
        let profiles = config.get_compiler_profiles();
        let output_dir = PathBuf::from("out/cc_single_file");
        Self {
            config,
            profiles,
            output_dir,
        }
    }

    /// Get the first source directory from scan config (relative path)
    fn source_dir(&self) -> PathBuf {
        PathBuf::from(self.config.scan.scan_dirs().first().map(|s| s.as_str()).unwrap_or(""))
    }

    /// Check if cc processing should be enabled
    fn should_process(&self) -> bool {
        let src = self.source_dir();
        src.as_os_str().is_empty() || src.exists()
    }

    /// Find all C/C++ source files. Returns (path, is_cpp) pairs.
    fn find_source_files(&self, file_index: &FileIndex) -> Vec<(PathBuf, bool)> {
        file_index.scan(&self.config.scan, true)
            .into_iter()
            .map(|p| {
                let is_cpp = p.extension().and_then(|s| s.to_str()) == Some("cc");
                (p, is_cpp)
            })
            .collect()
    }

    /// Get executable path for a source file with a specific compiler profile.
    /// Preserves the full source path relative to project root.
    /// E.g., src/a.cc -> out/cc_single_file/src/a.elf
    /// With profile: src/a.cc -> out/cc_single_file/<profile_name>/src/a.elf
    fn get_executable_path(&self, source: &Path, profile: &CompilerProfile) -> PathBuf {
        // Keep the full source path, just change the extension
        let stem = source.with_extension("");
        let name = format!("{}{}", stem.display(), profile.output_suffix);

        if profile.name.is_empty() {
            self.output_dir.join(name)
        } else {
            self.output_dir.join(&profile.name).join(name)
        }
    }

    /// Find a compiler profile by name
    fn find_profile(&self, name: &str) -> Option<&CompilerProfile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// Add include paths and compile flags (before, base, after) to a command.
    fn add_compile_flags(&self, cmd: &mut Command, profile: &CompilerProfile, is_cpp: bool, source_flags: &SourceFlags) {
        let flags = if is_cpp { &profile.cxxflags } else { &profile.cflags };
        for inc in &self.config.include_paths {
            cmd.arg(format!("-I{}", inc));
        }
        for arg in &source_flags.compile_args_before {
            cmd.arg(arg);
        }
        for flag in flags {
            cmd.arg(flag);
        }
        for arg in &source_flags.compile_args_after {
            cmd.arg(arg);
        }
    }

    /// Compile a single source file directly to an executable using a specific profile.
    fn compile_source(&self, source: &Path, executable: &Path, profile: &CompilerProfile, is_cpp: bool) -> Result<()> {
        let compiler = if is_cpp { &profile.cxx } else { &profile.cc };
        let source_flags = parse_source_flags(source, &profile.name)?;

        // Ensure output directory exists
        crate::processors::ensure_output_dir(executable)?;

        let mut cmd = Command::new(compiler);
        self.add_compile_flags(&mut cmd, profile, is_cpp, &source_flags);
        cmd.arg("-o").arg(executable).arg(source);
        for arg in &source_flags.link_args_before {
            cmd.arg(arg);
        }
        for flag in &profile.ldflags {
            cmd.arg(flag);
        }
        for arg in &source_flags.link_args_after {
            cmd.arg(arg);
        }

        if crate::runtime_flags::show_child_processes() {
            let profile_tag = if profile.name.is_empty() { String::new() } else { format!(":{}", profile.name) };
            println!("[{}{}] {}", crate::processors::names::CC_SINGLE_FILE, profile_tag, format_command(&cmd));
        }

        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("Compilation of {}", source.display()))
    }

    /// Extract profile name from product metadata
    fn get_profile_from_product(&self, product: &Product) -> Result<&CompilerProfile> {
        // Profile name is stored in the output path structure
        // out/cc_single_file/<profile_name>/... or out/cc_single_file/... (legacy)
        if let Some(output) = product.outputs.first()
            && let Ok(relative) = output.strip_prefix(&self.output_dir) {
                // Check if first component is a profile name
                if let Some(first) = relative.components().next() {
                    let first_str = first.as_os_str().to_string_lossy();
                    if let Some(profile) = self.find_profile(&first_str) {
                        return Ok(profile);
                    }
                }
            }
        // Fall back to first profile (legacy mode)
        self.profiles.first()
            .ok_or_else(|| anyhow::anyhow!("no compiler profiles configured"))
    }

    /// Shared implementation for discover and discover_for_clean.
    /// When `for_clean` is true, skips config hash and extra inputs (only needs output mapping).
    fn discover_impl(&self, graph: &mut BuildGraph, file_index: &FileIndex, for_clean: bool) -> Result<()> {
        if !self.should_process() {
            return Ok(());
        }

        let source_files = self.find_source_files(file_index);
        if source_files.is_empty() {
            return Ok(());
        }

        let cfg_hash = if for_clean { None } else { Some(output_config_hash(&self.config, &[])) };
        let extra = if for_clean { Vec::new() } else { resolve_extra_inputs(&self.config.extra_inputs)? };

        for profile in &self.profiles {
            let variant = if profile.name.is_empty() { None } else { Some(profile.name.as_str()) };

            for (source, _is_cpp) in &source_files {
                if should_exclude_for_profile(source, &profile.name) {
                    continue;
                }

                let executable = self.get_executable_path(source, profile);

                let mut inputs = Vec::with_capacity(1 + extra.len());
                inputs.push(source.clone());
                inputs.extend_from_slice(&extra);

                graph.add_product_with_variant(
                    inputs,
                    vec![executable],
                    crate::processors::names::CC_SINGLE_FILE,
                    cfg_hash.clone(),
                    variant,
                )?;
            }
        }

        Ok(())
    }
}

impl ProductDiscovery for CcSingleFileProcessor {
    fn description(&self) -> &str {
        "Compile C/C++ source files into executables (single-file)"
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        crate::processors::ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.should_process() && !self.find_source_files(file_index).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        // Collect unique compilers from all profiles
        let mut tools: Vec<String> = self.profiles.iter()
            .flat_map(|p| vec![p.cc.clone(), p.cxx.clone()])
            .collect();
        tools.sort();
        tools.dedup();
        tools
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        self.discover_impl(graph, file_index, false)
    }

    /// Fast discovery for clean: only find outputs, skip header scanning
    fn discover_for_clean(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        self.discover_impl(graph, file_index, true)
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let source = product.primary_input();
        let executable = product.primary_output();
        let is_cpp = source.extension().and_then(|s| s.to_str()) == Some("cc");
        let profile = self.get_profile_from_product(product)?;
        self.compile_source(source, executable, profile, is_cpp)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::CC_SINGLE_FILE, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
