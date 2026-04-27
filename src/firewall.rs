//! Windows Firewall rule management.
//!
//! 7 Days to Die uses TCP+UDP on `ServerPort`, `ServerPort+1`, and
//! `ServerPort+2`. Even with the router forwarding those ports, Windows
//! Firewall on the host will drop inbound packets unless we add rules
//! to allow them. This module handles that:
//!
//!   - `add_rules(port)` — creates Allow-Inbound rules for all six
//!     port/proto combinations, scoped to public+private profiles,
//!     using `netsh advfirewall`.
//!   - `remove_rules()` — deletes everything we added (matched by name).
//!   - `list_rules()` — best-effort listing of our rules so the UI can
//!     show "✓ allowed" status.
//!
//! All rules are named with a fixed prefix so we can safely add/remove
//! the whole set without touching anything the user (or another tool)
//! configured manually.
//!
//! Elevation: `netsh advfirewall firewall add rule` requires admin.
//! The caller surfaces UAC failures back to the user.
//!
//! Linux: `iptables` / `nftables` policies vary so wildly between
//! distros (and most desktop installs leave INPUT wide open by default)
//! that auto-managing rules causes more confusion than it solves. We
//! return a friendly "no firewall management on this OS" instead.

use serde::{Deserialize, Serialize};

const RULE_PREFIX: &str = "7DTD Server Manager";

/// 7DTD reserves ServerPort and the next two ports for the Steam
/// side-channel. Same set the UPnP module forwards.
const PORT_OFFSETS: &[u16] = &[0, 1, 2];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleResult {
    /// Ports we successfully added rules for.
    pub added_ports: Vec<u16>,
    /// Ports we successfully removed rules for.
    pub removed_ports: Vec<u16>,
    /// Free-text per-rule errors (one per failed netsh invocation).
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleStatus {
    /// True if we found at least one of our rules already in place.
    pub any_present: bool,
    /// Names of our rules that exist.
    pub present: Vec<String>,
    /// True on platforms where we don't manage firewall rules.
    pub unsupported: bool,
}

/// Build the rule name for a given port/protocol. We embed both so an
/// admin can see what's what in `wf.msc` without guessing.
fn rule_name(port: u16, proto: &str) -> String {
    format!("{RULE_PREFIX} ({proto} {port})")
}

// ─── Windows ────────────────────────────────────────────────────────────

#[cfg(windows)]
pub fn add_rules(server_port: u16) -> RuleResult {
    let mut added = Vec::new();
    let mut notes = Vec::new();

    for off in PORT_OFFSETS {
        let port = match server_port.checked_add(*off) {
            Some(p) => p,
            None => {
                notes.push(format!("skipped {server_port}+{off}: port overflow"));
                continue;
            }
        };
        let mut ok_for_port = true;
        for proto in ["TCP", "UDP"] {
            if let Err(e) = netsh_add_rule(port, proto) {
                notes.push(format!("could not add {proto} {port}: {e}"));
                ok_for_port = false;
            }
        }
        if ok_for_port { added.push(port); }
    }
    RuleResult { added_ports: added, removed_ports: Vec::new(), notes }
}

#[cfg(windows)]
pub fn remove_rules() -> RuleResult {
    let mut removed_ports = std::collections::BTreeSet::new();
    let mut notes = Vec::new();

    // We don't know which ports were originally added (the user may have
    // changed ServerPort since), so we delete by name prefix. netsh
    // supports `delete rule name="..."` exactly — there's no wildcard,
    // so we iterate the rules we know about.
    let names = list_our_rule_names().unwrap_or_default();
    for name in &names {
        match netsh_delete_rule_by_name(name) {
            Ok(()) => {
                if let Some(port) = parse_port_from_rule_name(name) {
                    removed_ports.insert(port);
                }
            }
            Err(e) => notes.push(format!("could not delete '{name}': {e}")),
        }
    }
    RuleResult {
        added_ports: Vec::new(),
        removed_ports: removed_ports.into_iter().collect(),
        notes,
    }
}

#[cfg(windows)]
pub fn status() -> RuleStatus {
    match list_our_rule_names() {
        Ok(names) => RuleStatus {
            any_present: !names.is_empty(),
            present: names,
            unsupported: false,
        },
        Err(_) => RuleStatus { any_present: false, present: Vec::new(), unsupported: false },
    }
}

#[cfg(windows)]
fn netsh_add_rule(port: u16, proto: &str) -> Result<(), String> {
    let name = rule_name(port, proto);
    // Idempotency: delete any existing rule with the same name first so
    // re-runs don't pile up duplicates.
    let _ = netsh_delete_rule_by_name(&name);
    run_netsh(&[
        "advfirewall", "firewall", "add", "rule",
        &format!("name={name}"),
        "dir=in",
        "action=allow",
        &format!("protocol={proto}"),
        &format!("localport={port}"),
        "profile=private,public",
        "enable=yes",
        "description=Auto-added by 7DTD Server Manager. Safe to remove.",
    ])
}

#[cfg(windows)]
fn netsh_delete_rule_by_name(name: &str) -> Result<(), String> {
    run_netsh(&[
        "advfirewall", "firewall", "delete", "rule",
        &format!("name={name}"),
    ])
}

/// Run `netsh` with the given args. Returns Ok on rc=0, otherwise the
/// captured stderr/stdout combined.
#[cfg(windows)]
fn run_netsh(args: &[&str]) -> Result<(), String> {
    use std::process::Command;

    // CREATE_NO_WINDOW = 0x08000000. Without this, every netsh call pops
    // a black console window for a frame, which is jarring as hell in a
    // GUI-fronted tool.
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let out = Command::new("netsh")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not invoke netsh: {e}"))?;

    if out.status.success() {
        // netsh emits "Ok." on stdout for successful add/delete. Don't
        // bother surfacing it.
        return Ok(());
    }

    // netsh reports "The requested operation requires elevation." (rc=1)
    // when not run as admin. Surface that with a clearer message.
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let combined = format!("{stdout}{stderr}").trim().to_string();
    if combined.contains("requires elevation") || combined.contains("Access is denied") {
        Err("administrator privileges required — relaunch the manager as administrator \
             (right-click the .exe → Run as administrator)".into())
    } else if combined.contains("No rules match the specified criteria") {
        // Treat "delete: not found" as success — we're idempotent.
        Ok(())
    } else if combined.is_empty() {
        Err(format!("netsh exited with status {}", out.status))
    } else {
        Err(combined)
    }
}

/// Run `netsh advfirewall firewall show rule name=all` and return the
/// names of rules that match our prefix.
#[cfg(windows)]
fn list_our_rule_names() -> Result<Vec<String>, String> {
    use std::process::Command;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let out = Command::new("netsh")
        .args(["advfirewall", "firewall", "show", "rule", "name=all"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not invoke netsh: {e}"))?;

    if !out.status.success() {
        return Err(format!("netsh show exited with status {}", out.status));
    }

    // netsh output is line-based, with rule blocks like:
    //     Rule Name:      7DTD Server Manager (TCP 26900)
    //     ----------------------------------------------------------------------
    //     Enabled:        Yes
    //     ...
    //
    // We only care about the "Rule Name:" lines whose value starts with
    // our prefix. Some Windows locales translate "Rule Name:" — we
    // also accept the localized German "Regelname:" / French "Nom de la règle :"
    // but the easiest portable fix is to also match by rule name *content*:
    // any line whose trimmed text starts with our prefix is a name line.
    let text = String::from_utf8_lossy(&out.stdout);
    let mut names = Vec::new();

    for line in text.lines() {
        // Look for lines containing our prefix. Either form works:
        //   "Rule Name:                            7DTD Server Manager (...)"
        //   "Regelname:                            7DTD Server Manager (...)"
        if let Some(idx) = line.find(RULE_PREFIX) {
            // Take from the prefix to end-of-line as the rule name.
            let name = line[idx..].trim().to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    Ok(names)
}

/// Pull the port number back out of "7DTD Server Manager (TCP 26900)"
/// for reporting. Returns None if the format doesn't match.
#[cfg(windows)]
fn parse_port_from_rule_name(name: &str) -> Option<u16> {
    let open  = name.rfind('(')?;
    let close = name.rfind(')')?;
    let inner = name.get(open + 1..close)?; // e.g. "TCP 26900"
    let mut parts = inner.split_whitespace();
    let _proto = parts.next()?;
    let port_str = parts.next()?;
    port_str.parse::<u16>().ok()
}

// ─── Non-Windows stubs ──────────────────────────────────────────────────

#[cfg(not(windows))]
pub fn add_rules(_server_port: u16) -> RuleResult {
    RuleResult {
        added_ports: Vec::new(),
        removed_ports: Vec::new(),
        notes: vec![
            "Firewall management is only automated on Windows. On Linux/macOS, \
             the OS firewall on a desktop install is usually open to inbound on \
             non-privileged ports by default — if not, you'll need to allow \
             ServerPort..ServerPort+2 (TCP/UDP) in iptables/nftables/ufw/etc. \
             yourself.".into()
        ],
    }
}

#[cfg(not(windows))]
pub fn remove_rules() -> RuleResult {
    RuleResult {
        added_ports: Vec::new(),
        removed_ports: Vec::new(),
        notes: vec!["nothing to do — no firewall rules managed on this OS".into()],
    }
}

#[cfg(not(windows))]
pub fn status() -> RuleStatus {
    RuleStatus { any_present: false, present: Vec::new(), unsupported: true }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn parses_port_from_name() {
        assert_eq!(parse_port_from_rule_name("7DTD Server Manager (TCP 26900)"), Some(26900));
        assert_eq!(parse_port_from_rule_name("7DTD Server Manager (UDP 26902)"), Some(26902));
        assert_eq!(parse_port_from_rule_name("not a rule"), None);
    }
}
