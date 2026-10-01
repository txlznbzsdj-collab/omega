use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = omega::cli::run(args);
    ExitCode::from(code as u8)
}
