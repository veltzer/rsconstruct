//! Metadata of actions (`action.yml`), local and popular, a port of
//! actionlint's `action_metadata.go` plus the embedded popular-actions data
//! set generated from `popular_actions.go`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use serde::Deserialize;

use super::yaml::{self, Kind, Node, go_quote};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ActionMetadataInput {
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub deprecation_message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ActionMetadataRuns {
    pub using: String,
    pub main: String,
    pub pre: String,
    pub pre_if: String,
    pub post: String,
    pub post_if: String,
    /// `Some` when `steps:` is present, even empty.
    pub steps: Option<usize>,
    pub image: String,
    pub pre_entrypoint: String,
    pub entrypoint: String,
    pub post_entrypoint: String,
    pub args: bool,
    pub env: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ActionMetadataBranding {
    pub icon: String,
    pub color: String,
}

#[derive(Debug, Clone, Default)]
pub struct ActionMetadata {
    dir: PathBuf,
    file: String,
    pub name: String,
    pub description: String,
    /// Keyed by lower-cased input name.
    pub inputs: BTreeMap<String, ActionMetadataInput>,
    /// Lower-cased output name to its name as written.
    pub outputs: BTreeMap<String, String>,
    pub skip_inputs: bool,
    pub skip_outputs: bool,
    pub runs: ActionMetadataRuns,
    pub branding: ActionMetadataBranding,
}

impl ActionMetadata {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(&self.file)
    }
}

#[derive(Deserialize)]
struct PopularAction {
    name: String,
    inputs: BTreeMap<String, ActionMetadataInput>,
    outputs: BTreeMap<String, String>,
    skip_inputs: bool,
    skip_outputs: bool,
}

#[derive(Deserialize)]
struct PopularActionsFile {
    actions: BTreeMap<String, PopularAction>,
    outdated: Vec<String>,
}

const POPULAR_ACTIONS_JSON: &str = include_str!("popular_actions.json");

/// The popular actions by spec, and the specs known to be outdated.
type PopularActions = (HashMap<String, Arc<ActionMetadata>>, HashSet<String>);

static POPULAR: LazyLock<PopularActions> = LazyLock::new(|| {
    let file: PopularActionsFile =
        serde_json::from_str(POPULAR_ACTIONS_JSON).expect("embedded popular_actions.json is valid");
    let actions = file
        .actions
        .into_iter()
        .map(|(spec, a)| {
            (
                spec,
                Arc::new(ActionMetadata {
                    name: a.name,
                    inputs: a.inputs,
                    outputs: a.outputs,
                    skip_inputs: a.skip_inputs,
                    skip_outputs: a.skip_outputs,
                    ..ActionMetadata::default()
                }),
            )
        })
        .collect();
    (actions, file.outdated.into_iter().collect())
});

/// The metadata of a popular action, by `owner/repo@ref` spec.
pub fn popular_action(spec: &str) -> Option<Arc<ActionMetadata>> {
    POPULAR.0.get(spec).cloned()
}

/// Whether the spec is a known action whose runner is no longer available.
pub fn is_outdated_popular_action(spec: &str) -> bool {
    POPULAR.1.contains(spec)
}

/// Reads and caches the metadata of local actions (`uses: ./path`) of one
/// repository. A missing action is remembered and not reported: it may be
/// cloned at run time.
pub struct LocalActionsCache {
    root: Option<PathBuf>,
    cache: Mutex<HashMap<String, Option<Arc<ActionMetadata>>>>,
}

impl LocalActionsCache {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// `(metadata, was_cached)`. The error is reported the first time only.
    pub fn find_metadata(&self, spec: &str) -> Result<(Option<Arc<ActionMetadata>>, bool), String> {
        let Some(root) = &self.root else {
            return Ok((None, false));
        };
        if !spec.starts_with("./") {
            return Ok((None, false));
        }
        if let Some(m) = self.cache.lock().expect("cache lock").get(spec) {
            return Ok((m.clone(), true));
        }
        let dir = root.join(spec.trim_start_matches("./"));
        let Some((bytes, file)) = read_local_action_metadata_file(&dir) else {
            self.cache
                .lock()
                .expect("cache lock")
                .insert(spec.to_string(), None);
            return Ok((None, false));
        };
        match parse_action_metadata(&bytes) {
            Ok(mut meta) => {
                meta.dir = dir;
                meta.file = file;
                let meta = Arc::new(meta);
                self.cache
                    .lock()
                    .expect("cache lock")
                    .insert(spec.to_string(), Some(meta.clone()));
                Ok((Some(meta), false))
            }
            Err(m) => {
                self.cache
                    .lock()
                    .expect("cache lock")
                    .insert(spec.to_string(), None);
                Err(format!(
                    "could not parse action metadata in {}: {m}",
                    go_quote(&dir.display().to_string())
                ))
            }
        }
    }
}

fn read_local_action_metadata_file(dir: &Path) -> Option<(Vec<u8>, String)> {
    for f in ["action.yaml", "action.yml"] {
        if let Ok(b) = std::fs::read(dir.join(f)) {
            return Some((b, f.to_string()));
        }
    }
    None
}

/// go-yaml's "must be mapping node" message for a custom unmarshaler.
fn expected_mapping(where_: &str, n: &Node) -> String {
    format!(
        "yaml: {where_} must be mapping node but {} node was found at line:{}, col:{}",
        n.kind.name(),
        n.line,
        n.column
    )
}

/// go-yaml's type error for one field; the value is quoted for scalars only.
fn cannot_unmarshal(n: &Node, into: &str) -> String {
    if n.kind == Kind::Scalar {
        format!(
            "line {}: cannot unmarshal {} `{}` into {into}",
            n.line, n.tag, n.value
        )
    } else {
        format!("line {}: cannot unmarshal {} into {into}", n.line, n.tag)
    }
}

fn scalar_string(n: &Node) -> Result<String, String> {
    match n.kind {
        Kind::Scalar => Ok(n.value.clone()),
        _ => Err(cannot_unmarshal(n, "string")),
    }
}

/// go-yaml decodes a scalar into a bool field: `!!bool` values, plus the YAML
/// 1.1 words it still accepts for compatibility.
pub fn scalar_bool(n: &Node) -> Result<bool, String> {
    if n.kind == Kind::Scalar {
        if n.tag == "!!bool" {
            return Ok(n.value == "true");
        }
        if n.tag == "!!str" {
            match n.value.as_str() {
                "y" | "Y" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" => return Ok(true),
                "n" | "N" | "no" | "No" | "NO" | "off" | "Off" | "OFF" => return Ok(false),
                _ => {}
            }
        }
    }
    Err(cannot_unmarshal(n, "bool"))
}

/// Parses an `action.yml`, with go-yaml's error texts where actionlint
/// would show them.
pub fn parse_action_metadata(bytes: &[u8]) -> Result<ActionMetadata, String> {
    let (doc, _) = yaml::parse(bytes);
    let doc = doc.map_err(|e| format!("yaml: {}", e.message))?;
    let mut meta = ActionMetadata::default();
    let Some(root) = doc.content.first() else {
        return Ok(meta);
    };
    if root.kind != Kind::Mapping {
        if root.kind == Kind::Scalar && root.tag == "!!null" {
            return Ok(meta);
        }
        return Err(cannot_unmarshal(root, "actionlint.ActionMetadata"));
    }
    let mut type_errors: Vec<String> = Vec::new();
    for (k, v) in root.pairs() {
        match k.value.as_str() {
            "name" => match scalar_string(v) {
                Ok(s) => meta.name = s,
                Err(e) => type_errors.push(e),
            },
            "description" => match scalar_string(v) {
                Ok(s) => meta.description = s,
                Err(e) => type_errors.push(e),
            },
            "inputs" => {
                if v.kind == Kind::Scalar && v.tag == "!!null" {
                    continue;
                }
                // Errors from go-yaml's custom unmarshaler are prefixed with
                // the line of the node it was given, and parsing goes on.
                if v.kind != Kind::Mapping {
                    type_errors.push(format!(
                        "line {}: {}",
                        v.line,
                        expected_mapping("inputs", v)
                    ));
                    continue;
                }
                let mut failed: Option<String> = None;
                let mut pending_error: Option<String> = None;
                for (ik, iv) in v.pairs() {
                    let id = ik.value.to_lowercase();
                    if meta.inputs.contains_key(&id) {
                        failed = Some(format!("input {} is duplicated", go_quote(&ik.value)));
                        break;
                    }
                    let mut input = ActionMetadataInput {
                        name: ik.value.clone(),
                        ..ActionMetadataInput::default()
                    };
                    let mut required = false;
                    let mut has_default = false;
                    if iv.kind == Kind::Mapping {
                        for (fk, fv) in iv.pairs() {
                            match fk.value.as_str() {
                                "required" => match scalar_bool(fv) {
                                    Ok(b) => required = b,
                                    Err(e) => {
                                        failed = Some(e);
                                        break;
                                    }
                                },
                                "default" => {
                                    has_default = !(fv.kind == Kind::Scalar && fv.tag == "!!null");
                                }
                                "deprecationMessage" => {
                                    input.deprecated = true;
                                    input.deprecation_message =
                                        scalar_string(fv).unwrap_or_default().trim().to_string();
                                }
                                "description" => {}
                                other => {
                                    pending_error = Some(format!(
                                        "unexpected key {} for definition of input {}",
                                        go_quote(other),
                                        go_quote(&ik.value)
                                    ));
                                }
                            }
                        }
                        if failed.is_some() {
                            break;
                        }
                    } else if !(iv.kind == Kind::Scalar && iv.tag == "!!null") {
                        failed = Some(cannot_unmarshal(
                            iv,
                            "struct { Required bool \"yaml:\\\"required\\\"\"; Default *string \"yaml:\\\"default\\\"\"; DeprecationMessage string \"yaml:\\\"deprecationMessage\\\"\" }",
                        ));
                        break;
                    }
                    input.required = required && !has_default;
                    meta.inputs.insert(id, input);
                }
                if let Some(e) = failed.or(pending_error) {
                    // A nested type error already names its own line and is
                    // merged as is; a message from the unmarshaler gets the
                    // line of the node it was given.
                    if e.starts_with("line ") {
                        type_errors.push(e);
                    } else {
                        type_errors.push(format!("line {}: {e}", v.line));
                    }
                }
            }
            "outputs" => {
                if v.kind == Kind::Scalar && v.tag == "!!null" {
                    continue;
                }
                if v.kind != Kind::Mapping {
                    type_errors.push(format!(
                        "line {}: {}",
                        v.line,
                        expected_mapping("outputs", v)
                    ));
                    continue;
                }
                for (ok, _) in v.pairs() {
                    let id = ok.value.to_lowercase();
                    if meta.outputs.contains_key(&id) {
                        type_errors.push(format!(
                            "line {}: output {} is duplicated",
                            v.line,
                            go_quote(&ok.value)
                        ));
                        break;
                    }
                    meta.outputs.insert(id, ok.value.clone());
                }
            }
            "runs" => {
                if v.kind != Kind::Mapping {
                    if !(v.kind == Kind::Scalar && v.tag == "!!null") {
                        type_errors.push(cannot_unmarshal(v, "actionlint.ActionMetadataRuns"));
                    }
                    continue;
                }
                for (rk, rv) in v.pairs() {
                    let field = match rk.value.as_str() {
                        "using" => Some(&mut meta.runs.using),
                        "main" => Some(&mut meta.runs.main),
                        "pre" => Some(&mut meta.runs.pre),
                        "pre-if" => Some(&mut meta.runs.pre_if),
                        "post" => Some(&mut meta.runs.post),
                        "post-if" => Some(&mut meta.runs.post_if),
                        "image" => Some(&mut meta.runs.image),
                        "pre-entrypoint" => Some(&mut meta.runs.pre_entrypoint),
                        "entrypoint" => Some(&mut meta.runs.entrypoint),
                        "post-entrypoint" => Some(&mut meta.runs.post_entrypoint),
                        _ => None,
                    };
                    if let Some(field) = field {
                        match scalar_string(rv) {
                            Ok(s) => *field = s,
                            Err(e) => type_errors.push(e),
                        }
                        continue;
                    }
                    match rk.value.as_str() {
                        "steps" => {
                            if rv.kind == Kind::Sequence {
                                meta.runs.steps = Some(rv.content.len());
                            } else if !(rv.kind == Kind::Scalar && rv.tag == "!!null") {
                                type_errors.push(cannot_unmarshal(rv, "[]interface {}"));
                            }
                        }
                        "args" => {
                            if rv.kind == Kind::Sequence {
                                meta.runs.args = true;
                            } else if !(rv.kind == Kind::Scalar && rv.tag == "!!null") {
                                type_errors.push(cannot_unmarshal(rv, "[]interface {}"));
                            }
                        }
                        "env" => {
                            if rv.kind == Kind::Mapping {
                                meta.runs.env = true;
                            } else if !(rv.kind == Kind::Scalar && rv.tag == "!!null") {
                                type_errors.push(cannot_unmarshal(rv, "map[string]interface {}"));
                            }
                        }
                        _ => {}
                    }
                }
            }
            "branding" => {
                if v.kind != Kind::Mapping {
                    continue;
                }
                for (bk, bv) in v.pairs() {
                    match bk.value.as_str() {
                        "icon" => meta.branding.icon = scalar_string(bv).unwrap_or_default(),
                        "color" => meta.branding.color = scalar_string(bv).unwrap_or_default(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if type_errors.len() == 1 {
        return Err(type_errors.remove(0));
    }
    if !type_errors.is_empty() {
        // go-yaml's TypeError text, with actionlint's newline removal:
        // "yaml: unmarshal errors:\n  e1\n  e2" becomes one line.
        let mut msg = String::from("yaml: unmarshal errors:");
        for e in &type_errors {
            msg.push_str("  ");
            msg.push_str(e);
        }
        return Err(msg);
    }
    Ok(meta)
}
