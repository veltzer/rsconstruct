//! The `deprecated-commands` rule: `::set-output::` and friends in `run:`.

use std::sync::LazyLock;

use regex::Regex;

use super::{Rule, RuleBase};
use crate::actionlint::ast::{Exec, Step};
use crate::actionlint::yaml::go_quote;

static DEPRECATED_COMMANDS_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:::(save-state|set-output|set-env)\s+name=[a-zA-Z][a-zA-Z_-]*::\S+|::(add-path)::\S+)",
    )
    .expect("valid regex")
});

pub struct RuleDeprecatedCommands {
    base: RuleBase,
}

impl RuleDeprecatedCommands {
    pub const fn new() -> Self {
        Self {
            base: RuleBase::new("deprecated-commands"),
        }
    }
}

impl Rule for RuleDeprecatedCommands {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_step(&mut self, step: &Step) {
        let Some(Exec::Run(r)) = &step.exec else {
            return;
        };
        let Some(run) = &r.run else {
            return;
        };
        for m in DEPRECATED_COMMANDS_PATTERN.captures_iter(&run.value) {
            let c = m.get(1).or_else(|| m.get(2)).map_or("", |g| g.as_str());
            let a = match c {
                "set-output" => "echo \"{name}={value}\" >> $GITHUB_OUTPUT",
                "save-state" => "echo \"{name}={value}\" >> $GITHUB_STATE",
                "set-env" => "echo \"{name}={value}\" >> $GITHUB_ENV",
                "add-path" => "echo \"{path}\" >> $GITHUB_PATH",
                _ => continue,
            };
            self.base.error(
                run.pos,
                format!(
                    "workflow command {} was deprecated. use `{a}` instead: https://docs.github.com/en/actions/using-workflows/workflow-commands-for-github-actions",
                    go_quote(c)
                ),
            );
        }
    }
}
