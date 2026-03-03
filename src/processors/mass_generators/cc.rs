use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{CcConfig, CcManifest, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, scan_root_valid, run_command, check_command_output};

pub struct CcProcessor {
    config: CcConfig,
}

impl CcProcessor {
    pub fn new(config: CcConfig) -> Self {
        Self { config }
    }

    /// Determine whether a source file is C++ based on extension.
    fn is_cxx(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("cc" | "cpp" | "cxx" | "C")
        )
    }

    /// Choose the compiler for a source file.
    fn compiler_for(&self, manifest: &CcManifest, source: &Path) -> String {
        if Self::is_cxx(source) {
            manifest.cxx.clone()
        } else {
            manifest.cc.clone()
        }
    }

    /// Choose cflags or cxxflags for a source file.
    fn lang_flags_for<'a>(manifest: &'a CcManifest, source: &Path) -> &'a [String] {
        if Self::is_cxx(source) {
            &manifest.cxxflags
        } else {
            &manifest.cflags
        }
    }

    /// Parse a cc.yaml file.
    fn parse_manifest(yaml_path: &Path) -> Result<CcManifest> {
        let content = fs::read_to_string(yaml_path)
            .with_context(|| format!("Failed to read {}", yaml_path.display()))?;
        let manifest: CcManifest = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", yaml_path.display()))?;
        Ok(manifest)
    }

    /// Compile a single source file to an object file.
    fn compile_object(&self, manifest: &CcManifest, source: &Path, obj: &Path, extra_cflags: &[String]) -> Result<()> {
        if let Some(parent) = obj.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create object directory: {}", parent.display()))?;
        }
        let compiler = self.compiler_for(manifest, source);
        let mut cmd = Command::new(&compiler);
        cmd.arg("-c");
        // Global flags
        for flag in Self::lang_flags_for(manifest, source) {
            cmd.arg(flag);
        }
        // Extra flags (per-target)
        for flag in extra_cflags {
            cmd.arg(flag);
        }
        // Include dirs
        for dir in &manifest.include_dirs {
            cmd.arg(format!("-I{}", dir));
        }
        cmd.arg("-o").arg(obj).arg(source);
        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("{} -c {}", compiler, source.display()))
    }

    /// Build a static library from object files.
    fn build_static_lib(lib_path: &Path, objects: &[PathBuf]) -> Result<()> {
        if let Some(parent) = lib_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut cmd = Command::new("ar");
        cmd.arg("rcs").arg(lib_path);
        for obj in objects {
            cmd.arg(obj);
        }
        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("ar rcs {}", lib_path.display()))
    }

    /// Build a shared library from object files.
    fn build_shared_lib(&self, manifest: &CcManifest, lib_path: &Path, objects: &[PathBuf], ldflags: &[String]) -> Result<()> {
        if let Some(parent) = lib_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let compiler = &manifest.cc;
        let mut cmd = Command::new(compiler);
        cmd.arg("-shared").arg("-o").arg(lib_path);
        for obj in objects {
            cmd.arg(obj);
        }
        for flag in &manifest.ldflags {
            cmd.arg(flag);
        }
        for flag in ldflags {
            cmd.arg(flag);
        }
        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("{} -shared -o {}", compiler, lib_path.display()))
    }

    /// Link object files into an executable.
    fn link_program(&self, manifest: &CcManifest, exe_path: &Path, objects: &[PathBuf], lib_dir: &Path, link_libs: &[String], ldflags: &[String]) -> Result<()> {
        if let Some(parent) = exe_path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Use C++ compiler if any object came from C++ source
        let compiler = &manifest.cc;
        let mut cmd = Command::new(compiler);
        cmd.arg("-o").arg(exe_path);
        for obj in objects {
            cmd.arg(obj);
        }
        if !link_libs.is_empty() {
            cmd.arg(format!("-L{}", lib_dir.display()));
            for lib in link_libs {
                cmd.arg(format!("-l{}", lib));
            }
        }
        for flag in &manifest.ldflags {
            cmd.arg(flag);
        }
        for flag in ldflags {
            cmd.arg(flag);
        }
        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("{} -o {}", compiler, exe_path.display()))
    }

    /// Single-invocation build for a program (all sources in one command).
    fn single_invocation_program(&self, manifest: &CcManifest, exe_path: &Path, sources: &[PathBuf], lib_dir: &Path, link_libs: &[String], ldflags: &[String]) -> Result<()> {
        if let Some(parent) = exe_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let has_cxx = sources.iter().any(|s| Self::is_cxx(s));
        let compiler = if has_cxx { &manifest.cxx } else { &manifest.cc };
        let mut cmd = Command::new(compiler);
        // Add global flags
        let global_flags = if has_cxx { &manifest.cxxflags } else { &manifest.cflags };
        for flag in global_flags {
            cmd.arg(flag);
        }
        for dir in &manifest.include_dirs {
            cmd.arg(format!("-I{}", dir));
        }
        cmd.arg("-o").arg(exe_path);
        for source in sources {
            cmd.arg(source);
        }
        if !link_libs.is_empty() {
            cmd.arg(format!("-L{}", lib_dir.display()));
            for lib in link_libs {
                cmd.arg(format!("-l{}", lib));
            }
        }
        for flag in &manifest.ldflags {
            cmd.arg(flag);
        }
        for flag in ldflags {
            cmd.arg(flag);
        }
        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("{} -o {}", compiler, exe_path.display()))
    }

    /// Execute a full cc.yaml build.
    fn execute_build(&self, yaml_path: &Path) -> Result<()> {
        let manifest = Self::parse_manifest(yaml_path)?;
        let project_dir = yaml_path.parent().unwrap_or(Path::new(""));
        let output_dir = if project_dir.as_os_str().is_empty() {
            PathBuf::from(&manifest.output_dir)
        } else {
            project_dir.join(&manifest.output_dir)
        };
        let obj_dir = output_dir.join("obj");
        let lib_dir = output_dir.join("lib");
        let bin_dir = output_dir.join("bin");

        // Build libraries
        for lib in &manifest.libraries {
            let build_shared = matches!(lib.lib_type.as_str(), "shared" | "both");
            let build_static = matches!(lib.lib_type.as_str(), "static" | "both");

            let mut extra_cflags = lib.cflags.clone();
            if build_shared {
                extra_cflags.push("-fPIC".into());
            }
            // Add library-specific include dirs
            for dir in &lib.include_dirs {
                extra_cflags.push(format!("-I{}", dir));
            }

            // Compile objects
            let target_obj_dir = obj_dir.join(&lib.name);
            let mut objects = Vec::new();
            for source_str in &lib.sources {
                let source = if project_dir.as_os_str().is_empty() {
                    PathBuf::from(source_str)
                } else {
                    project_dir.join(source_str)
                };
                let obj_name = format!("{}.o", source.file_stem().context("source has no stem")?.to_string_lossy());
                let obj = target_obj_dir.join(&obj_name);
                if !self.config.single_invocation {
                    self.compile_object(&manifest, &source, &obj, &extra_cflags)?;
                }
                objects.push(obj);
            }

            if self.config.single_invocation {
                // For libraries in single invocation mode, still need to compile objects
                // since we need .o files for ar/linking
                for (i, source_str) in lib.sources.iter().enumerate() {
                    let source = if project_dir.as_os_str().is_empty() {
                        PathBuf::from(source_str)
                    } else {
                        project_dir.join(source_str)
                    };
                    self.compile_object(&manifest, &source, &objects[i], &extra_cflags)?;
                }
            }

            if build_static {
                let lib_path = lib_dir.join(format!("lib{}.a", lib.name));
                Self::build_static_lib(&lib_path, &objects)?;
            }
            if build_shared {
                let lib_path = lib_dir.join(format!("lib{}.so", lib.name));
                self.build_shared_lib(&manifest, &lib_path, &objects, &lib.ldflags)?;
            }
        }

        // Build programs
        for prog in &manifest.programs {
            let exe_path = bin_dir.join(&prog.name);
            let sources: Vec<PathBuf> = prog.sources.iter().map(|s| {
                if project_dir.as_os_str().is_empty() {
                    PathBuf::from(s)
                } else {
                    project_dir.join(s)
                }
            }).collect();

            if self.config.single_invocation {
                self.single_invocation_program(&manifest, &exe_path, &sources, &lib_dir, &prog.link, &prog.ldflags)?;
            } else {
                let target_obj_dir = obj_dir.join(&prog.name);
                let mut objects = Vec::new();

                let mut extra_cflags: Vec<String> = prog.cflags.clone();
                for dir in &prog.include_dirs {
                    extra_cflags.push(format!("-I{}", dir));
                }

                for source in &sources {
                    let obj_name = format!("{}.o", source.file_stem().context("source has no stem")?.to_string_lossy());
                    let obj = target_obj_dir.join(&obj_name);
                    self.compile_object(&manifest, source, &obj, &extra_cflags)?;
                    objects.push(obj);
                }
                self.link_program(&manifest, &exe_path, &objects, &lib_dir, &prog.link, &prog.ldflags)?;
            }
        }

        Ok(())
    }
}

impl ProductDiscovery for CcProcessor {
    fn description(&self) -> &str {
        "Build C/C++ projects from cc.yaml manifests"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::MassGenerator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan) && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.cc.clone(), self.config.cxx.clone(), "ar".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !scan_root_valid(&self.config.scan) {
            return Ok(());
        }
        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }
        let hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        for yaml_path in files {
            // Parse the manifest to discover source files as inputs
            let manifest = match Self::parse_manifest(&yaml_path) {
                Ok(m) => m,
                Err(e) => {
                    anyhow::bail!("Failed to parse {}: {}", yaml_path.display(), e);
                }
            };

            let project_dir = yaml_path.parent().unwrap_or(Path::new(""));

            // Collect all source files as inputs
            let mut inputs: Vec<PathBuf> = Vec::new();
            inputs.push(yaml_path.clone()); // cc.yaml itself is an input

            for lib in &manifest.libraries {
                for source in &lib.sources {
                    let path = if project_dir.as_os_str().is_empty() {
                        PathBuf::from(source)
                    } else {
                        project_dir.join(source)
                    };
                    inputs.push(path);
                }
            }
            for prog in &manifest.programs {
                for source in &prog.sources {
                    let path = if project_dir.as_os_str().is_empty() {
                        PathBuf::from(source)
                    } else {
                        project_dir.join(source)
                    };
                    inputs.push(path);
                }
            }

            inputs.extend_from_slice(&extra);

            if self.config.cache_output_dir {
                let output_dir = if project_dir.as_os_str().is_empty() {
                    PathBuf::from(&manifest.output_dir)
                } else {
                    project_dir.join(&manifest.output_dir)
                };
                graph.add_product_with_output_dir(
                    inputs, vec![], crate::processors::names::CC, hash.clone(), output_dir,
                )?;
            } else {
                graph.add_product(inputs, vec![], crate::processors::names::CC, hash.clone())?;
            }
        }
        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_build(product.primary_input())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        if let Some(ref output_dir) = product.output_dir
            && output_dir.exists()
        {
            if verbose {
                println!("Removing cc output directory: {}", output_dir.display());
            }
            fs::remove_dir_all(output_dir.as_ref())?;
            return Ok(1);
        }
        Ok(0)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
