use std::io::IsTerminal;

use indicatif::{ProgressBar, ProgressStyle};

use crate::errors;

/// Width of the progress bar in terminal columns.
const BAR_WIDTH: usize = 40;

/// Create a progress bar with the standard rsconstruct style.
///
/// Returns a hidden bar when:
/// - `hidden` is true (verbose mode, JSON mode, or quiet mode), OR
/// - stdout is not a terminal (piped or redirected).
///
/// The stdout-TTY check matters because a bar drawn to stderr while stdout
/// is being captured by `| cat`, `| less`, or a CI log pipeline produces
/// noise: ANSI escape codes interleave with the captured text and the user
/// isn't watching live anyway. indicatif only auto-hides when *its own*
/// channel (stderr) is non-TTY; we additionally honor stdout's interactivity
/// because that's the channel the user is actually consuming.
pub fn create_bar(total: u64, hidden: bool) -> ProgressBar {
    if hidden || !std::io::stdout().is_terminal() {
        return ProgressBar::hidden();
    }
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "[{{elapsed_precise}}] {{bar:{BAR_WIDTH}}} {{pos}}/{{len}} {{msg}}"
            ))
            .expect(errors::INVALID_PROGRESS_TEMPLATE)
            .progress_chars("=> "),
    );
    pb
}
