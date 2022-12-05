use std::process::ExitCode;

use sloggers::Build;
use sloggers::terminal::{TerminalLoggerBuilder, Destination};
use sloggers::types::Severity;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {

    let mut builder = TerminalLoggerBuilder::new();
    builder.level(Severity::Trace);
    builder.destination(Destination::Stderr);

    let log = builder.build().unwrap();

    match dprom::export::run(log.clone()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            slog::crit!(log, "{:?}", e);
            ExitCode::FAILURE
        }
    }
}
