//! Detect virtual network adapters from VPN tools (Hamachi, Radmin VPN,
//! Tailscale, ZeroTier) so we can surface their IPs as fallback options
//! when port forwarding can't work (CGNAT, locked-down ISPs, etc.).
//!
//! We don't depend on any platform-specific crates — just shell out to
//! the OS's standard "list network interfaces" command and parse it.
//! On Windows that's `ipconfig`, on Unix it's `ifconfig` or `ip addr`.
//!
//! Each VPN tool is identified by either:
//!   - a well-known IPv4 prefix (Hamachi 25.x, Radmin 26.x, Tailscale 100.64-127.x),
//!   - an adapter description containing the tool's name.
//! We use both signals so we don't get fooled by e.g. a regular user
//! whose ISP happens to assign 25.x (rare, but possible).

use std::net::{IpAddr, Ipv4Addr};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VpnKind {
    Hamachi,
    Radmin,
    Tailscale,
    Zerotier,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnAdapter {
    pub kind: VpnKind,
    pub display_name: String,
    pub ip: IpAddr,
    /// The OS-level adapter description, useful for showing the user
    /// what we matched on.
    pub adapter_label: String,
}

/// Match an IPv4 against the well-known VPN prefixes.
fn classify_by_ip(ip: IpAddr) -> Option<VpnKind> {
    let IpAddr::V4(v4) = ip else { return None; };
    let o = v4.octets();
    match o[0] {
        25                                => Some(VpnKind::Hamachi),
        26                                => Some(VpnKind::Radmin),
        // Tailscale 100.64.0.0/10 — same range as CGNAT (RFC 6598). The
        // adapter-name check disambiguates real Tailscale from CGNAT WAN.
        100 if (o[1] & 0xC0) == 0x40      => Some(VpnKind::Tailscale),
        _                                 => None,
    }
}

/// Match by interface description (case-insensitive substring).
fn classify_by_name(label: &str) -> Option<VpnKind> {
    let l = label.to_ascii_lowercase();
    if l.contains("hamachi")        { return Some(VpnKind::Hamachi);   }
    if l.contains("radmin")         { return Some(VpnKind::Radmin);    }
    if l.contains("tailscale")      { return Some(VpnKind::Tailscale); }
    if l.contains("zerotier")       { return Some(VpnKind::Zerotier);  }
    None
}

fn display_for(kind: &VpnKind) -> &'static str {
    match kind {
        VpnKind::Hamachi   => "Hamachi (LogMeIn)",
        VpnKind::Radmin    => "Radmin VPN",
        VpnKind::Tailscale => "Tailscale",
        VpnKind::Zerotier  => "ZeroTier",
        VpnKind::Unknown   => "Unknown VPN",
    }
}

/// List virtual VPN adapters this machine has, with IPs. Empty = none
/// detected. We never error out — failure to read interfaces just
/// returns an empty list, since this is a non-critical helper.
pub fn detect_adapters() -> Vec<VpnAdapter> {
    let raw = match list_interfaces_raw() {
        Some(s) => s,
        None    => return Vec::new(),
    };
    parse_interfaces(&raw)
}

/// Run the platform-appropriate command and return its stdout as a String.
fn list_interfaces_raw() -> Option<String> {
    #[cfg(windows)]
    let out = Command::new("ipconfig").arg("/all").output().ok()?;

    #[cfg(not(windows))]
    let out = {
        // Try `ip addr` first (modern Linux), fall back to `ifconfig`.
        Command::new("ip").args(["addr"]).output()
            .or_else(|_| Command::new("ifconfig").output())
            .ok()?
    };

    if !out.status.success() { return None; }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse the output of `ipconfig /all` (Windows) or `ip addr` / `ifconfig`
/// (Unix). The format on each platform is slightly different but they
/// all have the same shape: blocks of text per adapter, each containing
/// a name/description and zero or more "IPv4 Address" lines.
///
/// Strategy: we walk the output line by line, tracking the "current
/// adapter description". Whenever we see an IPv4 line, we associate it
/// with the most recently seen description. Then we filter to only the
/// VPN-style ones at the end.
fn parse_interfaces(raw: &str) -> Vec<VpnAdapter> {
    let mut out: Vec<VpnAdapter> = Vec::new();
    let mut current_label = String::new();

    for line in raw.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() { continue; }

        // ── Windows ipconfig: blocks start with "Ethernet adapter X:" or
        //    "Unknown adapter Y:" at column 0. The next non-blank lines
        //    are indented properties.
        // ── Linux `ip addr`: lines like "3: tailscale0: <...>" at col 0,
        //    properties indented.
        // ── Unix ifconfig: similar - "tailscale0: flags=..." at col 0.
        let starts_in_col_zero = !line.starts_with(' ') && !line.starts_with('\t');
        if starts_in_col_zero {
            // New adapter block. Use the whole line as the label - it's
            // what the user will see when we tell them which VPN we
            // matched on.
            current_label = trimmed.to_string();
            continue;
        }

        // Description lines on Windows: "   Description . . . . . . . . . . . : Hamachi"
        let lc = trimmed.to_ascii_lowercase();
        if lc.contains("description") && lc.contains(':') {
            if let Some(rest) = trimmed.split(':').nth(1) {
                let desc = rest.trim();
                if !desc.is_empty() {
                    // Replace the current label with the more descriptive
                    // value if it looks more useful.
                    current_label = desc.to_string();
                }
            }
            continue;
        }

        // IPv4 line:
        //   Windows: "   IPv4 Address. . . . . . . . . . . : 25.x.y.z(Preferred)"
        //   Linux ip:"    inet 25.x.y.z/8 brd ... scope global tailscale0"
        //   ifconfig:"    inet 25.x.y.z netmask 255.0.0.0 ..."
        if lc.contains("ipv4 address") || lc.starts_with("inet ") || lc.contains("inet ") && !lc.contains("inet6") {
            if let Some(ip) = extract_ipv4(trimmed) {
                let kind = classify_by_ip(IpAddr::V4(ip))
                    .or_else(|| classify_by_name(&current_label));
                if let Some(kind) = kind {
                    out.push(VpnAdapter {
                        display_name: display_for(&kind).to_string(),
                        kind,
                        ip: IpAddr::V4(ip),
                        adapter_label: current_label.clone(),
                    });
                }
            }
        }
    }

    // Deduplicate (same IP can be reported by both ipconfig description
    // and ifconfig short name). Keep the first occurrence.
    let mut seen: std::collections::HashSet<IpAddr> = std::collections::HashSet::new();
    out.retain(|a| seen.insert(a.ip));
    out
}

/// Pull the first IPv4 dotted-quad from a string, ignoring trailing
/// suffixes like "(Preferred)" on Windows or "/24" on Linux.
fn extract_ipv4(s: &str) -> Option<Ipv4Addr> {
    let mut current = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() || c == '.' {
            current.push(c);
        } else {
            if let Ok(ip) = current.parse::<Ipv4Addr>() {
                return Some(ip);
            }
            current.clear();
        }
    }
    current.parse::<Ipv4Addr>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_v4_with_suffix() {
        assert_eq!(extract_ipv4("IPv4 Address. . . . : 25.1.2.3(Preferred)"),
                   Some("25.1.2.3".parse().unwrap()));
        assert_eq!(extract_ipv4("    inet 100.64.5.6/32 scope global"),
                   Some("100.64.5.6".parse().unwrap()));
    }

    #[test]
    fn classifies_hamachi() {
        assert_eq!(classify_by_ip("25.1.2.3".parse().unwrap()), Some(VpnKind::Hamachi));
        assert_eq!(classify_by_ip("26.1.2.3".parse().unwrap()), Some(VpnKind::Radmin));
        assert_eq!(classify_by_ip("100.64.0.5".parse().unwrap()), Some(VpnKind::Tailscale));
        assert_eq!(classify_by_ip("192.168.1.1".parse().unwrap()), None);
    }
}
