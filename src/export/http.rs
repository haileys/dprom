use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::{Arc, RwLock};

use futures::stream::{Stream, StreamExt};
use warp::Filter;

use crate::export::metric::{MetricName, MetricValue, Record};
use crate::export::config;

const UPDATE_CHUNK_SIZE: usize = 64; // chosen arbritrarily

pub async fn run(
    log: slog::Logger,
    metric_stream: impl Stream<Item = Record> + Send + 'static,
    config: config::Http,
) -> Result<(), anyhow::Error> {
    let live = Arc::new(LiveMetrics::new(log.clone(), metric_stream));

    let root = warp::path!().then(root);

    let metrics = warp::path!("metrics").then(move || {
        let live = live.clone();
        async move { metrics(&live).await }
    });

    let routes = warp::get().and(root.or(metrics));

    let server = warp::serve(routes);

    match config.tls {
        None => {
            server.run(config.listen).await;
        }
        Some(tls) => {
            let server = server
                .tls()
                .key_path(tls.key)
                .cert_path(tls.cert);

            match tls.verify {
                Some(verify) => {
                    let server = server
                        .client_auth_required_path(verify.ca);

                    server.run(config.listen).await;
                }
                None => {
                    server.run(config.listen).await;
                }
            }
        }
    }

    Ok(())
}

async fn root() -> impl warp::Reply {
    let version = env!("CARGO_PKG_VERSION");
    warp::reply::html(format!("<pre>dprom-export {version}\n\n<a href=\"/metrics\">/metrics</a>\n</pre>\n"))
}

async fn metrics(live: &LiveMetrics) -> String {
    let mut output = String::new();

    for (name, value) in live.read().iter() {
        let type_ = match value {
            MetricValue::Gauge(_) => "gauge"
        };

        let _ = write!(&mut output, "# TYPE {} {}\n", name, type_);
        let _ = write!(&mut output, "{} {}\n", name, value);
    }

    output
}

#[derive(Clone)]
struct LiveMetrics {
    map: Arc<RwLock<MetricMap>>,
}

// btree to keep it nicely sorted for output :)
type MetricMap = BTreeMap<MetricName, MetricValue>;

impl LiveMetrics {
    pub fn new(log: slog::Logger, stream: impl Stream<Item = Record> + Send + 'static) -> Self {
        let map = Arc::new(RwLock::new(MetricMap::default()));

        tokio::spawn({
            let map = map.clone();
            async move {
                let stream = stream.ready_chunks(UPDATE_CHUNK_SIZE);
                futures::pin_mut!(stream);

                while let Some(chunk) = stream.next().await {
                    receive_chunk(&map, chunk)
                }

                // should we restart here?
                slog::warn!(log, "metric stream stopped! metrics no longer live");
            }
        });

        LiveMetrics { map }
    }

    pub fn read(&self) -> std::sync::RwLockReadGuard<MetricMap> {
        self.map.read().unwrap()
    }
}

fn receive_chunk(map: &RwLock<MetricMap>, chunk: Vec<Record>) {
    let mut map = map.write().unwrap();

    for (name, value) in chunk {
        match value {
            Some(value) => { map.insert(name, value); }
            None => { map.remove(&name); }
        }
    }
}

