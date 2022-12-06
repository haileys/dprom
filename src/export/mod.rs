pub mod config;
pub mod context;
pub mod dbus;
pub mod http;
pub mod metric;

use futures::future;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub enum Opt {
    Cli(CliOpt),
    Config(ConfigOpt),
}

#[derive(StructOpt, Debug)]
pub struct CliOpt {
    #[structopt(long)]
    system: bool,
    #[structopt(long)]
    session: bool,
    #[structopt(short, long)]
    listen: std::net::SocketAddr,
}

#[derive(StructOpt, Debug)]
pub struct ConfigOpt {
    #[structopt(short, long)]
    config: std::path::PathBuf,
}

pub async fn run(log: slog::Logger, opt: Opt) -> anyhow::Result<()> {
    let config = config_from_opt(opt).await?;

    let (export, metric_stream) = metric::Export::new();

    let dbus = tokio::spawn(dbus::run(log.clone(), export, config.dbus));
    let http = tokio::spawn(http::run(log.clone(), metric_stream, config.http));

    future::select(dbus, http).await.factor_first().0??;
    Ok(())
}

async fn config_from_opt(opt: Opt) -> anyhow::Result<config::Config> {
    match opt {
        Opt::Cli(opt) => {
            Ok(config::Config {
                dbus: config::Dbus {
                    system: opt.system,
                    session: opt.session,
                },
                http: config::Http {
                    listen: opt.listen,
                    tls: None,
                },
            })
        }
        Opt::Config(opt) => {
            config::open(&opt.config).await
        }
    }
}
