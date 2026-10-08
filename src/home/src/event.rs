use crate::model::{Nav, Res};

/// Everything the session's main loop reacts to.
#[derive(Debug)]
pub enum Event {
    Nav(Nav),
    Click(i32, i32),
    Hover(i32, i32),
    Res(Res),
    /// The X window was uncovered / covered (YARG's window appearing hides the splash).
    Visible(bool),
    Redraw,
    YargExited { code: Option<i32>, secs: u64 },
}
