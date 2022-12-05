use std::path::{Path, PathBuf};
use std::time::Duration;

use structopt::StructOpt;
use tokio::sync::watch;
use zbus::dbus_interface;
use zbus::zvariant::ObjectPath;

type WatchValue = Result<f64, String>;

fn file_watch(path: PathBuf) -> watch::Receiver<WatchValue> {
    // TODO - use inotify instead:
    const REFRESH_DURATION: Duration = Duration::from_secs(5);

    let (tx, rx) = watch::channel(Err(
        format!("{}: not yet ready", path.to_string_lossy())));

    tokio::spawn(async move {
        loop {
            match read(&path).await {
                Ok(value) => {
                    match tx.send(Ok(value)) {
                        Ok(()) => {}
                        Err(_) => {
                            // other side closed
                            break;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("failed to read {}: {:?}", path.to_string_lossy(), e);
                }
            }

            tokio::time::sleep(REFRESH_DURATION).await;
        }

        async fn read(path: &Path) -> WatchValue {
            let value = tokio::fs::read_to_string(path)
                .await
                .map_err(|e| format!("{}: {:?}", path.to_string_lossy(), e))?
                .trim()
                .parse()
                .map_err(|e| format!("{}: {:?}", path.to_string_lossy(), e))?;

            Ok(value)
        }
    });

    rx
}

struct DProm {
    metrics: Vec<ObjectPath<'static>>,
}

#[dbus_interface(name = "org.hails.dprom.DProm1")]
impl DProm {
    #[dbus_interface(property)]
    fn metrics(&self) -> &[ObjectPath<'static>] {
        &self.metrics
    }
}

struct FileGauge {
    name: String,
    watch: watch::Receiver<WatchValue>,
}

#[dbus_interface(name = "org.hails.dprom.Gauge1")]
impl FileGauge {
    #[dbus_interface(property)]
    fn name(&self) -> &str {
        &self.name
    }

    #[dbus_interface(property)]
    fn value(&self) -> f64 {
        self.watch.borrow().clone().unwrap_or_else(|e| {
            eprintln!("failed to read gauge value: {}", e);
            0.0
        })
    }
}

#[derive(StructOpt, Debug)]
struct Opt {
    #[structopt(long)]
    name: String,
    #[structopt(long)]
    file: PathBuf,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::from_args();
    let mut watch = file_watch(opt.file);
    let object_path = ObjectPath::try_from(format!("/org/hails/dprom/gauge/{}", opt.name))?;

    let connection = zbus::ConnectionBuilder::session()?
        .serve_at("/org/hails/dprom", DProm {
            metrics: vec![object_path.clone()],
        })?
        .serve_at(object_path.clone(), FileGauge {
            name: opt.name,
            watch: watch.clone(),
        })?
        .build()
        .await?;

    let interface = connection
        .object_server()
        .interface::<_, FileGauge>(object_path)
        .await?;

    loop {
        watch.changed().await?;

        let signal_context = interface.signal_context();
        interface.get().await
            .value_changed(signal_context)
            .await?;
    }
}
