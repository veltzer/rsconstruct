//! The `workflow-call` rule: `uses:` of reusable workflows at a job, and the
//! inputs and secrets passed to local ones.

use super::{Rule, RuleBase};
use crate::actionlint::ast::{Event, Job, Pos, Workflow, WorkflowCall};
use crate::actionlint::parse::sorted_quotes;
use crate::actionlint::reusable_workflow::LocalReusableWorkflowCache;
use crate::actionlint::yaml::go_quote;

pub struct RuleWorkflowCall<'a> {
    base: RuleBase,
    workflow_call_event_pos: Option<Pos>,
    workflow_path: String,
    cache: &'a LocalReusableWorkflowCache,
}

impl<'a> RuleWorkflowCall<'a> {
    pub fn new(workflow_path: &str, cache: &'a LocalReusableWorkflowCache) -> Self {
        Self {
            base: RuleBase::new("workflow-call"),
            workflow_call_event_pos: None,
            workflow_path: workflow_path.to_string(),
            cache,
        }
    }

    fn check_workflow_call_uses_local(&mut self, call: &WorkflowCall) {
        let Some(u) = &call.uses else {
            return;
        };
        let m = match self.cache.find_metadata(&u.value) {
            Ok(m) => m,
            Err(e) => {
                self.base.error(u.pos, e);
                return;
            }
        };
        let Some(m) = m else {
            return;
        };
        for (n, i) in &m.inputs {
            if i.required && !call.inputs.contains_key(n) {
                self.base.error(
                    u.pos,
                    format!(
                        "input {} is required by {} reusable workflow",
                        go_quote(&i.name),
                        go_quote(&u.value)
                    ),
                );
            }
        }
        for (n, i) in &call.inputs {
            if !m.inputs.contains_key(n) {
                let note = if m.inputs.is_empty() {
                    "no input is defined".to_string()
                } else {
                    let names: Vec<&str> = m.inputs.values().map(|i| i.name.as_str()).collect();
                    if names.len() == 1 {
                        format!("defined input is {}", go_quote(names[0]))
                    } else {
                        format!("defined inputs are {}", sorted_quotes(&names))
                    }
                };
                self.base.error(
                    i.name.pos,
                    format!(
                        "input {} is not defined in {} reusable workflow. {note}",
                        go_quote(&i.name.value),
                        go_quote(&u.value)
                    ),
                );
            }
        }
        if !call.inherit_secrets {
            for (n, s) in &m.secrets {
                if s.required && !call.secrets.contains_key(n) {
                    self.base.error(
                        u.pos,
                        format!(
                            "secret {} is required by {} reusable workflow",
                            go_quote(&s.name),
                            go_quote(&u.value)
                        ),
                    );
                }
            }
            for (n, s) in &call.secrets {
                if !m.secrets.contains_key(n) {
                    let note = if m.secrets.is_empty() {
                        "no secret is defined".to_string()
                    } else {
                        let names: Vec<&str> =
                            m.secrets.values().map(|s| s.name.as_str()).collect();
                        if names.len() == 1 {
                            format!("defined secret is {}", go_quote(names[0]))
                        } else {
                            format!("defined secrets are {}", sorted_quotes(&names))
                        }
                    };
                    self.base.error(
                        s.name.pos,
                        format!(
                            "secret {} is not defined in {} reusable workflow. {note}",
                            go_quote(&s.name.value),
                            go_quote(&u.value)
                        ),
                    );
                }
            }
        }
    }
}

pub fn is_workflow_call_uses_local_format(u: &str) -> bool {
    let Some(rest) = u.strip_prefix("./") else {
        return false;
    };
    if rest.find('@').is_some_and(|i| i > 0) {
        return false;
    }
    !rest.is_empty()
}

pub fn is_workflow_call_uses_repo_format(u: &str) -> bool {
    if u.starts_with('.') {
        return false;
    }
    let Some(i) = u.find('/').filter(|i| *i > 0) else {
        return false;
    };
    let u = &u[i + 1..];
    let Some(i) = u.find('/').filter(|i| *i > 0) else {
        return false;
    };
    let u = &u[i + 1..];
    let Some(i) = u.find('@').filter(|i| *i > 0) else {
        return false;
    };
    !u[i + 1..].is_empty()
}

impl Rule for RuleWorkflowCall<'_> {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_workflow_pre(&mut self, workflow: &Workflow) {
        for e in workflow.on.iter().flatten() {
            if let Event::WorkflowCall(e) = e {
                self.workflow_call_event_pos = Some(e.pos);
                self.cache.write_workflow_call_event(&self.workflow_path, e);
                break;
            }
        }
    }

    fn visit_job_pre(&mut self, job: &Job) {
        let Some(call) = &job.workflow_call else {
            return;
        };
        let Some(u) = &call.uses else {
            return;
        };
        if u.value.is_empty() || u.contains_expression() {
            return;
        }
        if is_workflow_call_uses_local_format(&u.value) {
            self.check_workflow_call_uses_local(call);
            return;
        }
        if is_workflow_call_uses_repo_format(&u.value) {
            return;
        }
        if u.value.starts_with("./") {
            self.cache.write_null(&u.value);
        }
        self.base.error(
            u.pos,
            format!(
                "reusable workflow call {} at \"uses\" is not following the format \"owner/repo/path/to/workflow.yml@ref\" nor \"./path/to/workflow.yml\". see https://docs.github.com/en/actions/learn-github-actions/reusing-workflows for more details",
                go_quote(&u.value)
            ),
        );
    }
}
