pub mod config;
pub mod context;
pub mod dbus;
pub mod http;
pub mod metric;

use futures::future;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub struct Opt {
    #[structopt(short, long)]
    config: std::path::PathBuf,
}

pub async fn run(log: slog::Logger, opt: Opt) -> anyhow::Result<()> {
    let config = config::open(&opt.config).await?;

    let (export, metric_stream) = metric::Export::new();

    let dbus = tokio::spawn(dbus::run(log.clone(), export, config.dbus));
    let http = tokio::spawn(http::run(log.clone(), metric_stream, config.http));

    future::select(dbus, http).await.factor_first().0??;
    Ok(())
}
