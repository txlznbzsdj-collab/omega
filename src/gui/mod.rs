//! Native graphical front end.
//!
//! Kept free of any toolkit dependency: the window is built straight on the
//! Win32 API so the whole front end costs a few kilobytes on top of the
//! calculator engine, and so results with hundreds of thousands of digits can
//! be moved between controls as plain text.

#[cfg(windows)]
pub mod win32;
