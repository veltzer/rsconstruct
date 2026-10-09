//! Unit tests for the hadolint port. Expected findings were taken from
//! hadolint 2.15.1 run on the same input.

use super::config::{Config, Severity};
use super::{Failure, lint};

fn run(input: &str) -> Vec<String> {
    let report = lint(input, &Config::default()).expect("parses");
    report
        .failures
        .iter()
        .map(|f: &Failure| format!("{} {} {}: {}", f.line, f.code, f.severity.text(), f.message))
        .collect()
}

#[test]
fn clean_dockerfile_reports_nothing() {
    let doc = "FROM ubuntu:24.04\nRUN apt-get update && apt-get install -y --no-install-recommends curl=8.5.0-2 \\\n    && rm -rf /var/lib/apt/lists/*\nCOPY . /app\nWORKDIR /app\nCMD [\"./run\"]\n";
    assert_eq!(run(doc), Vec::<String>::new());
}

#[test]
fn apt_get_install_findings() {
    let doc = "FROM ubuntu\nRUN apt-get install foo\n";
    assert_eq!(
        run(doc),
        vec![
            "1 DL3006 warning: Always tag the version of an image explicitly",
            "2 DL3008 warning: Pin versions in apt get install. Instead of `apt-get install <package>` use `apt-get install <package>=<version>`",
            "2 DL3014 warning: Use the `-y` switch to avoid manual input `apt-get -y install <package>`",
            "2 DL3015 info: Avoid additional packages by specifying `--no-install-recommends`",
        ]
    );
}

#[test]
fn pragmas_and_overrides() {
    let doc = "# hadolint global ignore=DL3006\nFROM ubuntu\n# hadolint ignore=DL3008,DL3015\nRUN apt-get install -y foo\nRUN apt-get install -y bar\n";
    assert_eq!(
        run(doc),
        vec![
            "5 DL3008 warning: Pin versions in apt get install. Instead of `apt-get install <package>` use `apt-get install <package>=<version>`",
            "5 DL3015 info: Avoid additional packages by specifying `--no-install-recommends`",
            "5 DL3059 info: Multiple consecutive `RUN` instructions. Consider consolidation.",
        ]
    );
    let mut cfg = Config::default();
    cfg.apply_yaml(
        "ignored:\n  - DL3059\noverride:\n  error:\n    - DL3015\nfailure-threshold: error\n",
    )
    .unwrap();
    let report = lint(doc, &cfg).unwrap();
    let codes: Vec<(String, Severity)> = report
        .failures
        .iter()
        .map(|f| (f.code.clone(), f.severity))
        .collect();
    assert_eq!(
        codes,
        vec![
            ("DL3008".to_string(), Severity::Warning),
            ("DL3015".to_string(), Severity::Error)
        ]
    );
    assert!(report.fails);
    cfg.failure_threshold = Severity::Error;
    cfg.error_rules.clear();
    assert!(!lint(doc, &cfg).unwrap().fails);
}

#[test]
fn shell_view_of_run_scripts() {
    let doc = "FROM alpine:3.20\nRUN cd /tmp && sudo apk add curl && echo \"$(wget -q x | tee y)\" ; pip install a\n";
    assert_eq!(
        run(doc),
        vec![
            "2 DL3003 warning: Use WORKDIR to switch to a directory",
            "2 DL3004 error: Do not use sudo as it leads to unpredictable behavior. Use a tool like gosu to enforce root",
            "2 DL3013 warning: Pin versions in pip. Instead of `pip install <package>` use `pip install <package>==<version>` or `pip install --requirement <requirements file>`",
            "2 DL3042 warning: Avoid use of cache directory with pip. Use `pip install --no-cache-dir <package>`",
            "2 DL4006 warning: Set the SHELL option -o pipefail before RUN with a pipe in it. If you are using /bin/sh in an alpine image or if your shell is symlinked to busybox then consider explicitly setting your SHELL to /bin/ash, or disable this check",
        ]
    );
}

#[test]
fn multi_stage_rules() {
    let doc = "FROM golang:1.22 AS build\nWORKDIR /src\nRUN go install github.com/x/y\nFROM scratch\nCOPY --from=build /src/bin /bin\nCOPY --from=nowhere a b\nUSER root\nMAINTAINER me\nCMD [\"/bin/y\"]\nCMD [\"/bin/z\"]\n";
    assert_eq!(
        run(doc),
        vec![
            "3 DL3062 warning: Pin versions in go. Instead of `go install <package>` use `go install <package>@<version>`",
            "6 DL3022 warning: `COPY --from` should reference a previously defined `FROM` alias",
            "6 DL3045 warning: `COPY` to a relative destination without `WORKDIR` set.",
            "7 DL3002 warning: Last USER should not be root",
            "7 DL3066 info: Non-numeric user-id may not be resolvable by host system",
            "8 DL4000 error: MAINTAINER is deprecated",
            "10 DL4003 warning: Multiple `CMD` instructions found. If you list more than one `CMD` then only the last `CMD` will take effect",
        ]
    );
}

#[test]
fn parse_error_is_reported_with_position() {
    let err = lint("FROM ubuntu:24.04\nRUNN echo\n", &Config::default()).unwrap_err();
    assert_eq!((err.line, err.column), (2, 1));
}

#[test]
fn ignore_pragma_grammar() {
    use super::parse_ignore_pragma;
    assert_eq!(
        parse_ignore_pragma(" hadolint ignore=DL3008, DL3009 # why", &[]),
        Some(vec!["DL3008".to_string(), "DL3009".to_string()])
    );
    assert_eq!(
        parse_ignore_pragma("hadolint ignore = SC2086", &[]),
        Some(vec!["SC2086".to_string()])
    );
    assert_eq!(
        parse_ignore_pragma(" hadolint stage ignore=DL3008", &["stage"]),
        Some(vec!["DL3008".to_string()])
    );
    assert_eq!(
        parse_ignore_pragma(" hadolint stage ignore=DL3008", &[]),
        None
    );
    assert_eq!(parse_ignore_pragma(" just a comment", &[]), None);
    assert_eq!(
        parse_ignore_pragma(" hadolint ignore=DL3008 trailing", &[]),
        None
    );
}
