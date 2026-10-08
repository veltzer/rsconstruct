//! The `events` rule: the `on:` section. Webhook names and activity types,
//! filters, schedules, `workflow_dispatch` and `workflow_call` inputs.

use std::path::Path;

use super::{Rule, RuleBase};
use crate::actionlint::ast::{
    DispatchInputType, Event, ImageVersionEvent, Pos, Str, WebhookEvent, WebhookEventFilter,
    Workflow, WorkflowCallEvent, WorkflowCallInputType, WorkflowDispatchEvent, filter_is_empty,
};
use crate::actionlint::cron;
use crate::actionlint::parse::{quotes, sorted_quotes};
use crate::actionlint::yaml::{go_parse_float, go_quote};

/// Webhook events and their activity types; `None` for an event that has no
/// `types` filter at all.
pub const ALL_WEBHOOK_TYPES: &[(&str, Option<&[&str]>)] = &[
    (
        "branch_protection_rule",
        Some(&["created", "edited", "deleted"]),
    ),
    (
        "check_run",
        Some(&["created", "rerequested", "completed", "requested_action"]),
    ),
    ("check_suite", Some(&["completed"])),
    ("create", Some(&[])),
    ("delete", Some(&[])),
    ("deployment", Some(&[])),
    ("deployment_status", Some(&[])),
    (
        "discussion",
        Some(&[
            "created",
            "edited",
            "deleted",
            "transferred",
            "pinned",
            "unpinned",
            "labeled",
            "unlabeled",
            "locked",
            "unlocked",
            "category_changed",
            "answered",
            "unanswered",
        ]),
    ),
    (
        "discussion_comment",
        Some(&["created", "edited", "deleted"]),
    ),
    ("fork", Some(&[])),
    ("gollum", Some(&[])),
    ("image_version", Some(&[])),
    ("issue_comment", Some(&["created", "edited", "deleted"])),
    (
        "issues",
        Some(&[
            "opened",
            "edited",
            "deleted",
            "transferred",
            "pinned",
            "unpinned",
            "closed",
            "reopened",
            "assigned",
            "unassigned",
            "labeled",
            "unlabeled",
            "locked",
            "unlocked",
            "milestoned",
            "demilestoned",
            "typed",
            "untyped",
        ]),
    ),
    ("label", Some(&["created", "edited", "deleted"])),
    ("merge_group", Some(&["checks_requested"])),
    (
        "milestone",
        Some(&["created", "closed", "opened", "edited", "deleted"]),
    ),
    ("page_build", Some(&[])),
    ("public", Some(&[])),
    (
        "pull_request",
        Some(&[
            "assigned",
            "unassigned",
            "labeled",
            "unlabeled",
            "opened",
            "edited",
            "closed",
            "reopened",
            "synchronize",
            "converted_to_draft",
            "locked",
            "unlocked",
            "enqueued",
            "dequeued",
            "milestoned",
            "demilestoned",
            "ready_for_review",
            "review_requested",
            "review_request_removed",
            "auto_merge_enabled",
            "auto_merge_disabled",
        ]),
    ),
    (
        "pull_request_review",
        Some(&["submitted", "edited", "dismissed"]),
    ),
    (
        "pull_request_review_comment",
        Some(&["created", "edited", "deleted"]),
    ),
    (
        "pull_request_target",
        Some(&[
            "assigned",
            "unassigned",
            "labeled",
            "unlabeled",
            "opened",
            "edited",
            "closed",
            "reopened",
            "synchronize",
            "converted_to_draft",
            "locked",
            "unlocked",
            "enqueued",
            "dequeued",
            "milestoned",
            "demilestoned",
            "ready_for_review",
            "review_requested",
            "review_request_removed",
            "auto_merge_enabled",
            "auto_merge_disabled",
        ]),
    ),
    ("push", Some(&[])),
    ("registry_package", Some(&["published", "updated"])),
    (
        "release",
        Some(&[
            "published",
            "unpublished",
            "created",
            "edited",
            "deleted",
            "prereleased",
            "released",
        ]),
    ),
    ("repository_dispatch", None),
    ("schedule", Some(&[])),
    ("status", Some(&[])),
    ("watch", Some(&["started"])),
    ("workflow_call", Some(&[])),
    ("workflow_dispatch", Some(&[])),
    (
        "workflow_run",
        Some(&["completed", "requested", "in_progress"]),
    ),
];

/// Whether Go's `time.LoadLocation` would know the name: the system zoneinfo
/// database has a file for it.
pub fn timezone_exists(name: &str) -> bool {
    if name.is_empty() || name == "UTC" {
        return true;
    }
    if name.starts_with('/') || name.starts_with('\\') || name.contains("..") {
        return false;
    }
    Path::new("/usr/share/zoneinfo").join(name).is_file()
}

pub struct RuleEvents {
    base: RuleBase,
}

impl RuleEvents {
    pub const fn new() -> Self {
        Self {
            base: RuleBase::new("events"),
        }
    }

    fn check_event(&mut self, event: &Event) {
        match event {
            Event::Scheduled(e) => {
                for s in &e.schedules {
                    self.check_cron(&s.cron);
                    if let Some(tz) = &s.timezone {
                        self.check_timezone(tz);
                    }
                }
            }
            Event::WorkflowDispatch(e) => self.check_workflow_dispatch_event(e),
            Event::RepositoryDispatch(_) => {}
            Event::WorkflowCall(e) => self.check_workflow_call_event(e),
            Event::Webhook(e) => self.check_webhook_event(e),
            Event::ImageVersion(e) => Self::check_image_version_event(e),
        }
    }

    fn check_cron(&mut self, spec: &Str) {
        let sched = match cron::parse(&spec.value) {
            Ok(s) => s,
            Err(e) => {
                self.base.error(
                    spec.pos,
                    format!(
                        "invalid CRON format {} in schedule event: {e}",
                        go_quote(&spec.value)
                    ),
                );
                return;
            }
        };
        let Some(start) = sched.next(0) else {
            return;
        };
        let Some(next) = sched.next(start) else {
            return;
        };
        let diff = next - start;
        if diff < 60 * 5 {
            self.base.error(
                spec.pos,
                format!(
                    "scheduled job runs too frequently. it runs once per {diff} seconds. the shortest interval is once every 5 minutes"
                ),
            );
        }
    }

    fn check_timezone(&mut self, tz: &Str) {
        let v = tz.value.as_str();
        let ok = !v.is_empty()
            && !v.eq_ignore_ascii_case("utc")
            && !v.eq_ignore_ascii_case("local")
            && timezone_exists(v);
        if !ok {
            self.base.error(
                tz.pos,
                format!(
                    "invalid timezone {} in schedule event. it must be a valid IANA timezone name",
                    go_quote(v)
                ),
            );
        }
    }

    fn filter_not_available(&mut self, pos: Pos, filter: &str, hook: &str, available: &[&str]) {
        let e = if available.len() < 2 {
            "event"
        } else {
            "events"
        };
        self.base.error(
            pos,
            format!(
                "{} filter is not available for {hook} event. it is only for {} {e}",
                go_quote(filter),
                available.join(", ")
            ),
        );
    }

    fn check_exclusive_filters(
        &mut self,
        filter: Option<&WebhookEventFilter>,
        ignore: Option<&WebhookEventFilter>,
        hook: &str,
        available: &[&str],
    ) {
        if available.contains(&hook) {
            if !filter_is_empty(filter) && !filter_is_empty(ignore) {
                let (f, i) = (filter.expect("non-empty"), ignore.expect("non-empty"));
                let mut p = f.name.pos;
                if p.is_before(i.name.pos) {
                    p = i.name.pos;
                }
                self.base.error(
                    p,
                    format!(
                        "both {} and {} filters cannot be used for the same event {}. note: use '!' to negate patterns",
                        go_quote(&f.name.value),
                        go_quote(&i.name.value),
                        go_quote(hook)
                    ),
                );
            }
        } else {
            if !filter_is_empty(filter) {
                let f = filter.expect("non-empty");
                self.filter_not_available(f.name.pos, &f.name.value, hook, available);
            }
            if !filter_is_empty(ignore) {
                let i = ignore.expect("non-empty");
                self.filter_not_available(i.name.pos, &i.name.value, hook, available);
            }
        }
    }

    fn check_webhook_event(&mut self, event: &WebhookEvent) {
        let hook = event.hook.value.as_str();
        let Some((_, types)) = ALL_WEBHOOK_TYPES.iter().find(|(name, _)| *name == hook) else {
            self.base.error(
                event.pos,
                format!(
                    "unknown Webhook event {}. see https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#webhook-events for list of all Webhook event names",
                    go_quote(hook)
                ),
            );
            return;
        };
        self.check_types(&event.hook, &event.types, types.unwrap_or(&[]));
        if hook == "workflow_run" {
            if event.workflows.is_empty() {
                self.base.error(
                    event.pos,
                    "no workflow is configured for \"workflow_run\" event".to_string(),
                );
            }
        } else if !event.workflows.is_empty() {
            self.base.error(
                event.pos,
                format!(
                    "\"workflows\" cannot be configured for {} event. it is only for workflow_run event",
                    go_quote(hook)
                ),
            );
        }
        self.check_exclusive_filters(
            event.paths.as_ref(),
            event.paths_ignore.as_ref(),
            hook,
            &["push", "pull_request", "pull_request_target"],
        );
        self.check_exclusive_filters(
            event.branches.as_ref(),
            event.branches_ignore.as_ref(),
            hook,
            &[
                "merge_group",
                "push",
                "pull_request",
                "pull_request_target",
                "workflow_run",
            ],
        );
        self.check_exclusive_filters(
            event.tags.as_ref(),
            event.tags_ignore.as_ref(),
            hook,
            &["push"],
        );
    }

    fn check_types(&mut self, hook: &Str, types: &[Str], expected: &[&str]) {
        if expected.is_empty() && !types.is_empty() {
            self.base.error(
                hook.pos,
                format!(
                    "\"types\" cannot be specified for {} Webhook event",
                    go_quote(&hook.value)
                ),
            );
            return;
        }
        for ty in types {
            if !expected.contains(&ty.value.as_str()) {
                self.base.error(
                    ty.pos,
                    format!(
                        "invalid activity type {} for {} Webhook event. available types are {}",
                        go_quote(&ty.value),
                        go_quote(&hook.value),
                        sorted_quotes(expected)
                    ),
                );
            }
        }
    }

    fn check_workflow_call_event(&mut self, event: &WorkflowCallEvent) {
        for i in &event.inputs {
            let Some(default) = &i.default else {
                continue;
            };
            if !default.contains_expression() {
                match i.ty {
                    WorkflowCallInputType::Number => {
                        if go_parse_float(&default.value).is_none() {
                            self.base.error(
                                default.pos,
                                format!(
                                    "input of workflow_call event {} is typed as number but its default value {} cannot be parsed as a float number: strconv.ParseFloat: parsing {}: invalid syntax",
                                    go_quote(&i.name.value),
                                    go_quote(&default.value),
                                    go_quote(&default.value)
                                ),
                            );
                        }
                    }
                    WorkflowCallInputType::Boolean => {
                        let d = default.value.to_lowercase();
                        if d != "true" && d != "false" {
                            self.base.error(
                                default.pos,
                                format!(
                                    "input of workflow_call event {} is typed as boolean. its default value must be true or false but got {}",
                                    go_quote(&i.name.value),
                                    go_quote(&default.value)
                                ),
                            );
                        }
                    }
                    _ => {}
                }
            }
            if i.is_required() {
                self.base.error(
                    default.pos,
                    format!(
                        "input {} of workflow_call event has the default value {}, but it is also required. if an input is marked as required, its default value will never be used",
                        go_quote(&i.name.value),
                        go_quote(&default.value)
                    ),
                );
            }
        }
    }

    fn check_workflow_dispatch_event(&mut self, event: &WorkflowDispatchEvent) {
        for (n, i) in &event.inputs {
            if i.ty == DispatchInputType::Choice {
                if i.options.is_empty() {
                    self.base.error(
                        i.name.pos,
                        format!(
                            "input type of {} is \"choice\" but \"options\" is not set",
                            go_quote(n)
                        ),
                    );
                    continue;
                }
                let mut seen: Vec<&str> = Vec::new();
                for o in &i.options {
                    if seen.contains(&o.value.as_str()) {
                        self.base.error(
                            o.pos,
                            format!(
                                "option {} is duplicated in options of {} input",
                                go_quote(&o.value),
                                go_quote(n)
                            ),
                        );
                        continue;
                    }
                    seen.push(&o.value);
                }
                if let Some(d) = &i.default
                    && !seen.contains(&d.value.as_str())
                {
                    let options: Vec<&str> = i.options.iter().map(|o| o.value.as_str()).collect();
                    self.base.error(
                        d.pos,
                        format!(
                            "default value {} of {} input is not included in its options {}",
                            go_quote(&d.value),
                            go_quote(n),
                            go_quote(&quotes(&options))
                        ),
                    );
                }
            } else {
                if !i.options.is_empty() {
                    self.base.error(
                        i.name.pos,
                        format!(
                            "\"options\" can not be set to {} input because its input type is not \"choice\"",
                            go_quote(n)
                        ),
                    );
                }
                if let Some(d) = &i.default {
                    match i.ty {
                        DispatchInputType::Number => {
                            if go_parse_float(&d.value).is_none() {
                                self.base.error(
                                    d.pos,
                                    format!(
                                        "type of {} input is \"number\" but its default value {} cannot be parsed as a float number: strconv.ParseFloat: parsing {}: invalid syntax",
                                        go_quote(&i.name.value),
                                        go_quote(&d.value),
                                        go_quote(&d.value)
                                    ),
                                );
                            }
                        }
                        DispatchInputType::Boolean => {
                            let v = d.value.to_lowercase();
                            if v != "true" && v != "false" {
                                self.base.error(
                                    d.pos,
                                    format!(
                                        "type of {} input is \"boolean\". its default value {} must be \"true\" or \"false\"",
                                        go_quote(n),
                                        go_quote(&d.value)
                                    ),
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if event.inputs.len() > 25 {
            self.base.error(
                event.pos,
                format!(
                    "maximum number of inputs for \"workflow_dispatch\" event is 25 but {} inputs are provided. see https://docs.github.com/en/actions/using-workflows/events-that-trigger-workflows#providing-inputs",
                    event.inputs.len()
                ),
            );
        }
    }

    const fn check_image_version_event(_event: &ImageVersionEvent) {}
}

impl Rule for RuleEvents {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_workflow_pre(&mut self, workflow: &Workflow) {
        for e in workflow.on.iter().flatten() {
            self.check_event(e);
        }
    }
}
