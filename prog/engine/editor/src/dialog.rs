//! Native file dialogs. Call from UI / idle — **never** from `Sample::frame`.
//!
//! A modal dialog blocks the thread. The GPU loop must not sit inside
//! `begin_frame`/`end_frame` without a fence (roadmap §15, D73).

use std::path::PathBuf;

const FILTER_NAME: &str = "Harpia scene";
const FILTER_EXT: [&str; 2] = ["scene", "ron"];

/// Open-file picker. `None` if the user cancels or the portal is missing.
pub fn pick_authoring_scene() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(FILTER_NAME, &FILTER_EXT)
        .pick_file()
}

/// Save-file picker. `None` if the user cancels or the portal is missing.
pub fn save_authoring_scene() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(FILTER_NAME, &FILTER_EXT)
        .save_file()
}

#[cfg(test)]
mod tests {
    #[test]
    fn rfd_links_without_opening_a_dialog() {
        let _ = rfd::FileDialog::new().add_filter("Harpia scene", &["scene", "ron"]);
    }
}
