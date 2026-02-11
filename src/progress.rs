use indicatif::{ProgressBar, ProgressStyle};

use crate::errors;

/// Width of the progress bar in terminal columns.
const BAR_WIDTH: usize = 40;

/// Create a progress bar with the standard rsb style.
///
/// Returns a hidden bar when `hidden` is true (verbose mode or JSON mode),
/// otherwise a visible bar with the standard template and style.
pub fn create_bar(total: u64, hidden: bool) -> ProgressBar {
    if hidden {
        return ProgressBar::hidden();
    }
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(&format!("[{{elapsed_precise}}] {{bar:{BAR_WIDTH}}} {{pos}}/{{len}} {{msg}}"))
            .expect(errors::INVALID_PROGRESS_TEMPLATE)
            .progress_chars("=> "),
    );
    pb
}
