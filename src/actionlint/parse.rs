//! The workflow parser, a port of actionlint's `parse.go`: it walks the
//! YAML tree, builds the [`Workflow`] syntax tree and reports every
//! structural problem as a `syntax-check` error, in actionlint's words.

use std::collections::BTreeMap;

use super::LintError;
use super::ast::{
    Bool, Concurrency, Container, Credentials, Defaults, DefaultsRun, DispatchInput,
    DispatchInputType, Env, EnvVar, Environment, Event, Exec, ExecAction, ExecRun, Float,
    ImageVersionEvent, Input, Int, Job, Matrix, MatrixAssign, MatrixCombination,
    MatrixCombinations, MatrixRow, Output, PermissionScope, Permissions, Pos, RawYamlValue,
    RepositoryDispatchEvent, Runner, ScheduleEntry, ScheduledEvent, Service, Services, Snapshot,
    Step, Str, Strategy, WebhookEvent, WebhookEventFilter, Workflow, WorkflowCall,
    WorkflowCallEvent, WorkflowCallEventInput, WorkflowCallEventOutput, WorkflowCallEventSecret,
    WorkflowCallInput, WorkflowCallInputType, WorkflowCallSecret, WorkflowDispatchEvent,
    go_format_float, is_expr_assigned,
};
use super::yaml::{self, Kind, Node, go_parse_float, go_quote};

/// Parses a workflow file. Parsing goes on after an error so that all
/// problems are reported; the tree is `None` only when the YAML itself could
/// not be read.
pub fn parse(source: &[u8]) -> (Option<Workflow>, Vec<LintError>) {
    let (result, anchors) = yaml::parse(source);
    let doc = match result {
        Ok(doc) => doc,
        Err(e) => {
            return (
                None,
                vec![LintError {
                    message: format!("could not parse as YAML: {}", e.message),
                    line: e.line,
                    column: e.column,
                    kind: "syntax-check",
                }],
            );
        }
    };
    let mut parser = Parser { errors: Vec::new() };
    for (message, line, column) in anchors.errors {
        parser.errors.push(LintError {
            message,
            line,
            column,
            kind: "syntax-check",
        });
    }
    let workflow = parser.parse_workflow(&doc);
    (Some(workflow), parser.errors)
}

struct Entry<'n> {
    /// The key, lower-cased unless the mapping is case-sensitive.
    id: String,
    key: Str,
    val: &'n Node,
}

struct Parser {
    errors: Vec<LintError>,
}

const fn pos_at(n: &Node) -> Pos {
    Pos {
        line: n.line,
        col: n.column,
    }
}

fn new_string(n: &Node) -> Str {
    Str {
        value: n.value.clone(),
        quoted: n.quoted,
        pos: pos_at(n),
    }
}

/// Go's `sortedQuotes`: the strings sorted, each `%q`, joined by ", ".
pub fn sorted_quotes<S: AsRef<str>>(items: &[S]) -> String {
    let mut v: Vec<&str> = items.iter().map(AsRef::as_ref).collect();
    v.sort_unstable();
    quotes(&v)
}

/// Go's `quotes`: each `%q`, joined by ", ", in the given order.
pub fn quotes<S: AsRef<str>>(items: &[S]) -> String {
    items
        .iter()
        .map(|s| go_quote(s.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Parser {
    fn error(&mut self, n: &Node, message: String) {
        self.errors.push(LintError {
            message,
            line: n.line,
            column: n.column,
            kind: "syntax-check",
        });
    }

    fn error_at(&mut self, pos: Pos, message: String) {
        self.errors.push(LintError {
            message,
            line: pos.line,
            column: pos.col,
            kind: "syntax-check",
        });
    }

    fn unexpected_key(&mut self, s: &Str, sec: &str, expected: &[&str]) {
        let sec = if sec.contains(' ') {
            sec.to_string()
        } else {
            format!("{} section", go_quote(sec))
        };
        let m = match expected.len() {
            1 => format!(
                "expected {} key for {sec} but got {}",
                go_quote(expected[0]),
                go_quote(&s.value)
            ),
            0 => format!("unexpected key {} for {sec}", go_quote(&s.value)),
            _ => format!(
                "unexpected key {} for {sec}. expected one of {}",
                go_quote(&s.value),
                sorted_quotes(expected)
            ),
        };
        self.error_at(s.pos, m);
    }

    fn check_not_empty(&mut self, sec: &str, len: usize, n: &Node) -> bool {
        if len == 0 {
            self.error(n, format!("{} section should not be empty", go_quote(sec)));
            return false;
        }
        true
    }

    fn check_sequence(&mut self, sec: &str, n: &Node, allow_empty: bool) -> bool {
        if n.kind != Kind::Sequence {
            self.error(
                n,
                format!(
                    "{} section must be sequence node but got {} node with {} tag",
                    go_quote(sec),
                    n.kind.name(),
                    go_quote(&n.tag)
                ),
            );
            return false;
        }
        allow_empty || self.check_not_empty(sec, n.content.len(), n)
    }

    fn check_string(&mut self, n: &Node, allow_empty: bool) -> bool {
        if n.kind != Kind::Scalar {
            self.error(
                n,
                format!(
                    "expected scalar node for string value but found {} node with {} tag",
                    n.kind.name(),
                    go_quote(&n.tag)
                ),
            );
            return false;
        }
        if !allow_empty && n.value.is_empty() {
            self.error(n, "string should not be empty".to_string());
            return false;
        }
        true
    }

    fn missing_expression(&mut self, n: &Node, expecting: &str) {
        self.error(
            n,
            format!("expecting a single ${{{{...}}}} expression or {expecting}, but found plain text node"),
        );
    }

    fn parse_expression(&mut self, n: &Node, expecting: &str) -> Option<Str> {
        if !is_expr_assigned(&n.value) {
            self.missing_expression(n, expecting);
            return None;
        }
        Some(new_string(n))
    }

    fn may_parse_expression(n: &Node) -> Option<Str> {
        if n.tag != "!!str" || !is_expr_assigned(&n.value) {
            return None;
        }
        Some(new_string(n))
    }

    fn parse_string(&mut self, n: &Node, allow_empty: bool) -> Str {
        if !self.check_string(n, allow_empty) {
            return Str {
                value: String::new(),
                quoted: false,
                pos: pos_at(n),
            };
        }
        new_string(n)
    }

    fn parse_string_sequence(
        &mut self,
        sec: &str,
        n: &Node,
        allow_empty: bool,
        allow_elem_empty: bool,
    ) -> Option<Vec<Str>> {
        if !self.check_sequence(sec, n, allow_empty) {
            return None;
        }
        Some(
            n.content
                .iter()
                .map(|c| self.parse_string(c, allow_elem_empty))
                .collect(),
        )
    }

    fn parse_string_or_string_sequence(
        &mut self,
        sec: &str,
        n: &Node,
        allow_empty: bool,
        allow_elem_empty: bool,
    ) -> Option<Vec<Str>> {
        if n.kind == Kind::Scalar {
            if allow_empty && n.tag == "!!null" {
                return Some(Vec::new());
            }
            return Some(vec![self.parse_string(n, allow_elem_empty)]);
        }
        self.parse_string_sequence(sec, n, allow_empty, allow_elem_empty)
    }

    fn parse_bool(&mut self, n: &Node) -> Option<Bool> {
        if n.kind != Kind::Scalar || (n.tag != "!!bool" && n.tag != "!!str") {
            self.error(
                n,
                format!(
                    "expected bool value but found {} node with {} tag",
                    n.kind.name(),
                    go_quote(&n.tag)
                ),
            );
            return None;
        }
        if n.tag == "!!str" {
            let e = self.parse_expression(n, "boolean literal \"true\" or \"false\"");
            return Some(Bool {
                value: false,
                expression: e,
                pos: pos_at(n),
            });
        }
        Some(Bool {
            value: n.value == "true",
            expression: None,
            pos: pos_at(n),
        })
    }

    fn parse_int(&mut self, n: &Node) -> Option<Int> {
        if n.kind != Kind::Scalar || (n.tag != "!!int" && n.tag != "!!str") {
            self.error(
                n,
                format!(
                    "expected scalar node for integer value but found {} node with {} tag",
                    n.kind.name(),
                    go_quote(&n.tag)
                ),
            );
            return None;
        }
        if n.tag == "!!str" {
            let e = self.parse_expression(n, "integer literal")?;
            return Some(Int {
                value: 0,
                expression: Some(e),
                pos: pos_at(n),
            });
        }
        match go_atoi(&n.value) {
            Ok(i) => Some(Int {
                value: i,
                expression: None,
                pos: pos_at(n),
            }),
            Err(err) => {
                self.error(
                    n,
                    format!("invalid integer value: {}: {err}", go_quote(&n.value)),
                );
                None
            }
        }
    }

    fn parse_float(&mut self, n: &Node) -> Option<Float> {
        if n.kind != Kind::Scalar || (n.tag != "!!float" && n.tag != "!!int" && n.tag != "!!str") {
            self.error(
                n,
                format!(
                    "expected scalar node for float value but found {} node with {} tag",
                    n.kind.name(),
                    go_quote(&n.tag)
                ),
            );
            return None;
        }
        if n.tag == "!!str" {
            let e = self.parse_expression(n, "float number literal")?;
            return Some(Float {
                value: 0.0,
                expression: Some(e),
                pos: pos_at(n),
            });
        }
        match go_parse_float(&n.value) {
            Some(f) if !f.is_nan() => Some(Float {
                value: f,
                expression: None,
                pos: pos_at(n),
            }),
            _ => {
                self.error(
                    n,
                    format!(
                        "invalid float value: {}: strconv.ParseFloat: parsing {}: invalid syntax",
                        go_quote(&n.value),
                        go_quote(&n.value)
                    ),
                );
                None
            }
        }
    }

    /// Iterates a mapping's entries, reporting duplicate keys, the merge
    /// key, and emptiness. `where_` is the already formatted description.
    fn parse_mapping<'n>(
        &mut self,
        where_: &str,
        n: &'n Node,
        allow_empty: bool,
        case_sensitive: bool,
    ) -> Vec<Entry<'n>> {
        let mut entries = Vec::new();
        if n.kind == Kind::Scalar && n.tag == "!!null" {
            if !allow_empty {
                self.error(
                    n,
                    format!("{where_} should not be empty. please remove this section if it's unnecessary"),
                );
            }
            return entries;
        }
        if n.kind != Kind::Mapping {
            self.error(
                n,
                format!(
                    "{where_} is {} node but mapping node is expected",
                    n.kind.name()
                ),
            );
            return entries;
        }
        let mut keys: BTreeMap<String, Pos> = BTreeMap::new();
        for (k, v) in n.pairs() {
            let key = self.parse_string(k, false);
            if key.value == "<<" {
                self.error_at(
                    key.pos,
                    "GitHub Actions does not support YAML merge key \"<<\"".to_string(),
                );
                continue;
            }
            let id = if case_sensitive {
                key.value.clone()
            } else {
                key.value.to_lowercase()
            };
            if let Some(prev) = keys.get(&id) {
                let note = if case_sensitive {
                    ""
                } else {
                    ". note that this key is case insensitive"
                };
                self.error_at(
                    key.pos,
                    format!(
                        "key {} is duplicated in {where_}. previously defined at {prev}{note}",
                        go_quote(&key.value)
                    ),
                );
                continue;
            }
            keys.insert(id.clone(), key.pos);
            entries.push(Entry { id, key, val: v });
        }
        if !allow_empty && entries.is_empty() {
            self.error(
                n,
                format!(
                    "{where_} should not be empty. please remove this section if it's unnecessary"
                ),
            );
        }
        entries
    }

    fn parse_section_mapping<'n>(
        &mut self,
        section: &str,
        n: &'n Node,
        allow_empty: bool,
        case_sensitive: bool,
    ) -> Vec<Entry<'n>> {
        let where_ = format!("{} section", go_quote(section));
        self.parse_mapping(&where_, n, allow_empty, case_sensitive)
    }

    fn parse_schedule_event(&mut self, n: &Node) -> Option<ScheduledEvent> {
        if !self.check_sequence("schedule", n, false) {
            return None;
        }
        let mut schedules = Vec::new();
        for c in &n.content {
            let mut cron = None;
            let mut timezone = None;
            for e in self.parse_mapping("element of \"schedule\" section", c, false, true) {
                match e.id.as_str() {
                    "cron" => {
                        let s = self.parse_string(e.val, false);
                        if !s.value.is_empty() {
                            cron = Some(s);
                        }
                    }
                    "timezone" => {
                        let s = self.parse_string(e.val, false);
                        if !s.value.is_empty() {
                            timezone = Some(s);
                        }
                    }
                    _ => self.unexpected_key(
                        &e.key,
                        "element of \"schedule\" section",
                        &["cron", "timezone"],
                    ),
                }
            }
            if let Some(cron) = cron {
                schedules.push(ScheduleEntry { cron, timezone });
            }
        }
        Some(ScheduledEvent { schedules })
    }

    fn parse_workflow_dispatch_event_input(&mut self, name: Str, n: &Node) -> DispatchInput {
        let mut ret = DispatchInput {
            name,
            description: None,
            required: None,
            default: None,
            ty: DispatchInputType::None,
            options: Vec::new(),
        };
        for e in self.parse_mapping("input settings of workflow_dispatch event", n, true, true) {
            match e.id.as_str() {
                "description" => ret.description = Some(self.parse_string(e.val, true)),
                "required" => ret.required = self.parse_bool(e.val),
                "default" => ret.default = Some(self.parse_string(e.val, true)),
                "type" => {
                    if !self.check_string(e.val, false) {
                        continue;
                    }
                    ret.ty = match e.val.value.as_str() {
                        "string" => DispatchInputType::String,
                        "number" => DispatchInputType::Number,
                        "boolean" => DispatchInputType::Boolean,
                        "choice" => DispatchInputType::Choice,
                        "environment" => DispatchInputType::Environment,
                        other => {
                            self.error(
                                e.val,
                                format!(
                                    "input type of workflow_dispatch event must be one of \"string\", \"number\", \"boolean\", \"choice\", \"environment\" but got {}",
                                    go_quote(other)
                                ),
                            );
                            ret.ty
                        }
                    };
                }
                "options" => {
                    ret.options = self
                        .parse_string_sequence("options", e.val, false, false)
                        .unwrap_or_default();
                }
                _ => self.unexpected_key(&e.key, "inputs", &["description", "required", "default"]),
            }
        }
        ret
    }

    fn parse_workflow_dispatch_event(&mut self, pos: Pos, n: &Node) -> WorkflowDispatchEvent {
        let mut ret = WorkflowDispatchEvent {
            inputs: BTreeMap::new(),
            pos,
        };
        for e in self.parse_section_mapping("workflow_dispatch", n, true, true) {
            if e.id != "inputs" {
                self.unexpected_key(&e.key, "workflow_dispatch", &["inputs"]);
                continue;
            }
            for i in self.parse_section_mapping("inputs", e.val, true, false) {
                let input = self.parse_workflow_dispatch_event_input(i.key, i.val);
                ret.inputs.insert(i.id, input);
            }
        }
        ret
    }

    fn parse_repository_dispatch_event(&mut self, n: &Node) -> RepositoryDispatchEvent {
        let mut ret = RepositoryDispatchEvent { types: Vec::new() };
        for e in self.parse_section_mapping("repository_dispatch", n, true, true) {
            if e.id == "types" {
                ret.types = self
                    .parse_string_or_string_sequence("types", e.val, false, false)
                    .unwrap_or_default();
            } else {
                self.unexpected_key(&e.key, "repository_dispatch", &["types"]);
            }
        }
        ret
    }

    fn parse_webhook_event_filter(&mut self, name: Str, n: &Node) -> WebhookEventFilter {
        let values = self
            .parse_string_or_string_sequence(&name.value, n, false, false)
            .unwrap_or_default();
        WebhookEventFilter { name, values }
    }

    fn parse_webhook_event(&mut self, name: Str, n: &Node) -> WebhookEvent {
        let mut ret = WebhookEvent {
            pos: name.pos,
            hook: name.clone(),
            types: Vec::new(),
            branches: None,
            branches_ignore: None,
            tags: None,
            tags_ignore: None,
            paths: None,
            paths_ignore: None,
            workflows: Vec::new(),
        };
        for e in self.parse_section_mapping(&name.value, n, true, true) {
            match e.id.as_str() {
                "types" => {
                    ret.types = self
                        .parse_string_or_string_sequence(&e.key.value, e.val, false, false)
                        .unwrap_or_default();
                }
                "branches" => ret.branches = Some(self.parse_webhook_event_filter(e.key, e.val)),
                "branches-ignore" => {
                    ret.branches_ignore = Some(self.parse_webhook_event_filter(e.key, e.val));
                }
                "tags" => ret.tags = Some(self.parse_webhook_event_filter(e.key, e.val)),
                "tags-ignore" => {
                    ret.tags_ignore = Some(self.parse_webhook_event_filter(e.key, e.val));
                }
                "paths" => ret.paths = Some(self.parse_webhook_event_filter(e.key, e.val)),
                "paths-ignore" => {
                    ret.paths_ignore = Some(self.parse_webhook_event_filter(e.key, e.val));
                }
                "workflows" => {
                    ret.workflows = self
                        .parse_string_or_string_sequence(&e.key.value, e.val, false, false)
                        .unwrap_or_default();
                }
                _ => self.unexpected_key(
                    &e.key,
                    &name.value,
                    &[
                        "types",
                        "branches",
                        "branches-ignore",
                        "tags",
                        "tags-ignore",
                        "paths",
                        "paths-ignore",
                        "workflows",
                    ],
                ),
            }
        }
        ret
    }

    fn parse_workflow_call_event_input(
        &mut self,
        id: String,
        name: Str,
        n: &Node,
    ) -> WorkflowCallEventInput {
        let mut ret = WorkflowCallEventInput {
            name: name.clone(),
            description: None,
            default: None,
            required: None,
            ty: WorkflowCallInputType::Invalid,
            id,
        };
        let mut typed = false;
        for e in self.parse_mapping("input of workflow_call event", n, true, true) {
            match e.id.as_str() {
                "description" => ret.description = Some(self.parse_string(e.val, true)),
                "required" => ret.required = self.parse_bool(e.val),
                "default" => ret.default = Some(self.parse_string(e.val, true)),
                "type" => {
                    typed = true;
                    if !self.check_string(e.val, false) {
                        continue;
                    }
                    ret.ty = match e.val.value.as_str() {
                        "boolean" => WorkflowCallInputType::Boolean,
                        "number" => WorkflowCallInputType::Number,
                        "string" => WorkflowCallInputType::String,
                        other => {
                            self.error(
                                e.val,
                                format!(
                                    "invalid value {} for input type of workflow_call event. it must be one of \"boolean\", \"number\", or \"string\"",
                                    go_quote(other)
                                ),
                            );
                            ret.ty
                        }
                    };
                }
                _ => self.unexpected_key(
                    &e.key,
                    "inputs at workflow_call event",
                    &["description", "required", "default", "type"],
                ),
            }
        }
        if !typed {
            self.error_at(
                name.pos,
                format!(
                    "\"type\" is missing at {} input of workflow_call event",
                    go_quote(&name.value)
                ),
            );
        }
        ret
    }

    fn parse_workflow_call_event_secret(&mut self, name: Str, n: &Node) -> WorkflowCallEventSecret {
        let mut ret = WorkflowCallEventSecret {
            name,
            description: None,
            required: None,
        };
        for e in self.parse_mapping("secret of workflow_call event", n, true, true) {
            match e.id.as_str() {
                "description" => ret.description = Some(self.parse_string(e.val, true)),
                "required" => ret.required = self.parse_bool(e.val),
                _ => self.unexpected_key(&e.key, "secrets", &["description", "required"]),
            }
        }
        ret
    }

    fn parse_workflow_call_event_output(&mut self, name: Str, n: &Node) -> WorkflowCallEventOutput {
        let mut output = WorkflowCallEventOutput {
            name: name.clone(),
            description: None,
            value: None,
        };
        for e in self.parse_mapping("output of workflow_call event", n, true, true) {
            match e.id.as_str() {
                "description" => output.description = Some(self.parse_string(e.val, true)),
                "value" => output.value = Some(self.parse_string(e.val, false)),
                _ => self.unexpected_key(
                    &e.key,
                    "outputs at workflow_call event",
                    &["description", "value"],
                ),
            }
        }
        if output.value.is_none() {
            self.error_at(
                name.pos,
                format!(
                    "\"value\" is missing at {} output of workflow_call event",
                    go_quote(&name.value)
                ),
            );
        }
        output
    }

    fn parse_workflow_call_event(&mut self, pos: Pos, n: &Node) -> WorkflowCallEvent {
        let mut ret = WorkflowCallEvent {
            inputs: Vec::new(),
            secrets: None,
            outputs: BTreeMap::new(),
            pos,
        };
        for e in self.parse_section_mapping("workflow_call", n, true, true) {
            match e.id.as_str() {
                "inputs" => {
                    for i in self.parse_section_mapping("inputs", e.val, true, false) {
                        let input = self.parse_workflow_call_event_input(i.id, i.key, i.val);
                        ret.inputs.push(input);
                    }
                }
                "secrets" => {
                    let mut secrets = BTreeMap::new();
                    for s in self.parse_section_mapping("secrets", e.val, true, false) {
                        let secret = self.parse_workflow_call_event_secret(s.key, s.val);
                        secrets.insert(s.id, secret);
                    }
                    ret.secrets = Some(secrets);
                }
                "outputs" => {
                    for o in self.parse_section_mapping("outputs", e.val, true, false) {
                        let output = self.parse_workflow_call_event_output(o.key, o.val);
                        ret.outputs.insert(o.id, output);
                    }
                }
                _ => {
                    self.unexpected_key(&e.key, "workflow_call", &["inputs", "secrets", "outputs"]);
                }
            }
        }
        ret
    }

    fn parse_image_version_event(&mut self, n: &Node) -> ImageVersionEvent {
        let mut ret = ImageVersionEvent {
            names: Vec::new(),
            versions: Vec::new(),
        };
        for e in self.parse_section_mapping("image_version", n, true, true) {
            match e.id.as_str() {
                "names" => {
                    ret.names = self
                        .parse_string_sequence("names", e.val, false, false)
                        .unwrap_or_default();
                }
                "versions" => {
                    ret.versions = self
                        .parse_string_sequence("versions", e.val, false, false)
                        .unwrap_or_default();
                }
                _ => self.unexpected_key(&e.key, "image_version", &["names", "versions"]),
            }
        }
        ret
    }

    fn parse_event_with_no_config(&mut self, n: &Node) -> Option<Event> {
        let s = self.parse_string(n, false);
        let pos = pos_at(n);
        match s.value.as_str() {
            "" => None,
            "schedule" => {
                self.error(
                    n,
                    "schedule event must be configured with mapping".to_string(),
                );
                None
            }
            "repository_dispatch" => Some(Event::RepositoryDispatch(RepositoryDispatchEvent {
                types: Vec::new(),
            })),
            "workflow_dispatch" => Some(Event::WorkflowDispatch(WorkflowDispatchEvent {
                inputs: BTreeMap::new(),
                pos,
            })),
            "workflow_call" => Some(Event::WorkflowCall(WorkflowCallEvent {
                inputs: Vec::new(),
                secrets: None,
                outputs: BTreeMap::new(),
                pos,
            })),
            "image_version" => Some(Event::ImageVersion(ImageVersionEvent {
                names: Vec::new(),
                versions: Vec::new(),
            })),
            _ => Some(Event::Webhook(Box::new(WebhookEvent {
                hook: s,
                types: Vec::new(),
                branches: None,
                branches_ignore: None,
                tags: None,
                tags_ignore: None,
                paths: None,
                paths_ignore: None,
                workflows: Vec::new(),
                pos,
            }))),
        }
    }

    fn parse_events(&mut self, n: &Node) -> Option<Vec<Event>> {
        match n.kind {
            Kind::Scalar => Some(self.parse_event_with_no_config(n).into_iter().collect()),
            Kind::Mapping => {
                let mut ret = Vec::new();
                for e in self.parse_section_mapping("on", n, false, true) {
                    let pos = e.key.pos;
                    match e.id.as_str() {
                        "schedule" => {
                            if let Some(ev) = self.parse_schedule_event(e.val) {
                                ret.push(Event::Scheduled(ev));
                            }
                        }
                        "workflow_dispatch" => {
                            ret.push(Event::WorkflowDispatch(
                                self.parse_workflow_dispatch_event(pos, e.val),
                            ));
                        }
                        "repository_dispatch" => {
                            ret.push(Event::RepositoryDispatch(
                                self.parse_repository_dispatch_event(e.val),
                            ));
                        }
                        "workflow_call" => ret.push(Event::WorkflowCall(
                            self.parse_workflow_call_event(pos, e.val),
                        )),
                        "image_version" => {
                            ret.push(Event::ImageVersion(self.parse_image_version_event(e.val)));
                        }
                        _ => ret.push(Event::Webhook(Box::new(
                            self.parse_webhook_event(e.key, e.val),
                        ))),
                    }
                }
                Some(ret)
            }
            Kind::Sequence => {
                self.check_not_empty("on", n.content.len(), n);
                let mut ret = Vec::new();
                for c in &n.content {
                    if let Some(e) = self.parse_event_with_no_config(c) {
                        ret.push(e);
                    }
                }
                Some(ret)
            }
            other => {
                self.error(
                    n,
                    format!(
                        "\"on\" section value is expected to be mapping or sequence but found {} node",
                        other.name()
                    ),
                );
                None
            }
        }
    }

    fn parse_permissions(&mut self, n: &Node) -> Permissions {
        let mut ret = Permissions {
            all: None,
            scopes: BTreeMap::new(),
        };
        if n.kind == Kind::Scalar {
            ret.all = Some(self.parse_string(n, false));
        } else {
            for e in self.parse_section_mapping("permissions", n, true, false) {
                let value = self.parse_string(e.val, false);
                ret.scopes
                    .insert(e.id, PermissionScope { name: e.key, value });
            }
        }
        ret
    }

    fn parse_env(&mut self, n: &Node) -> Env {
        if n.kind == Kind::Scalar {
            return Env {
                vars: BTreeMap::new(),
                expression: self.parse_expression(n, "mapping value for \"env\" section"),
            };
        }
        let mut vars = BTreeMap::new();
        for e in self.parse_section_mapping("env", n, false, false) {
            let value = self.parse_string(e.val, true);
            vars.insert(e.id, EnvVar { name: e.key, value });
        }
        Env {
            vars,
            expression: None,
        }
    }

    fn parse_defaults(&mut self, n: &Node) -> Defaults {
        let mut ret = Defaults { run: None };
        for e in self.parse_section_mapping("defaults", n, false, true) {
            if e.id != "run" {
                self.unexpected_key(&e.key, "defaults", &["run"]);
                continue;
            }
            let mut run = DefaultsRun {
                shell: None,
                working_directory: None,
            };
            for r in self.parse_section_mapping("run", e.val, false, true) {
                match r.id.as_str() {
                    "shell" => run.shell = Some(self.parse_string(r.val, false)),
                    "working-directory" => {
                        run.working_directory = Some(self.parse_string(r.val, false));
                    }
                    _ => self.unexpected_key(&r.key, "run", &["shell", "working-directory"]),
                }
            }
            ret.run = Some(run);
        }
        if ret.run.is_none() {
            self.error(
                n,
                "\"defaults\" section should have \"run\" section".to_string(),
            );
        }
        ret
    }

    fn parse_concurrency(&mut self, pos: Pos, n: &Node) -> Concurrency {
        let mut ret = Concurrency {
            group: None,
            cancel_in_progress: None,
        };
        if n.kind == Kind::Scalar {
            ret.group = Some(self.parse_string(n, false));
            return ret;
        }
        for e in self.parse_section_mapping("concurrency", n, false, true) {
            match e.id.as_str() {
                "group" => ret.group = Some(self.parse_string(e.val, false)),
                "cancel-in-progress" => ret.cancel_in_progress = self.parse_bool(e.val),
                _ => self.unexpected_key(&e.key, "concurrency", &["group", "cancel-in-progress"]),
            }
        }
        if ret.group.is_none() {
            self.error_at(
                pos,
                "group name is missing in \"concurrency\" section".to_string(),
            );
        }
        ret
    }

    fn parse_environment(&mut self, pos: Pos, n: &Node) -> Environment {
        let mut ret = Environment {
            name: None,
            url: None,
            deployment: None,
        };
        if n.kind == Kind::Scalar {
            ret.name = Some(self.parse_string(n, false));
            return ret;
        }
        for e in self.parse_section_mapping("environment", n, false, true) {
            match e.id.as_str() {
                "name" => ret.name = Some(self.parse_string(e.val, false)),
                "url" => ret.url = Some(self.parse_string(e.val, false)),
                "deployment" => ret.deployment = self.parse_bool(e.val),
                _ => self.unexpected_key(&e.key, "environment", &["deployment", "name", "url"]),
            }
        }
        if ret.name.is_none() {
            self.error_at(
                pos,
                "name is missing in \"environment\" section".to_string(),
            );
        }
        ret
    }

    fn parse_outputs(&mut self, n: &Node) -> BTreeMap<String, Output> {
        let mut ret = BTreeMap::new();
        for e in self.parse_section_mapping("outputs", n, false, false) {
            let value = self.parse_string(e.val, true);
            ret.insert(e.id, Output { value });
        }
        self.check_not_empty("outputs", ret.len(), n);
        ret
    }

    fn parse_raw_yaml_value(&mut self, n: &Node) -> Option<RawYamlValue> {
        match n.kind {
            Kind::Scalar => Some(RawYamlValue::String {
                value: n.value.clone(),
                pos: pos_at(n),
            }),
            Kind::Sequence => {
                let mut elems = Vec::new();
                for c in &n.content {
                    if let Some(v) = self.parse_raw_yaml_value(c) {
                        elems.push(v);
                    }
                }
                Some(RawYamlValue::Array {
                    elems,
                    pos: pos_at(n),
                })
            }
            Kind::Mapping => {
                let mut props = BTreeMap::new();
                for e in self.parse_mapping("matrix row value", n, true, false) {
                    if let Some(v) = self.parse_raw_yaml_value(e.val) {
                        props.insert(e.id, v);
                    }
                }
                Some(RawYamlValue::Object {
                    props,
                    pos: pos_at(n),
                })
            }
            other => {
                self.error(
                    n,
                    format!(
                        "unexpected {} node on parsing value in matrix row",
                        other.name()
                    ),
                );
                None
            }
        }
    }

    fn parse_matrix_combinations(&mut self, sec: &str, n: &Node) -> Option<MatrixCombinations> {
        if n.kind == Kind::Scalar {
            return Some(MatrixCombinations {
                combinations: Vec::new(),
                expression: self.parse_expression(n, "array of matrix combination"),
            });
        }
        if !self.check_sequence(sec, n, false) {
            return None;
        }
        let mut ret = Vec::new();
        for c in &n.content {
            if c.kind == Kind::Scalar {
                if let Some(e) = self.parse_expression(c, "mapping of matrix combination") {
                    ret.push(MatrixCombination {
                        assigns: BTreeMap::new(),
                        expression: Some(e),
                    });
                }
                continue;
            }
            let mut assigns = BTreeMap::new();
            let where_ = format!("element in {} section", go_quote(sec));
            for e in self.parse_mapping(&where_, c, false, false) {
                if let Some(v) = self.parse_raw_yaml_value(e.val) {
                    assigns.insert(
                        e.id,
                        MatrixAssign {
                            key: e.key,
                            value: v,
                        },
                    );
                }
            }
            ret.push(MatrixCombination {
                assigns,
                expression: None,
            });
        }
        Some(MatrixCombinations {
            combinations: ret,
            expression: None,
        })
    }

    fn parse_matrix(&mut self, pos: Pos, n: &Node) -> Matrix {
        if n.kind == Kind::Scalar {
            return Matrix {
                rows: BTreeMap::new(),
                include: None,
                exclude: None,
                expression: self.parse_expression(n, "matrix"),
                pos: pos_at(n),
            };
        }
        let mut ret = Matrix {
            rows: BTreeMap::new(),
            include: None,
            exclude: None,
            expression: None,
            pos,
        };
        for e in self.parse_section_mapping("matrix", n, false, false) {
            match e.id.as_str() {
                "include" => ret.include = self.parse_matrix_combinations("include", e.val),
                "exclude" => ret.exclude = self.parse_matrix_combinations("exclude", e.val),
                _ => {
                    if e.val.kind == Kind::Scalar {
                        let expression =
                            self.parse_expression(e.val, "array value for matrix variations");
                        ret.rows.insert(
                            e.id,
                            MatrixRow {
                                name: None,
                                values: None,
                                expression,
                            },
                        );
                        continue;
                    }
                    if !self.check_sequence("matrix values", e.val, false) {
                        continue;
                    }
                    let mut values = Vec::new();
                    for c in &e.val.content {
                        if let Some(v) = self.parse_raw_yaml_value(c) {
                            values.push(v);
                        }
                    }
                    ret.rows.insert(
                        e.id,
                        MatrixRow {
                            name: Some(e.key),
                            values: Some(values),
                            expression: None,
                        },
                    );
                }
            }
        }
        ret
    }

    fn parse_max_parallel(&mut self, n: &Node) -> Option<Int> {
        let i = self.parse_int(n);
        if let Some(i) = &i
            && i.expression.is_none()
            && i.value <= 0
        {
            self.error(
                n,
                format!(
                    "value at \"max-parallel\" must be greater than zero: {}",
                    i.value
                ),
            );
        }
        i
    }

    fn parse_strategy(&mut self, n: &Node) -> Strategy {
        let mut ret = Strategy {
            matrix: None,
            fail_fast: None,
            max_parallel: None,
        };
        for e in self.parse_section_mapping("strategy", n, false, true) {
            match e.id.as_str() {
                "matrix" => ret.matrix = Some(self.parse_matrix(e.key.pos, e.val)),
                "fail-fast" => ret.fail_fast = self.parse_bool(e.val),
                "max-parallel" => ret.max_parallel = self.parse_max_parallel(e.val),
                _ => self.unexpected_key(
                    &e.key,
                    "strategy",
                    &["matrix", "fail-fast", "max-parallel"],
                ),
            }
        }
        ret
    }

    fn parse_credentials(&mut self, pos: Pos, n: &Node) -> Option<Credentials> {
        let mut ret = Credentials {
            username: None,
            password: None,
            expression: None,
        };
        if let Some(e) = Self::may_parse_expression(n) {
            ret.expression = Some(e);
            return Some(ret);
        }
        for e in self.parse_section_mapping("credentials", n, false, true) {
            match e.id.as_str() {
                "username" => ret.username = Some(self.parse_string(e.val, false)),
                "password" => ret.password = Some(self.parse_string(e.val, false)),
                _ => self.unexpected_key(&e.key, "credentials", &["username", "password"]),
            }
        }
        if ret.username.is_none() || ret.password.is_none() {
            self.error_at(
                pos,
                "both \"username\" and \"password\" must be specified in \"credentials\" section"
                    .to_string(),
            );
            return None;
        }
        Some(ret)
    }

    fn parse_container(&mut self, sec: &str, pos: Pos, n: &Node) -> Container {
        let mut ret = Container {
            image: None,
            credentials: None,
            env: None,
            ports: Vec::new(),
            volumes: Vec::new(),
            options: None,
        };
        if n.kind == Kind::Scalar {
            ret.image = Some(self.parse_string(n, false));
            return ret;
        }
        for e in self.parse_section_mapping(sec, n, false, true) {
            match e.id.as_str() {
                "image" => ret.image = Some(self.parse_string(e.val, false)),
                "credentials" => ret.credentials = self.parse_credentials(e.key.pos, e.val),
                "env" => ret.env = Some(self.parse_env(e.val)),
                "ports" => {
                    ret.ports = self
                        .parse_string_sequence("ports", e.val, true, false)
                        .unwrap_or_default();
                }
                // actionlint stores volumes into Ports (a long-standing slip);
                // kept so that the two tools report the same things.
                "volumes" => {
                    ret.ports = self
                        .parse_string_sequence("volumes", e.val, true, false)
                        .unwrap_or_default();
                }
                "options" => ret.options = Some(self.parse_string(e.val, true)),
                _ => self.unexpected_key(
                    &e.key,
                    sec,
                    &["image", "credentials", "env", "ports", "volumes", "options"],
                ),
            }
        }
        if ret.image.is_none() {
            self.error_at(
                pos,
                format!("\"image\" is missing in {} section", go_quote(sec)),
            );
        }
        ret
    }

    fn parse_services(&mut self, n: &Node) -> Services {
        let mut ret = Services {
            value: BTreeMap::new(),
            expression: None,
        };
        if let Some(e) = Self::may_parse_expression(n) {
            ret.expression = Some(e);
        } else {
            for e in self.parse_section_mapping("services", n, false, false) {
                let container = self.parse_container("services", e.key.pos, e.val);
                ret.value.insert(
                    e.id,
                    Service {
                        name: e.key,
                        container,
                    },
                );
            }
        }
        ret
    }

    fn parse_timeout_minutes(&mut self, n: &Node) -> Option<Float> {
        let f = self.parse_float(n);
        if let Some(f) = &f
            && f.expression.is_none()
            && f.value <= 0.0
        {
            self.error(
                n,
                format!(
                    "value at \"timeout-minutes\" must be greater than zero: {}",
                    go_format_float(f.value)
                ),
            );
        }
        f
    }

    fn parse_step_exec_action(&mut self, entries: &[Entry<'_>], is_docker: bool) -> ExecAction {
        let mut ret = ExecAction {
            uses: None,
            inputs: BTreeMap::new(),
            entrypoint: None,
            args: None,
        };
        for e in entries {
            match e.id.as_str() {
                "uses" => ret.uses = Some(self.parse_string(e.val, false)),
                "with" => {
                    ret.inputs = BTreeMap::new();
                    for w in self.parse_section_mapping("with", e.val, false, false) {
                        if is_docker {
                            match w.id.as_str() {
                                "entrypoint" => {
                                    ret.entrypoint = Some(self.parse_string(w.val, false));
                                    continue;
                                }
                                "args" => {
                                    ret.args = Some(self.parse_string(w.val, true));
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        let value = self.parse_string(w.val, true);
                        ret.inputs.insert(w.id, Input { name: w.key, value });
                    }
                }
                "id" | "if" | "name" | "env" | "continue-on-error" | "timeout-minutes" => {}
                _ => self.unexpected_key(
                    &e.key,
                    "step to execute action",
                    &[
                        "id",
                        "if",
                        "name",
                        "env",
                        "continue-on-error",
                        "timeout-minutes",
                        "uses",
                        "with",
                    ],
                ),
            }
        }
        ret
    }

    fn parse_step_exec_run(&mut self, entries: &[Entry<'_>]) -> ExecRun {
        let mut ret = ExecRun {
            run: None,
            shell: None,
            working_directory: None,
            run_pos: Pos::default(),
        };
        for e in entries {
            match e.id.as_str() {
                "run" => {
                    ret.run = Some(self.parse_string(e.val, false));
                    ret.run_pos = e.key.pos;
                }
                "shell" => ret.shell = Some(self.parse_string(e.val, false)),
                "working-directory" => {
                    ret.working_directory = Some(self.parse_string(e.val, false));
                }
                "id" | "if" | "name" | "env" | "continue-on-error" | "timeout-minutes" => {}
                _ => self.unexpected_key(
                    &e.key,
                    "step to run shell command",
                    &[
                        "id",
                        "if",
                        "name",
                        "env",
                        "continue-on-error",
                        "timeout-minutes",
                        "run",
                        "shell",
                        "working-directory",
                    ],
                ),
            }
        }
        ret
    }

    fn parse_step(&mut self, n: &Node) -> Step {
        let mut ret = Step {
            id: None,
            if_cond: None,
            name: None,
            exec: None,
            env: None,
            continue_on_error: None,
            timeout_minutes: None,
        };
        #[derive(PartialEq, Eq, Clone, Copy)]
        enum StepKind {
            Unknown,
            Action,
            Docker,
            Run,
        }
        let mut kind = StepKind::Unknown;
        let entries = self.parse_mapping("element of \"steps\" section", n, false, true);
        for e in &entries {
            match e.id.as_str() {
                "id" => ret.id = Some(self.parse_string(e.val, false)),
                "if" => ret.if_cond = Some(self.parse_string(e.val, false)),
                "name" => ret.name = Some(self.parse_string(e.val, true)),
                "env" => ret.env = Some(self.parse_env(e.val)),
                "continue-on-error" => ret.continue_on_error = self.parse_bool(e.val),
                "timeout-minutes" => ret.timeout_minutes = self.parse_timeout_minutes(e.val),
                "uses" => {
                    kind = if e.val.value.starts_with("docker://") {
                        StepKind::Docker
                    } else {
                        StepKind::Action
                    };
                }
                "run" => kind = StepKind::Run,
                _ => {}
            }
        }
        match kind {
            StepKind::Action | StepKind::Docker => {
                ret.exec = Some(Exec::Action(
                    self.parse_step_exec_action(&entries, kind == StepKind::Docker),
                ));
            }
            StepKind::Run => ret.exec = Some(Exec::Run(self.parse_step_exec_run(&entries))),
            StepKind::Unknown => self.error(
                n,
                "step must run script with \"run\" section or run action with \"uses\" section"
                    .to_string(),
            ),
        }
        ret
    }

    fn parse_steps(&mut self, n: &Node) -> Option<Vec<Step>> {
        if !self.check_sequence("steps", n, false) {
            return None;
        }
        Some(n.content.iter().map(|c| self.parse_step(c)).collect())
    }

    fn parse_runs_on(&mut self, n: &Node) -> Runner {
        if let Some(expr) = Self::may_parse_expression(n) {
            return Runner {
                labels: Vec::new(),
                labels_expr: Some(expr),
                group: None,
            };
        }
        if n.kind == Kind::Scalar || n.kind == Kind::Sequence {
            let labels = self
                .parse_string_or_string_sequence("runs-on", n, false, false)
                .unwrap_or_default();
            return Runner {
                labels,
                labels_expr: None,
                group: None,
            };
        }
        let mut r = Runner {
            labels: Vec::new(),
            labels_expr: None,
            group: None,
        };
        for e in self.parse_section_mapping("runs-on", n, false, true) {
            match e.id.as_str() {
                "labels" => {
                    if let Some(expr) = Self::may_parse_expression(e.val) {
                        r.labels_expr = Some(expr);
                        continue;
                    }
                    r.labels = self
                        .parse_string_or_string_sequence("labels", e.val, false, false)
                        .unwrap_or_default();
                }
                "group" => r.group = Some(self.parse_string(e.val, false)),
                _ => self.unexpected_key(&e.key, "runs-on", &["labels", "group"]),
            }
        }
        r
    }

    fn parse_snapshot(&mut self, pos: Pos, n: &Node) -> Option<Snapshot> {
        match n.kind {
            Kind::Scalar => Some(Snapshot {
                image_name: Some(self.parse_string(n, false)),
                version: None,
                if_cond: None,
            }),
            Kind::Mapping => {
                let mut ret = Snapshot {
                    image_name: None,
                    version: None,
                    if_cond: None,
                };
                // actionlint names this section "on" in its mapping messages.
                for e in self.parse_section_mapping("on", n, false, true) {
                    match e.id.as_str() {
                        "image-name" => ret.image_name = Some(self.parse_string(e.val, false)),
                        "version" => ret.version = Some(self.parse_string(e.val, false)),
                        "if" => ret.if_cond = Some(self.parse_string(e.val, false)),
                        _ => self.unexpected_key(
                            &e.key,
                            "snapshot",
                            &["image-name", "version", "if"],
                        ),
                    }
                }
                if ret.image_name.is_none() {
                    self.error_at(
                        pos,
                        "\"snapshot\" section must have \"image-name\" configuration".to_string(),
                    );
                }
                Some(ret)
            }
            other => {
                self.error(
                    n,
                    format!(
                        "\"snapshot\" section value must be string or mapping but found {} node",
                        other.name()
                    ),
                );
                None
            }
        }
    }

    fn parse_job(&mut self, id: Str, n: &Node) -> Job {
        let mut ret = Job {
            pos: id.pos,
            id: id.clone(),
            name: None,
            needs: Vec::new(),
            runs_on: None,
            permissions: None,
            environment: None,
            concurrency: None,
            outputs: BTreeMap::new(),
            env: None,
            defaults: None,
            if_cond: None,
            steps: None,
            timeout_minutes: None,
            strategy: None,
            continue_on_error: None,
            container: None,
            services: None,
            workflow_call: None,
            snapshot: None,
        };
        let mut call = WorkflowCall::default();
        let mut steps_only_key: Option<Str> = None;
        let mut call_only_key: Option<Str> = None;

        let where_ = format!("{} job", go_quote(&id.value));
        for e in self.parse_mapping(&where_, n, false, true) {
            let (k, v) = (e.key, e.val);
            match e.id.as_str() {
                "name" => ret.name = Some(self.parse_string(v, true)),
                "needs" => {
                    if v.kind == Kind::Scalar {
                        ret.needs = vec![self.parse_string(v, false)];
                    } else {
                        ret.needs = self
                            .parse_string_sequence("needs", v, false, false)
                            .unwrap_or_default();
                    }
                }
                "runs-on" => {
                    ret.runs_on = Some(self.parse_runs_on(v));
                    steps_only_key = Some(k);
                }
                "permissions" => ret.permissions = Some(self.parse_permissions(v)),
                "environment" => {
                    ret.environment = Some(self.parse_environment(k.pos, v));
                    steps_only_key = Some(k);
                }
                "concurrency" => ret.concurrency = Some(self.parse_concurrency(k.pos, v)),
                "outputs" => {
                    ret.outputs = self.parse_outputs(v);
                    steps_only_key = Some(k);
                }
                "env" => {
                    ret.env = Some(self.parse_env(v));
                    steps_only_key = Some(k);
                }
                "defaults" => {
                    ret.defaults = Some(self.parse_defaults(v));
                    steps_only_key = Some(k);
                }
                "if" => ret.if_cond = Some(self.parse_string(v, false)),
                "steps" => {
                    ret.steps = self.parse_steps(v);
                    steps_only_key = Some(k);
                }
                "timeout-minutes" => {
                    ret.timeout_minutes = self.parse_timeout_minutes(v);
                    steps_only_key = Some(k);
                }
                "strategy" => ret.strategy = Some(self.parse_strategy(v)),
                "continue-on-error" => {
                    ret.continue_on_error = self.parse_bool(v);
                    steps_only_key = Some(k);
                }
                "container" => {
                    ret.container = Some(self.parse_container("container", k.pos, v));
                    steps_only_key = Some(k);
                }
                "services" => ret.services = Some(self.parse_services(v)),
                "uses" => {
                    call.uses = Some(self.parse_string(v, false));
                    call_only_key = Some(k);
                }
                "with" => {
                    call.inputs = BTreeMap::new();
                    for w in self.parse_section_mapping("with", v, false, false) {
                        let value = self.parse_string(w.val, true);
                        call.inputs
                            .insert(w.id, WorkflowCallInput { name: w.key, value });
                    }
                    call_only_key = Some(k);
                }
                "secrets" => {
                    if v.kind == Kind::Scalar {
                        if v.value == "inherit" {
                            call.inherit_secrets = true;
                        } else {
                            self.error(
                                v,
                                format!(
                                    "expected mapping node for secrets or \"inherit\" string node but found {} node",
                                    go_quote(&v.value)
                                ),
                            );
                        }
                    } else {
                        call.secrets = BTreeMap::new();
                        for s in self.parse_section_mapping("secrets", v, false, false) {
                            let value = self.parse_string(s.val, true);
                            call.secrets
                                .insert(s.id, WorkflowCallSecret { name: s.key, value });
                        }
                    }
                    call_only_key = Some(k);
                }
                "snapshot" => ret.snapshot = self.parse_snapshot(k.pos, v),
                _ => self.unexpected_key(
                    &k,
                    "job",
                    &[
                        "name",
                        "needs",
                        "runs-on",
                        "permissions",
                        "environment",
                        "concurrency",
                        "outputs",
                        "env",
                        "defaults",
                        "if",
                        "steps",
                        "timeout-minutes",
                        "strategy",
                        "continue-on-error",
                        "container",
                        "services",
                        "uses",
                        "with",
                        "secrets",
                        "snapshot",
                    ],
                ),
            }
        }

        if call.uses.is_some() {
            if let Some(k) = steps_only_key {
                self.error_at(
                    k.pos,
                    format!(
                        "when a reusable workflow is called with \"uses\", {} is not available. only following keys are allowed: \"name\", \"uses\", \"with\", \"secrets\", \"needs\", \"if\", and \"permissions\" in job {}",
                        go_quote(&k.value),
                        go_quote(&id.value)
                    ),
                );
            } else {
                ret.workflow_call = Some(call);
            }
        } else {
            if ret.steps.is_none() {
                self.error_at(
                    id.pos,
                    format!(
                        "\"steps\" section is missing in job {}",
                        go_quote(&id.value)
                    ),
                );
            }
            if ret.runs_on.is_none() {
                self.error_at(
                    id.pos,
                    format!(
                        "\"runs-on\" section is missing in job {}",
                        go_quote(&id.value)
                    ),
                );
            }
            if let Some(k) = call_only_key {
                self.error_at(
                    k.pos,
                    format!(
                        "{} is only available for a reusable workflow call with \"uses\" but \"uses\" is not found in job {}",
                        go_quote(&k.value),
                        go_quote(&id.value)
                    ),
                );
            }
        }
        ret
    }

    fn parse_jobs(&mut self, n: &Node) -> BTreeMap<String, Job> {
        let mut ret = BTreeMap::new();
        for e in self.parse_section_mapping("jobs", n, false, false) {
            let job = self.parse_job(e.key, e.val);
            ret.insert(e.id, job);
        }
        ret
    }

    fn parse_workflow(&mut self, doc: &Node) -> Workflow {
        let mut w = Workflow::default();
        let doc_pos = Pos {
            line: if doc.line == 0 { 1 } else { doc.line },
            col: if doc.column == 0 { 1 } else { doc.column },
        };
        let Some(root) = doc.content.first() else {
            self.error_at(doc_pos, "workflow is empty".to_string());
            return w;
        };
        for e in self.parse_section_mapping("workflow", root, false, true) {
            let (k, v) = (e.key, e.val);
            match e.id.as_str() {
                "name" => w.name = Some(self.parse_string(v, true)),
                "on" => w.on = self.parse_events(v),
                "permissions" => w.permissions = Some(self.parse_permissions(v)),
                "env" => w.env = Some(self.parse_env(v)),
                "defaults" => w.defaults = Some(self.parse_defaults(v)),
                "concurrency" => w.concurrency = Some(self.parse_concurrency(k.pos, v)),
                "jobs" => w.jobs = Some(self.parse_jobs(v)),
                "run-name" => w.run_name = Some(self.parse_string(v, false)),
                _ => self.unexpected_key(
                    &k,
                    "workflow",
                    &[
                        "name",
                        "run-name",
                        "on",
                        "permissions",
                        "env",
                        "defaults",
                        "concurrency",
                        "jobs",
                    ],
                ),
            }
        }
        if w.on.is_none() {
            self.error_at(doc_pos, "\"on\" section is missing in workflow".to_string());
        }
        if w.jobs.is_none() {
            self.error_at(
                doc_pos,
                "\"jobs\" section is missing in workflow".to_string(),
            );
        }
        w
    }
}

/// Go's `strconv.Atoi`, with its error text.
fn go_atoi(s: &str) -> Result<i64, String> {
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "strconv.Atoi: parsing {}: invalid syntax",
            go_quote(s)
        ));
    }
    s.parse::<i64>()
        .map_err(|_| format!("strconv.Atoi: parsing {}: value out of range", go_quote(s)))
}
