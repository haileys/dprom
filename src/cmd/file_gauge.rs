use std::process::ExitCode;

use sloggers::Build;
use sloggers::terminal::{TerminalLoggerBuilder, Destination};
use sloggers::types::Severity;
use structopt::StructOpt;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let opt = dprom::file_gauge::Opt::from_args();

    let mut builder = TerminalLoggerBuilder::new();
    builder.level(Severity::Trace);
    builder.destination(Destination::Stderr);

    let log = builder.build().unwrap();

    match dprom::file_gauge::run(log.clone(), opt).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            slog::crit!(log, "{:?}", e);
            ExitCode::FAILURE
        }
    }
}

