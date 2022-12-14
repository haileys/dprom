//! # DBus interface proxy for: `org.hails.dprom.Gauge1`
//!
//! This code was generated by `zbus-xmlgen` `3.0.0` from DBus introspection data.
//! Source: `org.hails.dprom.Gauge1.xml`.
//!
//! You may prefer to adapt it, instead of using it verbatim.
//!
//! More information can be found in the
//! [Writing a client proxy](https://dbus.pages.freedesktop.org/zbus/client.html)
//! section of the zbus documentation.
//!

use zbus::dbus_proxy;

#[dbus_proxy(interface = "org.hails.dprom.Gauge1")]
trait Gauge1 {
    /// Name property
    #[dbus_proxy(property)]
    fn name(&self) -> zbus::Result<String>;

    /// Value property
    #[dbus_proxy(property)]
    fn value(&self) -> zbus::Result<f64>;
}
