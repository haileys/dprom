use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::stream::{FuturesUnordered, TryStreamExt};
use structopt::StructOpt;
use tokio::sync::watch;

pub mod config;
pub mod dbus;

pub type WatchValue = Option<f64>;

struct Ctx {
    log: slog::Logger,
}

impl Ctx {
    pub fn with_path(&self, path: PathBuf) -> PathCtx {
        PathCtx {
            log: self.log.new(slog::o!("path" => path.to_string_lossy().to_string())),
            path,
        }
    }
}

struct PathCtx {
    log: slog::Logger,
    path: PathBuf,
}

fn file_watch(ctx: PathCtx, watch: &config::Watch) -> watch::Receiver<WatchValue> {
    let (tx, rx) = watch::channel(None);
    let refresh_secs = watch.refresh_secs;

    tokio::spawn(async move {
        loop {
            match read(&ctx.path).await {
                Ok(value) => {
                    match tx.send(Some(value)) {
                        Ok(()) => {}
                        Err(_) => {
                            // other side closed
                            break;
                        }
                    }
                }
                Err(e) => {
                    slog::error!(ctx.log, "error reading file: {:?}", e);
                }
            }

            tokio::time::sleep(refresh_secs).await;
        }

        async fn read(path: &Path) -> anyhow::Result<f64> {
            let value = tokio::fs::read_to_string(path)
                .await?
                .trim()
                .parse()?;

            Ok(value)
        }
    });

    rx
}

#[derive(StructOpt, Debug)]
pub struct Opt {
    #[structopt(short, long)]
    config: std::path::PathBuf,
}

pub async fn run(log: slog::Logger, opt: Opt) -> anyhow::Result<()> {
    let config = config::open(&opt.config).await?;

    let ctx = Ctx { log };

    // create Gauge models
    let gauges = config.gauges.0.into_iter()
        .map(move |(name, path)| {
            let watch = file_watch(ctx.with_path(path.clone()), &config.watch);
            dbus::Gauge { name, watch }
        })
        .collect::<Vec<_>>();

    let metric_paths = gauges.iter()
        .map(|g| g.object_path())
        .collect::<Vec<_>>();

    // create DProm model
    let dprom = dbus::DProm {
        metrics: metric_paths.clone(),
    };

    // connect to dbus
    let conn = Arc::new(build_connection(dprom, gauges).await?);

    // spawn refresh tasks
    metric_paths.iter()
        .map(|path| {
            let conn = conn.clone();
            async move {
                let interface = conn
                    .object_server()
                    .interface::<_, dbus::Gauge>(path)
                    .await?;

                let mut watch = interface.get().await.watch().clone();

                loop {
                    watch.changed().await?;

                    let signal_context = interface.signal_context();

                    interface.get().await
                        .value_changed(signal_context)
                        .await?;
                }
            }
        })
        .collect::<FuturesUnordered<_>>()
        .try_collect::<()>()
        .await
}

async fn build_connection(
    dprom: dbus::DProm,
    gauges: Vec<dbus::Gauge>,
) -> anyhow::Result<zbus::Connection> {
    let conn = zbus::ConnectionBuilder::session()?
        .serve_at("/org/hails/dprom", dprom)?;

    let conn = gauges.into_iter()
        .try_fold(conn, |conn, gauge| {
            conn.serve_at(gauge.object_path(), gauge)
        })?;

    Ok(conn.build().await?)
}
