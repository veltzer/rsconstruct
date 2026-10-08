//! The workflow syntax tree, a port of actionlint's `ast.go`.
//!
//! Every value keeps the position it came from, and a value that may be
//! written as a `${{ }}` expression keeps that expression instead of a
//! literal. Maps whose keys are case-insensitive are keyed by the lower-cased
//! name, as actionlint does.

use std::collections::BTreeMap;
use std::fmt;

use super::yaml::go_quote;

/// A 1-based line and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

impl Pos {
    pub const fn is_before(self, other: Self) -> bool {
        if self.line != other.line {
            return self.line < other.line;
        }
        self.col < other.col
    }
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line:{},col:{}", self.line, self.col)
    }
}

/// A string value with its position and whether it was quoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Str {
    pub value: String,
    pub quoted: bool,
    pub pos: Pos,
}

impl Str {
    pub fn contains_expression(&self) -> bool {
        contains_expression(&self.value)
    }

    pub fn is_expression_assigned(&self) -> bool {
        is_expr_assigned(&self.value)
    }
}

/// Whether the text has a `${{ ... }}` placeholder.
pub fn contains_expression(s: &str) -> bool {
    match (s.find("${{"), s.find("}}")) {
        (Some(start), Some(end)) => start < end,
        _ => false,
    }
}

/// Whether the text is exactly one `${{ ... }}` expression.
pub fn is_expr_assigned(s: &str) -> bool {
    let v = s.trim();
    v.starts_with("${{") && v.ends_with("}}") && v.matches("${{").count() == 1
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bool {
    pub value: bool,
    pub expression: Option<Str>,
    pub pos: Pos,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Int {
    pub value: i64,
    pub expression: Option<Str>,
    pub pos: Pos,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Float {
    pub value: f64,
    pub expression: Option<Str>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct WebhookEventFilter {
    pub name: Str,
    pub values: Vec<Str>,
}

/// actionlint's `IsEmpty`, which also treats a missing filter as empty.
pub fn filter_is_empty(filter: Option<&WebhookEventFilter>) -> bool {
    filter.is_none_or(|f| f.values.is_empty())
}

#[derive(Debug, Clone)]
pub struct WebhookEvent {
    pub hook: Str,
    pub types: Vec<Str>,
    pub branches: Option<WebhookEventFilter>,
    pub branches_ignore: Option<WebhookEventFilter>,
    pub tags: Option<WebhookEventFilter>,
    pub tags_ignore: Option<WebhookEventFilter>,
    pub paths: Option<WebhookEventFilter>,
    pub paths_ignore: Option<WebhookEventFilter>,
    pub workflows: Vec<Str>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct ScheduleEntry {
    pub cron: Str,
    pub timezone: Option<Str>,
}

#[derive(Debug, Clone)]
pub struct ScheduledEvent {
    pub schedules: Vec<ScheduleEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DispatchInputType {
    #[default]
    None,
    String,
    Number,
    Boolean,
    Choice,
    Environment,
}

#[derive(Debug, Clone)]
pub struct DispatchInput {
    pub name: Str,
    pub description: Option<Str>,
    pub required: Option<Bool>,
    pub default: Option<Str>,
    pub ty: DispatchInputType,
    pub options: Vec<Str>,
}

#[derive(Debug, Clone)]
pub struct WorkflowDispatchEvent {
    /// Keyed by lower-cased input name.
    pub inputs: BTreeMap<String, DispatchInput>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct RepositoryDispatchEvent {
    pub types: Vec<Str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkflowCallInputType {
    #[default]
    Invalid,
    Boolean,
    Number,
    String,
}

#[derive(Debug, Clone)]
pub struct WorkflowCallEventInput {
    pub name: Str,
    pub description: Option<Str>,
    pub default: Option<Str>,
    pub required: Option<Bool>,
    pub ty: WorkflowCallInputType,
    /// Lower-cased name.
    pub id: String,
}

impl WorkflowCallEventInput {
    pub fn is_required(&self) -> bool {
        self.required.as_ref().is_some_and(|r| r.value)
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowCallEventSecret {
    pub name: Str,
    pub description: Option<Str>,
    pub required: Option<Bool>,
}

#[derive(Debug, Clone)]
pub struct WorkflowCallEventOutput {
    pub name: Str,
    pub description: Option<Str>,
    pub value: Option<Str>,
}

#[derive(Debug, Clone)]
pub struct WorkflowCallEvent {
    /// In source order: an input's default may refer to earlier inputs.
    pub inputs: Vec<WorkflowCallEventInput>,
    /// `None` when `secrets:` is absent; keyed by lower-cased name.
    pub secrets: Option<BTreeMap<String, WorkflowCallEventSecret>>,
    pub outputs: BTreeMap<String, WorkflowCallEventOutput>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct ImageVersionEvent {
    pub names: Vec<Str>,
    pub versions: Vec<Str>,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// Boxed: a webhook event carries six filters and is far larger than
    /// the other variants.
    Webhook(Box<WebhookEvent>),
    Scheduled(ScheduledEvent),
    WorkflowDispatch(WorkflowDispatchEvent),
    RepositoryDispatch(RepositoryDispatchEvent),
    WorkflowCall(WorkflowCallEvent),
    ImageVersion(ImageVersionEvent),
}

#[derive(Debug, Clone)]
pub struct PermissionScope {
    pub name: Str,
    pub value: Str,
}

#[derive(Debug, Clone)]
pub struct Permissions {
    pub all: Option<Str>,
    pub scopes: BTreeMap<String, PermissionScope>,
}

#[derive(Debug, Clone)]
pub struct DefaultsRun {
    pub shell: Option<Str>,
    pub working_directory: Option<Str>,
}

#[derive(Debug, Clone)]
pub struct Defaults {
    pub run: Option<DefaultsRun>,
}

#[derive(Debug, Clone)]
pub struct Concurrency {
    pub group: Option<Str>,
    pub cancel_in_progress: Option<Bool>,
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub name: Option<Str>,
    pub url: Option<Str>,
    pub deployment: Option<Bool>,
}

#[derive(Debug, Clone)]
pub struct ExecRun {
    pub run: Option<Str>,
    pub shell: Option<Str>,
    pub working_directory: Option<Str>,
    pub run_pos: Pos,
}

#[derive(Debug, Clone)]
pub struct Input {
    pub name: Str,
    pub value: Str,
}

#[derive(Debug, Clone)]
pub struct ExecAction {
    pub uses: Option<Str>,
    /// Keyed by lower-cased input name.
    pub inputs: BTreeMap<String, Input>,
    pub entrypoint: Option<Str>,
    pub args: Option<Str>,
}

#[derive(Debug, Clone)]
pub enum Exec {
    Run(ExecRun),
    Action(ExecAction),
}

/// A value in a matrix: any YAML value, kept raw.
#[derive(Debug, Clone)]
pub enum RawYamlValue {
    Object {
        props: BTreeMap<String, Self>,
        pos: Pos,
    },
    Array {
        elems: Vec<Self>,
        pos: Pos,
    },
    String {
        value: String,
        pos: Pos,
    },
}

impl RawYamlValue {
    pub const fn pos(&self) -> Pos {
        match self {
            Self::Object { pos, .. } | Self::Array { pos, .. } | Self::String { pos, .. } => *pos,
        }
    }

    /// Structural equality, ignoring positions.
    pub fn equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Object { props: a, .. }, Self::Object { props: b, .. }) => {
                a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| v.equals(w)))
            }
            (Self::Array { elems: a, .. }, Self::Array { elems: b, .. }) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.equals(y))
            }
            (Self::String { value: a, .. }, Self::String { value: b, .. }) => a == b,
            _ => false,
        }
    }
}

impl fmt::Display for RawYamlValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Object { props, .. } => {
                let parts: Vec<String> = props
                    .iter()
                    .map(|(k, v)| format!("{}: {v}", go_quote(k)))
                    .collect();
                write!(f, "{{{}}}", parts.join(", "))
            }
            Self::Array { elems, .. } => {
                let parts: Vec<String> = elems.iter().map(ToString::to_string).collect();
                write!(f, "[{}]", parts.join(", "))
            }
            Self::String { value, .. } => f.write_str(&go_quote(value)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatrixRow {
    pub name: Option<Str>,
    /// `None` when the row is given as an expression.
    pub values: Option<Vec<RawYamlValue>>,
    pub expression: Option<Str>,
}

#[derive(Debug, Clone)]
pub struct MatrixAssign {
    pub key: Str,
    pub value: RawYamlValue,
}

#[derive(Debug, Clone)]
pub struct MatrixCombination {
    pub assigns: BTreeMap<String, MatrixAssign>,
    pub expression: Option<Str>,
}

#[derive(Debug, Clone)]
pub struct MatrixCombinations {
    pub combinations: Vec<MatrixCombination>,
    pub expression: Option<Str>,
}

impl MatrixCombinations {
    pub fn contains_expression(&self) -> bool {
        self.expression.is_some() || self.combinations.iter().any(|c| c.expression.is_some())
    }
}

#[derive(Debug, Clone)]
pub struct Matrix {
    pub rows: BTreeMap<String, MatrixRow>,
    pub include: Option<MatrixCombinations>,
    pub exclude: Option<MatrixCombinations>,
    pub expression: Option<Str>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct Strategy {
    pub matrix: Option<Matrix>,
    pub fail_fast: Option<Bool>,
    pub max_parallel: Option<Int>,
}

#[derive(Debug, Clone)]
pub struct EnvVar {
    pub name: Str,
    pub value: Str,
}

#[derive(Debug, Clone)]
pub struct Env {
    /// Keyed by lower-cased name; empty when `expression` is set.
    pub vars: BTreeMap<String, EnvVar>,
    pub expression: Option<Str>,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub id: Option<Str>,
    pub if_cond: Option<Str>,
    pub name: Option<Str>,
    pub exec: Option<Exec>,
    pub env: Option<Env>,
    pub continue_on_error: Option<Bool>,
    pub timeout_minutes: Option<Float>,
}

#[derive(Debug, Clone)]
pub struct Credentials {
    pub username: Option<Str>,
    pub password: Option<Str>,
    pub expression: Option<Str>,
}

#[derive(Debug, Clone)]
pub struct Container {
    pub image: Option<Str>,
    pub credentials: Option<Credentials>,
    pub env: Option<Env>,
    pub ports: Vec<Str>,
    pub volumes: Vec<Str>,
    pub options: Option<Str>,
}

#[derive(Debug, Clone)]
pub struct Service {
    pub name: Str,
    pub container: Container,
}

#[derive(Debug, Clone)]
pub struct Services {
    pub value: BTreeMap<String, Service>,
    pub expression: Option<Str>,
}

#[derive(Debug, Clone)]
pub struct Output {
    pub value: Str,
}

#[derive(Debug, Clone)]
pub struct Runner {
    pub labels: Vec<Str>,
    pub labels_expr: Option<Str>,
    pub group: Option<Str>,
}

#[derive(Debug, Clone)]
pub struct WorkflowCallInput {
    pub name: Str,
    pub value: Str,
}

#[derive(Debug, Clone)]
pub struct WorkflowCallSecret {
    pub name: Str,
    pub value: Str,
}

#[derive(Debug, Clone, Default)]
pub struct WorkflowCall {
    pub uses: Option<Str>,
    pub inputs: BTreeMap<String, WorkflowCallInput>,
    pub secrets: BTreeMap<String, WorkflowCallSecret>,
    pub inherit_secrets: bool,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub image_name: Option<Str>,
    pub version: Option<Str>,
    pub if_cond: Option<Str>,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: Str,
    pub name: Option<Str>,
    pub needs: Vec<Str>,
    pub runs_on: Option<Runner>,
    pub permissions: Option<Permissions>,
    pub environment: Option<Environment>,
    pub concurrency: Option<Concurrency>,
    pub outputs: BTreeMap<String, Output>,
    pub env: Option<Env>,
    pub defaults: Option<Defaults>,
    pub if_cond: Option<Str>,
    /// `None` when the section is missing or not a sequence.
    pub steps: Option<Vec<Step>>,
    pub timeout_minutes: Option<Float>,
    pub strategy: Option<Strategy>,
    pub continue_on_error: Option<Bool>,
    pub container: Option<Container>,
    pub services: Option<Services>,
    pub workflow_call: Option<WorkflowCall>,
    pub snapshot: Option<Snapshot>,
    pub pos: Pos,
}

#[derive(Debug, Clone, Default)]
pub struct Workflow {
    pub name: Option<Str>,
    pub run_name: Option<Str>,
    /// `None` when `on:` is missing or of an unusable kind.
    pub on: Option<Vec<Event>>,
    pub permissions: Option<Permissions>,
    pub env: Option<Env>,
    pub defaults: Option<Defaults>,
    pub concurrency: Option<Concurrency>,
    /// Keyed by lower-cased job id; `None` when `jobs:` is missing.
    pub jobs: Option<BTreeMap<String, Job>>,
}

impl Workflow {
    pub fn find_workflow_call_event(&self) -> Option<&WorkflowCallEvent> {
        self.on.as_ref()?.iter().find_map(|e| match e {
            Event::WorkflowCall(c) => Some(c),
            _ => None,
        })
    }

    pub fn jobs(&self) -> impl Iterator<Item = &Job> {
        self.jobs.iter().flat_map(|m| m.values())
    }
}

/// Go's `%v` of a float64: shortest round-trip digits, exponent form only
/// for very large or very small magnitudes.
pub fn go_format_float(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 {
            "+Inf".to_string()
        } else {
            "-Inf".to_string()
        };
    }
    if v == 0.0 {
        return if v.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }
    let exponent = v.abs().log10().floor() as i32;
    if (-4..21).contains(&exponent) {
        let s = format!("{v}");
        return s
            .strip_suffix(".0")
            .map_or_else(|| s.clone(), str::to_string);
    }
    let s = format!("{v:e}");
    let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exp.abs())
}
