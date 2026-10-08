//! Expectations here were taken from actionlint v1.7.12 run on the same
//! inputs with `-shellcheck= -pyflakes= -oneline`.

use std::path::PathBuf;

use super::{Config, Linter};

fn lint(src: &str) -> Vec<String> {
    let linter = Linter::new(None, PathBuf::from("."), None);
    linter
        .lint("test.yaml", src.as_bytes())
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn lint_with_config(src: &str, config: &str) -> Vec<String> {
    let linter = Linter::new(
        None,
        PathBuf::from("."),
        Some(Config::parse(config).unwrap()),
    );
    linter
        .lint("test.yaml", src.as_bytes())
        .iter()
        .map(ToString::to_string)
        .collect()
}

const OK: &str =
    "on: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n";

#[test]
fn a_minimal_workflow_passes() {
    assert_eq!(lint(OK), Vec::<String>::new());
}

#[test]
fn syntax_check_reports_unexpected_keys_and_missing_sections() {
    assert_eq!(
        lint("NAME: x\non: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n"),
        vec![
            "1:1: unexpected key \"NAME\" for \"workflow\" section. expected one of \"concurrency\", \"defaults\", \"env\", \"jobs\", \"name\", \"on\", \"permissions\", \"run-name\" [syntax-check]",
            "4:3: \"steps\" section is missing in job \"test\" [syntax-check]",
        ]
    );
    assert_eq!(lint(""), vec!["1:1: workflow is empty [syntax-check]"]);
    assert_eq!(
        lint("jobs: {}\n"),
        vec![
            "1:1: \"on\" section is missing in workflow [syntax-check]",
            "1:7: \"jobs\" section should not be empty. please remove this section if it's unnecessary [syntax-check]",
        ]
    );
}

#[test]
fn yaml_errors_are_reported_as_actionlint_does() {
    // go-yaml's position for a parser error: the context mark's line, and a
    // 0-based column.
    assert_eq!(
        lint(
            "on: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo\n     x: 1\n"
        ),
        vec!["3:4: could not parse as YAML: did not find expected key [syntax-check]"]
    );
    assert_eq!(
        lint("on: push\njobs:\n  test:\n  - a\n"),
        vec![
            "3:3: \"runs-on\" section is missing in job \"test\" [syntax-check]",
            "3:3: \"steps\" section is missing in job \"test\" [syntax-check]",
            "4:3: \"test\" job is sequence node but mapping node is expected [syntax-check]",
        ]
    );
}

#[test]
fn anchors_must_be_used_and_not_recursive() {
    let src = "on: push\nenv: &e\n  A: 1\njobs:\n  test:\n    runs-on: ubuntu-latest\n    env: &unused\n      B: 2\n    steps:\n      - run: echo\n        env: *e\n";
    assert_eq!(
        lint(src),
        vec!["7:10: anchor \"unused\" is defined but not used [syntax-check]"]
    );
    let src = "on: push\njobs:\n  test: &j\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo\n        env: *j\n";
    assert_eq!(
        lint(src),
        vec![
            "7:14: \"env\" section is alias node but mapping node is expected [syntax-check]",
            "7:14: recursive alias \"j\" is found. anchor was declared at line:3, column:9 [syntax-check]",
        ]
    );
}

#[test]
fn expression_type_errors() {
    let src = "on: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo ${{ github.foo }}\n      - run: echo ${{ matrix.os }}\n        if: ${{ 1 }}\n";
    assert_eq!(
        lint(src),
        vec![
            "6:23: property \"foo\" is not defined in object type {action: string; action_path: string; action_ref: string; action_repository: string; action_status: string; actor: string; actor_id: string; api_url: string; artifact_cache_size_limit: number; base_ref: string; env: string; event: object; event_name: string; event_path: string; graphql_url: string; head_ref: string; job: string; output: string; path: string; ref: string; ref_name: string; ref_protected: bool; ref_type: string; repository: string; repository_id: string; repository_owner: string; repository_owner_id: string; repository_visibility: string; repositoryurl: string; retention_days: number; run_attempt: string; run_id: string; run_number: string; secret_source: string; server_url: string; sha: string; state: string; step_summary: string; token: string; triggering_actor: string; workflow: string; workflow_ref: string; workflow_sha: string; workspace: string} [expression]",
            "7:23: property \"os\" is not defined in object type {} [expression]",
            "8:13: constant expression \"1\" in condition. remove the if: section [if-cond]",
        ]
    );
}

#[test]
fn events_runner_labels_and_config() {
    let src = "on:\n  pussh:\n  schedule:\n    - cron: '* * * * *'\njobs:\n  test:\n    runs-on: my-runner\n    steps:\n      - run: echo\n";
    assert_eq!(
        lint(src),
        vec![
            "2:3: unknown Webhook event \"pussh\". see https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#webhook-events for list of all Webhook event names [events]",
            "4:13: scheduled job runs too frequently. it runs once per 60 seconds. the shortest interval is once every 5 minutes [events]",
            "7:14: label \"my-runner\" is unknown. available labels are \"windows-latest\", \"windows-latest-8-cores\", \"windows-2025\", \"windows-2025-vs2026\", \"windows-2022\", \"windows-11-arm\", \"ubuntu-slim\", \"ubuntu-latest\", \"ubuntu-latest-4-cores\", \"ubuntu-latest-8-cores\", \"ubuntu-latest-16-cores\", \"ubuntu-24.04\", \"ubuntu-24.04-arm\", \"ubuntu-22.04\", \"ubuntu-22.04-arm\", \"macos-latest\", \"macos-latest-xlarge\", \"macos-latest-large\", \"macos-26-intel\", \"macos-26-xlarge\", \"macos-26-large\", \"macos-26\", \"macos-15-intel\", \"macos-15-xlarge\", \"macos-15-large\", \"macos-15\", \"macos-14-xlarge\", \"macos-14-large\", \"macos-14\", \"self-hosted\", \"x64\", \"arm\", \"arm64\", \"linux\", \"macos\", \"windows\". if it is a custom label for self-hosted runner, set list of labels in actionlint.yaml config file [runner-label]",
        ]
    );
    let with_label = lint_with_config(src, "self-hosted-runner:\n  labels: [my-runner]\n");
    assert_eq!(with_label.len(), 2, "{with_label:?}");
}

#[test]
fn popular_actions_inputs_are_checked() {
    let src = "on: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n        with:\n          fetch-deph: 0\n";
    assert_eq!(
        lint(src),
        vec![
            "8:11: input \"fetch-deph\" is not defined in action \"actions/checkout@v4\". available inputs are \"clean\", \"fetch-depth\", \"fetch-tags\", \"filter\", \"github-server-url\", \"lfs\", \"path\", \"persist-credentials\", \"ref\", \"repository\", \"set-safe-directory\", \"show-progress\", \"sparse-checkout\", \"sparse-checkout-cone-mode\", \"ssh-key\", \"ssh-known-hosts\", \"ssh-strict\", \"ssh-user\", \"submodules\", \"token\" [action]",
        ]
    );
}
