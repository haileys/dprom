use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};

use derive_more::Display;
use futures::stream::{self, Stream, StreamExt};
use tokio::sync::mpsc;
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};

const MPSC_BUFFER: usize = 50;

pub type Record = (MetricName, Option<MetricValue>);

pub struct Export {
    shared: Mutex<ExportShared>,
    gone_tx: mpsc::UnboundedSender<MetricName>,
    record_tx: mpsc::Sender<Record>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Display)]
pub struct MetricName(Arc<String>);

impl<T> From<T> for MetricName where T: Into<String> {
    fn from(x: T) -> Self {
        MetricName(Arc::new(x.into()))
    }
}

#[derive(Clone, Debug)]
pub enum MetricValue {
    Gauge(f64),
}

struct ExportShared {
    uniqs: HashMap<MetricName, NonZeroU64>,
    serial: NonZeroU64,
}

impl ExportShared {
    fn get_uniq(&self, name: &MetricName) -> Option<NonZeroU64> {
        self.uniqs.get(name).copied()
    }

    /// returns true if this uniq was the current owner
    fn remove_uniq(&mut self, name: &MetricName, uniq: NonZeroU64) -> bool {
        if self.get_uniq(name) == Some(uniq) {
            self.uniqs.remove(name);
            true
        } else {
            false
        }
    }
}

impl Default for ExportShared {
    fn default() -> Self {
        ExportShared {
            uniqs: HashMap::new(),
            serial: NonZeroU64::new(1).unwrap(),
        }
    }
}

pub struct MetricHandle<'a> {
    export: &'a Export,
    uniq: NonZeroU64,
    name: MetricName,
}

impl Export {
    pub fn new() -> (Self, impl Stream<Item = Record>) {
        let (record_tx, record_rx) = mpsc::channel(MPSC_BUFFER);
        let (gone_tx, gone_rx) = mpsc::unbounded_channel();

        let export = Export {
            shared: Default::default(),
            record_tx,
            gone_tx,
        };

        let record_stream = ReceiverStream::new(record_rx);
        let gone_stream = UnboundedReceiverStream::new(gone_rx)
            .map(|name| (name, None));

        (export, stream::select(gone_stream, record_stream))
    }

    pub fn metric(&self, name: impl Into<MetricName>) -> MetricHandle<'_> {
        let mut shared = self.shared.lock().unwrap();
        let name = name.into();
        let uniq = next(&mut shared.serial);

        let prev = shared.uniqs.insert(name.clone(), uniq);

        if prev.is_some() {
            // TODO - log that we're re-registering a metric name
        }

        MetricHandle {
            export: self,
            uniq,
            name,
        }
    }
}

impl<'a> MetricHandle<'a> {
    pub async fn gauge(&self, value: f64) {
        self.measure(MetricValue::Gauge(value)).await;
    }

    pub async fn measure(&self, value: MetricValue) {
        let record = {
            let shared = self.export.shared.lock().unwrap();

            if shared.get_uniq(&self.name) == Some(self.uniq) {
                let name = self.name.clone();
                Some((name, Some(value)))
            } else {
                None
            }
        };

        if let Some(record) = record {
            let _ = self.export.record_tx.send(record).await;
        }
    }
}

impl<'a> Drop for MetricHandle<'a> {
    fn drop(&mut self) {
        let mut shared = self.export.shared.lock().unwrap();

        if shared.remove_uniq(&self.name, self.uniq) {
            let name = self.name.clone();

            // only fails if nobody's listening, which is fine:
            let _ = self.export.gone_tx.send(name);
        }
    }
}

fn next(serial: &mut NonZeroU64) -> NonZeroU64 {
    let val = *serial;
    *serial = serial.checked_add(1).unwrap();
    val
}
