use std::path::Path;
use serde_derive::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub dbus: Dbus,
    pub http: Http,
}

#[derive(Deserialize)]
pub struct Dbus {
    #[serde(default = "bool_true")]
    pub system: bool,
    #[serde(default = "bool_false")]
    pub session: bool,
}

fn bool_true() -> bool {
    true
}

fn bool_false() -> bool {
    false
}

#[derive(Deserialize)]
pub struct Http {
    pub listen: std::net::SocketAddr,
    pub tls: Option<Tls>,
}

#[derive(Deserialize)]
pub struct Tls {
    pub cert: std::path::PathBuf,
    pub key: std::path::PathBuf,
    pub verify: Option<Verify>,
}

#[derive(Deserialize)]
pub struct Verify {
    pub ca: std::path::PathBuf,
}

pub async fn open(path: &Path) -> Result<Config, anyhow::Error> {
    let toml = tokio::fs::read_to_string(path).await?;
    Ok(toml::from_str(&toml)?)
}
