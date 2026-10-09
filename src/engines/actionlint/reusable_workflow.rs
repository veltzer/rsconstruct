//! Metadata of local reusable workflows (`uses: ./.github/workflows/x.yml`
//! at a job), a port of actionlint's `reusable_workflow.go`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::action_metadata::scalar_bool;
use super::ast::{WorkflowCallEvent, WorkflowCallInputType};
use super::expr::types::ExprType;
use super::yaml::{self, Kind, Node, go_quote};

#[derive(Debug, Clone)]
pub struct ReusableWorkflowMetadataInput {
    pub name: String,
    pub required: bool,
    pub ty: ExprType,
}

#[derive(Debug, Clone)]
pub struct ReusableWorkflowMetadataSecret {
    pub name: String,
    pub required: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ReusableWorkflowMetadata {
    /// Keyed by lower-cased name.
    pub inputs: BTreeMap<String, ReusableWorkflowMetadataInput>,
    /// Lower-cased name to name as written.
    pub outputs: BTreeMap<String, String>,
    pub secrets: BTreeMap<String, ReusableWorkflowMetadataSecret>,
}

pub struct LocalReusableWorkflowCache {
    root: Option<PathBuf>,
    cwd: PathBuf,
    cache: Mutex<HashMap<String, Option<Arc<ReusableWorkflowMetadata>>>>,
}

fn expected_mapping(where_: &str, n: &Node) -> String {
    format!(
        "yaml: {where_} must be mapping node but {} node was found at line:{}, col:{}",
        n.kind.name(),
        n.line,
        n.column
    )
}

fn cannot_unmarshal(n: &Node, into: &str) -> String {
    format!(
        "yaml: unmarshal errors: line {}: cannot unmarshal {} `{}` into {into}",
        n.line, n.tag, n.value
    )
}

impl LocalReusableWorkflowCache {
    pub fn new(root: Option<PathBuf>, cwd: PathBuf) -> Self {
        Self {
            root,
            cwd,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Remembers that the spec has no usable metadata.
    pub fn write_null(&self, spec: &str) {
        self.cache
            .lock()
            .expect("cache lock")
            .insert(spec.to_string(), None);
    }

    pub fn find_metadata(
        &self,
        spec: &str,
    ) -> Result<Option<Arc<ReusableWorkflowMetadata>>, String> {
        let Some(root) = &self.root else {
            return Ok(None);
        };
        if !spec.starts_with("./") || super::ast::contains_expression(spec) {
            return Ok(None);
        }
        if let Some(m) = self.cache.lock().expect("cache lock").get(spec) {
            return Ok(m.clone());
        }
        let file = root.join(spec.trim_start_matches("./"));
        let src = match std::fs::read(&file) {
            Ok(src) => src,
            Err(e) => {
                self.write_null(spec);
                return Err(format!(
                    "could not read reusable workflow file for {}: open {}: {}",
                    go_quote(spec),
                    file.display(),
                    go_io_error(&e)
                ));
            }
        };
        match parse_reusable_workflow_metadata(&src) {
            Ok(m) => {
                let m = Arc::new(m);
                self.cache
                    .lock()
                    .expect("cache lock")
                    .insert(spec.to_string(), Some(m.clone()));
                Ok(Some(m))
            }
            Err(e) => {
                self.write_null(spec);
                Err(format!(
                    "error while parsing reusable workflow {}: {}",
                    go_quote(spec),
                    e.replace('\n', " ")
                ))
            }
        }
    }

    fn conv_workflow_path_to_spec(&self, p: &str) -> Option<String> {
        let root = self.root.as_ref()?;
        let path = Path::new(p);
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        };
        let abs = normalize(&abs);
        let root = normalize(root);
        let rel = abs.strip_prefix(&root).ok()?;
        let mut spec = rel.to_string_lossy().replace('\\', "/");
        if !spec.starts_with("./") {
            spec = format!("./{spec}");
        }
        Some(spec)
    }

    /// Records the `workflow_call` event of the workflow being linted, so
    /// that another workflow calling it finds its metadata without reading
    /// the file again.
    pub fn write_workflow_call_event(&self, wpath: &str, event: &WorkflowCallEvent) {
        let Some(spec) = self.conv_workflow_path_to_spec(wpath) else {
            return;
        };
        let mut cache = self.cache.lock().expect("cache lock");
        if cache.contains_key(&spec) {
            return;
        }
        let mut m = ReusableWorkflowMetadata::default();
        for i in &event.inputs {
            let ty = match i.ty {
                WorkflowCallInputType::Boolean => ExprType::Bool,
                WorkflowCallInputType::Number => ExprType::Number,
                WorkflowCallInputType::String => ExprType::String,
                WorkflowCallInputType::Invalid => ExprType::Any,
            };
            m.inputs.insert(
                i.id.clone(),
                ReusableWorkflowMetadataInput {
                    name: i.name.value.clone(),
                    required: i.required.as_ref().is_some_and(|r| r.value) && i.default.is_none(),
                    ty,
                },
            );
        }
        for (n, o) in &event.outputs {
            m.outputs.insert(n.clone(), o.name.value.clone());
        }
        for (n, s) in event.secrets.iter().flatten() {
            m.secrets.insert(
                n.clone(),
                ReusableWorkflowMetadataSecret {
                    name: s.name.value.clone(),
                    required: s.required.as_ref().is_some_and(|r| r.value),
                },
            );
        }
        cache.insert(spec, Some(Arc::new(m)));
    }
}

/// Lexically normalizes `.` and `..` components.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn go_io_error(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        _ => e.to_string(),
    }
}

fn decode_inputs(n: &Node) -> Result<BTreeMap<String, ReusableWorkflowMetadataInput>, String> {
    if n.kind != Kind::Mapping {
        return Err(expected_mapping("on.workflow_call.inputs", n));
    }
    let mut md = BTreeMap::new();
    for (k, v) in n.pairs() {
        let mut required = false;
        let mut has_default = false;
        let mut ty = ExprType::Any;
        if v.kind == Kind::Mapping {
            for (fk, fv) in v.pairs() {
                match fk.value.as_str() {
                    "required" => {
                        required =
                            scalar_bool(fv).map_err(|e| format!("yaml: unmarshal errors: {e}"))?;
                    }
                    "default" => has_default = !(fv.kind == Kind::Scalar && fv.tag == "!!null"),
                    "type" => {
                        if fv.kind != Kind::Scalar {
                            return Err(cannot_unmarshal(fv, "string"));
                        }
                        ty = match fv.value.as_str() {
                            "boolean" => ExprType::Bool,
                            "number" => ExprType::Number,
                            "string" => ExprType::String,
                            _ => ExprType::Any,
                        };
                    }
                    _ => {}
                }
            }
        } else if !(v.kind == Kind::Scalar && v.tag == "!!null") {
            return Err(cannot_unmarshal(
                v,
                "actionlint.ReusableWorkflowMetadataInput",
            ));
        }
        md.insert(
            k.value.to_lowercase(),
            ReusableWorkflowMetadataInput {
                name: k.value.clone(),
                required: required && !has_default,
                ty,
            },
        );
    }
    Ok(md)
}

fn decode_secrets(n: &Node) -> Result<BTreeMap<String, ReusableWorkflowMetadataSecret>, String> {
    if n.kind != Kind::Mapping {
        return Err(expected_mapping("on.workflow_call.secrets", n));
    }
    let mut md = BTreeMap::new();
    for (k, v) in n.pairs() {
        let mut required = false;
        if v.kind == Kind::Mapping {
            for (fk, fv) in v.pairs() {
                if fk.value == "required" {
                    required =
                        scalar_bool(fv).map_err(|e| format!("yaml: unmarshal errors: {e}"))?;
                }
            }
        } else if !(v.kind == Kind::Scalar && v.tag == "!!null") {
            return Err(cannot_unmarshal(
                v,
                "actionlint.ReusableWorkflowMetadataSecret",
            ));
        }
        md.insert(
            k.value.to_lowercase(),
            ReusableWorkflowMetadataSecret {
                name: k.value.clone(),
                required,
            },
        );
    }
    Ok(md)
}

fn decode_outputs(n: &Node) -> Result<BTreeMap<String, String>, String> {
    if n.kind != Kind::Mapping {
        return Err(expected_mapping("on.workflow_call.outputs", n));
    }
    Ok(n.pairs()
        .map(|(k, _)| (k.value.to_lowercase(), k.value.clone()))
        .collect())
}

/// Reads the `on.workflow_call` section of a workflow file.
pub fn parse_reusable_workflow_metadata(src: &[u8]) -> Result<ReusableWorkflowMetadata, String> {
    let (doc, _) = yaml::parse(src);
    let doc = doc.map_err(|e| format!("yaml: {}", e.message))?;
    let root = doc.content.first();
    let on = root
        .filter(|r| r.kind == Kind::Mapping)
        .and_then(|r| r.pairs().find(|(k, _)| k.value == "on").map(|(_, v)| v));
    let Some(n) = on else {
        return Err("\"on:\" is not found".to_string());
    };
    match n.kind {
        Kind::Mapping => {
            for (k, v) in n.pairs() {
                if k.value.to_lowercase() == "workflow_call" {
                    let mut m = ReusableWorkflowMetadata::default();
                    if v.kind == Kind::Mapping {
                        for (sk, sv) in v.pairs() {
                            match sk.value.as_str() {
                                "inputs" => m.inputs = decode_inputs(sv)?,
                                "outputs" => m.outputs = decode_outputs(sv)?,
                                "secrets" => m.secrets = decode_secrets(sv)?,
                                _ => {}
                            }
                        }
                    }
                    return Ok(m);
                }
            }
        }
        Kind::Scalar => {
            if n.value.to_lowercase() == "workflow_call" {
                return Ok(ReusableWorkflowMetadata::default());
            }
        }
        Kind::Sequence
            if n.content
                .iter()
                .any(|c| c.value.to_lowercase() == "workflow_call") =>
        {
            return Ok(ReusableWorkflowMetadata::default());
        }
        _ => {}
    }
    Err(format!(
        "\"workflow_call\" event trigger is not found in \"on:\" at line:{}, column:{}",
        n.line, n.column
    ))
}
