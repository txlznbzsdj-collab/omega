//! The `omega` calculator as a native window.

fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(omega::gui::win32::main() as u8)
}
