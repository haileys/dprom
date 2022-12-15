use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
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
    let config = config::open(&opt.config).await
        .with_context(|| "config::open")?;

    let ctx = Ctx { log: log.clone() };

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

    // connect to dbus and serve objects
    let conn = Arc::new(serve_objects(&log, dprom, gauges).await
        .with_context(|| "serve_objects")?);

    if metric_paths.is_empty() {
        slog::warn!(log, "No gauges configured");
    }

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

async fn serve_objects(
    log: &slog::Logger,
    dprom: dbus::DProm,
    gauges: Vec<dbus::Gauge>,
) -> anyhow::Result<zbus::Connection> {
    let builders = [
        ("session", zbus::ConnectionBuilder::session()),
        ("system", zbus::ConnectionBuilder::system()),
    ];

    let mut last_error = None;

    for (conn_kind, builder) in builders {
        slog::trace!(log, "trying {} dbus", conn_kind);

        let result = try_connection(builder, dprom.clone(), gauges.clone())
            .await
            .with_context(|| format!("try_connection: {}", conn_kind));

        match result {
            Ok(conn) => {
                slog::info!(log, "Connected to {} bus", conn_kind);
                return Ok(conn);
            }
            Err(e) => {
                slog::trace!(log, "Error connecting to {} bus: {:?}", conn_kind, e);
                last_error = Some(e);
                continue;
            }
        }
    }

    let Some(last_error) = last_error else {
        anyhow::bail!("expected error in serve_objects");
    };

    return Err(last_error);

    async fn try_connection<'a>(
        builder: zbus::Result<zbus::ConnectionBuilder<'a>>,
        dprom: dbus::DProm,
        gauges: Vec<dbus::Gauge>,
    ) -> anyhow::Result<zbus::Connection> {
        let conn = builder
            .with_context(|| "build connection")?
            .serve_at("/org/hails/dprom", dprom.clone())
            .with_context(|| "serve_at")?;

        let conn = gauges.into_iter()
            .try_fold(conn, |conn, gauge| {
                let path = gauge.object_path();
                conn.serve_at(&path, gauge)
                    .with_context(|| format!("serve gauge: {}", path))
            })?;

        Ok(conn.build().await?)
    }
}
