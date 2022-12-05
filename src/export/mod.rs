pub mod context;
pub mod dbus;
pub mod http;
pub mod metric;

use futures::future;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub struct Opt {
    #[structopt(flatten)]
    dbus: DbusOpt,
    #[structopt(flatten)]
    http: HttpOpt,
}

#[derive(StructOpt, Debug)]
pub struct DbusOpt {
    #[structopt(long)]
    system: bool,
    #[structopt(long)]
    session: bool,
}

#[derive(StructOpt, Debug)]
pub struct HttpOpt {
    #[structopt(long)]
    listen: std::net::SocketAddr,
}

pub async fn run(log: slog::Logger, opt: Opt) -> anyhow::Result<()> {
    let (export, metric_stream) = metric::Export::new();

    let dbus = tokio::spawn(dbus::run(log.clone(), export, opt.dbus));
    let http = tokio::spawn(http::run(log.clone(), metric_stream, opt.http));

    future::select(dbus, http).await.factor_first().0??;
    Ok(())
}
