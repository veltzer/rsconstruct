//! Centralized network downloads.
//!
//! Every download rsconstruct performs — binary release assets, `.deb`
//! packages, remote-cache objects — goes through this module. Call sites
//! MUST NOT invoke `curl` directly; see
//! `docs/src/internal/download-policy.md` for the rule and its rationale.
//!
//! The point of the module is that retry policy, timeouts, and error
//! context live in exactly one place. A bare `Command::new("curl")`
//! elsewhere silently opts out of all three.

use std::process::Command;

/// Number of times curl retries a transient failure before giving up.
///
/// Deliberately small. This exists to absorb a connection dropped at setup
/// (see the module docs and the download policy); it is not a mechanism for
/// riding out a degraded mirror, which no retry count can fix.
const RETRY_ATTEMPTS: &str = "3";

/// Seconds to wait between retries. Fixed rather than exponential: with
/// three attempts, backoff math would add complexity without changing the
/// outcome.
const RETRY_DELAY_SECS: &str = "2";

/// Give up on a connection that cannot be *established* within this many
/// seconds. This bounds only the TCP/TLS handshake, never the transfer
/// itself, so a slow-but-progressing mirror is unaffected.
const CONNECT_TIMEOUT_SECS: &str = "30";

/// The retry and timeout flags shared by every download.
///
/// `--retry-connrefused` and `--retry-all-errors` are what make the flags
/// cover the failure this policy was written for: plain `--retry` handles
/// HTTP 5xx and a few transport errors, but a connection reset during
/// handshake (curl exit 35) is not in that set by default.
const fn retry_args() -> [&'static str; 8] {
    [
        "--retry",
        RETRY_ATTEMPTS,
        "--retry-delay",
        RETRY_DELAY_SECS,
        "--retry-connrefused",
        "--retry-all-errors",
        "--connect-timeout",
        CONNECT_TIMEOUT_SECS,
    ]
}

/// Build the argv for downloading `url` to `dest`, retries included.
///
/// Returned as an owned argv rather than executed here because the binary
/// installer runs its commands through its own executor (it inherits the
/// terminal so `sudo` can prompt) and separately renders them for
/// `--dry-run` preview. Both paths must show and run the same flags.
pub fn curl_argv(url: &str, dest: &str) -> Vec<String> {
    curl_argv_with_token_var(url, dest, github_token_var())
}

/// [`curl_argv`] with the token variable passed in, so tests need not
/// touch the process environment.
///
/// Downloads from `raw.githubusercontent.com` are authenticated when a
/// GitHub token is set. Anonymous requests there are rate-limited per
/// source IP, and GitHub-hosted runners share egress IPs, so a busy pool
/// gets HTTP 429 no matter how often it retries (CI run 37439740429,
/// fetching `checkpatch.pl`). It is the same limit, and the same remedy,
/// as the releases API (see `tools::resolve_latest_deb_asset`).
///
/// The token never appears in argv: `--variable %NAME` has curl read it
/// from the environment and `--expand-header` substitutes it, so argv —
/// which `tool install --dry-run` prints and `ps` shows — carries only the
/// variable's name. (Both flags need curl 8.3 or later.) Other hosts get no
/// header; curl drops it on a redirect to another host anyway.
fn curl_argv_with_token_var(url: &str, dest: &str, token_var: Option<&str>) -> Vec<String> {
    let mut argv = vec!["curl".to_string(), "-fsSL".to_string()];
    argv.extend(retry_args().iter().map(|s| (*s).to_string()));
    if let Some(var) = token_var
        && url.starts_with("https://raw.githubusercontent.com/")
    {
        argv.extend([
            "--variable".to_string(),
            format!("%{var}"),
            "--expand-header".to_string(),
            format!("Authorization: Bearer {{{{{var}}}}}"),
        ]);
    }
    argv.extend(["-o".to_string(), dest.to_string(), url.to_string()]);
    argv
}

/// The environment variable holding a GitHub token, if one is set.
///
/// `GITHUB_TOKEN` is the name Actions uses and `GH_TOKEN` the one the `gh`
/// CLI uses; accepting both means the same binary is authenticated on a
/// runner and on a developer machine that has `gh` set up, with no extra
/// configuration in either place. An empty value is treated as unset —
/// `env: GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}` expands to the empty
/// string rather than disappearing when the secret is unavailable, and
/// sending `Bearer ` with no token earns a 401 instead of the anonymous
/// access that would otherwise still work.
pub fn github_token_var() -> Option<&'static str> {
    github_token_var_from(|name| std::env::var(name).ok())
}

/// The variable-selection half of [`github_token_var`], split out so it can
/// be tested without mutating the process environment — `set_var` is
/// global state and the test harness runs threads in parallel.
fn github_token_var_from(lookup: impl Fn(&str) -> Option<String>) -> Option<&'static str> {
    // The emptiness filter is inside find, not after it: an empty
    // GITHUB_TOKEN must fall through to GH_TOKEN rather than mask it.
    ["GITHUB_TOKEN", "GH_TOKEN"]
        .into_iter()
        .find(|name| lookup(name).is_some_and(|token| !token.trim().is_empty()))
}

/// Apply the shared retry and timeout flags to an already-configured curl
/// `Command`.
///
/// Used by call sites that need curl flags this module does not own — the
/// remote cache passes `--head`, `-w %{http_code}`, and `-X PUT`. They keep
/// their own flags and inherit retry behavior from here.
pub fn apply_retry_args(cmd: &mut Command) {
    cmd.args(retry_args());
}

/// Run `attempt` up to `RETRY_ATTEMPTS` times, sleeping `RETRY_DELAY_SECS`
/// between tries, and return the last error if all of them fail.
///
/// This is the `ureq` counterpart to [`retry_args`]: HTTP calls made from
/// Rust rather than through curl get the same policy. `ureq` performs no
/// retries of its own — a connection reset surfaces directly to the caller
/// — so without this an in-process fetch would be less robust than a
/// subprocess one.
///
/// Every attempt is retried: `ureq` reports transport failures and HTTP
/// status errors through the same error type, and distinguishing them
/// would buy nothing at three attempts against an idempotent GET.
pub fn with_retry<T, E, F>(mut attempt: F) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
{
    // Parsed rather than duplicated as integer consts so the curl flags and
    // this loop can never drift apart.
    let attempts: u32 = RETRY_ATTEMPTS.parse().unwrap_or(3);
    let delay = std::time::Duration::from_secs(RETRY_DELAY_SECS.parse().unwrap_or(2));
    let mut last = None;
    for i in 0..attempts.max(1) {
        if i > 0 {
            std::thread::sleep(delay);
        }
        match attempt() {
            Ok(value) => return Ok(value),
            Err(err) => last = Some(err),
        }
    }
    // The loop runs at least once, so `last` is populated on every path
    // that reaches here.
    Err(last.expect("at least one attempt ran"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_argv_includes_retry_flags() {
        let argv = curl_argv("https://example.com/x.gz", "/tmp/x.dl");
        assert!(argv.contains(&"--retry".to_string()));
        assert!(argv.contains(&"--retry-connrefused".to_string()));
        assert!(argv.contains(&"--retry-all-errors".to_string()));
        assert!(argv.contains(&"--connect-timeout".to_string()));
    }

    #[test]
    fn curl_argv_puts_url_last_and_dest_after_o() {
        let argv = curl_argv("https://example.com/x.gz", "/tmp/x.dl");
        assert_eq!(argv[0], "curl");
        assert_eq!(argv.last().unwrap(), "https://example.com/x.gz");
        let o = argv.iter().position(|a| a == "-o").expect("-o present");
        assert_eq!(argv[o + 1], "/tmp/x.dl");
    }

    /// A raw.githubusercontent.com download is authenticated through curl's
    /// own variable expansion: argv names the variable, never the token.
    #[test]
    fn raw_github_downloads_are_authenticated_by_variable_name() {
        let url = "https://raw.githubusercontent.com/torvalds/linux/master/scripts/checkpatch.pl";
        let argv = curl_argv_with_token_var(url, "/tmp/c.dl", Some("GITHUB_TOKEN"));
        let at = argv
            .iter()
            .position(|a| a == "--variable")
            .expect("--variable present");
        assert_eq!(argv[at + 1], "%GITHUB_TOKEN");
        let at = argv
            .iter()
            .position(|a| a == "--expand-header")
            .expect("--expand-header present");
        assert_eq!(argv[at + 1], "Authorization: Bearer {{GITHUB_TOKEN}}");
        assert_eq!(argv.last().unwrap(), url, "the URL stays last");

        // No token set: anonymous, as before.
        let anonymous = curl_argv_with_token_var(url, "/tmp/c.dl", None);
        assert!(!anonymous.contains(&"--variable".to_string()));
    }

    /// Only raw.githubusercontent.com gets the header; a token is never
    /// sent to an unrelated host.
    #[test]
    fn other_hosts_get_no_token() {
        for url in [
            "https://example.com/x.gz",
            "https://github.com/biomejs/biome/releases/latest/download/biome-linux-x64",
        ] {
            let argv = curl_argv_with_token_var(url, "/tmp/x.dl", Some("GITHUB_TOKEN"));
            assert!(!argv.contains(&"--expand-header".to_string()), "{url}");
        }
    }

    /// `GITHUB_TOKEN` wins when both are set, an empty value falls through
    /// to the next name rather than masking it (Actions expands a missing
    /// secret to the empty string), and all-unset means anonymous access.
    #[test]
    fn github_token_var_prefers_github_token_and_skips_empty() {
        let lookup = |vars: &[(&str, &str)], name: &str| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_string())
        };

        assert_eq!(
            github_token_var_from(|n| lookup(&[("GITHUB_TOKEN", "a"), ("GH_TOKEN", "b")], n)),
            Some("GITHUB_TOKEN")
        );
        assert_eq!(
            github_token_var_from(|n| lookup(&[("GH_TOKEN", "b")], n)),
            Some("GH_TOKEN")
        );
        // The masking case: empty GITHUB_TOKEN must not hide GH_TOKEN.
        assert_eq!(
            github_token_var_from(|n| lookup(&[("GITHUB_TOKEN", ""), ("GH_TOKEN", "b")], n)),
            Some("GH_TOKEN")
        );
        assert_eq!(
            github_token_var_from(|n| lookup(&[("GITHUB_TOKEN", "   ")], n)),
            None
        );
        assert_eq!(github_token_var_from(|n| lookup(&[], n)), None);
    }

    /// A transfer that is slow but progressing must never be killed: that
    /// was the documented failure mode of the previous timeout attempt.
    #[test]
    fn no_total_transfer_timeout() {
        let argv = curl_argv("https://example.com/x.gz", "/tmp/x.dl");
        assert!(!argv.contains(&"--max-time".to_string()));
    }

    #[test]
    fn with_retry_returns_first_success_without_retrying() {
        let mut calls = 0;
        let out: Result<u8, ()> = with_retry(|| {
            calls += 1;
            Ok(7)
        });
        assert_eq!(out, Ok(7));
        assert_eq!(calls, 1);
    }

    #[test]
    fn with_retry_succeeds_after_a_transient_failure() {
        let mut calls = 0;
        let out: Result<u8, &str> = with_retry(|| {
            calls += 1;
            if calls < 2 { Err("reset") } else { Ok(9) }
        });
        assert_eq!(out, Ok(9));
        assert_eq!(calls, 2);
    }

    #[test]
    fn with_retry_gives_up_and_returns_last_error() {
        let mut calls = 0;
        let out: Result<u8, &str> = with_retry(|| {
            calls += 1;
            Err("down")
        });
        assert_eq!(out, Err("down"));
        assert_eq!(calls, 3);
    }
}
