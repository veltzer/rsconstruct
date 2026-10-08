//! The `job-needs` rule: undefined job IDs in `needs:` and dependency
//! cycles.

use std::collections::BTreeMap;

use super::{Rule, RuleBase};
use crate::actionlint::ast::{Job, Pos, Workflow};
use crate::actionlint::yaml::go_quote;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    New,
    Active,
    Finished,
}

#[derive(Debug)]
struct JobNode {
    needs: Vec<String>,
    status: Status,
    pos: Pos,
}

pub struct RuleJobNeeds {
    base: RuleBase,
    nodes: BTreeMap<String, JobNode>,
}

impl RuleJobNeeds {
    pub const fn new() -> Self {
        Self {
            base: RuleBase::new("job-needs"),
            nodes: BTreeMap::new(),
        }
    }
}

impl Rule for RuleJobNeeds {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_job_pre(&mut self, job: &Job) {
        let mut needs: Vec<String> = Vec::new();
        for j in &job.needs {
            let id = j.value.to_lowercase();
            if needs.contains(&id) {
                self.base.error(
                    j.pos,
                    format!(
                        "job ID {} duplicates in \"needs\" section. note that job ID is case insensitive",
                        go_quote(&j.value)
                    ),
                );
                continue;
            }
            if !id.is_empty() {
                needs.push(id);
            }
        }
        let id = job.id.value.to_lowercase();
        if id.is_empty() {
            return;
        }
        if let Some(prev) = self.nodes.get(&id) {
            self.base.error(
                job.pos,
                format!(
                    "job ID {} duplicates. previously defined at {}. note that job ID is case insensitive",
                    go_quote(&job.id.value),
                    prev.pos
                ),
            );
        }
        self.nodes.insert(
            id,
            JobNode {
                needs,
                status: Status::New,
                pos: job.id.pos,
            },
        );
    }

    fn visit_workflow_post(&mut self, _workflow: &Workflow) {
        let mut valid = true;
        let ids: Vec<String> = self.nodes.keys().cloned().collect();
        for id in &ids {
            let (needs, pos) = {
                let node = &self.nodes[id];
                (node.needs.clone(), node.pos)
            };
            for dep in &needs {
                if !self.nodes.contains_key(dep) {
                    self.base.error(
                        pos,
                        format!(
                            "job {} needs job {} which does not exist in this workflow",
                            go_quote(id),
                            go_quote(dep)
                        ),
                    );
                    valid = false;
                }
            }
        }
        if !valid {
            return;
        }
        if let Some((from, to)) = self.detect_first_cycle() {
            let mut edges: BTreeMap<String, String> = BTreeMap::new();
            edges.insert(from.clone(), to.clone());
            self.collect_cycle(&to, &mut edges);
            let mut start = from;
            for n in edges.keys() {
                if self.nodes[n].pos.is_before(self.nodes[&start].pos) {
                    start.clone_from(n);
                }
            }
            let mut msg = String::from(
                "cyclic dependencies in \"needs\" job configurations are detected. detected cycle is ",
            );
            msg.push_str(&go_quote(&start));
            let mut to = edges[&start].clone();
            loop {
                msg.push_str(" -> ");
                msg.push_str(&go_quote(&to));
                let from = to;
                to = edges.get(&from).cloned().unwrap_or_default();
                if from == start || to.is_empty() {
                    break;
                }
            }
            let pos = self.nodes[&start].pos;
            self.base.error(pos, msg);
        }
    }
}

impl RuleJobNeeds {
    fn collect_cycle(&self, src: &str, edges: &mut BTreeMap<String, String>) -> bool {
        let deps = self.nodes[src].needs.clone();
        for dest in deps {
            if self
                .nodes
                .get(&dest)
                .is_none_or(|n| n.status != Status::Active)
            {
                continue;
            }
            edges.insert(src.to_string(), dest.clone());
            if edges.contains_key(&dest) {
                return true;
            }
            if self.collect_cycle(&dest, edges) {
                return true;
            }
            edges.remove(src);
        }
        false
    }

    fn detect_first_cycle(&mut self) -> Option<(String, String)> {
        let ids: Vec<String> = self.nodes.keys().cloned().collect();
        for id in ids {
            if self.nodes[&id].status == Status::New
                && let Some(e) = self.detect_cyclic_node(&id)
            {
                return Some(e);
            }
        }
        None
    }

    fn detect_cyclic_node(&mut self, v: &str) -> Option<(String, String)> {
        self.nodes.get_mut(v)?.status = Status::Active;
        let deps = self.nodes[v].needs.clone();
        for w in deps {
            match self.nodes.get(&w).map(|n| n.status) {
                Some(Status::Active) => return Some((v.to_string(), w)),
                Some(Status::New) => {
                    if let Some(e) = self.detect_cyclic_node(&w) {
                        return Some(e);
                    }
                }
                _ => {}
            }
        }
        if let Some(n) = self.nodes.get_mut(v) {
            n.status = Status::Finished;
        }
        None
    }
}
