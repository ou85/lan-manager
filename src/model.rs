use anyhow::{Result, bail};
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, net::Ipv4Addr};

pub const SCHEMA_VERSION: u32 = 1;
#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password_hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub model: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub subnet_id: String,
    #[serde(default)]
    pub firmware: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub due: String,
    pub status: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub notes: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subnet {
    pub id: String,
    pub name: String,
    pub cidr: String,
    #[serde(default)]
    pub gateway: String,
    #[serde(default)]
    pub vlan: Option<u16>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServicePort {
    pub id: String,
    pub port: u16,
    pub protocol: String,
    pub host: String,
    pub service: String,
    pub access: String,
    #[serde(default)]
    pub url: String,
    pub status: String,
    #[serde(default)]
    pub notes: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub credentials: Credentials,
    pub devices: Vec<Device>,
    pub subnets: Vec<Subnet>,
    #[serde(default)]
    pub ports: Vec<ServicePort>,
}
#[derive(Serialize)]
pub struct Inventory {
    pub devices: Vec<Device>,
    pub subnets: Vec<Subnet>,
    pub ports: Vec<ServicePort>,
}
impl From<Snapshot> for Inventory {
    fn from(s: Snapshot) -> Self {
        Self {
            devices: s.devices,
            subnets: s.subnets,
            ports: s.ports,
        }
    }
}
pub fn valid_id(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok()
}
pub fn usable(net: Ipv4Net, ip: Ipv4Addr) -> bool {
    net.contains(&ip) && (net.prefix_len() >= 31 || (ip != net.network() && ip != net.broadcast()))
}
pub fn validate(snapshot: &Snapshot) -> Result<()> {
    if snapshot.schema_version != SCHEMA_VERSION {
        bail!("Unsupported database schema version");
    }
    if snapshot.devices.len() > 10_000
        || snapshot.subnets.len() > 1_000
        || snapshot.ports.len() > 10_000
    {
        bail!("Inventory limit reached");
    }
    let mut ids = HashSet::new();
    let mut nets = Vec::new();
    for s in &snapshot.subnets {
        if !valid_id(&s.id) || !ids.insert(s.id.clone()) {
            bail!("Invalid or duplicate subnet ID");
        }
        if s.name.trim().is_empty() || s.name.len() > 100 {
            bail!("Subnet name must contain 1–100 bytes");
        }
        let net: Ipv4Net = s
            .cidr
            .parse()
            .map_err(|_| anyhow::anyhow!("Enter a valid IPv4 CIDR"))?;
        if net.addr() != net.network() {
            bail!("Use the network address in CIDR, e.g. {}", net.trunc());
        }
        if nets
            .iter()
            .any(|n: &Ipv4Net| n.contains(&net.network()) || net.contains(&n.network()))
        {
            bail!("Subnets must not overlap");
        }
        if !s.gateway.is_empty() {
            let ip: Ipv4Addr = s
                .gateway
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid gateway address"))?;
            if !usable(net, ip) {
                bail!("Gateway must be a usable address in its subnet");
            }
        }
        if s.vlan.is_some_and(|v| !(1..=4094).contains(&v)) {
            bail!("VLAN must be between 1 and 4094");
        }
        nets.push(net);
    }
    let mut ips = HashSet::new();
    ids.clear();
    for d in &snapshot.devices {
        if !valid_id(&d.id) || !ids.insert(d.id.clone()) {
            bail!("Invalid or duplicate device ID");
        }
        if d.name.trim().is_empty() || d.name.len() > 100 {
            bail!("Device name must contain 1–100 bytes");
        }
        if ![
            "Router",
            "Switch",
            "Server",
            "Storage",
            "Access point",
            "Computer",
        ]
        .contains(&d.kind.as_str())
        {
            bail!("Invalid device type");
        }
        if !["In service", "Powered off", "Maintenance"].contains(&d.status.as_str()) {
            bail!("Invalid device status");
        }
        if [&d.model, &d.location, &d.firmware, &d.target]
            .iter()
            .any(|s| s.len() > 200)
            || d.notes.len() > 4000
        {
            bail!("A field exceeds its maximum length");
        }
        if !d.due.is_empty()
            && (d.due.len() != 10 || chrono::NaiveDate::parse_from_str(&d.due, "%Y-%m-%d").is_err())
        {
            bail!("Invalid reminder date");
        }
        let ip = if d.ip.is_empty() {
            None
        } else {
            let ip: Ipv4Addr =
                d.ip.parse()
                    .map_err(|_| anyhow::anyhow!("Invalid IPv4 address"))?;
            if !ips.insert(ip) {
                bail!("This IP address is already assigned");
            }
            Some(ip)
        };
        if !d.subnet_id.is_empty() {
            let subnet = snapshot
                .subnets
                .iter()
                .find(|s| s.id == d.subnet_id)
                .ok_or_else(|| anyhow::anyhow!("Subnet not found"))?;
            let net: Ipv4Net = subnet.cidr.parse()?;
            if let Some(ip) = ip
                && !usable(net, ip)
            {
                bail!("IP address is not a usable host in the selected subnet");
            }
        }
        let mut current = d;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(&current.id) {
                bail!("This connection creates a network loop");
            }
            if current.parent.is_empty() {
                break;
            }
            current = snapshot
                .devices
                .iter()
                .find(|x| x.id == current.parent)
                .ok_or_else(|| anyhow::anyhow!("Parent device not found"))?;
        }
    }
    ids.clear();
    let mut bindings = HashSet::new();
    for p in &snapshot.ports {
        if !valid_id(&p.id) || !ids.insert(p.id.clone()) {
            bail!("Invalid or duplicate port ID");
        }
        if !["TCP", "UDP"].contains(&p.protocol.as_str()) {
            bail!("Protocol must be TCP or UDP");
        }
        if !["Localhost", "LAN", "Tailscale", "Internet"].contains(&p.access.as_str()) {
            bail!("Invalid access level");
        }
        if !["Active", "Planned", "Disabled"].contains(&p.status.as_str()) {
            bail!("Invalid port status");
        }
        if p.host.trim().is_empty()
            || p.host.len() > 100
            || p.service.trim().is_empty()
            || p.service.len() > 100
        {
            bail!("Host and service must contain 1–100 bytes");
        }
        if p.url.len() > 500 || p.notes.len() > 4000 {
            bail!("A field exceeds its maximum length");
        }
        if !bindings.insert((
            p.host.trim().to_ascii_lowercase(),
            p.port,
            p.protocol.clone(),
        )) {
            bail!("This host, port, and protocol are already recorded");
        }
    }
    Ok(())
}
