use std::sync::Arc;
use std::collections::HashMap;

use futures::future;
use futures::stream::{self, Stream, StreamExt, TryStreamExt, FuturesUnordered};
use zbus::names::{BusName, ErrorName, UniqueName};
use zbus::zvariant::OwnedObjectPath;

use crate::dbus::{dprom::DProm1Proxy, gauge::Gauge1Proxy};
use crate::export::config;
use crate::export::context::{Ctx, BusCtx, PathCtx};
use crate::export::metric::{Export, MetricName};
use crate::future::linger::{linger, Linger};

pub async fn run(log: slog::Logger, export: Export, config: config::Dbus) -> anyhow::Result<()> {
    let export = Arc::new(export);

    let futures = FuturesUnordered::new();

    if config.session {
        let log = log.new(slog::o!("dbus" => "session"));
        let conn = Arc::new(zbus::Connection::session().await?);
        let ctx = Ctx::new(log, conn, export.clone());
        futures.push(run_top(ctx));
    }

    if config.system {
        let log = log.new(slog::o!("dbus" => "system"));
        let conn = Arc::new(zbus::Connection::session().await?);
        let ctx = Ctx::new(log, conn, export.clone());
        futures.push(run_top(ctx));
    }

    if futures.is_empty() {
        slog::crit!(log, "no dbus connection configured. hint: pass --session or --system");
    }

    futures.try_collect::<()>().await?;
    Ok(())
}

enum NameEvent {
    Add(UniqueName<'static>),
    Del(UniqueName<'static>),
}

impl NameEvent {
    pub fn from_signal(msg: zbus::fdo::NameOwnerChanged) -> Result<Option<Self>, zbus::Error> {
        let args = msg.args()?;

        let name = match args.name {
            BusName::Unique(uniq) => uniq.to_owned(),
            BusName::WellKnown(_) => { return Ok(None); }
        };

        let event = match (args.old_owner.is_some(), args.new_owner.is_some()) {
            (false, true) => Some(NameEvent::Add(name)),
            (true, false) => Some(NameEvent::Del(name)),
            _ => None,
        };

        Ok(event)
    }
}

async fn run_top(ctx: Ctx) -> zbus::Result<()> {
    let dbus = zbus::fdo::DBusProxy::new(&ctx.conn).await?;

    let mut tasks = HashMap::<UniqueName<'static>, Linger<_>>::new();

    // must open name events stream before calling list_names to avoid race
    let name_events = dbus.receive_name_owner_changed().await?
        .filter_map(|signal| future::ready(NameEvent::from_signal(signal).transpose()));

    // start task for each unique bus name on the dbus, skipping wellknown names:
    tasks.extend(
        dbus.list_names().await?
            .into_iter()
            .filter_map(|name| match name.into_inner() {
                BusName::Unique(uniq) => Some(uniq),
                BusName::WellKnown(_) => None,
            })
            .map(|name| {
                let ctx = ctx.with_bus(name.clone());
                (name, linger(bus_task(ctx)))
            }));

    // process name events:
    futures::pin_mut!(name_events);
    while let Some(event) = name_events.next().await {
        match event? {
            NameEvent::Add(bus) => {
                let ctx = ctx.with_bus(bus.clone());
                tasks.insert(bus, linger(bus_task(ctx)));
            }
            NameEvent::Del(bus) => {
                tasks.remove(&bus);
            }
        }
    }

    return Ok(());

    async fn bus_task(ctx: BusCtx) {
        match run_bus(ctx.clone()).await {
            Ok(()) => {}
            Err(e) if is_unknown_dispatch_error(&e) => { /* ignore */ }
            Err(e) => {
                slog::error!(ctx.log, "bus error: {:?}", e);
            }
        }
    }
}

async fn run_bus(ctx: BusCtx) -> zbus::Result<()> {
    let dprom = DProm1Proxy::builder(&ctx.conn)
        .destination(ctx.bus.clone())?
        .path("/org/hails/dprom")?
        .build()
        .await?;

    let stream = metric_paths_stream(dprom).await?;

    // log the new bus at this point, since we will have errored and bailed already
    // if the bus is not a dprom bus:
    slog::debug!(ctx.log, "watching bus");

    stream.try_fold(HashMap::new(), |mut tasks, metric_paths| {
        let new_tasks = metric_paths.into_iter()
            .map(|path| {
                let task = tasks.remove(&path).unwrap_or_else(|| {
                    let ctx = ctx.with_path(path.clone());
                    linger(metric_task(ctx))
                });

                (path, task)
            })
            .collect();

        future::ok(new_tasks)
    }).await?;

    return Ok(());

    async fn metric_paths_stream(proxy: DProm1Proxy<'_>)
        -> Result<impl Stream<Item = Result<Vec<OwnedObjectPath>, zbus::Error>> + '_, zbus::Error>
    {
        // open receive stream first to prevent race
        let metrics_events = proxy.receive_metrics_changed().await
            .then(|change| async move { change.get().await });

        // get current value and prepend to stream
        let metric_paths = proxy.metrics().await?;

        Ok(stream::once(future::ok(metric_paths))
            .chain(metrics_events))
    }

    async fn metric_task(ctx: PathCtx) {
        slog::debug!(ctx.log, "watching metric");
        match run_metric(ctx.clone()).await {
            Ok(()) => {}
            Err(e) => {
                slog::error!(ctx.log, "error watching metric: {:?}", e);
            }
        }
    }
}

async fn run_metric(ctx: PathCtx) -> zbus::Result<()> {
    match protect_unknown_dispatch(run_gauge(&ctx).await)? {
        Some(()) => { return Ok(()); }
        None => { /* wrong type, try next type */ }
    }

    slog::warn!(ctx.log, "unknown metric type");
    Ok(())
}

async fn run_gauge(ctx: &PathCtx) -> zbus::Result<()> {
    let gauge = ctx.proxy::<Gauge1Proxy>().await?;
    let name = MetricName::from(gauge.name().await?);

    // open stream before reading first value to avoid race
    let stream = gauge.receive_value_changed().await
        .then(|change| async move { change.get().await });

    let value = gauge.value().await?;

    let stream = stream::once(future::ready(Ok(value))).chain(stream);
    futures::pin_mut!(stream);

    let metric = ctx.export.metric(name.clone());

    while let Some(result) = stream.next().await {
        let value = result?;
        slog::info!(ctx.log, "{} = {}", name, value);
        metric.gauge(value).await;
    }

    Ok(())
}

fn protect_unknown_dispatch<T>(result: Result<T, zbus::Error>)
    -> Result<Option<T>, zbus::Error>
{
    match result {
        Ok(val) => Ok(Some(val)),
        Err(e) if is_unknown_dispatch_error(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

fn is_unknown_dispatch_error(err: &zbus::Error) -> bool {
    use zbus::Error::FDO;
    use zbus::fdo::Error;

    match err {
        FDO(err) => match **err {
            | Error::UnknownObject(_)
            | Error::UnknownMethod(_)
            | Error::UnknownInterface(_)
            | Error::UnknownProperty(_) => true,
            _ => false,
        }
        zbus::Error::MethodError(name, _desc, _msg)
            if is_unknown_dispatch_error_name(name.inner()) => true,
        _ => false,
    }
}

fn is_unknown_dispatch_error_name(err: &ErrorName) -> bool {
    match err.as_str() {
        | "org.freedesktop.DBus.Error.UnknownInterface"
        | "org.freedesktop.DBus.Error.UnknownObject"
        | "org.freedesktop.DBus.Error.UnknownMethod"
        | "org.freedesktop.DBus.Error.UnknownProperty" => true,
        _ => false,
    }
}
