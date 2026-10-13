use crate::model::{Nav, Res};
use crate::sys::VolumeNow;

/// A volume key (a media key, or the Ctrl+Alt+arrow / M back-up for keyboards without them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VolKey {
    Up,
    Down,
    Mute,
}

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
    /// The screens were reconfigured (HDMI plugged in, lid shut): the window and canvas need the new size.
    ScreenChanged,
    /// A volume key was pressed (anywhere, also while YARG runs).
    Volume(VolKey),
    /// What the volume is now after a key (None: this output has no volume control here).
    VolumeShown(Option<VolumeNow>),
    /// A Wii guitar's batteries just ran low ("Wii guitar (D4) 12%"): shown at once, even over the game.
    LowBattery(String),
    /// A message to show on the dashboard when it next appears (e.g. an automatic rollback).
    Notice(String),
    YargExited { code: Option<i32>, secs: u64 },
}
