//! Native graphical front end.
//!
//! Kept free of any toolkit dependency: the window is built straight on the
//! Win32 API so the whole front end costs a few kilobytes on top of the
//! calculator engine, and so results with hundreds of thousands of digits can
//! be moved between controls as plain text.
//!
//! The pieces that are not Win32 are split out so they can be tested without a
//! window: [`edit`] holds the caret arithmetic and [`keypad`] the button grid.

pub mod edit;
pub mod keypad;
pub mod worker;

#[cfg(windows)]
pub mod win32;
