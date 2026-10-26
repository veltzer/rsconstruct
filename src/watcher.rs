use anyhow::{Context, Result};
use notify::{Event, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::builder::Builder;
use crate::cli::BuildOptions;
use crate::color;

/// The directories whose events never trigger a rebuild: the cache
/// directory and the configured `[build] output_dir`, both resolved once
/// against the project root and matched as path prefixes.
///
/// The previous rule matched the output directory as a single path
/// *component*, which never matched a multi-segment `output_dir` such as
/// `build/out` (so every output write re-triggered the build) and did match
/// any directory that merely shared the name (`src/out/foo.rs` was never
/// watched when `output_dir = "out"`).
struct IgnoredDirs {
    prefixes: Vec<PathBuf>,
    root: PathBuf,
}

impl IgnoredDirs {
    fn new(root: &Path, output_dir: &str) -> Self {
        let mut prefixes = vec![lexical_join(root, Path::new(".rsconstruct"))];
        let out = lexical_join(root, Path::new(output_dir));
        // An output directory that is the project root itself (`"."`) would
        // ignore every event; the output tree cannot be told from the sources
        // then, so nothing is ignored for it.
        if out != root {
            prefixes.push(out);
        }
        Self {
            prefixes,
            root: root.to_path_buf(),
        }
    }

    /// Whether `path` (absolute, or relative to the project root — the
    /// watcher reports whichever form the watched path had) lies under one
    /// of the ignored directories.
    fn contains(&self, path: &Path) -> bool {
        let absolute = lexical_join(&self.root, path);
        self.prefixes.iter().any(|p| absolute.starts_with(p))
    }
}

/// `root.join(path)` with `.` and `..` components folded away, so that
/// `./out`, `out/../out` and `out` all name the same prefix. Purely lexical:
/// no symlink resolution, no filesystem access.
fn lexical_join(root: &Path, path: &Path) -> PathBuf {
    use std::path::Component;
    let mut result = if path.is_absolute() {
        PathBuf::new()
    } else {
        root.to_path_buf()
    };
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result
}

/// Check if a path should be ignored (editor temp files, build artifacts).
fn should_ignore(path: &Path, ignored: &IgnoredDirs) -> bool {
    if ignored.contains(path) {
        return true;
    }

    // Case-sensitive comparisons are correct here: vim swap files and the
    // `.tmp` convention are lowercase by definition, and matching `.SWP`
    // would start ignoring real source files on case-insensitive filesystems.
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        // Editor temp/swap files
        if name.starts_with('.') && name.ends_with(".swp") {
            return true;
        }
        if name.ends_with('~') {
            return true;
        }
        if name.starts_with('#') && name.ends_with('#') {
            return true;
        }
        // Common editor temp patterns
        if name.ends_with(".tmp") {
            return true;
        }
    }

    false
}

/// Register watch paths with the watcher, returning the list of paths being watched.
fn register_watches(watcher: &mut impl Watcher, paths: &[PathBuf]) {
    for path in paths {
        let mode = if path.is_dir() {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        // Always warn: an unwatched path silently produces no rebuilds, which
        // is much harder to diagnose than this message.
        if let Err(e) = watcher.watch(path, mode) {
            eprintln!("Warning: could not watch {}: {}", path.display(), e);
        }
    }
}

pub fn watch(ctx: &crate::build_context::BuildContext, opts: &BuildOptions) -> Result<()> {
    let root = std::env::current_dir().context("Failed to read the current directory")?;
    let mut builder = Builder::new_with_overrides(ctx, &opts.iset, &opts.pset)?;
    let mut watch_paths = builder.watch_paths();
    let mut ignored = IgnoredDirs::new(&root, builder.output_dir());

    // Register watches BEFORE the initial build: edits made while the (possibly
    // long) initial build runs must queue up as events, not vanish.
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(tx)?;
    register_watches(&mut watcher, &watch_paths);

    // Initial build
    println!("{}", color::bold("Running initial build..."));
    if let Err(e) = builder.build(ctx, opts, Vec::new()) {
        println!("{}", color::red(&format!("Initial build error: {e}")));
    }
    drop(builder);

    println!(
        "{}",
        color::green("Watching for changes... (Ctrl+C to stop)")
    );

    let debounce_duration = Duration::from_millis(200);
    let poll_interval = Duration::from_millis(500);

    loop {
        // Wait for a relevant file-change event, periodically checking the interrupted flag.
        // Breaks with `true` on a real event, `false` if the watcher channel disconnects.
        let got_event = loop {
            if ctx.is_interrupted() {
                return Ok(());
            }
            match rx.recv_timeout(poll_interval) {
                Ok(Ok(event)) => {
                    let all_ignored = event.paths.iter().all(|p| should_ignore(p, &ignored));
                    if all_ignored {
                        continue;
                    }
                    break true;
                }
                Ok(Err(e)) => {
                    println!("{}", color::red(&format!("Watch error: {e}")));
                }
                // Nothing arrived within the poll interval: loop round and
                // re-check the interrupt flag.
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break false,
            }
        };
        if !got_event {
            break;
        }

        // Debounce: drain further events within the debounce window
        let deadline = Instant::now() + debounce_duration;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rx.recv_timeout(remaining) {
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        // Rebuild. A failed builder construction (e.g. rsconstruct.toml saved
        // in a momentarily-invalid state) must not kill watch mode: report it
        // and keep watching — the next save triggers another attempt.
        println!();
        println!("{}", color::bold("Change detected, rebuilding..."));
        // One context serves every rebuild, and its in-session caches
        // (checksums by path, the combined-input memo) are only valid for
        // one build run: they are consulted before any stat, and the
        // executor evicts only the files the build itself writes. The edit
        // that got us here is exactly what they would hide.
        ctx.clear_session_caches();
        match Builder::new_with_overrides(ctx, &opts.iset, &opts.pset) {
            Err(e) => {
                println!("{}", color::red(&format!("Config error: {e}")));
            }
            Ok(mut builder) => {
                let new_paths = builder.watch_paths();
                ignored = IgnoredDirs::new(&root, builder.output_dir());
                if let Err(e) = builder.build(ctx, opts, Vec::new()) {
                    println!("{}", color::red(&format!("Build error: {e}")));
                }

                // Update watches if paths changed (e.g., new scan dirs in config)
                for path in &new_paths {
                    if !watch_paths.contains(path) {
                        register_watches(&mut watcher, std::slice::from_ref(path));
                    }
                }
                // Unwatch paths that are no longer relevant
                for path in &watch_paths {
                    if !new_paths.contains(path) {
                        let _ = watcher.unwatch(path);
                    }
                }
                watch_paths = new_paths;
            }
        }

        println!(
            "{}",
            color::green("Watching for changes... (Ctrl+C to stop)")
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/proj")
    }

    /// The output directory is matched as a path prefix under the project
    /// root, whether the event path is absolute or root-relative, and in
    /// whatever spelling the config used.
    #[test]
    fn output_dir_is_ignored_as_a_prefix() {
        let ignored = IgnoredDirs::new(&root(), "out");
        assert!(should_ignore(Path::new("/proj/out/x"), &ignored));
        assert!(should_ignore(
            Path::new("/proj/out/deep/er/x.html"),
            &ignored
        ));
        assert!(
            should_ignore(Path::new("out/x"), &ignored),
            "root-relative event path"
        );
        assert!(should_ignore(
            Path::new("/proj/.rsconstruct/mtime.redb"),
            &ignored
        ));
        assert!(!should_ignore(Path::new("/proj/src/main.rs"), &ignored));
        assert!(
            !should_ignore(Path::new("/proj/output/x"), &ignored),
            "a prefix match is on components, not characters"
        );

        let spelled = IgnoredDirs::new(&root(), "./out/");
        assert!(should_ignore(Path::new("/proj/out/x"), &spelled));
    }

    /// A multi-segment `[build] output_dir` such as `build/out` never matched
    /// the old single-component rule, so every output write re-triggered
    /// the build.
    #[test]
    fn multi_segment_output_dir_is_ignored() {
        let ignored = IgnoredDirs::new(&root(), "build/out");
        assert!(should_ignore(Path::new("/proj/build/out/x"), &ignored));
        assert!(should_ignore(Path::new("build/out/x"), &ignored));
        assert!(
            !should_ignore(Path::new("/proj/build/src/x.c"), &ignored),
            "a sibling of the output dir under the same parent is a source"
        );
        assert!(
            !should_ignore(Path::new("/proj/out/x"), &ignored),
            "the last segment alone is not the output dir"
        );
    }

    /// A source directory that merely shares the output directory's name
    /// (`src/out/foo.rs` with `output_dir = "out"`) was silently never
    /// watched under the old component rule.
    #[test]
    fn a_source_dir_named_like_the_output_dir_is_not_ignored() {
        let ignored = IgnoredDirs::new(&root(), "out");
        assert!(!should_ignore(Path::new("/proj/src/out/foo.rs"), &ignored));
        assert!(!should_ignore(Path::new("src/out/foo.rs"), &ignored));
        assert!(!should_ignore(
            Path::new("/proj/lib/.rsconstruct-notes.md"),
            &ignored
        ));
    }

    /// `output_dir = "."` names the project root: ignoring it would ignore
    /// every event, so the output rule is dropped and only `.rsconstruct`
    /// stays ignored.
    #[test]
    fn output_dir_equal_to_root_ignores_nothing_extra() {
        let ignored = IgnoredDirs::new(&root(), ".");
        assert!(!should_ignore(Path::new("/proj/src/main.rs"), &ignored));
        assert!(should_ignore(Path::new("/proj/.rsconstruct/x"), &ignored));
    }

    #[test]
    fn editor_temp_files_are_still_ignored() {
        let ignored = IgnoredDirs::new(&root(), "out");
        assert!(should_ignore(Path::new("/proj/src/.main.rs.swp"), &ignored));
        assert!(should_ignore(Path::new("/proj/src/main.rs~"), &ignored));
        assert!(should_ignore(Path::new("/proj/src/#main.rs#"), &ignored));
        assert!(should_ignore(Path::new("/proj/src/main.tmp"), &ignored));
        assert!(!should_ignore(Path::new("/proj/src/main.rs"), &ignored));
    }

    #[test]
    fn lexical_join_folds_dot_and_dotdot() {
        assert_eq!(
            lexical_join(Path::new("/proj"), Path::new("./a/../b/./c")),
            PathBuf::from("/proj/b/c")
        );
        assert_eq!(
            lexical_join(Path::new("/proj"), Path::new("/abs/x")),
            PathBuf::from("/abs/x"),
            "an absolute path is kept, folded"
        );
    }
}
