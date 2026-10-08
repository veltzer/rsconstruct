//! Which contexts and special functions each workflow key may use, from
//! GitHub's context-availability table; a port of actionlint's generated
//! `availability.go`.

/// `(contexts, special functions)` available at a workflow key, lower-cased.
/// Empty contexts means none is allowed; an unknown key gives `None`.
pub fn workflow_key_availability(
    key: &str,
) -> Option<(&'static [&'static str], &'static [&'static str])> {
    const ALL_STEP: &[&str] = &[
        "env", "github", "inputs", "job", "matrix", "needs", "runner", "secrets", "steps",
        "strategy", "vars",
    ];
    Some(match key {
        "jobs.<job_id>.outputs.<output_id>" => (ALL_STEP, &[]),
        "jobs.<job_id>.steps.continue-on-error"
        | "jobs.<job_id>.steps.env"
        | "jobs.<job_id>.steps.name"
        | "jobs.<job_id>.steps.run"
        | "jobs.<job_id>.steps.timeout-minutes"
        | "jobs.<job_id>.steps.with"
        | "jobs.<job_id>.steps.working-directory" => (ALL_STEP, &["hashfiles"]),
        "jobs.<job_id>.container.env.<env_id>"
        | "jobs.<job_id>.services.<service_id>.env.<env_id>" => (
            &[
                "env", "github", "inputs", "job", "matrix", "needs", "runner", "secrets",
                "strategy", "vars",
            ],
            &[],
        ),
        "jobs.<job_id>.environment.url" => (
            &[
                "env", "github", "inputs", "job", "matrix", "needs", "runner", "steps", "strategy",
                "vars",
            ],
            &[],
        ),
        "jobs.<job_id>.steps.if" => (
            &[
                "env", "github", "inputs", "job", "matrix", "needs", "runner", "steps", "strategy",
                "vars",
            ],
            &["always", "cancelled", "failure", "hashfiles", "success"],
        ),
        "jobs.<job_id>.container.credentials"
        | "jobs.<job_id>.services.<service_id>.credentials" => (
            &[
                "env", "github", "inputs", "matrix", "needs", "secrets", "strategy", "vars",
            ],
            &[],
        ),
        "jobs.<job_id>.defaults.run" => (
            &[
                "env", "github", "inputs", "matrix", "needs", "strategy", "vars",
            ],
            &[],
        ),
        "on.workflow_call.outputs.<output_id>.value" => {
            (&["github", "inputs", "jobs", "vars"], &[])
        }
        "jobs.<job_id>.env" | "jobs.<job_id>.secrets.<secrets_id>" => (
            &[
                "github", "inputs", "matrix", "needs", "secrets", "strategy", "vars",
            ],
            &[],
        ),
        "jobs.<job_id>.concurrency"
        | "jobs.<job_id>.container"
        | "jobs.<job_id>.container.image"
        | "jobs.<job_id>.continue-on-error"
        | "jobs.<job_id>.environment"
        | "jobs.<job_id>.name"
        | "jobs.<job_id>.runs-on"
        | "jobs.<job_id>.services"
        | "jobs.<job_id>.timeout-minutes"
        | "jobs.<job_id>.with.<with_id>" => (
            &["github", "inputs", "matrix", "needs", "strategy", "vars"],
            &[],
        ),
        "jobs.<job_id>.strategy" => (&["github", "inputs", "needs", "vars"], &[]),
        "jobs.<job_id>.if" => (
            &["github", "inputs", "needs", "vars"],
            &["always", "cancelled", "failure", "success"],
        ),
        "env" => (&["github", "inputs", "secrets", "vars"], &[]),
        "concurrency" | "on.workflow_call.inputs.<inputs_id>.default" | "run-name" => {
            (&["github", "inputs", "vars"], &[])
        }
        "jobs.<job_id>.snapshot.if" => (
            &["github", "inputs", "matrix", "needs", "strategy", "vars"],
            &[],
        ),
        _ => return None,
    })
}
