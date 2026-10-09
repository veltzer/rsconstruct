//! The expression semantics checker, a port of actionlint's `expr_sema.go`:
//! types of contexts and functions, type checking of every operator, and
//! the extra checks on `format()`, `fromJSON()` and `case()`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use super::ExprError;
use super::insecure::UntrustedInputChecker;
use super::parser::{CompareKind, ExprNode, LogicalKind, error_at_token};
use super::types::{ExprType, ObjectType, type_of_json_value};
use crate::engines::actionlint::parse::{quotes, sorted_quotes};
use crate::engines::actionlint::yaml::go_quote;

fn ordinal(i: usize) -> String {
    let suffix = match i % 10 {
        1 if i % 100 != 11 => "st",
        2 if i % 100 != 12 => "nd",
        3 if i % 100 != 13 => "rd",
        _ => "th",
    };
    format!("{i}{suffix}")
}

/// The `{N}` placeholders a `format()` string refers to.
fn parse_format_func_specifiers(f: &str) -> std::collections::BTreeSet<usize> {
    let mut ret = std::collections::BTreeSet::new();
    let bytes = f.as_bytes();
    let none = usize::MAX;
    let (mut start, mut end) = (none, none);
    for (i, &b) in bytes.iter().enumerate() {
        if start == none {
            if b == b'{' {
                start = i + 1;
            }
            continue;
        }
        if end == none {
            if b == b'{' && i == start {
                start = none;
            } else if b == b'}' {
                if i == start {
                    start = none;
                } else {
                    end = i;
                }
            } else if !b.is_ascii_digit() {
                if b == b'{' {
                    start = i + 1;
                } else {
                    start = none;
                }
            }
            continue;
        }
        if b == b'}' {
            continue;
        }
        if (i - end) % 2 == 1
            && let Ok(v) = f[start..end].parse::<usize>()
        {
            ret.insert(v);
        }
        if b == b'{' {
            start = i + 1;
        } else {
            start = none;
        }
        end = none;
    }
    if start != none
        && end != none
        && (bytes.len() - end) % 2 == 1
        && let Ok(v) = f[start..end].parse::<usize>()
    {
        ret.insert(v);
    }
    ret
}

#[derive(Debug, Clone)]
pub struct FuncSignature {
    pub name: &'static str,
    pub ret: ExprType,
    pub params: Vec<ExprType>,
    pub variable_length_params: bool,
    pub is_const_func: bool,
}

impl std::fmt::Display for FuncSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ts: Vec<String> = self.params.iter().map(ToString::to_string).collect();
        let elip = if self.variable_length_params {
            "..."
        } else {
            ""
        };
        write!(f, "{}({}{elip}) -> {}", self.name, ts.join(", "), self.ret)
    }
}

const fn sig(
    name: &'static str,
    ret: ExprType,
    params: Vec<ExprType>,
    variable: bool,
    is_const: bool,
) -> FuncSignature {
    FuncSignature {
        name,
        ret,
        params,
        variable_length_params: variable,
        is_const_func: is_const,
    }
}

/// Signatures of the built-in functions, keyed by lower-cased name.
pub static BUILTIN_FUNC_SIGNATURES: LazyLock<BTreeMap<&'static str, Vec<FuncSignature>>> =
    LazyLock::new(|| {
        use ExprType::{Any, Bool, String as Str};
        let mut m = BTreeMap::new();
        m.insert(
            "contains",
            vec![
                sig("contains", Bool, vec![Str, Str], false, true),
                sig(
                    "contains",
                    Bool,
                    vec![ExprType::array(Any, false), Any],
                    false,
                    true,
                ),
            ],
        );
        m.insert(
            "startswith",
            vec![sig("startsWith", Bool, vec![Str, Str], false, true)],
        );
        m.insert(
            "endswith",
            vec![sig("endsWith", Bool, vec![Str, Str], false, true)],
        );
        m.insert(
            "format",
            vec![sig("format", Str, vec![Str, Any], true, true)],
        );
        m.insert(
            "join",
            vec![
                sig(
                    "join",
                    Str,
                    vec![ExprType::array(Str, false), Str],
                    false,
                    true,
                ),
                sig("join", Str, vec![ExprType::array(Str, false)], false, true),
            ],
        );
        m.insert("tojson", vec![sig("toJSON", Str, vec![Any], false, true)]);
        m.insert(
            "fromjson",
            vec![sig("fromJSON", Any, vec![Str], false, false)],
        );
        m.insert(
            "hashfiles",
            vec![sig("hashFiles", Str, vec![Str], true, false)],
        );
        m.insert("success", vec![sig("success", Bool, vec![], false, false)]);
        m.insert("always", vec![sig("always", Bool, vec![], false, false)]);
        m.insert(
            "cancelled",
            vec![sig("cancelled", Bool, vec![], false, false)],
        );
        m.insert("failure", vec![sig("failure", Bool, vec![], false, false)]);
        m.insert(
            "case",
            vec![sig("case", Any, vec![Bool, Any, Any], true, true)],
        );
        m
    });

fn strict(pairs: &[(&str, ExprType)]) -> ExprType {
    ExprType::Object(ObjectType::strict(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect(),
    ))
}

/// Types of the global contexts.
pub static BUILTIN_GLOBAL_VARIABLE_TYPES: LazyLock<BTreeMap<String, ExprType>> =
    LazyLock::new(|| {
        use ExprType::{Bool, Number, String as Str};
        let mut m = BTreeMap::new();
        m.insert(
            "github".to_string(),
            strict(&[
                ("action", Str),
                ("action_path", Str),
                ("action_ref", Str),
                ("action_repository", Str),
                ("action_status", Str),
                ("actor", Str),
                ("actor_id", Str),
                ("api_url", Str),
                ("artifact_cache_size_limit", Number),
                ("base_ref", Str),
                ("env", Str),
                ("event", ExprType::Object(ObjectType::empty())),
                ("event_name", Str),
                ("event_path", Str),
                ("graphql_url", Str),
                ("head_ref", Str),
                ("job", Str),
                ("output", Str),
                ("path", Str),
                ("ref", Str),
                ("ref_name", Str),
                ("ref_protected", Bool),
                ("ref_type", Str),
                ("repository", Str),
                ("repository_id", Str),
                ("repository_owner", Str),
                ("repository_owner_id", Str),
                ("repository_visibility", Str),
                ("repositoryurl", Str),
                ("retention_days", Number),
                ("run_attempt", Str),
                ("run_id", Str),
                ("run_number", Str),
                ("secret_source", Str),
                ("server_url", Str),
                ("sha", Str),
                ("state", Str),
                ("step_summary", Str),
                ("token", Str),
                ("triggering_actor", Str),
                ("workflow", Str),
                ("workflow_ref", Str),
                ("workflow_sha", Str),
                ("workspace", Str),
            ]),
        );
        m.insert("env".to_string(), ExprType::Object(ObjectType::map(Str)));
        m.insert(
            "job".to_string(),
            strict(&[
                ("check_run_id", Number),
                ("container", strict(&[("id", Str), ("network", Str)])),
                (
                    "services",
                    ExprType::Object(ObjectType::map(strict(&[
                        ("id", Str),
                        ("network", Str),
                        ("ports", ExprType::Object(ObjectType::map(Str))),
                    ]))),
                ),
                ("status", Str),
            ]),
        );
        m.insert(
            "steps".to_string(),
            ExprType::Object(ObjectType::empty_strict()),
        );
        m.insert(
            "runner".to_string(),
            strict(&[
                ("name", Str),
                ("os", Str),
                ("arch", Str),
                ("temp", Str),
                ("tool_cache", Str),
                ("debug", Str),
                ("environment", Str),
            ]),
        );
        m.insert(
            "secrets".to_string(),
            ExprType::Object(ObjectType::map(Str)),
        );
        m.insert(
            "strategy".to_string(),
            ExprType::Object(ObjectType::new(
                [
                    ("fail-fast".to_string(), Bool),
                    ("job-index".to_string(), Number),
                    ("job-total".to_string(), Number),
                    ("max-parallel".to_string(), Number),
                ]
                .into_iter()
                .collect(),
            )),
        );
        m.insert(
            "matrix".to_string(),
            ExprType::Object(ObjectType::empty_strict()),
        );
        m.insert(
            "needs".to_string(),
            ExprType::Object(ObjectType::empty_strict()),
        );
        m.insert(
            "inputs".to_string(),
            ExprType::Object(ObjectType::empty_strict()),
        );
        m.insert("vars".to_string(), ExprType::Object(ObjectType::map(Str)));
        m
    });

/// Functions that are only available at some workflow keys, with the keys.
pub static SPECIAL_FUNCTION_NAMES: LazyLock<BTreeMap<&'static str, Vec<&'static str>>> =
    LazyLock::new(|| {
        let mut m = BTreeMap::new();
        m.insert("always", vec!["jobs.<job_id>.if", "jobs.<job_id>.steps.if"]);
        m.insert(
            "cancelled",
            vec!["jobs.<job_id>.if", "jobs.<job_id>.steps.if"],
        );
        m.insert(
            "failure",
            vec!["jobs.<job_id>.if", "jobs.<job_id>.steps.if"],
        );
        m.insert(
            "hashfiles",
            vec![
                "jobs.<job_id>.steps.continue-on-error",
                "jobs.<job_id>.steps.env",
                "jobs.<job_id>.steps.if",
                "jobs.<job_id>.steps.name",
                "jobs.<job_id>.steps.run",
                "jobs.<job_id>.steps.timeout-minutes",
                "jobs.<job_id>.steps.with",
                "jobs.<job_id>.steps.working-directory",
            ],
        );
        m.insert(
            "success",
            vec!["jobs.<job_id>.if", "jobs.<job_id>.steps.if"],
        );
        m
    });

pub struct SemanticsChecker {
    vars: BTreeMap<String, ExprType>,
    errs: Vec<ExprError>,
    untrusted: Option<UntrustedInputChecker>,
    available_contexts: Vec<String>,
    available_special_funcs: Vec<String>,
    /// `None`: configuration variables are not checked.
    config_vars: Option<Vec<String>>,
}

impl SemanticsChecker {
    pub fn new(check_untrusted_input: bool, config_vars: Option<Vec<String>>) -> Self {
        Self {
            vars: BUILTIN_GLOBAL_VARIABLE_TYPES.clone(),
            errs: Vec::new(),
            untrusted: check_untrusted_input.then(UntrustedInputChecker::new),
            available_contexts: Vec::new(),
            available_special_funcs: Vec::new(),
            config_vars,
        }
    }

    fn errorf(&mut self, e: &ExprNode, message: String) {
        self.errs.push(error_at_token(e.token(), message));
    }

    pub fn update_matrix(&mut self, ty: ObjectType) {
        self.vars.insert("matrix".to_string(), ExprType::Object(ty));
    }

    pub fn update_steps(&mut self, ty: ObjectType) {
        self.vars.insert("steps".to_string(), ExprType::Object(ty));
    }

    pub fn update_needs(&mut self, ty: ObjectType) {
        self.vars.insert("needs".to_string(), ExprType::Object(ty));
    }

    pub fn update_secrets(&mut self, ty: &ObjectType) {
        let mut copied = ObjectType::strict(
            [
                ("github_token".to_string(), ExprType::String),
                ("actions_step_debug".to_string(), ExprType::String),
                ("actions_runner_debug".to_string(), ExprType::String),
            ]
            .into_iter()
            .collect(),
        );
        for (n, v) in &ty.props {
            copied.props.insert(n.clone(), v.clone());
        }
        self.vars
            .insert("secrets".to_string(), ExprType::Object(copied));
    }

    pub fn update_inputs(&mut self, ty: ObjectType) {
        let current = match self.vars.get("inputs") {
            Some(ExprType::Object(o)) => o.clone(),
            _ => ObjectType::empty_strict(),
        };
        if current.props.is_empty() && current.is_strict() {
            self.vars.insert("inputs".to_string(), ExprType::Object(ty));
            return;
        }
        // Both `workflow_call` and `workflow_dispatch` may trigger the
        // workflow; `inputs` then covers both.
        let merged = ExprType::Object(current).merge(&ExprType::Object(ty));
        self.vars.insert("inputs".to_string(), merged);
    }

    pub fn update_dispatch_inputs(&mut self, ty: ObjectType) {
        self.update_inputs(ty.clone());
        // `github.event.inputs.*` are always strings.
        let props = ty
            .props
            .keys()
            .map(|n| (n.clone(), ExprType::String))
            .collect();
        let inputs_ty = ExprType::Object(ObjectType::strict(props));
        if let Some(ExprType::Object(github)) = self.vars.get_mut("github")
            && let Some(ExprType::Object(event)) = github.props.get_mut("event")
        {
            event.props.insert("inputs".to_string(), inputs_ty);
        }
    }

    pub fn update_jobs(&mut self, ty: ObjectType) {
        self.vars.insert("jobs".to_string(), ExprType::Object(ty));
    }

    /// The contexts allowed where the expression sits (lower-cased).
    pub fn set_context_availability(&mut self, avail: Vec<String>) {
        self.available_contexts = avail;
    }

    pub fn set_special_function_availability(&mut self, avail: Vec<String>) {
        self.available_special_funcs = avail;
    }

    fn check_available_context(&mut self, n: &ExprNode, name: &str) {
        let ctx = name.to_lowercase();
        if self.available_contexts.contains(&ctx) {
            return;
        }
        let notes = match self.available_contexts.len() {
            0 => "no context is available here".to_string(),
            1 => format!("available context is {}", quotes(&self.available_contexts)),
            _ => format!(
                "available contexts are {}",
                quotes(&self.available_contexts)
            ),
        };
        self.errorf(
            n,
            format!(
                "context {} is not allowed here. {notes}. see https://docs.github.com/en/actions/learn-github-actions/contexts#context-availability for more details",
                go_quote(name)
            ),
        );
    }

    fn check_special_function_availability(&mut self, n: &ExprNode, callee: &str) {
        let f = callee.to_lowercase();
        let Some(allowed) = SPECIAL_FUNCTION_NAMES.get(f.as_str()) else {
            return;
        };
        if self.available_special_funcs.contains(&f) {
            return;
        }
        self.errorf(
            n,
            format!(
                "calling function {} is not allowed here. {} is only available in {}. see https://docs.github.com/en/actions/learn-github-actions/contexts#context-availability for more details",
                go_quote(callee),
                go_quote(callee),
                quotes(allowed)
            ),
        );
    }

    fn check_variable(&mut self, n: &ExprNode, name: &str) -> ExprType {
        let Some(v) = self.vars.get(name).cloned() else {
            let names: Vec<&String> = self.vars.keys().collect();
            let message = format!(
                "undefined variable {}. available variables are {}",
                go_quote(&n.token().value),
                sorted_quotes(&names)
            );
            self.errorf(n, message);
            return ExprType::Any;
        };
        self.check_available_context(n, name);
        v
    }

    fn check_object_deref(
        &mut self,
        n: &ExprNode,
        receiver: &ExprNode,
        property: &str,
    ) -> ExprType {
        match self.check(receiver) {
            ExprType::Any => ExprType::Any,
            ExprType::Object(ty) => {
                if let Some(t) = ty.props.get(property) {
                    return t.clone();
                }
                if let Some(mapped) = &ty.mapped {
                    if matches!(receiver, ExprNode::Variable { name, .. } if name == "vars") {
                        self.check_config_variables(n, property);
                    }
                    return (**mapped).clone();
                }
                if ty.is_strict() {
                    self.errorf(
                        n,
                        format!(
                            "property {} is not defined in object type {ty}",
                            go_quote(property)
                        ),
                    );
                }
                ExprType::Any
            }
            ExprType::Array(ty) => {
                if !ty.deref {
                    self.errorf(
                        n,
                        format!(
                            "receiver of object dereference {} must be type of object but got {}",
                            go_quote(property),
                            go_quote(&ExprType::Array(ty).to_string())
                        ),
                    );
                    return ExprType::Any;
                }
                match ty.elem.as_ref() {
                    ExprType::Any => ExprType::Array(ty),
                    ExprType::Object(et) => {
                        let mut elem = ExprType::Any;
                        if let Some(t) = et.props.get(property) {
                            elem = t.clone();
                        } else if let Some(m) = &et.mapped {
                            elem = (**m).clone();
                        } else if et.is_strict() {
                            self.errorf(
                                n,
                                format!(
                                    "property {} is not defined in object type {et} as element of filtered array",
                                    go_quote(property)
                                ),
                            );
                        }
                        ExprType::array(elem, true)
                    }
                    other => {
                        self.errorf(
                            n,
                            format!(
                                "property filtered by {} at object filtering must be type of object but got {}",
                                go_quote(property),
                                go_quote(&other.to_string())
                            ),
                        );
                        ExprType::Any
                    }
                }
            }
            other => {
                self.errorf(
                    n,
                    format!(
                        "receiver of object dereference {} must be type of object but got {}",
                        go_quote(property),
                        go_quote(&other.to_string())
                    ),
                );
                ExprType::Any
            }
        }
    }

    fn check_config_variables(&mut self, n: &ExprNode, property: &str) {
        if property.starts_with("github_") {
            self.errorf(
                n,
                format!(
                    "configuration variable name {} must not start with the GITHUB_ prefix (case insensitive). note: see the convention at https://docs.github.com/en/actions/learn-github-actions/variables#naming-conventions-for-configuration-variables",
                    go_quote(property)
                ),
            );
            return;
        }
        if !property
            .chars()
            .all(|r| r.is_ascii_digit() || r.is_ascii_lowercase() || r == '_')
        {
            self.errorf(
                n,
                format!(
                    "configuration variable name {} can only contain alphabets, decimal numbers, and '_'. note: see the convention at https://docs.github.com/en/actions/learn-github-actions/variables#naming-conventions-for-configuration-variables",
                    go_quote(property)
                ),
            );
            return;
        }
        let Some(config_vars) = &self.config_vars else {
            return;
        };
        if config_vars.is_empty() {
            self.errorf(
                n,
                format!(
                    "no configuration variable is allowed since the variables list is empty in actionlint.yaml. you may forget adding the variable {} to the list",
                    go_quote(property)
                ),
            );
            return;
        }
        if config_vars.iter().any(|v| v.eq_ignore_ascii_case(property)) {
            return;
        }
        let listed = sorted_quotes(config_vars);
        self.errorf(
            n,
            format!(
                "undefined configuration variable {}. defined configuration variables in actionlint.yaml are {listed}",
                go_quote(property)
            ),
        );
    }

    fn check_array_deref(&mut self, n: &ExprNode, receiver: &ExprNode) -> ExprType {
        match self.check(receiver) {
            ExprType::Any => ExprType::array(ExprType::Any, true),
            ExprType::Array(mut ty) => {
                ty.deref = true;
                ExprType::Array(ty)
            }
            ExprType::Object(ty) => {
                if let Some(mapped) = &ty.mapped {
                    return match mapped.as_ref() {
                        ExprType::Any => ExprType::array(ExprType::Any, true),
                        ExprType::Object(mty) => {
                            ExprType::array(ExprType::Object(mty.clone()), true)
                        }
                        mty => {
                            self.errorf(
                                n,
                                format!(
                                    "elements of object at receiver of object filtering `.*` must be type of object but got {}. the type of receiver was {}",
                                    go_quote(&mty.to_string()),
                                    go_quote(&ty.to_string())
                                ),
                            );
                            ExprType::Any
                        }
                    };
                }
                if !ty.props.values().any(|t| matches!(t, ExprType::Object(_))) {
                    self.errorf(
                        n,
                        format!(
                            "object type {} cannot be filtered by object filtering `.*` since it has no object element",
                            go_quote(&ty.to_string())
                        ),
                    );
                    return ExprType::Any;
                }
                ExprType::array(ExprType::Any, true)
            }
            other => {
                self.errorf(
                    n,
                    format!(
                        "receiver of object filtering `.*` must be type of array or object but got {}",
                        go_quote(&other.to_string())
                    ),
                );
                ExprType::Any
            }
        }
    }

    fn check_index_access(
        &mut self,
        n: &ExprNode,
        operand: &ExprNode,
        index: &ExprNode,
    ) -> ExprType {
        // The index is checked first so that untrusted-input tracking sees
        // nested accesses bottom-up.
        let idx = self.check(index);
        match self.check(operand) {
            ExprType::Any => ExprType::Any,
            ExprType::Array(ty) => match idx {
                ExprType::Any | ExprType::Number => *ty.elem,
                other => {
                    self.errorf(
                        index,
                        format!(
                            "index access of array must be type of number but got {}",
                            go_quote(&other.to_string())
                        ),
                    );
                    ExprType::Any
                }
            },
            ExprType::Object(ty) => match idx {
                ExprType::Any => ExprType::Any,
                ExprType::String => {
                    if let ExprNode::Str(lit, _) = index {
                        if let Some(prop) = ty.props.get(lit) {
                            return prop.clone();
                        }
                        if let Some(m) = &ty.mapped {
                            return (**m).clone();
                        }
                        if ty.is_strict() {
                            self.errorf(
                                n,
                                format!(
                                    "property {} is not defined in object type {ty}",
                                    go_quote(lit)
                                ),
                            );
                        }
                    }
                    if let Some(m) = &ty.mapped {
                        return (**m).clone();
                    }
                    ExprType::Any
                }
                other => {
                    self.errorf(
                        index,
                        format!(
                            "property access of object must be type of string but got {}",
                            go_quote(&other.to_string())
                        ),
                    );
                    ExprType::Any
                }
            },
            other => {
                self.errorf(
                    n,
                    format!(
                        "index access operand must be type of object or array but got {}",
                        go_quote(&other.to_string())
                    ),
                );
                ExprType::Any
            }
        }
    }

    fn check_func_signature(
        n: &ExprNode,
        args: &[ExprNode],
        sig: &FuncSignature,
        tys: &[ExprType],
    ) -> Option<ExprError> {
        let (lp, la) = (sig.params.len(), tys.len());
        if (sig.variable_length_params && lp > la) || (!sig.variable_length_params && lp != la) {
            let at_least = if sig.variable_length_params {
                "at least "
            } else {
                ""
            };
            return Some(error_at_token(
                n.token(),
                format!(
                    "number of arguments is wrong. function {} takes {at_least}{lp} parameters but {la} arguments are given",
                    go_quote(&sig.to_string())
                ),
            ));
        }
        for (i, (p, a)) in sig.params.iter().zip(tys).enumerate() {
            if !p.assignable(a) {
                return Some(error_at_token(
                    args[i].token(),
                    format!(
                        "{} argument of function call is not assignable. {} cannot be assigned to {}. called function type is {}",
                        ordinal(i + 1),
                        go_quote(&a.to_string()),
                        go_quote(&p.to_string()),
                        go_quote(&sig.to_string())
                    ),
                ));
            }
        }
        if sig.variable_length_params {
            let p = &sig.params[lp - 1];
            for (i, a) in tys[lp..].iter().enumerate() {
                if !p.assignable(a) {
                    return Some(error_at_token(
                        args[lp + i].token(),
                        format!(
                            "{} argument of function call is not assignable. {} cannot be assigned to {}. called function type is {}",
                            ordinal(lp + i + 1),
                            go_quote(&a.to_string()),
                            go_quote(&p.to_string()),
                            go_quote(&sig.to_string())
                        ),
                    ));
                }
            }
        }
        None
    }

    fn check_builtin_func_call(
        &mut self,
        n: &ExprNode,
        callee: &str,
        args: &[ExprNode],
        sig: &FuncSignature,
    ) -> ExprType {
        self.check_special_function_availability(n, callee);
        match callee.to_lowercase().as_str() {
            "format" => {
                let ExprNode::Str(lit, _) = &args[0] else {
                    return sig.ret.clone();
                };
                let l = args.len() - 1;
                let mut holders = parse_format_func_specifiers(lit);
                for i in 0..l {
                    if !holders.remove(&i) {
                        self.errorf(
                            n,
                            format!(
                                "format string {} does not contain placeholder {{{i}}}. remove argument which is unused in the format string",
                                go_quote(lit)
                            ),
                        );
                    }
                }
                for i in holders {
                    self.errorf(
                        n,
                        format!(
                            "format string {} contains placeholder {{{i}}} but only {l} arguments are given to format",
                            go_quote(lit)
                        ),
                    );
                }
            }
            "fromjson" => {
                let ExprNode::Str(lit, _) = &args[0] else {
                    return sig.ret.clone();
                };
                match serde_json::from_str::<serde_json::Value>(lit) {
                    Ok(v) => return type_of_json_value(&v),
                    Err(e) => {
                        if e.is_syntax() || e.is_eof() {
                            let offset = json_error_offset(lit, &e);
                            // Go's encoding/json wording for the common case.
                            let description = if e.is_eof() {
                                "unexpected end of JSON input".to_string()
                            } else {
                                e.to_string()
                            };
                            self.errorf(
                                &args[0],
                                format!("broken JSON string is passed to fromJSON() at offset {offset}: {description}"),
                            );
                        }
                    }
                }
            }
            "case" if args.len().is_multiple_of(2) => {
                self.errorf(
                        n,
                        format!(
                            "case() requires an odd number of arguments (pred/value pairs + default) but got {}",
                            args.len()
                        ),
                    );
            }
            _ => {}
        }
        sig.ret.clone()
    }

    fn check_func_call(&mut self, n: &ExprNode, callee: &str, args: &[ExprNode]) -> ExprType {
        let lower = callee.to_lowercase();
        let Some(sigs) = BUILTIN_FUNC_SIGNATURES.get(lower.as_str()) else {
            let names: Vec<&&str> = BUILTIN_FUNC_SIGNATURES.keys().collect();
            let message = format!(
                "undefined function {}. available functions are {}",
                go_quote(callee),
                sorted_quotes(&names.iter().map(|s| (**s).to_string()).collect::<Vec<_>>())
            );
            self.errorf(n, message);
            return ExprType::Any;
        };
        let tys: Vec<ExprType> = args.iter().map(|a| self.check(a)).collect();
        let mut errs = Vec::new();
        for sig in sigs {
            match Self::check_func_signature(n, args, sig, &tys) {
                None => return self.check_builtin_func_call(n, callee, args, sig),
                Some(e) => errs.push(e),
            }
        }
        self.errs.extend(errs);
        ExprType::Any
    }

    fn check_not_op(&mut self, n: &ExprNode, operand: &ExprNode) -> ExprType {
        let ty = self.check(operand);
        if !ExprType::Bool.assignable(&ty) {
            self.errorf(
                n,
                format!(
                    "type of operand of ! operator {} is not assignable to type \"bool\"",
                    go_quote(&ty.to_string())
                ),
            );
        }
        ExprType::Bool
    }

    fn validate_compare_op_operands(op: CompareKind, l: &ExprType, r: &ExprType) -> bool {
        match op {
            CompareKind::Eq | CompareKind::NotEq => match l {
                ExprType::Any | ExprType::Null => true,
                ExprType::Number | ExprType::Bool | ExprType::String => {
                    !matches!(r, ExprType::Object(_) | ExprType::Array(_))
                }
                ExprType::Object(_) => {
                    matches!(r, ExprType::Object(_) | ExprType::Null | ExprType::Any)
                }
                ExprType::Array(la) => match r {
                    ExprType::Array(ra) => {
                        Self::validate_compare_op_operands(op, &la.elem, &ra.elem)
                    }
                    ExprType::Null | ExprType::Any => true,
                    _ => false,
                },
            },
            _ => match l {
                ExprType::Any | ExprType::Number | ExprType::String => !matches!(
                    r,
                    ExprType::Null | ExprType::Bool | ExprType::Object(_) | ExprType::Array(_)
                ),
                _ => false,
            },
        }
    }

    fn check_compare_op(
        &mut self,
        n: &ExprNode,
        kind: CompareKind,
        left: &ExprNode,
        right: &ExprNode,
    ) -> ExprType {
        let l = self.check(left);
        let r = self.check(right);
        if !Self::validate_compare_op_operands(kind, &l, &r) {
            self.errorf(
                n,
                format!(
                    "{} value cannot be compared to {} value with {} operator",
                    go_quote(&l.to_string()),
                    go_quote(&r.to_string()),
                    go_quote(kind.symbol())
                ),
            );
        }
        ExprType::Bool
    }

    /// Type of `n` assuming its value is truthy or falsy, which narrows
    /// `l && r` / `l || r`.
    fn check_with_narrowing(&mut self, n: &ExprNode, is_truthy: bool) -> ExprType {
        match n {
            ExprNode::Logical { kind, left, right } => {
                match kind {
                    LogicalKind::And if is_truthy => {
                        self.check(left);
                        return self.check(right);
                    }
                    LogicalKind::Or if !is_truthy => {
                        self.check(left);
                        return self.check(right);
                    }
                    _ => {}
                }
                self.check_logical_op(*kind, left, right)
            }
            ExprNode::Not { operand, .. } => self.check_with_narrowing(operand, !is_truthy),
            _ => self.check(n),
        }
    }

    fn check_logical_op(
        &mut self,
        kind: LogicalKind,
        left: &ExprNode,
        right: &ExprNode,
    ) -> ExprType {
        match kind {
            LogicalKind::And => {
                let l = self.check_with_narrowing(left, false);
                let r = self.check(right);
                l.merge(&r)
            }
            LogicalKind::Or => {
                let l = self.check_with_narrowing(left, true);
                let r = self.check(right);
                l.merge(&r)
            }
        }
    }

    fn check(&mut self, expr: &ExprNode) -> ExprType {
        if let Some(u) = &mut self.untrusted {
            u.on_visit_node_enter(expr);
        }
        let ty = match expr {
            ExprNode::Variable { name, .. } => self.check_variable(expr, name),
            ExprNode::Null(_) => ExprType::Null,
            ExprNode::Bool(..) => ExprType::Bool,
            ExprNode::Str(..) => ExprType::String,
            ExprNode::Int(..) | ExprNode::Float(..) => ExprType::Number,
            ExprNode::ObjectDeref { receiver, property } => {
                self.check_object_deref(expr, receiver, property)
            }
            ExprNode::ArrayDeref { receiver } => self.check_array_deref(expr, receiver),
            ExprNode::IndexAccess { operand, index } => {
                self.check_index_access(expr, operand, index)
            }
            ExprNode::FuncCall { callee, args, .. } => self.check_func_call(expr, callee, args),
            ExprNode::Not { operand, .. } => self.check_not_op(expr, operand),
            ExprNode::Compare { kind, left, right } => {
                self.check_compare_op(expr, *kind, left, right)
            }
            ExprNode::Logical { kind, left, right } => self.check_logical_op(*kind, left, right),
        };
        if let Some(u) = &mut self.untrusted {
            u.on_visit_node_leave(expr);
        }
        ty
    }

    /// Checks the expression: its type, and every problem found.
    pub fn check_expr(&mut self, expr: &ExprNode) -> (ExprType, Vec<ExprError>) {
        self.errs.clear();
        if let Some(u) = &mut self.untrusted {
            u.init();
        }
        let ty = self.check(expr);
        let mut errs = std::mem::take(&mut self.errs);
        if let Some(u) = &mut self.untrusted {
            u.on_visit_end();
            errs.extend(u.errs().iter().cloned());
        }
        (ty, errs)
    }

    /// Whether the expression is a constant: literals, operators on
    /// constants, and pure built-in functions of constants.
    pub fn is_constant(expr: &ExprNode) -> bool {
        match expr {
            ExprNode::Null(_)
            | ExprNode::Bool(..)
            | ExprNode::Int(..)
            | ExprNode::Float(..)
            | ExprNode::Str(..) => true,
            ExprNode::Variable { .. }
            | ExprNode::ObjectDeref { .. }
            | ExprNode::ArrayDeref { .. }
            | ExprNode::IndexAccess { .. } => false,
            ExprNode::Not { operand, .. } => Self::is_constant(operand),
            ExprNode::Compare { left, right, .. } | ExprNode::Logical { left, right, .. } => {
                Self::is_constant(left) && Self::is_constant(right)
            }
            ExprNode::FuncCall { callee, args, .. } => {
                if !args.iter().all(Self::is_constant) {
                    return false;
                }
                BUILTIN_FUNC_SIGNATURES
                    .get(callee.to_lowercase().as_str())
                    .is_some_and(|sigs| sigs.iter().all(|s| s.is_const_func))
            }
        }
    }
}

/// The byte offset of a JSON syntax error, as Go's `json.SyntaxError.Offset`
/// counts it (bytes read so far, 1-based for the offending byte).
fn json_error_offset(text: &str, e: &serde_json::Error) -> usize {
    let (line, column) = (e.line(), e.column());
    let mut offset = 0;
    for (i, l) in text.split('\n').enumerate() {
        if i + 1 == line {
            return offset + column;
        }
        offset += l.len() + 1;
    }
    text.len()
}
