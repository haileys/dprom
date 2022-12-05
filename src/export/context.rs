use std::sync::Arc;

use zbus::names::UniqueName;
use zbus::zvariant::OwnedObjectPath;

use crate::export::metric::Export;

#[derive(Clone)]
pub struct Ctx {
    pub log: slog::Logger,
    pub conn: Arc<zbus::Connection>,
    pub export: Arc<Export>,
}

impl Ctx {
    pub fn new(log: slog::Logger, conn: Arc<zbus::Connection>, export: Arc<Export>) -> Self {
        Ctx {
            log,
            conn,
            export,
        }
    }

    pub fn with_bus(&self, bus: UniqueName<'static>) -> BusCtx {
        BusCtx {
            log: self.log.new(slog::o!("bus" => bus.to_string())),
            conn: self.conn.clone(),
            export: self.export.clone(),
            bus,
        }
    }
}

#[derive(Clone)]
pub struct BusCtx {
    pub log: slog::Logger,
    pub conn: Arc<zbus::Connection>,
    pub export: Arc<Export>,
    pub bus: UniqueName<'static>,
}

impl BusCtx {
    pub fn with_path(&self, path: OwnedObjectPath) -> PathCtx {
        PathCtx {
            log: self.log.new(slog::o!("path" => path.to_string())),
            conn: self.conn.clone(),
            export: self.export.clone(),
            bus: self.bus.clone(),
            path,
        }
    }
}

#[derive(Clone)]
pub struct PathCtx {
    pub log: slog::Logger,
    pub conn: Arc<zbus::Connection>,
    pub export: Arc<Export>,
    pub bus: UniqueName<'static>,
    pub path: OwnedObjectPath,
}

impl PathCtx {
    pub async fn proxy<T: From<zbus::Proxy<'static>> + zbus::ProxyDefault>(&self) -> zbus::Result<T> {
        Ok(zbus::ProxyBuilder::new(&self.conn.clone())
            .destination(self.bus.clone())?
            .path(self.path.clone())?
            .build()
            .await?)
    }
}
