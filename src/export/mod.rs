pub mod context;
pub mod dbus;
pub mod metric;

use futures::stream::{self, StreamExt};

enum Event {
    Dbus(Result<(), zbus::Error>),
    Metric(metric::Record),
}

pub async fn run(log: slog::Logger) -> anyhow::Result<()> {
    let (export, metric_stream) = metric::Export::new();
    let dbus_stream = stream::once(dbus::run(log.clone(), export));

    let events = stream::select(
        dbus_stream.map(Event::Dbus),
        metric_stream.map(Event::Metric),
    );

    futures::pin_mut!(events);

    while let Some(event) = events.next().await {
        match event {
            Event::Dbus(result) => { result?; }
            Event::Metric((name, value)) => {
                slog::info!(log, "{} = {:?}", name, value);
            }
        }
    }

    Ok(())
}
