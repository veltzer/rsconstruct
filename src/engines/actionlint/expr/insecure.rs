//! Detection of potentially untrusted inputs used in scripts, a port of
//! actionlint's `expr_insecure.go`. It follows property accesses on the
//! `github` context through the tree of inputs an attacker can control and
//! reports a leaf reached directly (not through `contains()` and friends).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use super::ExprError;
use super::parser::{ExprNode, error_at_token};
use crate::engines::actionlint::parse::sorted_quotes;

/// A node in the tree of untrusted inputs; a leaf has no children.
#[derive(Debug)]
pub struct InputMap {
    /// The dotted path from the root, e.g. `github.event.issue.title`.
    pub path: String,
    pub children: Option<BTreeMap<String, Self>>,
}

fn leaf(path: &str) -> InputMap {
    InputMap {
        path: path.to_string(),
        children: None,
    }
}

fn node(path: &str, children: Vec<InputMap>) -> InputMap {
    let map = children
        .into_iter()
        .map(|c| (c.path.rsplit('.').next().unwrap_or("").to_string(), c))
        .collect();
    InputMap {
        path: path.to_string(),
        children: Some(map),
    }
}

fn author(prefix: &str) -> InputMap {
    node(
        &format!("{prefix}.author"),
        vec![
            leaf(&format!("{prefix}.author.email")),
            leaf(&format!("{prefix}.author.name")),
        ],
    )
}

/// The inputs GitHub's documentation names as attacker-controlled.
pub static BUILTIN_UNTRUSTED_INPUTS: LazyLock<BTreeMap<String, InputMap>> = LazyLock::new(|| {
    let github = node(
        "github",
        vec![
            node(
                "github.event",
                vec![
                    node(
                        "github.event.issue",
                        vec![
                            leaf("github.event.issue.title"),
                            leaf("github.event.issue.body"),
                        ],
                    ),
                    node(
                        "github.event.pull_request",
                        vec![
                            leaf("github.event.pull_request.title"),
                            leaf("github.event.pull_request.body"),
                            node(
                                "github.event.pull_request.head",
                                vec![
                                    leaf("github.event.pull_request.head.ref"),
                                    leaf("github.event.pull_request.head.label"),
                                    node(
                                        "github.event.pull_request.head.repo",
                                        vec![leaf(
                                            "github.event.pull_request.head.repo.default_branch",
                                        )],
                                    ),
                                ],
                            ),
                        ],
                    ),
                    node(
                        "github.event.comment",
                        vec![leaf("github.event.comment.body")],
                    ),
                    node(
                        "github.event.review",
                        vec![leaf("github.event.review.body")],
                    ),
                    node(
                        "github.event.review_comment",
                        vec![leaf("github.event.review_comment.body")],
                    ),
                    node(
                        "github.event.pages",
                        vec![node(
                            "github.event.pages.*",
                            vec![leaf("github.event.pages.*.page_name")],
                        )],
                    ),
                    node(
                        "github.event.commits",
                        vec![node(
                            "github.event.commits.*",
                            vec![
                                leaf("github.event.commits.*.message"),
                                author("github.event.commits.*"),
                            ],
                        )],
                    ),
                    node(
                        "github.event.head_commit",
                        vec![
                            leaf("github.event.head_commit.message"),
                            author("github.event.head_commit"),
                        ],
                    ),
                    node(
                        "github.event.discussion",
                        vec![
                            leaf("github.event.discussion.title"),
                            leaf("github.event.discussion.body"),
                        ],
                    ),
                ],
            ),
            leaf("github.head_ref"),
        ],
    );
    let mut roots = BTreeMap::new();
    roots.insert("github".to_string(), github);
    roots
});

fn is_safe_func_call(callee: &str) -> bool {
    matches!(
        callee.to_lowercase().as_str(),
        "contains" | "startswith" | "endswith"
    )
}

pub struct UntrustedInputChecker {
    roots: &'static BTreeMap<String, InputMap>,
    filtering_object: bool,
    cur: Vec<&'static InputMap>,
    /// The variable node that started the current access chain.
    start: Option<ExprNode>,
    errs: Vec<ExprError>,
    safe_calls: u32,
}

impl UntrustedInputChecker {
    pub fn new() -> Self {
        Self {
            roots: &BUILTIN_UNTRUSTED_INPUTS,
            filtering_object: false,
            cur: Vec::new(),
            start: None,
            errs: Vec::new(),
            safe_calls: 0,
        }
    }

    fn reset(&mut self) {
        self.start = None;
        self.filtering_object = false;
        self.cur.clear();
    }

    fn on_var(&mut self, n: &ExprNode) {
        let ExprNode::Variable { name, .. } = n else {
            return;
        };
        if let Some(root) = self.roots.get(name) {
            self.start = Some(n.clone());
            self.cur.push(root);
        }
    }

    fn on_prop_access(&mut self, name: &str) {
        let mut next = Vec::with_capacity(self.cur.len());
        for cur in &self.cur {
            if let Some(c) = cur.children.as_ref().and_then(|ch| ch.get(name)) {
                next.push(c);
            }
        }
        self.cur = next;
    }

    fn on_index_access(&mut self) {
        if self.filtering_object {
            // `github.event.*.body[0]` reads as `github.event.commits[0].body`.
            self.filtering_object = false;
            return;
        }
        let mut next = Vec::with_capacity(self.cur.len());
        for cur in &self.cur {
            if let Some(c) = cur.children.as_ref().and_then(|ch| ch.get("*")) {
                next.push(c);
            }
        }
        self.cur = next;
    }

    fn on_object_filter(&mut self) {
        self.filtering_object = true;
        let mut next = Vec::new();
        for cur in &self.cur {
            if let Some(c) = cur.children.as_ref().and_then(|ch| ch.get("*")) {
                next.push(c);
                continue;
            }
            if let Some(children) = &cur.children {
                next.extend(children.values());
            }
        }
        self.cur = next;
    }

    fn end(&mut self) {
        let mut inputs: Vec<String> = Vec::new();
        for cur in &self.cur {
            if cur.children.is_some() {
                continue;
            }
            inputs.push(cur.path.clone());
        }
        if let Some(start) = &self.start {
            if inputs.len() == 1 {
                self.errs.push(error_at_token(
                    start.token(),
                    format!(
                        "{} is potentially untrusted. avoid using it directly in inline scripts. instead, pass it through an environment variable. see https://docs.github.com/en/actions/reference/security/secure-use#good-practices-for-mitigating-script-injection-attacks for more details",
                        crate::engines::actionlint::yaml::go_quote(&inputs[0])
                    ),
                ));
            } else if inputs.len() > 1 {
                self.errs.push(error_at_token(
                    start.token(),
                    format!(
                        "object filter extracts potentially untrusted properties {}. avoid using the value directly in inline scripts. instead, pass the value through an environment variable. see https://docs.github.com/en/actions/reference/security/secure-use#good-practices-for-mitigating-script-injection-attacks for more details",
                        sorted_quotes(&inputs)
                    ),
                ));
            }
        }
        self.reset();
    }

    pub fn on_visit_node_enter(&mut self, n: &ExprNode) {
        if let ExprNode::FuncCall { callee, .. } = n
            && is_safe_func_call(callee)
        {
            self.safe_calls += 1;
        }
    }

    pub fn on_visit_node_leave(&mut self, n: &ExprNode) {
        if self.safe_calls > 0 {
            if let ExprNode::FuncCall { callee, .. } = n
                && is_safe_func_call(callee)
            {
                self.safe_calls -= 1;
            }
            return;
        }
        match n {
            ExprNode::Variable { .. } => {
                self.end();
                self.on_var(n);
            }
            ExprNode::ObjectDeref { property, .. } => self.on_prop_access(property),
            ExprNode::IndexAccess { index, .. } => {
                if let ExprNode::Str(lit, _) = index.as_ref() {
                    self.on_prop_access(lit);
                } else {
                    self.on_index_access();
                }
            }
            ExprNode::ArrayDeref { .. } => self.on_object_filter(),
            _ => self.end(),
        }
    }

    pub fn on_visit_end(&mut self) {
        self.end();
    }

    pub fn errs(&self) -> &[ExprError] {
        &self.errs
    }

    pub fn init(&mut self) {
        self.errs.clear();
        self.safe_calls = 0;
        self.reset();
    }
}
