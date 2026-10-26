use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    OnceTool, PlannedOutput, add_planned_products, fresh_state_dir, install_staged,
    relative_slash_path, require_planned_output,
};
use crate::config::{StandardConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{Processor, anchor_display_dir, check_command_output, run_command};

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CcLibraryDef {
    pub name: String,
    #[serde(default = "default_cc_lib_type")]
    pub lib_type: String,
    pub sources: Vec<String>,
    #[serde(default)]
    pub include_dirs: Vec<String>,
    #[serde(default)]
    pub cflags: Vec<String>,
    #[serde(default)]
    pub cxxflags: Vec<String>,
    #[serde(default)]
    pub ldflags: Vec<String>,
}

fn default_cc_lib_type() -> String {
    "shared".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CcProgramDef {
    pub name: String,
    pub sources: Vec<String>,
    #[serde(default)]
    pub link: Vec<String>,
    #[serde(default)]
    pub include_dirs: Vec<String>,
    #[serde(default)]
    pub cflags: Vec<String>,
    #[serde(default)]
    pub cxxflags: Vec<String>,
    #[serde(default)]
    pub ldflags: Vec<String>,
}

/// What one target (library or program) adds to the compilation of each of
/// its sources, on top of the manifest's own settings.
struct TargetFlags<'a> {
    include_dirs: &'a [String],
    cflags: &'a [String],
    cxxflags: &'a [String],
    /// Compile position-independent code: the objects of a shared library.
    pic: bool,
}

impl CcLibraryDef {
    /// The library's flags; `pic` when any of its objects go into a `.so`.
    fn target_flags(&self, pic: bool) -> TargetFlags<'_> {
        TargetFlags {
            include_dirs: &self.include_dirs,
            cflags: &self.cflags,
            cxxflags: &self.cxxflags,
            pic,
        }
    }
}

impl CcProgramDef {
    fn target_flags(&self) -> TargetFlags<'_> {
        TargetFlags {
            include_dirs: &self.include_dirs,
            cflags: &self.cflags,
            cxxflags: &self.cxxflags,
            pic: false,
        }
    }
}

/// Parsed contents of a cc.yaml file, resolved against the `[processor.mass_generator.cc]`
/// config: the six inheritable fields (`cc`, `cxx`, `cflags`, `cxxflags`,
/// `ldflags`, `include_dirs`) default to the config's values, so
/// `rsconstruct.toml` sets project-wide defaults and each cc.yaml overrides
/// per directory. Construct via [`CcManifest::parse`] — the raw serde shape
/// lives in `CcManifestRaw` so "field absent" (inherit) is distinguishable
/// from "field explicitly set".
#[derive(Debug, Clone)]
pub struct CcManifest {
    pub cc: String,
    pub cxx: String,
    pub cflags: Vec<String>,
    pub cxxflags: Vec<String>,
    pub ldflags: Vec<String>,
    pub include_dirs: Vec<String>,
    pub libraries: Vec<CcLibraryDef>,
    pub programs: Vec<CcProgramDef>,
}

/// The raw serde shape of cc.yaml. Inheritable fields are optional; absent
/// means "inherit the `[processor.mass_generator.cc]` config value".
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CcManifestRaw {
    cc: Option<String>,
    cxx: Option<String>,
    cflags: Option<Vec<String>>,
    cxxflags: Option<Vec<String>>,
    ldflags: Option<Vec<String>>,
    include_dirs: Option<Vec<String>>,
    #[serde(default)]
    libraries: Vec<CcLibraryDef>,
    #[serde(default)]
    programs: Vec<CcProgramDef>,
}

impl CcManifest {
    /// Parse cc.yaml content, inheriting unset fields from the config.
    pub fn parse(content: &str, defaults: &CcConfig) -> Result<Self, serde_norway::Error> {
        let raw: CcManifestRaw = serde_norway::from_str(content)?;
        Ok(Self {
            cc: raw.cc.unwrap_or_else(|| defaults.cc.clone()),
            cxx: raw.cxx.unwrap_or_else(|| defaults.cxx.clone()),
            cflags: raw.cflags.unwrap_or_else(|| defaults.cflags.clone()),
            cxxflags: raw.cxxflags.unwrap_or_else(|| defaults.cxxflags.clone()),
            ldflags: raw.ldflags.unwrap_or_else(|| defaults.ldflags.clone()),
            include_dirs: raw
                .include_dirs
                .unwrap_or_else(|| defaults.include_dirs.clone()),
            libraries: raw.libraries,
            programs: raw.programs,
        })
    }
}

/// CC (full C/C++ project) config. Custom: cc, cxx, cflags, cxxflags, ldflags, `include_dirs`, `single_invocation`.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CcConfig {
    #[serde(default = "crate::config::default_cc_compiler")]
    pub cc: String,
    #[serde(default = "crate::config::default_cxx_compiler")]
    pub cxx: String,
    #[serde(default)]
    pub cflags: Vec<String>,
    #[serde(default)]
    pub cxxflags: Vec<String>,
    #[serde(default)]
    pub ldflags: Vec<String>,
    #[serde(default)]
    pub include_dirs: Vec<String>,
    #[serde(default)]
    pub single_invocation: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for CcConfig {
    fn default() -> Self {
        Self {
            cc: "gcc".into(),
            cxx: "g++".into(),
            cflags: Vec::new(),
            cxxflags: Vec::new(),
            ldflags: Vec::new(),
            include_dirs: Vec::new(),
            single_invocation: false,
            standard: StandardConfig::default(),
        }
    }
}

pub struct CcProcessor {
    config: CcConfig,
    /// Compilers the discovered cc.yaml manifests actually resolve to.
    /// Filled during `discover` so `required_tools()` names the real
    /// compilers by execution time (the debug declared-tools assertion
    /// checks against it), not just the config defaults.
    manifest_compilers: std::sync::Mutex<std::collections::BTreeSet<String>>,
    runs: Mutex<BTreeMap<PathBuf, Arc<OnceTool>>>,
}

impl CcProcessor {
    pub const fn new(config: CcConfig) -> Self {
        Self {
            config,
            manifest_compilers: std::sync::Mutex::new(std::collections::BTreeSet::new()),
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

    /// Determine whether a source file is C++ based on extension.
    fn is_cxx(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("cc" | "cpp" | "cxx" | "C")
        )
    }

    /// Choose the compiler for a source file.
    fn compiler_for(manifest: &CcManifest, source: &Path) -> String {
        if Self::is_cxx(source) {
            manifest.cxx.clone()
        } else {
            manifest.cc.clone()
        }
    }

    /// The manifest's `cflags` (C) or `cxxflags` (C++).
    fn lang_flags(manifest: &CcManifest, is_cxx: bool) -> &[String] {
        if is_cxx {
            &manifest.cxxflags
        } else {
            &manifest.cflags
        }
    }

    /// The compile flags for one language in one target, in order: the
    /// manifest's `cflags`/`cxxflags`, the target's `cflags`/`cxxflags`,
    /// `-fPIC` for a shared library's objects, then `-I` for the target's
    /// `include_dirs` and after them the manifest's. A C source gets only
    /// the `cflags` at both levels, a C++ source only the `cxxflags`.
    fn compile_flags(manifest: &CcManifest, is_cxx: bool, target: &TargetFlags) -> Vec<String> {
        let mut flags: Vec<String> = Self::lang_flags(manifest, is_cxx).to_vec();
        flags.extend_from_slice(if is_cxx {
            target.cxxflags
        } else {
            target.cflags
        });
        if target.pic {
            flags.push("-fPIC".into());
        }
        for dir in target.include_dirs.iter().chain(&manifest.include_dirs) {
            flags.push(format!("-I{dir}"));
        }
        flags
    }

    /// Parse a cc.yaml file, inheriting unset fields from the
    /// `[processor.mass_generator.cc]` config (project-wide defaults, per-directory
    /// overrides).
    fn parse_manifest(&self, yaml_path: &Path) -> Result<CcManifest> {
        let content = fs::read_to_string(yaml_path)
            .with_context(|| format!("Failed to read {}", yaml_path.display()))?;
        let manifest = CcManifest::parse(&content, &self.config)
            .with_context(|| format!("Failed to parse {}", yaml_path.display()))?;
        Ok(manifest)
    }

    /// Compile a single source file to an object file of `target`, with the
    /// flags of the source's language (see `compile_flags`).
    /// All paths are relative to the project root.
    fn compile_object(
        ctx: &crate::build_context::BuildContext,
        manifest: &CcManifest,
        source: &Path,
        obj: &Path,
        target: &TargetFlags,
    ) -> Result<()> {
        crate::processor::ensure_output_dir(obj)?;
        let compiler = Self::compiler_for(manifest, source);
        let mut cmd = Command::new(&compiler);
        cmd.arg("-c");
        cmd.args(Self::compile_flags(manifest, Self::is_cxx(source), target));
        cmd.arg("-o").arg(obj).arg(source);
        let output = run_command(ctx, &cmd)?;
        check_command_output(
            &output,
            format_args!("{} -c {}", compiler, source.display()),
        )
    }

    /// Build a static library from object files.
    fn build_static_lib(
        ctx: &crate::build_context::BuildContext,
        lib_path: &Path,
        objects: &[PathBuf],
    ) -> Result<()> {
        crate::processor::ensure_output_dir(lib_path)?;
        let mut cmd = Command::new("ar");
        cmd.arg("rcs").arg(lib_path);
        for obj in objects {
            cmd.arg(obj);
        }
        let output = run_command(ctx, &cmd)?;
        check_command_output(&output, format_args!("ar rcs {}", lib_path.display()))
    }

    /// Object file path for a source, mirroring the manifest-relative source
    /// path under the target's obj dir so equal stems in different directories
    /// (`src1/util.c`, `src2/util.c`) cannot collide. `..`/`.` components are
    /// dropped to keep the object inside the obj dir.
    fn object_path_for(target_obj_dir: &Path, source_rel: &str) -> PathBuf {
        let mut rel: PathBuf = Path::new(source_rel)
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(p) => Some(p),
                _ => None,
            })
            .collect();
        rel.set_extension("o");
        target_obj_dir.join(rel)
    }

    /// Build a shared library from object files.
    /// `has_cxx` selects the C++ driver, which C++ objects need at link time.
    fn build_shared_lib(
        ctx: &crate::build_context::BuildContext,
        manifest: &CcManifest,
        lib_path: &Path,
        objects: &[PathBuf],
        ldflags: &[String],
        has_cxx: bool,
    ) -> Result<()> {
        crate::processor::ensure_output_dir(lib_path)?;
        let compiler = if has_cxx { &manifest.cxx } else { &manifest.cc };
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
        let output = run_command(ctx, &cmd)?;
        check_command_output(
            &output,
            format_args!("{} -shared -o {}", compiler, lib_path.display()),
        )
    }

    /// Link object files into an executable.
    /// `has_cxx` selects the C++ driver, which C++ objects need at link time.
    #[allow(clippy::too_many_arguments)]
    fn link_program(
        ctx: &crate::build_context::BuildContext,
        manifest: &CcManifest,
        exe_path: &Path,
        objects: &[PathBuf],
        lib_dir: &Path,
        link_libs: &[String],
        ldflags: &[String],
        has_cxx: bool,
    ) -> Result<()> {
        crate::processor::ensure_output_dir(exe_path)?;
        let compiler = if has_cxx { &manifest.cxx } else { &manifest.cc };
        let mut cmd = Command::new(compiler);
        cmd.arg("-o").arg(exe_path);
        for obj in objects {
            cmd.arg(obj);
        }
        if !link_libs.is_empty() {
            cmd.arg(format!("-L{}", lib_dir.display()));
            for lib in link_libs {
                cmd.arg(format!("-l{lib}"));
            }
        }
        for flag in &manifest.ldflags {
            cmd.arg(flag);
        }
        for flag in ldflags {
            cmd.arg(flag);
        }
        let output = run_command(ctx, &cmd)?;
        check_command_output(
            &output,
            format_args!("{} -o {}", compiler, exe_path.display()),
        )
    }

    /// Single-invocation build for a program (all sources in one command).
    /// One command has one language: C++ when any source is C++, which is
    /// what the C++ driver compiles every file as, so the command carries
    /// the `cxxflags` (manifest's, then the program's); otherwise the
    /// `cflags`. Include dirs follow as for a separate compile.
    fn single_invocation_program(
        ctx: &crate::build_context::BuildContext,
        manifest: &CcManifest,
        exe_path: &Path,
        sources: &[PathBuf],
        lib_dir: &Path,
        prog: &CcProgramDef,
    ) -> Result<()> {
        crate::processor::ensure_output_dir(exe_path)?;
        let has_cxx = sources.iter().any(|s| Self::is_cxx(s));
        let compiler = if has_cxx { &manifest.cxx } else { &manifest.cc };
        let mut cmd = Command::new(compiler);
        cmd.args(Self::compile_flags(manifest, has_cxx, &prog.target_flags()));
        cmd.arg("-o").arg(exe_path);
        for source in sources {
            cmd.arg(source);
        }
        if !prog.link.is_empty() {
            cmd.arg(format!("-L{}", lib_dir.display()));
            for lib in &prog.link {
                cmd.arg(format!("-l{lib}"));
            }
        }
        for flag in &manifest.ldflags {
            cmd.arg(flag);
        }
        for flag in &prog.ldflags {
            cmd.arg(flag);
        }
        let output = run_command(ctx, &cmd)?;
        check_command_output(
            &output,
            format_args!("{} -o {}", compiler, exe_path.display()),
        )
    }

    /// Compute the output directory for a cc.yaml file.
    /// Output goes under `out/processor.mass_generator.cc/<relative-path-to-cc.yaml-dir>/`.
    fn output_dir_for(yaml_path: &Path) -> PathBuf {
        let anchor_dir = crate::processor::parent_dir_or_empty(yaml_path);
        if anchor_dir.as_os_str().is_empty() {
            PathBuf::from("out/processor.mass_generator.cc")
        } else {
            Path::new("out/processor.mass_generator.cc").join(anchor_dir)
        }
    }

    /// Plan all outputs for a single `cc.yaml`: every `.o` (when not
    /// `single_invocation`), `.a`, `.so`, and program binary.
    fn plan_manifest(
        &self,
        yaml_path: &Path,
        manifest: &CcManifest,
    ) -> (PathBuf, Vec<PlannedOutput>, BTreeSet<String>) {
        let anchor_dir = crate::processor::parent_dir_or_empty(yaml_path);
        let output_dir = Self::output_dir_for(yaml_path);
        let obj_dir = output_dir.join("obj");
        let lib_dir = output_dir.join("lib");
        let bin_dir = output_dir.join("bin");

        let mut outputs = Vec::new();
        let mut relative = BTreeSet::new();
        let mut lib_sources_by_name: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();

        for lib in &manifest.libraries {
            let build_shared = matches!(lib.lib_type.as_str(), "shared" | "both");
            let build_static = matches!(lib.lib_type.as_str(), "static" | "both");
            let target_obj_dir = obj_dir.join(&lib.name);

            let mut lib_sources = vec![yaml_path.to_path_buf()];
            let mut resolved_only = Vec::new();
            for source_str in &lib.sources {
                let source = crate::processor::resolve_anchor_path(anchor_dir, source_str);
                let obj = Self::object_path_for(&target_obj_dir, source_str);
                relative.insert(relative_slash_path(&output_dir, &obj));
                outputs.push(PlannedOutput {
                    path: obj,
                    sources: vec![yaml_path.to_path_buf(), source.clone()],
                });
                lib_sources.push(source.clone());
                resolved_only.push(source);
            }
            lib_sources_by_name.insert(lib.name.clone(), resolved_only);

            if build_static {
                let lib_path = lib_dir.join(format!("lib{}.a", lib.name));
                relative.insert(relative_slash_path(&output_dir, &lib_path));
                outputs.push(PlannedOutput {
                    path: lib_path,
                    sources: lib_sources.clone(),
                });
            }
            if build_shared {
                let lib_path = lib_dir.join(format!("lib{}.so", lib.name));
                relative.insert(relative_slash_path(&output_dir, &lib_path));
                outputs.push(PlannedOutput {
                    path: lib_path,
                    sources: lib_sources,
                });
            }
        }

        for prog in &manifest.programs {
            let target_obj_dir = obj_dir.join(&prog.name);
            let mut prog_sources = vec![yaml_path.to_path_buf()];
            for source_str in &prog.sources {
                let source = crate::processor::resolve_anchor_path(anchor_dir, source_str);
                if !self.config.single_invocation {
                    let obj = Self::object_path_for(&target_obj_dir, source_str);
                    relative.insert(relative_slash_path(&output_dir, &obj));
                    outputs.push(PlannedOutput {
                        path: obj,
                        sources: vec![yaml_path.to_path_buf(), source.clone()],
                    });
                }
                prog_sources.push(source);
            }
            for linked in &prog.link {
                if let Some(extra_lib_sources) = lib_sources_by_name.get(linked) {
                    for src in extra_lib_sources {
                        if !prog_sources.contains(src) {
                            prog_sources.push(src.clone());
                        }
                    }
                }
            }
            let exe_path = bin_dir.join(&prog.name);
            relative.insert(relative_slash_path(&output_dir, &exe_path));
            outputs.push(PlannedOutput {
                path: exe_path,
                sources: prog_sources,
            });
        }

        (output_dir, outputs, relative)
    }

    /// Execute a full cc.yaml build into a staging directory, check it against
    /// the plan, and move the planned files into `output_dir`.
    fn execute_build(
        &self,
        ctx: &crate::build_context::BuildContext,
        yaml_path: &Path,
        instance_name: &str,
    ) -> Result<()> {
        let manifest = self.parse_manifest(yaml_path)?;
        let (output_dir, _outputs, planned_relative) = self.plan_manifest(yaml_path, &manifest);
        let anchor_dir = crate::processor::parent_dir_or_empty(yaml_path);
        let flat = yaml_path.to_string_lossy().replace('/', "_");
        let staging = fresh_state_dir(instance_name, &format!(".{flat}.staging"))?;
        let obj_dir = staging.join("obj");
        let lib_dir = staging.join("lib");
        let bin_dir = staging.join("bin");

        // Build libraries
        for lib in &manifest.libraries {
            let build_shared = matches!(lib.lib_type.as_str(), "shared" | "both");
            let build_static = matches!(lib.lib_type.as_str(), "static" | "both");
            let target = lib.target_flags(build_shared);

            let target_obj_dir = obj_dir.join(&lib.name);
            let mut objects = Vec::new();
            for source_str in &lib.sources {
                let source = crate::processor::resolve_anchor_path(anchor_dir, source_str);
                let obj = Self::object_path_for(&target_obj_dir, source_str);
                Self::compile_object(ctx, &manifest, &source, &obj, &target)?;
                objects.push(obj);
            }
            let has_cxx = lib.sources.iter().any(|s| Self::is_cxx(Path::new(s)));

            if build_static {
                let lib_path = lib_dir.join(format!("lib{}.a", lib.name));
                Self::build_static_lib(ctx, &lib_path, &objects)?;
            }
            if build_shared {
                let lib_path = lib_dir.join(format!("lib{}.so", lib.name));
                Self::build_shared_lib(ctx, &manifest, &lib_path, &objects, &lib.ldflags, has_cxx)?;
            }
        }

        // Build programs
        for prog in &manifest.programs {
            let exe_path = bin_dir.join(&prog.name);

            let sources: Vec<PathBuf> = prog
                .sources
                .iter()
                .map(|s| crate::processor::resolve_anchor_path(anchor_dir, s))
                .collect();

            if self.config.single_invocation {
                Self::single_invocation_program(
                    ctx, &manifest, &exe_path, &sources, &lib_dir, prog,
                )?;
            } else {
                let target_obj_dir = obj_dir.join(&prog.name);
                let mut objects = Vec::new();
                let target = prog.target_flags();

                for (source_str, source) in prog.sources.iter().zip(&sources) {
                    let obj = Self::object_path_for(&target_obj_dir, source_str);
                    Self::compile_object(ctx, &manifest, source, &obj, &target)?;
                    objects.push(obj);
                }
                let has_cxx = sources.iter().any(|s| Self::is_cxx(s));
                Self::link_program(
                    ctx,
                    &manifest,
                    &exe_path,
                    &objects,
                    &lib_dir,
                    &prog.link,
                    &prog.ldflags,
                    has_cxx,
                )?;
            }
        }

        install_staged(
            &staging,
            &planned_relative,
            &output_dir,
            instance_name,
            "cc",
            false,
        )
    }
}

impl Processor for CcProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    /// Products come from each scanned manifest on disk. `discover` also
    /// records each manifest's compilers in `manifest_compilers`, but only
    /// ever adds to that set, so discovering file by file builds the same
    /// set as one pass over everything.
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

    fn required_tools(&self) -> Vec<String> {
        let mut tools: Vec<String> = vec![
            self.config.cc.clone(),
            self.config.cxx.clone(),
            "ar".to_string(),
        ];
        for compiler in self.manifest_compilers.lock().unwrap().iter() {
            if !tools.contains(compiler) {
                tools.push(compiler.clone());
            }
        }
        tools
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
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        let mut all_planned = Vec::new();
        for yaml_path in files {
            let manifest = match self.parse_manifest(&yaml_path) {
                Ok(m) => m,
                Err(e) => {
                    anyhow::bail!("Failed to parse {}: {}", yaml_path.display(), e);
                }
            };

            {
                let mut compilers = self.manifest_compilers.lock().unwrap();
                compilers.insert(manifest.cc.clone());
                compilers.insert(manifest.cxx.clone());
            }

            let (_output_dir, outputs, _relative) = self.plan_manifest(&yaml_path, &manifest);
            all_planned.extend(outputs);
        }
        add_planned_products(graph, &all_planned, instance_name, &config_hash, &extra)
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let yaml_path = product.primary_input();
        let display_dir = anchor_display_dir(yaml_path);
        let instance_name = &product.processor;
        let guard = self.run_guard(yaml_path);
        guard.run("cc", || {
            self.execute_build(ctx, yaml_path, instance_name)
                .with_context(|| format!("cc build failed in {display_dir}"))
        })?;
        require_planned_output(product, "cc")
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(CcProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 2,
        name: "cc",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "cc", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Default C compiler executable (overridable per cc.yaml)" },
            crate::config::FieldSpec { name: "cxx", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Default C++ compiler executable (overridable per cc.yaml)" },
            crate::config::FieldSpec { name: "cflags", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Default C compiler flags (overridable per cc.yaml)" },
            crate::config::FieldSpec { name: "cxxflags", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Default C++ compiler flags (overridable per cc.yaml)" },
            crate::config::FieldSpec { name: "ldflags", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Default linker flags (overridable per cc.yaml)" },
            crate::config::FieldSpec { name: "include_dirs", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Default header search directories (overridable per cc.yaml)" },
            crate::config::FieldSpec { name: "single_invocation", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Compile all sources in one compiler call" },
        ],
        omit_standard_fields: &["command", "formats", "args", "dep_auto", "output_dir"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["cc.yaml"], src_exclude_dirs: &[] }),
        defaults: None,
        defconfig_json: crate::registries::default_config_json::<CcConfig>,
        keywords: &["c", "cpp", "builder", "compiler", "gcc", "clang", "cc", "h", "hpp"],
        description: "Build C/C++ projects from cc.yaml manifests",
        is_builtin: false,
        is_rust: false,
        supports_batch: false,
        caches_outputs: true,
        max_jobs_cap: Some(1),
    }
}
