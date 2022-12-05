//! # DBus interface proxy for: `org.hails.dprom.DProm1`
//!
//! This code was generated by `zbus-xmlgen` `3.0.0` from DBus introspection data.
//! Source: `org.hails.dprom.DProm1.xml`.
//!
//! You may prefer to adapt it, instead of using it verbatim.
//!
//! More information can be found in the
//! [Writing a client proxy](https://dbus.pages.freedesktop.org/zbus/client.html)
//! section of the zbus documentation.
//!

use zbus::dbus_proxy;

#[dbus_proxy(interface = "org.hails.dprom.DProm1")]
trait DProm1 {
    /// Metrics property
    #[dbus_proxy(property)]
    fn metrics(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;
}
