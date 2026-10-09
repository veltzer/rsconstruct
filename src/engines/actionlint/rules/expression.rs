//! The `expression` rule: every `${{ }}` in the workflow is parsed and
//! type-checked, with the contexts available where it sits. A port of
//! actionlint's `rule_expression.go`.

use std::collections::BTreeMap;

use super::{Rule, RuleBase};
use crate::engines::actionlint::action_metadata::{
    ActionMetadata, LocalActionsCache, popular_action,
};
use crate::engines::actionlint::ast::{
    Bool, Concurrency, Container, Defaults, DispatchInputType, Env, Event, Exec, Float, Int, Job,
    Matrix, MatrixRow, Pos, RawYamlValue, Snapshot, Step, Str, WebhookEventFilter, Workflow,
    WorkflowCall, WorkflowCallEventOutput, WorkflowCallInputType, is_expr_assigned,
};
use crate::engines::actionlint::availability::workflow_key_availability;
use crate::engines::actionlint::config::Config;
use crate::engines::actionlint::expr::ExprError;
use crate::engines::actionlint::expr::parser::{ExprNode, Parser};
use crate::engines::actionlint::expr::sema::SemanticsChecker;
use crate::engines::actionlint::expr::types::{ExprType, ObjectType};
use crate::engines::actionlint::reusable_workflow::LocalReusableWorkflowCache;
use crate::engines::actionlint::yaml::{go_parse_float, go_quote};

struct TypedExpr {
    ty: ExprType,
    pos: Pos,
}

pub struct RuleExpression<'a> {
    base: RuleBase,
    matrix_ty: Option<ObjectType>,
    steps_ty: Option<ObjectType>,
    needs_ty: Option<ObjectType>,
    secrets_ty: Option<ObjectType>,
    inputs_ty: Option<ObjectType>,
    dispatch_inputs_ty: Option<ObjectType>,
    jobs_ty: Option<ObjectType>,
    workflow: Option<Workflow>,
    local_actions: &'a LocalActionsCache,
    local_workflows: &'a LocalReusableWorkflowCache,
    config_vars: Option<Vec<String>>,
}

impl<'a> RuleExpression<'a> {
    pub fn new(
        local_actions: &'a LocalActionsCache,
        local_workflows: &'a LocalReusableWorkflowCache,
        config: Option<&Config>,
    ) -> Self {
        Self {
            base: RuleBase::new("expression"),
            matrix_ty: None,
            steps_ty: None,
            needs_ty: None,
            secrets_ty: None,
            inputs_ty: None,
            dispatch_inputs_ty: None,
            jobs_ty: None,
            workflow: None,
            local_actions,
            local_workflows,
            config_vars: config.and_then(|c| c.variables.clone()),
        }
    }

    fn get_action_outputs_type(&mut self, spec: Option<&Str>) -> ObjectType {
        let Some(spec) = spec else {
            return ObjectType::map(ExprType::String);
        };
        if spec.value.starts_with("./") {
            return match self.local_actions.find_metadata(&spec.value) {
                Err(e) => {
                    self.base.error(spec.pos, e);
                    ObjectType::map(ExprType::String)
                }
                Ok((None, _)) => ObjectType::map(ExprType::String),
                Ok((Some(meta), _)) => type_of_action_outputs(&meta),
            };
        }
        // github-script sets outputs through `core.setOutput`: anything goes.
        if spec.value.starts_with("actions/github-script@") {
            return ObjectType::empty();
        }
        if let Some(meta) = popular_action(&spec.value) {
            return type_of_action_outputs(&meta);
        }
        ObjectType::map(ExprType::String)
    }

    fn get_workflow_call_outputs_type(&mut self, call: &WorkflowCall) -> ObjectType {
        let Some(uses) = &call.uses else {
            return ObjectType::map(ExprType::String);
        };
        match self.local_workflows.find_metadata(&uses.value) {
            Err(e) => {
                self.base.error(uses.pos, e);
                ObjectType::map(ExprType::String)
            }
            Ok(None) => ObjectType::map(ExprType::String),
            Ok(Some(m)) => {
                let props = m
                    .outputs
                    .keys()
                    .map(|n| (n.clone(), ExprType::String))
                    .collect();
                ObjectType::strict(props)
            }
        }
    }

    fn check_one_expression(
        &mut self,
        s: Option<&Str>,
        what: &str,
        workflow_key: &str,
    ) -> Option<ExprType> {
        let s = s?;
        let ts = self.check_exprs_in(&s.value, s.pos, s.quoted, false, workflow_key)?;
        if ts.len() != 1 {
            self.base.error(
                s.pos,
                format!(
                    "one ${{{{ }}}} expression should be included in {} value but got {} expressions",
                    go_quote(what),
                    ts.len()
                ),
            );
            return None;
        }
        Some(ts.into_iter().next().expect("one expression").ty)
    }

    fn check_object_ty(&mut self, ty: Option<ExprType>, pos: Pos, what: &str) -> Option<ExprType> {
        let ty = ty?;
        match ty {
            ExprType::Object(_) | ExprType::Any => Some(ty),
            other => {
                self.base.error(
                    pos,
                    format!(
                        "type of expression at {} must be object but found type {other}",
                        go_quote(what)
                    ),
                );
                None
            }
        }
    }

    fn check_array_ty(&mut self, ty: Option<ExprType>, pos: Pos, what: &str) -> Option<ExprType> {
        let ty = ty?;
        match ty {
            ExprType::Array(_) | ExprType::Any => Some(ty),
            other => {
                self.base.error(
                    pos,
                    format!(
                        "type of expression at {} must be array but found type {other}",
                        go_quote(what)
                    ),
                );
                None
            }
        }
    }

    fn check_number_ty(&mut self, ty: Option<ExprType>, pos: Pos, what: &str) -> Option<ExprType> {
        let ty = ty?;
        match ty {
            ExprType::Number | ExprType::Any => Some(ty),
            other => {
                self.base.error(
                    pos,
                    format!(
                        "type of expression at {} must be number but found type {other}",
                        go_quote(what)
                    ),
                );
                None
            }
        }
    }

    fn check_object_expression(
        &mut self,
        s: Option<&Str>,
        what: &str,
        workflow_key: &str,
    ) -> Option<ExprType> {
        let ty = self.check_one_expression(s, what, workflow_key)?;
        let pos = s.map_or_else(Pos::default, |s| s.pos);
        self.check_object_ty(Some(ty), pos, what)
    }

    fn check_array_expression(
        &mut self,
        s: Option<&Str>,
        what: &str,
        workflow_key: &str,
    ) -> Option<ExprType> {
        let ty = self.check_one_expression(s, what, workflow_key)?;
        let pos = s.map_or_else(Pos::default, |s| s.pos);
        self.check_array_ty(Some(ty), pos, what)
    }

    fn check_number_expression(
        &mut self,
        s: Option<&Str>,
        what: &str,
        workflow_key: &str,
    ) -> Option<ExprType> {
        let ty = self.check_one_expression(s, what, workflow_key)?;
        let pos = s.map_or_else(Pos::default, |s| s.pos);
        self.check_number_ty(Some(ty), pos, what)
    }

    fn check_env(&mut self, env: Option<&Env>, workflow_key: &str) {
        let Some(env) = env else {
            return;
        };
        if env.expression.is_none() {
            for e in env.vars.values() {
                self.check_string(Some(&e.name), workflow_key);
                self.check_string(Some(&e.value), workflow_key);
            }
            return;
        }
        self.check_object_expression(env.expression.as_ref(), "env", workflow_key);
    }

    fn check_container(&mut self, c: Option<&Container>, workflow_key: &str, child_prefix: &str) {
        let Some(c) = c else {
            return;
        };
        let child_key = if child_prefix.is_empty() {
            workflow_key.to_string()
        } else {
            format!("{workflow_key}.{child_prefix}")
        };
        self.check_string(c.image.as_ref(), workflow_key);
        if let Some(cred) = &c.credentials {
            let k = format!("{child_key}.credentials");
            if cred.expression.is_some() {
                self.check_object_expression(cred.expression.as_ref(), "credentials", &k);
            } else {
                self.check_string(cred.username.as_ref(), &k);
                self.check_string(cred.password.as_ref(), &k);
            }
        }
        self.check_env(c.env.as_ref(), &format!("{child_key}.env.<env_id>"));
        self.check_strings(&c.ports, workflow_key);
        self.check_strings(&c.volumes, workflow_key);
        self.check_string(c.options.as_ref(), workflow_key);
    }

    fn check_concurrency(&mut self, c: Option<&Concurrency>, workflow_key: &str) {
        let Some(c) = c else {
            return;
        };
        self.check_string(c.group.as_ref(), workflow_key);
        self.check_bool(c.cancel_in_progress.as_ref(), workflow_key);
    }

    fn check_defaults(&mut self, d: Option<&Defaults>, workflow_key: &str) {
        let Some(run) = d.and_then(|d| d.run.as_ref()) else {
            return;
        };
        self.check_string(run.shell.as_ref(), workflow_key);
        self.check_string(run.working_directory.as_ref(), workflow_key);
    }

    fn check_workflow_call(&mut self, c: Option<&WorkflowCall>) {
        let Some(c) = c else {
            return;
        };
        let Some(uses) = &c.uses else {
            return;
        };
        self.check_string(Some(uses), "");
        let m = match self.local_workflows.find_metadata(&uses.value) {
            Ok(m) => m,
            Err(e) => {
                self.base.error(uses.pos, e);
                None
            }
        };
        for (n, i) in &c.inputs {
            let ts = self.check_string(Some(&i.value), "jobs.<job_id>.with.<with_id>");
            let Some(m) = &m else {
                continue;
            };
            let Some(mi) = m.inputs.get(n) else {
                continue;
            };
            if matches!(mi.ty, ExprType::Any) {
                continue;
            }
            let v = i.value.value.trim();
            let mut ty = ExprType::String;
            match ts.as_ref().map(Vec::len) {
                Some(0) => {
                    ty = match v {
                        "null" => ExprType::Null,
                        "true" | "false" => ExprType::Bool,
                        _ => {
                            if go_parse_float(v).is_some() {
                                ExprType::Number
                            } else {
                                ExprType::String
                            }
                        }
                    };
                }
                Some(1) => {
                    if i.value.is_expression_assigned()
                        && let Some(ts) = &ts
                    {
                        ty = ts[0].ty.clone();
                    }
                }
                _ => {}
            }
            if !mi.ty.assignable(&ty) {
                self.base.error(
                    i.value.pos,
                    format!(
                        "input {} is typed as {} by reusable workflow {}. {ty} value cannot be assigned",
                        go_quote(&mi.name),
                        mi.ty,
                        go_quote(&uses.value)
                    ),
                );
            }
        }
        for s in c.secrets.values() {
            self.check_string(Some(&s.value), "jobs.<job_id>.secrets.<secrets_id>");
        }
    }

    fn check_snapshot(&mut self, s: Option<&Snapshot>) {
        if let Some(s) = s {
            self.check_if_condition(s.if_cond.as_ref(), "jobs.<job_id>.snapshot.if");
        }
    }

    fn check_webhook_event_filter(&mut self, f: Option<&WebhookEventFilter>) {
        if let Some(f) = f {
            self.check_strings(&f.values, "");
        }
    }

    fn check_strings(&mut self, ss: &[Str], workflow_key: &str) {
        for s in ss {
            self.check_string(Some(s), workflow_key);
        }
    }

    fn check_if_condition(&mut self, s: Option<&Str>, workflow_key: &str) {
        let Some(s) = s else {
            return;
        };
        let mut cond_ty: Option<ExprType> = None;
        if s.contains_expression() {
            if let Some(ts) = self.check_string(Some(s), workflow_key)
                && ts.len() == 1
                && s.is_expression_assigned()
            {
                cond_ty = Some(ts[0].ty.clone());
            }
        } else {
            let src = format!("{}}}}}", s.value);
            let (line, col) = (s.pos.line, s.pos.col);
            let (parsed, _) = Parser::parse(&src);
            match parsed {
                Err(err) => {
                    self.expr_error(&err, line, col);
                    return;
                }
                Ok(expr) => {
                    if let Some(ty) =
                        self.check_semantics_of_expr_node(&expr, line, col, false, workflow_key)
                    {
                        cond_ty = Some(ty);
                    }
                }
            }
        }
        if let Some(ty) = cond_ty
            && !ExprType::Bool.assignable(&ty)
        {
            self.base.error(
                s.pos,
                format!(
                    "\"if\" condition should be type \"bool\" but got type {}",
                    go_quote(&ty.to_string())
                ),
            );
        }
    }

    fn check_template_evaluated_type(&mut self, ts: &[TypedExpr]) {
        for t in ts {
            if matches!(
                t.ty,
                ExprType::Object(_) | ExprType::Array(_) | ExprType::Null
            ) {
                self.base.error(
                    t.pos,
                    format!(
                        "object, array, and null values should not be evaluated in template with ${{{{ }}}} but evaluating the value of type {}",
                        t.ty
                    ),
                );
            }
        }
    }

    fn check_string(&mut self, s: Option<&Str>, workflow_key: &str) -> Option<Vec<TypedExpr>> {
        let s = s?;
        let ts = self.check_exprs_in(&s.value, s.pos, s.quoted, false, workflow_key)?;
        self.check_template_evaluated_type(&ts);
        Some(ts)
    }

    fn check_script_string(&mut self, s: Option<&Str>, workflow_key: &str) {
        let Some(s) = s else {
            return;
        };
        if let Some(ts) = self.check_exprs_in(&s.value, s.pos, s.quoted, true, workflow_key) {
            self.check_template_evaluated_type(&ts);
        }
    }

    fn check_bool(&mut self, b: Option<&Bool>, workflow_key: &str) {
        let Some(b) = b else {
            return;
        };
        let Some(expr) = &b.expression else {
            return;
        };
        let Some(ty) = self.check_one_expression(Some(expr), "bool value", workflow_key) else {
            return;
        };
        if !matches!(ty, ExprType::Bool | ExprType::Any) {
            self.base.error(
                expr.pos,
                format!("type of expression must be bool but found type {ty}"),
            );
        }
    }

    fn check_int(&mut self, i: Option<&Int>, workflow_key: &str) {
        if let Some(i) = i {
            self.check_number_expression(i.expression.as_ref(), "integer value", workflow_key);
        }
    }

    fn check_float(&mut self, f: Option<&Float>, workflow_key: &str) {
        if let Some(f) = f {
            self.check_number_expression(f.expression.as_ref(), "float number value", workflow_key);
        }
    }

    /// Checks every `${{ }}` in the string. `None` when one failed to parse
    /// or check; `Some(empty)` when there is none.
    fn check_exprs_in(
        &mut self,
        s: &str,
        pos: Pos,
        quoted: bool,
        check_untrusted: bool,
        workflow_key: &str,
    ) -> Option<Vec<TypedExpr>> {
        let (line, mut col) = (pos.line, pos.col);
        if quoted {
            col += 1;
        }
        let mut offset = 0usize;
        let mut rest = s;
        let mut ts = Vec::new();
        while let Some(idx) = rest.find("${{") {
            let start = idx + 3;
            rest = &rest[start..];
            offset += start;
            let col_here = col + u32::try_from(offset).unwrap_or(u32::MAX);
            let (ty, offset_after, ok) =
                self.check_semantics(rest, line, col_here, check_untrusted, workflow_key);
            if !ok {
                return None;
            }
            let Some(ty) = ty else {
                return Some(Vec::new());
            };
            if offset_after == 0 {
                return Some(Vec::new());
            }
            ts.push(TypedExpr {
                ty,
                pos: Pos {
                    line,
                    col: col_here - 3,
                },
            });
            rest = &rest[offset_after..];
            offset += offset_after;
        }
        Some(ts)
    }

    fn expr_error(&mut self, err: &ExprError, line_base: u32, col_base: u32) {
        let pos = convert_expr_line_col_to_pos(err.line, err.column, line_base, col_base);
        self.base.error(pos, err.message.clone());
    }

    fn check_semantics_of_expr_node(
        &mut self,
        expr: &ExprNode,
        line: u32,
        col: u32,
        check_untrusted: bool,
        workflow_key: &str,
    ) -> Option<ExprType> {
        let mut c = SemanticsChecker::new(check_untrusted, self.config_vars.clone());
        if let Some(m) = &self.matrix_ty {
            c.update_matrix(m.clone());
        }
        if let Some(s) = &self.steps_ty {
            c.update_steps(s.clone());
        }
        if let Some(n) = &self.needs_ty {
            c.update_needs(n.clone());
        }
        if let Some(s) = &self.secrets_ty {
            c.update_secrets(s);
        }
        if let Some(i) = &self.inputs_ty {
            c.update_inputs(i.clone());
        }
        if let Some(d) = &self.dispatch_inputs_ty {
            c.update_dispatch_inputs(d.clone());
        }
        if let Some(j) = &self.jobs_ty {
            c.update_jobs(j.clone());
        }
        if !workflow_key.is_empty() {
            let (ctx, sp) = workflow_key_availability(workflow_key).unwrap_or((&[], &[]));
            c.set_context_availability(ctx.iter().map(|s| (*s).to_string()).collect());
            c.set_special_function_availability(sp.iter().map(|s| (*s).to_string()).collect());
        }
        let (ty, errs) = c.check_expr(expr);
        let ok = errs.is_empty();
        for err in &errs {
            self.expr_error(err, line, col);
        }
        ok.then_some(ty)
    }

    /// Parses and checks one expression at the start of `src`; the offset is
    /// where the lexer stopped (right after `}}`).
    fn check_semantics(
        &mut self,
        src: &str,
        line: u32,
        col: u32,
        check_untrusted: bool,
        workflow_key: &str,
    ) -> (Option<ExprType>, usize, bool) {
        let (parsed, offset) = Parser::parse(src);
        match parsed {
            Err(err) => {
                self.expr_error(&err, line, col);
                (None, offset, false)
            }
            Ok(expr) => {
                let ty = self.check_semantics_of_expr_node(
                    &expr,
                    line,
                    col,
                    check_untrusted,
                    workflow_key,
                );
                let ok = ty.is_some();
                (ty, offset, ok)
            }
        }
    }

    fn calc_needs_type(&mut self, job: &Job) -> ObjectType {
        let mut o = ObjectType::empty_strict();
        let Some(workflow) = self.workflow.clone() else {
            return o;
        };
        for id in &job.needs {
            let i = id.value.to_lowercase();
            if i == job.id.value {
                continue;
            }
            if o.props.contains_key(&i) {
                continue;
            }
            let Some(j) = workflow.jobs.as_ref().and_then(|jobs| jobs.get(&i)) else {
                continue;
            };
            let outputs = match &j.workflow_call {
                None => {
                    let props = j
                        .outputs
                        .keys()
                        .map(|n| (n.clone(), ExprType::String))
                        .collect();
                    ObjectType::strict(props)
                }
                Some(call) => self.get_workflow_call_outputs_type(call),
            };
            o.props.insert(
                i,
                ExprType::Object(ObjectType::strict(
                    [
                        ("outputs".to_string(), ExprType::Object(outputs)),
                        ("result".to_string(), ExprType::String),
                    ]
                    .into_iter()
                    .collect(),
                )),
            );
        }
        o
    }

    fn check_matrix_expression(&mut self, expr: Option<&Str>) -> ObjectType {
        let Some(ExprType::Object(mut mat_ty)) =
            self.check_object_expression(expr, "matrix", "jobs.<job_id>.strategy")
        else {
            return ObjectType::empty();
        };
        if let Some(inc_ty) = mat_ty.props.remove("include")
            && let ExprType::Array(a) = inc_ty
            && let ExprType::Object(o) = a.elem.as_ref()
        {
            for (n, p) in &o.props {
                let merged = match mat_ty.props.get(n) {
                    None => p.clone(),
                    Some(t) => t.merge(p),
                };
                mat_ty.props.insert(n.clone(), merged);
            }
        }
        mat_ty.props.remove("exclude");
        mat_ty
    }

    fn check_matrix(&mut self, m: &Matrix) -> ObjectType {
        if m.expression.is_some() {
            return self.check_matrix_expression(m.expression.as_ref());
        }
        if let Some(exclude) = &m.exclude {
            if exclude.expression.is_some() {
                if let Some(ExprType::Array(ty)) = self.check_array_expression(
                    exclude.expression.as_ref(),
                    "exclude",
                    "jobs.<job_id>.strategy",
                ) {
                    let pos = exclude
                        .expression
                        .as_ref()
                        .map_or_else(Pos::default, |e| e.pos);
                    self.check_object_ty(Some(*ty.elem), pos, "exclude");
                }
            } else {
                for combi in &exclude.combinations {
                    if combi.expression.is_some() {
                        self.check_object_expression(
                            combi.expression.as_ref(),
                            "exclude",
                            "jobs.<job_id>.strategy",
                        );
                        continue;
                    }
                    for a in combi.assigns.values() {
                        self.check_raw_yaml_value(&a.value);
                    }
                }
            }
        }

        let mut o = ObjectType::empty_strict();
        for (n, r) in &m.rows {
            let ty = self.check_matrix_row(r);
            o.props.insert(n.clone(), ty);
        }

        let Some(include) = &m.include else {
            return o;
        };
        if include.expression.is_some() {
            if let Some(ExprType::Array(a)) = self.check_one_expression(
                include.expression.as_ref(),
                "include",
                "jobs.<job_id>.strategy",
            ) && let ExprType::Object(ret) = ExprType::Object(o.clone()).merge(&a.elem)
            {
                return ret;
            }
            return ObjectType::empty();
        }
        for combi in &include.combinations {
            if combi.expression.is_some() {
                // actionlint checks `m.Include.Expression` here (nil for a
                // per-element expression), so the element is skipped.
                let ty = self.check_one_expression(
                    include.expression.as_ref(),
                    "matrix combination at element of include section",
                    "jobs.<job_id>.strategy",
                );
                let Some(ty) = ty else {
                    continue;
                };
                match ExprType::Object(o.clone()).merge(&ty) {
                    ExprType::Object(merged) => o = merged,
                    _ => o.make_loose(),
                }
                continue;
            }
            for (n, assign) in &combi.assigns {
                let mut ty = self.check_raw_yaml_value(&assign.value);
                if let Some(t) = o.props.get(n) {
                    ty = t.merge(&ty);
                }
                o.props.insert(n.clone(), ty);
            }
        }
        o
    }

    fn check_matrix_row(&mut self, r: &MatrixRow) -> ExprType {
        if r.expression.is_some() {
            if let Some(ExprType::Array(a)) = self.check_array_expression(
                r.expression.as_ref(),
                "matrix row",
                "jobs.<job_id>.strategy",
            ) {
                return *a.elem;
            }
            return ExprType::Any;
        }
        let mut ty: Option<ExprType> = None;
        for v in r.values.iter().flatten() {
            let t = self.check_raw_yaml_value(v);
            ty = Some(match ty {
                None => t,
                Some(prev) => prev.merge(&t),
            });
        }
        ty.unwrap_or(ExprType::Any)
    }

    fn check_workflow_call_outputs(
        &mut self,
        outputs: &BTreeMap<String, WorkflowCallEventOutput>,
        jobs: &BTreeMap<String, Job>,
    ) {
        if outputs.is_empty() || jobs.is_empty() {
            return;
        }
        let mut props = BTreeMap::new();
        for (n, j) in jobs {
            let o = if j.workflow_call.is_some() {
                ObjectType::empty()
            } else {
                ObjectType::strict(
                    j.outputs
                        .keys()
                        .map(|n| (n.clone(), ExprType::String))
                        .collect(),
                )
            };
            props.insert(
                n.clone(),
                ExprType::Object(ObjectType::strict(
                    std::iter::once(("outputs".to_string(), ExprType::Object(o))).collect(),
                )),
            );
        }
        self.jobs_ty = Some(ObjectType::strict(props));
        for o in outputs.values() {
            self.check_string(
                o.value.as_ref(),
                "on.workflow_call.outputs.<output_id>.value",
            );
        }
    }

    fn check_raw_yaml_value(&mut self, v: &RawYamlValue) -> ExprType {
        match v {
            RawYamlValue::Object { props, .. } => {
                let m = props
                    .iter()
                    .map(|(k, p)| (k.clone(), self.check_raw_yaml_value(p)))
                    .collect();
                ExprType::Object(ObjectType::strict(m))
            }
            RawYamlValue::Array { elems, .. } => {
                if elems.is_empty() {
                    return ExprType::array(ExprType::Any, false);
                }
                let mut elem = self.check_raw_yaml_value(&elems[0]);
                for v in &elems[1..] {
                    let t = self.check_raw_yaml_value(v);
                    elem = elem.merge(&t);
                }
                ExprType::array(elem, false)
            }
            RawYamlValue::String { value, pos } => self.check_raw_yaml_string(value, *pos),
        }
    }

    fn check_raw_yaml_string(&mut self, value: &str, pos: Pos) -> ExprType {
        let ts = self.check_exprs_in(value, pos, false, false, "jobs.<job_id>.strategy");
        if is_expr_assigned(value) {
            return match ts {
                Some(ts) if ts.len() == 1 => ts.into_iter().next().expect("one").ty,
                _ => ExprType::Any,
            };
        }
        let s = value.trim();
        if s == "true" || s == "false" {
            return ExprType::Bool;
        }
        if s == "null" {
            return ExprType::Null;
        }
        if go_parse_float(s).is_some() {
            return ExprType::Number;
        }
        ExprType::String
    }
}

const fn convert_expr_line_col_to_pos(line: u32, col: u32, line_base: u32, col_base: u32) -> Pos {
    Pos {
        line: line + line_base - 1,
        col: col + col_base - 1,
    }
}

fn type_of_action_outputs(meta: &ActionMetadata) -> ObjectType {
    if meta.skip_outputs {
        return ObjectType::empty();
    }
    let props = meta
        .outputs
        .keys()
        .map(|n| (n.to_lowercase(), ExprType::String))
        .collect();
    ObjectType::strict(props)
}

impl Rule for RuleExpression<'_> {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_workflow_pre(&mut self, n: &Workflow) {
        self.check_string(n.name.as_ref(), "");
        for e in n.on.iter().flatten() {
            match e {
                Event::Webhook(e) => {
                    self.check_strings(&e.types, "");
                    self.check_webhook_event_filter(e.branches.as_ref());
                    self.check_webhook_event_filter(e.branches_ignore.as_ref());
                    self.check_webhook_event_filter(e.tags.as_ref());
                    self.check_webhook_event_filter(e.tags_ignore.as_ref());
                    self.check_webhook_event_filter(e.paths.as_ref());
                    self.check_webhook_event_filter(e.paths_ignore.as_ref());
                    self.check_strings(&e.workflows, "");
                }
                Event::Scheduled(e) => {
                    for s in &e.schedules {
                        self.check_string(Some(&s.cron), "");
                        self.check_string(s.timezone.as_ref(), "");
                    }
                }
                Event::WorkflowDispatch(e) => {
                    let mut ity = ObjectType::empty_strict();
                    for (id, i) in &e.inputs {
                        self.check_string(i.description.as_ref(), "");
                        self.check_string(i.default.as_ref(), "");
                        self.check_bool(i.required.as_ref(), "");
                        self.check_strings(&i.options, "");
                        let ty = match i.ty {
                            DispatchInputType::Boolean => ExprType::Bool,
                            DispatchInputType::Number => ExprType::Number,
                            DispatchInputType::String
                            | DispatchInputType::Choice
                            | DispatchInputType::Environment => ExprType::String,
                            DispatchInputType::None => ExprType::Any,
                        };
                        ity.props.insert(id.clone(), ty);
                    }
                    self.dispatch_inputs_ty = Some(ity);
                }
                Event::RepositoryDispatch(e) => self.check_strings(&e.types, ""),
                Event::WorkflowCall(e) => {
                    // `inputs` is set before the inputs are checked: a
                    // default may refer to an earlier input.
                    self.inputs_ty = Some(ObjectType::empty_strict());
                    for i in &e.inputs {
                        self.check_string(i.description.as_ref(), "");
                        let ts = self.check_string(
                            i.default.as_ref(),
                            "on.workflow_call.inputs.<inputs_id>.default",
                        );
                        let ty = match i.ty {
                            WorkflowCallInputType::String => ExprType::String,
                            WorkflowCallInputType::Boolean => {
                                if let Some(ts) = &ts
                                    && ts.len() == 1
                                    && i.default.as_ref().is_some_and(Str::is_expression_assigned)
                                    && !matches!(ts[0].ty, ExprType::Bool | ExprType::Any)
                                {
                                    let pos =
                                        i.default.as_ref().map_or_else(Pos::default, |d| d.pos);
                                    self.base.error(
                                        pos,
                                        format!(
                                            "type of input {} must be bool but found type {}",
                                            go_quote(&i.name.value),
                                            ts[0].ty
                                        ),
                                    );
                                }
                                ExprType::Bool
                            }
                            WorkflowCallInputType::Number => {
                                if let Some(ts) = &ts
                                    && ts.len() == 1
                                    && i.default.as_ref().is_some_and(Str::is_expression_assigned)
                                    && !matches!(ts[0].ty, ExprType::Number | ExprType::Any)
                                {
                                    let pos =
                                        i.default.as_ref().map_or_else(Pos::default, |d| d.pos);
                                    self.base.error(
                                        pos,
                                        format!(
                                            "type of input {} must be number but found type {}",
                                            go_quote(&i.name.value),
                                            ts[0].ty
                                        ),
                                    );
                                }
                                ExprType::Number
                            }
                            WorkflowCallInputType::Invalid => ExprType::Any,
                        };
                        if let Some(ity) = &mut self.inputs_ty {
                            ity.props.insert(i.id.clone(), ty);
                        }
                    }
                    if let Some(secrets) = &e.secrets {
                        let mut sty = ObjectType::empty_strict();
                        for (id, s) in secrets {
                            sty.props.insert(id.clone(), ExprType::String);
                            self.check_string(s.description.as_ref(), "");
                        }
                        self.secrets_ty = Some(sty);
                    }
                    for o in e.outputs.values() {
                        self.check_string(o.description.as_ref(), "");
                    }
                }
                Event::ImageVersion(e) => {
                    self.check_strings(&e.names, "");
                    self.check_strings(&e.versions, "");
                }
            }
        }
        self.check_string(n.run_name.as_ref(), "run-name");
        self.check_env(n.env.as_ref(), "env");
        self.check_defaults(n.defaults.as_ref(), "");
        self.check_concurrency(n.concurrency.as_ref(), "concurrency");
        self.workflow = Some(n.clone());
    }

    fn visit_workflow_post(&mut self, n: &Workflow) {
        if let Some(e) = n.find_workflow_call_event()
            && let Some(jobs) = &n.jobs
        {
            let outputs = e.outputs.clone();
            self.check_workflow_call_outputs(&outputs, jobs);
        }
        self.workflow = None;
    }

    fn visit_job_pre(&mut self, n: &Job) {
        self.needs_ty = Some(self.calc_needs_type(n));
        if let Some(m) = n.strategy.as_ref().and_then(|s| s.matrix.as_ref()) {
            self.matrix_ty = Some(self.check_matrix(m));
        }
        self.check_string(n.name.as_ref(), "jobs.<job_id>.name");
        self.check_strings(&n.needs, "");
        if let Some(runs_on) = &n.runs_on {
            if let Some(expr) = &runs_on.labels_expr {
                if let Some(ty) = self.check_one_expression(
                    Some(expr),
                    "runner label at \"runs-on\" section",
                    "jobs.<job_id>.runs-on",
                ) && !matches!(ty, ExprType::Array(_) | ExprType::String | ExprType::Any)
                {
                    self.base.error(
                        expr.pos,
                        format!(
                            "type of expression at \"runs-on\" must be string or array but found type {}",
                            go_quote(&ty.to_string())
                        ),
                    );
                }
            } else {
                for l in &runs_on.labels {
                    self.check_string(Some(l), "jobs.<job_id>.runs-on");
                }
            }
            self.check_string(runs_on.group.as_ref(), "jobs.<job_id>.runs-on");
        }
        self.check_concurrency(n.concurrency.as_ref(), "jobs.<job_id>.concurrency");
        self.check_env(n.env.as_ref(), "jobs.<job_id>.env");
        self.check_defaults(n.defaults.as_ref(), "jobs.<job_id>.defaults.run");
        self.check_if_condition(n.if_cond.as_ref(), "jobs.<job_id>.if");
        if let Some(s) = &n.strategy {
            self.check_bool(s.fail_fast.as_ref(), "jobs.<job_id>.strategy");
            self.check_int(s.max_parallel.as_ref(), "jobs.<job_id>.strategy");
        }
        self.check_bool(
            n.continue_on_error.as_ref(),
            "jobs.<job_id>.continue-on-error",
        );
        self.check_float(n.timeout_minutes.as_ref(), "jobs.<job_id>.timeout-minutes");
        self.check_container(n.container.as_ref(), "jobs.<job_id>.container", "");
        if let Some(services) = &n.services {
            self.check_object_expression(
                services.expression.as_ref(),
                "services",
                "jobs.<job_id>.services",
            );
            for s in services.value.values() {
                self.check_container(Some(&s.container), "jobs.<job_id>.services", "<service_id>");
            }
        }
        self.check_workflow_call(n.workflow_call.as_ref());
        self.check_snapshot(n.snapshot.as_ref());
        self.steps_ty = Some(ObjectType::empty_strict());
    }

    fn visit_job_post(&mut self, n: &Job) {
        if let Some(env) = &n.environment {
            self.check_string(env.name.as_ref(), "jobs.<job_id>.environment");
            self.check_string(env.url.as_ref(), "jobs.<job_id>.environment.url");
            self.check_bool(env.deployment.as_ref(), "jobs.<job_id>.environment");
        }
        for output in n.outputs.values() {
            self.check_string(Some(&output.value), "jobs.<job_id>.outputs.<output_id>");
        }
        self.matrix_ty = None;
        self.steps_ty = None;
        self.needs_ty = None;
    }

    fn visit_step(&mut self, n: &Step) {
        self.check_string(n.name.as_ref(), "jobs.<job_id>.steps.name");
        self.check_if_condition(n.if_cond.as_ref(), "jobs.<job_id>.steps.if");
        let mut spec: Option<Str> = None;
        match &n.exec {
            Some(Exec::Run(e)) => {
                self.check_script_string(e.run.as_ref(), "jobs.<job_id>.steps.run");
                self.check_string(e.shell.as_ref(), "");
                self.check_string(
                    e.working_directory.as_ref(),
                    "jobs.<job_id>.steps.working-directory",
                );
            }
            Some(Exec::Action(e)) => {
                self.check_string(e.uses.as_ref(), "");
                for (name, i) in &e.inputs {
                    let is_script = e
                        .uses
                        .as_ref()
                        .is_some_and(|u| u.value.starts_with("actions/github-script@"))
                        && name == "script";
                    if is_script {
                        self.check_script_string(Some(&i.value), "jobs.<job_id>.steps.with");
                    } else {
                        self.check_string(Some(&i.value), "jobs.<job_id>.steps.with");
                    }
                }
                self.check_string(e.entrypoint.as_ref(), "jobs.<job_id>.steps.with");
                self.check_string(e.args.as_ref(), "jobs.<job_id>.steps.with");
                spec.clone_from(&e.uses);
            }
            None => {}
        }
        self.check_env(n.env.as_ref(), "jobs.<job_id>.steps.env");
        self.check_bool(
            n.continue_on_error.as_ref(),
            "jobs.<job_id>.steps.continue-on-error",
        );
        self.check_float(
            n.timeout_minutes.as_ref(),
            "jobs.<job_id>.steps.timeout-minutes",
        );

        if let Some(id) = &n.id {
            if id.contains_expression() {
                self.check_string(Some(id), "");
                if let Some(steps) = &mut self.steps_ty {
                    steps.make_loose();
                }
            }
            let outputs = self.get_action_outputs_type(spec.as_ref());
            let key = id.value.to_lowercase();
            if let Some(steps) = &mut self.steps_ty {
                steps.props.insert(
                    key,
                    ExprType::Object(ObjectType::strict(
                        [
                            ("outputs".to_string(), ExprType::Object(outputs)),
                            ("conclusion".to_string(), ExprType::String),
                            ("outcome".to_string(), ExprType::String),
                        ]
                        .into_iter()
                        .collect(),
                    )),
                );
            }
        }
    }
}
