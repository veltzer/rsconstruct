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

/// Check if a path should be ignored (editor temp files, build artifacts)
fn should_ignore(path: &Path) -> bool {
    // Ignore .rsb cache directory (match as a path component, not substring)
    if path.components().any(|c| c.as_os_str() == ".rsb") {
        return true;
    }

    let path_str = path.to_string_lossy();

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
    }

    false
}

/// Register watch paths with the watcher, returning the list of paths being watched.
fn register_watches(
    watcher: &mut impl Watcher,
    paths: &[PathBuf],
    verbose: bool,
) {
    for path in paths {
        let mode = if path.is_dir() {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        if let Err(e) = watcher.watch(path, mode)
            && verbose {
                println!("Warning: could not watch {}: {}", path.display(), e);
            }
    }
}

pub fn watch(opts: &BuildOptions, interrupted: Arc<AtomicBool>) -> Result<()> {
    // Initial build
    println!("{}", color::bold("Running initial build..."));
    let mut watch_paths;
    {
        let mut builder = Builder::new()?;
        watch_paths = builder.watch_paths();
        if let Err(e) = builder.build(opts, Arc::clone(&interrupted)) {
            println!("{}", color::red(&format!("Initial build error: {}", e)));
        }
    }

    // Set up file watcher
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(tx)?;

    // Watch project paths derived from config
    register_watches(&mut watcher, &watch_paths, opts.verbose);

    println!("{}", color::green("Watching for changes... (Ctrl+C to stop)"));

    let debounce_duration = Duration::from_millis(200);
    let poll_interval = Duration::from_millis(500);

    loop {
        // Wait for a relevant file-change event, periodically checking the interrupted flag.
        // Breaks with `true` on a real event, `false` if the watcher channel disconnects.
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
            let new_paths = builder.watch_paths();
            if let Err(e) = builder.build(opts, Arc::clone(&interrupted)) {
                println!("{}", color::red(&format!("Build error: {}", e)));
            }

            // Update watches if paths changed (e.g., new scan dirs in config)
            for path in &new_paths {
                if !watch_paths.contains(path) {
                    register_watches(&mut watcher, std::slice::from_ref(path), opts.verbose);
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

        println!("{}", color::green("Watching for changes... (Ctrl+C to stop)"));
    }

    Ok(())
}
