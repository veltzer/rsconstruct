use anyhow::Result;
use notify::{Event, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::builder::Builder;
use crate::cli::BuildOptions;
use crate::color;

/// Collect directories to watch based on project conventions
fn collect_watch_paths(project_root: &Path) -> Vec<PathBuf> {
    let candidates = [
        "rsb.toml",
        "templates",
        "config",
        "sleep",
        "src",
        "tests",
        "pyproject.toml",
        "plugins",
    ];
    candidates
        .iter()
        .map(|c| project_root.join(c))
        .filter(|p| p.exists())
        .collect()
}

/// Check if a path should be ignored (editor temp files, build artifacts)
fn should_ignore(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    // Ignore .rsb cache directory
    if path_str.contains(".rsb") {
        return true;
    }

    // Ignore out/ directory (generated stubs)
    if path_str.contains("/out/") || path_str.starts_with("out/") {
        return true;
    }

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
        if name.starts_with("__") {
            return true;
        }
    }

    false
}

pub fn watch(opts: &BuildOptions, interrupted: Arc<AtomicBool>) -> Result<()> {
    let project_root = std::env::current_dir()?;

    // Initial build
    println!("{}", color::bold("Running initial build..."));
    {
        let mut builder = Builder::new()?;
        if let Err(e) = builder.build(opts, Arc::clone(&interrupted)) {
            println!("{}", color::red(&format!("Initial build error: {}", e)));
        }
    }

    // Set up file watcher
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(tx)?;

    // Watch project paths
    let mut watch_paths = collect_watch_paths(&project_root);
    for path in &watch_paths {
        let mode = if path.is_dir() {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        if let Err(e) = watcher.watch(path, mode)
            && opts.verbose {
                println!("Warning: could not watch {}: {}", path.display(), e);
            }
    }

    println!("{}", color::green("Watching for changes... (Ctrl+C to stop)"));

    let debounce_duration = Duration::from_millis(200);
    let poll_interval = Duration::from_millis(500);

    loop {
        // Wait for first event, periodically checking the interrupted flag
        let got_event = loop {
            if interrupted.load(Ordering::SeqCst) {
                return Ok(());
            }
            match rx.recv_timeout(poll_interval) {
                Ok(Ok(event)) => {
                    let dominated_by_ignored = event.paths.iter().all(|p| should_ignore(p));
                    if dominated_by_ignored {
                        continue;
                    }
                    break true;
                }
                Ok(Err(e)) => {
                    println!("{}", color::red(&format!("Watch error: {}", e)));
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
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

        // Rebuild
        println!();
        println!("{}", color::bold("Change detected, rebuilding..."));
        {
            let mut builder = Builder::new()?;
            if let Err(e) = builder.build(opts, Arc::clone(&interrupted)) {
                println!("{}", color::red(&format!("Build error: {}", e)));
            }
        }

        // Re-collect watch paths in case new directories appeared
        let new_paths = collect_watch_paths(&project_root);
        for path in &new_paths {
            if !watch_paths.contains(path) {
                let mode = if path.is_dir() {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                };
                if let Err(e) = watcher.watch(path, mode)
                    && opts.verbose {
                        println!("Warning: could not watch {}: {}", path.display(), e);
                    }
            }
        }
        watch_paths = new_paths;

        println!("{}", color::green("Watching for changes... (Ctrl+C to stop)"));
    }

    Ok(())
}
