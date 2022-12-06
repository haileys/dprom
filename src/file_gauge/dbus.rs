use tokio::sync::watch;
use zbus::dbus_interface;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

use crate::file_gauge::config::GaugeName;
use crate::file_gauge::WatchValue;

pub struct DProm {
    pub metrics: Vec<ObjectPath<'static>>,
}

#[dbus_interface(name = "org.hails.dprom.DProm1")]
impl DProm {
    #[dbus_interface(property)]
    pub fn metrics(&self) -> &[ObjectPath<'static>] {
        &self.metrics
    }
}

pub struct Gauge {
    pub name: GaugeName,
    pub watch: watch::Receiver<WatchValue>,
}

#[dbus_interface(name = "org.hails.dprom.Gauge1")]
impl Gauge {
    #[dbus_interface(property)]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    #[dbus_interface(property)]
    pub fn value(&self) -> f64 {
        self.watch.borrow().clone().unwrap_or_else(|| {
            0.0
        })
    }
}

impl Gauge {
    pub fn object_path(&self) -> ObjectPath<'static> {
        gauge_path(&self.name)
    }

    pub fn watch(&self) -> &watch::Receiver<WatchValue> {
        &self.watch
    }
}

pub fn gauge_path(name: &GaugeName) -> ObjectPath<'static> {
    OwnedObjectPath::try_from(format!("/org/hails/dprom/gauge/{}", name.as_str()))
        // validity is ensured by config::GaugeName:
        .unwrap()
        .into_inner()
}
