use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::collections::HashMap;

use futures::future::{self, Future};
use futures::stream::{self, Stream, StreamExt, TryStreamExt, FuturesUnordered};
use zbus::names::{BusName, ErrorName, UniqueName};
use zbus::zvariant::OwnedObjectPath;
use tokio::task::JoinSet;

use crate::dbus::{dprom::DProm1Proxy, gauge::Gauge1Proxy};
use crate::export::DbusOpt;
use crate::export::metric::Export;
use crate::export::context::{Ctx, BusCtx, PathCtx};

pub async fn run(log: slog::Logger, export: Export, opt: DbusOpt) -> anyhow::Result<()> {
    let export = Arc::new(export);

    let futures = FuturesUnordered::new();

    if opt.session {
        let log = log.new(slog::o!("dbus" => "session"));
        let conn = Arc::new(zbus::Connection::session().await?);
        let ctx = Ctx::new(log, conn, export.clone());
        futures.push(run_top(ctx));
    }

    if opt.system {
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

    let name_stream = dbus.receive_name_owner_changed().await?
        .filter_map(|signal| future::ready(NameEvent::from_signal(signal).transpose()));

    let existing_names = dbus.list_names().await?
        .into_iter()
        .filter_map(|name| match name.into_inner() {
            BusName::Unique(uniq) => Some(uniq),
            BusName::WellKnown(_) => None,
        })
        .map(NameEvent::Add)
        .map(Ok);

    let mut join_set = JoinSet::new();
    let mut abort_handles = HashMap::new();

    let name_stream = stream::iter(existing_names).chain(name_stream);

    futures::pin_mut!(name_stream);

    while let Some(event) = name_stream.next().await {
        match event? {
            NameEvent::Add(bus) => {
                let ctx = ctx.with_bus(bus.clone());

                // start the bus task
                let handle = join_set.spawn(async move {
                    let result = protect_unknown_dispatch(
                        run_bus(ctx.clone()).await);

                    if let Err(e) = result {
                        slog::error!(ctx.log, "bus error: {:?}", e);
                    }
                });

                let prev_handle = abort_handles.insert(bus, handle);
                if let Some(prev_handle) = prev_handle {
                    prev_handle.abort();
                }
            }
            NameEvent::Del(bus) => {
                if let Some(handle) = abort_handles.remove(&bus) {
                    handle.abort();
                }
            }
        }
    }

    Ok(())
}

async fn run_bus(ctx: BusCtx) -> zbus::Result<()> {
    let dprom = DProm1Proxy::builder(&ctx.conn)
        .destination(ctx.bus.clone())?
        .path("/org/hails/dprom")?
        .build()
        .await?;

    // setup receive stream first:
    let changes = dprom.receive_metrics_changed().await
        .then(|change| async move { change.get().await })
        .map_ok(|metric_paths| run_metrics(ctx.clone(), metric_paths));

    // start first metrics set:
    let init_fut = run_metrics(ctx.clone(),
        dprom.metrics().await?);

    // log the new bus at this point, since we will have errored and bailed already
    // if the bus is not a dprom bus
    slog::trace!(ctx.log, "watching bus");

    let run = future::poll_fn({
        let mut changes = Box::pin(changes);
        let mut fut_slot = Some(Box::pin(init_fut));

        move |cx| {
            match Pin::new(&mut changes).poll_next(cx) {
                // no changes, fall through to polling existing future:
                Poll::Pending => {}

                // there is new future to start polling instead:
                Poll::Ready(Some(Ok(fut))) => {
                    fut_slot = Some(Box::pin(fut));
                }

                // error reading update stream, bail out:
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Err(e))
                }

                // update stream finished, bail out:
                Poll::Ready(None) => {
                    return Poll::Ready(Ok(()));
                }
            }

            // poll current future if exists:
            if let Some(fut) = fut_slot.as_mut() {
                match fut.as_mut().poll(cx) {
                    // current future still running:
                    Poll::Pending => {}

                    // current future finished, clear slot and wait for new future from stream:
                    Poll::Ready(()) => {
                        fut_slot = None;
                    }
                }
            }

            Poll::Pending
        }
    });

    run.await
}

async fn run_metrics(ctx: BusCtx, paths: Vec<OwnedObjectPath>) {
    paths.into_iter()
        .map(|path| ctx.with_path(path))
        .map(|ctx| async move {
            slog::trace!(ctx.log, "watching metric");
            match run_metric(ctx.clone()).await {
                Ok(()) => {}
                Err(e) => {
                    slog::error!(ctx.log, "error watching metric: {:?}", e);
                }
            }
        })
        .collect::<FuturesUnordered<_>>()
        .collect::<()>()
        .await;
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
    let name = gauge.name().await?;

    // open stream before reading first value to avoid race
    let stream = gauge.receive_value_changed().await
        .then(|change| async move { change.get().await });

    let value = gauge.value().await?;

    let stream = stream::once(future::ready(Ok(value))).chain(stream);
    futures::pin_mut!(stream);

    let metric = ctx.export.metric(name);

    while let Some(result) = stream.next().await {
        metric.gauge(result?).await;
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
