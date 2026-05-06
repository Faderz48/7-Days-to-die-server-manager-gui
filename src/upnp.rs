//! UPnP port forwarding for the 7 Days to Die server.
//!
//! What this does, end-to-end:
//!   1. Reads the configured `ServerPort` from `serverconfig.xml`.
//!   2. SSDP-discovers the local UPnP gateway (your router) on the LAN.
//!   3. Asks the router to forward UDP+TCP {port, port+1, port+2} from
//!      WAN → this machine. (7DTD uses ServerPort + the next two for
//!      Steam protocol overhead.)
//!   4. Asks the router for its WAN-side public IP.
//!   5. Compares the public IP to the well-known CGNAT ranges
//!      (RFC 6598 100.64/10) and the reserved/private ranges. If the
//!      router itself doesn't have a real public address, we report
//!      back so the UI can warn — UPnP "succeeded" but you still won't
//!      be reachable from the internet.
//!
//! Failure modes we surface clearly:
//!   - "no UPnP gateway found"        → router has IGD/UPnP disabled, or
//!                                       OS firewall blocks SSDP, or
//!                                       you're on Wi-Fi guest isolation.
//!   - "router refused mapping"       → router supports UPnP but admin
//!                                       has disabled it (common on ISP
//!                                       boxes).
//!   - "behind CGNAT"                 → ISP gives you a private WAN IP.
//!                                       Forwarding works internally but
//!                                       outside-world traffic never
//!                                       reaches your router.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use igd_next::{aio::tokio::search_gateway, PortMappingProtocol, SearchOptions};
use serde::{Deserialize, Serialize};

/// Default lease in seconds. 7 days. Routers may clamp or ignore.
const LEASE_SECONDS: u32 = 7 * 24 * 3600;

/// 7DTD reserves the configured ServerPort plus the next two ports for
/// the Steam side-channel protocol. We forward all three.
const PORT_OFFSETS: &[u16] = &[0, 1, 2];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardResult {
    pub mapped_ports: Vec<u16>,
    pub public_ip: Option<IpAddr>,
    pub local_ip: Option<IpAddr>,
    pub cgnat: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmapResult {
    pub removed_ports: Vec<u16>,
    pub notes: Vec<String>,
}

/// Find which local IPv4 address the OS would use to reach the gateway.
/// We use this as the "internal client" address in the port mapping.
fn pick_local_ip(gateway_ip: IpAddr) -> Result<Ipv4Addr> {
    let IpAddr::V4(target) = gateway_ip else {
        return Err(anyhow!("UPnP gateway is IPv6 ({gateway_ip}); we only forward IPv4 here"));
    };
    // Trick: open a UDP socket "connected" to the gateway. The OS picks
    // the right outbound interface for us, and we read the local addr
    // off the socket without sending a single packet.
    let sock = std::net::UdpSocket::bind("0.0.0.0:0")
        .context("could not bind a probe socket")?;
    sock.connect(SocketAddrV4::new(target, 9))
        .context("could not connect probe socket to gateway")?;
    match sock.local_addr()?.ip() {
        IpAddr::V4(v4) => Ok(v4),
        IpAddr::V6(_)  => Err(anyhow!("OS gave us an IPv6 source for the gateway probe")),
    }
}

/// CGNAT (Carrier-Grade NAT) detection per RFC 6598. If your router's
/// own WAN IP is in `100.64.0.0/10`, your ISP is double-NATting you and
/// no amount of port forwarding on your router will let outside players
/// reach you. T-Mobile Home Internet, Starlink, and many cellular ISPs
/// do this by default.
fn is_cgnat(ip: IpAddr) -> bool {
    let IpAddr::V4(v4) = ip else { return false; };
    let octets = v4.octets();
    octets[0] == 100 && (octets[1] & 0b1100_0000) == 0b0100_0000 // 100.64.0.0/10
}

/// Returns true if `ip` is *not* a routable public IPv4 address.
fn is_non_public(ip: IpAddr) -> bool {
    let IpAddr::V4(v4) = ip else { return true; };
    v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified()
        || is_cgnat(ip)
}

/// Query the router's WAN-side IP address via UPnP without making any
/// changes (no port mappings added). Used by the "connection info" card
/// to show users their public IP without forcing them to click
/// auto-forward first. Returns None if there's no UPnP gateway.
pub async fn query_public_ip() -> Option<IpAddr> {
    let opts = SearchOptions {
        timeout: Some(Duration::from_secs(3)),
        ..Default::default()
    };
    let gateway = search_gateway(opts).await.ok()?;
    gateway.get_external_ip().await.ok()
}

/// Resolve which local IPv4 address this machine would use for outbound
/// internet traffic. Useful for the "connect via LAN" hint. We don't
/// hit the network — the trick is to UDP-connect to a routable address
/// and read the local addr the OS picked.
pub fn detect_lan_ip() -> Option<IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // 1.1.1.1:53 is a stable, well-known anycast — we never actually
    // send any packets. The connect() just makes the OS pick a route.
    sock.connect("1.1.1.1:53").ok()?;
    sock.local_addr().ok().map(|a| a.ip())
}

/// Forward ServerPort..ServerPort+2 (TCP and UDP) on the LAN gateway.
pub async fn forward(server_port: u16) -> Result<ForwardResult> {
    let opts = SearchOptions {
        timeout: Some(Duration::from_secs(4)),
        ..Default::default()
    };
    let gateway = search_gateway(opts).await
        .map_err(|e| anyhow!("no UPnP gateway found on the LAN: {e}. \
                              Make sure UPnP / IGD is enabled on your router."))?;

    let gateway_addr = gateway.addr.ip();
    let local = pick_local_ip(gateway_addr)
        .context("could not determine this machine's LAN IP")?;

    let mut mapped_ports = Vec::new();
    let mut notes = Vec::new();

    for off in PORT_OFFSETS {
        let port = match server_port.checked_add(*off) {
            Some(p) => p,
            None => {
                notes.push(format!("skipped port {server_port}+{off}: would overflow u16"));
                continue;
            }
        };
        for proto in [PortMappingProtocol::TCP, PortMappingProtocol::UDP] {
            let local_addr = SocketAddr::V4(SocketAddrV4::new(local, port));
            let res = gateway
                .add_port(proto, port, local_addr, LEASE_SECONDS, "7DTD Server Manager")
                .await;
            match res {
                Ok(_) => {}
                Err(e) => {
                    // Some old IGDv1 routers reject any non-zero lease. The
                    // canonical error message contains "OnlyPermanentLeases".
                    // Try once more with lease=0 (permanent).
                    let msg = e.to_string();
                    if msg.contains("OnlyPermanentLeases") || msg.contains("Permanent") {
                        if let Err(e2) = gateway
                            .add_port(proto, port, local_addr, 0, "7DTD Server Manager")
                            .await
                        {
                            notes.push(format!("could not map {proto:?} {port}: {e2}"));
                        }
                    } else {
                        notes.push(format!("could not map {proto:?} {port}: {e}"));
                    }
                }
            }
        }
        mapped_ports.push(port);
    }

    if mapped_ports.is_empty() {
        return Err(anyhow!("the router refused all port mappings — \
                            UPnP is probably disabled in its admin panel"));
    }

    // Ask the router for its public IP — used to warn about CGNAT.
    let public_ip = gateway.get_external_ip().await.ok();
    let cgnat = public_ip.map(is_non_public).unwrap_or(false);
    if cgnat {
        notes.push(
            "your router's WAN IP is private/CGNAT — outside players can't reach you \
             even with forwarding. Common with T-Mobile Home, Starlink, and some \
             cellular ISPs. Solutions: ask your ISP for a public IP, or use a tunnel \
             like Tailscale, ZeroTier, or Cloudflare Tunnel."
                .into(),
        );
    }

    Ok(ForwardResult {
        mapped_ports,
        public_ip,
        local_ip: Some(IpAddr::V4(local)),
        cgnat,
        notes,
    })
}

/// Remove the previously-added mappings. Best-effort.
pub async fn unmap(server_port: u16) -> Result<UnmapResult> {
    let opts = SearchOptions {
        timeout: Some(Duration::from_secs(4)),
        ..Default::default()
    };
    let gateway = search_gateway(opts).await
        .map_err(|e| anyhow!("no UPnP gateway found on the LAN: {e}"))?;

    let mut removed = Vec::new();
    let mut notes = Vec::new();

    for off in PORT_OFFSETS {
        let Some(port) = server_port.checked_add(*off) else { continue; };
        let mut any_removed = false;
        for proto in [PortMappingProtocol::TCP, PortMappingProtocol::UDP] {
            match gateway.remove_port(proto, port).await {
                Ok(_)  => any_removed = true,
                Err(e) => notes.push(format!("could not remove {proto:?} {port}: {e}")),
            }
        }
        if any_removed { removed.push(port); }
    }

    Ok(UnmapResult { removed_ports: removed, notes })
}
