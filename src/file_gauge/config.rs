use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde::de;

#[derive(Deserialize)]
pub struct Config {
    pub watch: Watch,
    pub gauges: Gauges,
}

#[derive(Deserialize)]
pub struct Watch {
    #[serde(deserialize_with = "parse_duration", default = "default_refresh_secs")]
    pub refresh_secs: Duration,
}

#[derive(Deserialize)]
pub struct Gauges(pub HashMap<GaugeName, PathBuf>);

#[derive(Deserialize, Hash, PartialEq, Eq)]
#[serde(transparent)]
pub struct GaugeName(#[serde(deserialize_with = "parse_gauge_name")] String);

impl GaugeName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

const fn default_refresh_secs() -> Duration {
    Duration::from_secs(5)
}

fn parse_duration<'de, D>(d: D) -> Result<Duration, D::Error>
    where D: de::Deserializer<'de>
{
    let duration = f64::deserialize(d)?;

    // see https://dbus.freedesktop.org/doc/dbus-specification.html#message-protocol-marshaling-object-path
    if duration >= 0.0 {
        Ok(Duration::from_secs_f64(duration))
    } else {
        Err(de::Error::invalid_value(de::Unexpected::Float(duration), &"duration must be positive"))
    }
}

fn parse_gauge_name<'de, D>(d: D) -> Result<String, D::Error>
    where D: de::Deserializer<'de>
{
    let name = String::deserialize(d)?;

    // see https://dbus.freedesktop.org/doc/dbus-specification.html#message-protocol-marshaling-object-path
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(name)
    } else {
        Err(de::Error::invalid_value(de::Unexpected::Str(&name), &"metric name may only use chars [A-Za-z0-9_]"))
    }
}

pub async fn open(path: &Path) -> Result<Config, anyhow::Error> {
    let toml = tokio::fs::read_to_string(path).await?;
    Ok(toml::from_str(&toml)?)
}
