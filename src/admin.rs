//! Parse and write `serveradmin.xml` — the file that tracks admins,
//! whitelist, blacklist, and command permissions.
//!
//! Format (V1.0+ uses `<user platform="Steam" userid="...">`; older
//! Alpha XML used `<user steamID="...">`. We support both on read and
//! write the modern shape on save.):
//!
//! ```xml
//! <adminTools>
//!   <admins>
//!     <user platform="Steam" userid="76561198..." name="..." permission_level="0"/>
//!   </admins>
//!   <whitelist>
//!     <user platform="Steam" userid="..."/>
//!   </whitelist>
//!   <blacklist>
//!     <user platform="Steam" userid="..." reason="grief"/>
//!   </blacklist>
//!   <permissions>
//!     <permission cmd="kick" permission_level="1"/>
//!   </permissions>
//! </adminTools>
//! ```

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdminFile {
    pub admins: Vec<AdminUser>,
    pub whitelist: Vec<AdminUser>,
    pub blacklist: Vec<AdminUser>,
    pub permissions: Vec<CommandPerm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUser {
    /// "Steam" or "Xbl" (Xbox Live) usually. Optional for older formats.
    #[serde(default)]
    pub platform: Option<String>,
    /// Steam64 ID or Xbox Live ID.
    pub user_id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// 0 (full access) … 1000 (default user). Used for admins; ignored
    /// for whitelist / blacklist entries.
    #[serde(default)]
    pub permission_level: Option<u16>,
    /// Optional reason — used for blacklist entries.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandPerm {
    pub cmd: String,
    pub permission_level: u16,
}

impl AdminFile {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let body = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&body)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let xml = self.to_xml()?;
        std::fs::write(path, xml)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn parse(xml: &str) -> Result<Self> {
        let mut reader = Reader::from_str(xml);
        reader.trim_text(true);

        let mut out = AdminFile::default();
        let mut buf = Vec::new();
        let mut section: Option<Section> = None;

        loop {
            match reader.read_event_into(&mut buf) {
                Err(e) => return Err(anyhow!("xml error at {}: {}", reader.buffer_position(), e)),
                Ok(Event::Eof) => break,

                Ok(Event::Start(e)) => {
                    section = match e.name().as_ref() {
                        b"admins"      => Some(Section::Admins),
                        b"whitelist"   => Some(Section::Whitelist),
                        b"blacklist"   => Some(Section::Blacklist),
                        b"permissions" => Some(Section::Permissions),
                        _ => section,
                    };
                }
                Ok(Event::End(e)) => {
                    match e.name().as_ref() {
                        b"admins" | b"whitelist" | b"blacklist" | b"permissions" => section = None,
                        _ => {}
                    }
                }

                Ok(Event::Empty(e)) => {
                    let tag = e.name().as_ref().to_vec();
                    let attrs = collect_attrs(&e, &reader);
                    match (section, tag.as_slice()) {
                        (Some(Section::Admins), b"user")    => out.admins.push(user_from_attrs(&attrs)),
                        (Some(Section::Whitelist), b"user") => out.whitelist.push(user_from_attrs(&attrs)),
                        (Some(Section::Blacklist), b"user") => out.blacklist.push(user_from_attrs(&attrs)),
                        (Some(Section::Permissions), b"permission") => {
                            if let (Some(cmd), Some(lvl)) = (
                                attrs.get("cmd").cloned(),
                                attrs.get("permission_level").and_then(|v| v.parse::<u16>().ok()),
                            ) {
                                out.permissions.push(CommandPerm { cmd, permission_level: lvl });
                            }
                        }
                        _ => {}
                    }
                }

                _ => {}
            }
            buf.clear();
        }

        Ok(out)
    }

    pub fn to_xml(&self) -> Result<String> {
        let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
        let root = BytesStart::new("adminTools");
        writer.write_event(Event::Start(root))?;

        write_user_section(&mut writer, "admins",    &self.admins, true)?;
        write_user_section(&mut writer, "whitelist", &self.whitelist, false)?;
        write_user_section(&mut writer, "blacklist", &self.blacklist, false)?;

        writer.write_event(Event::Start(BytesStart::new("permissions")))?;
        for p in &self.permissions {
            let mut e = BytesStart::new("permission");
            e.push_attribute(("cmd", p.cmd.as_str()));
            let lvl_str = p.permission_level.to_string();
            e.push_attribute(("permission_level", lvl_str.as_str()));
            writer.write_event(Event::Empty(e))?;
        }
        writer.write_event(Event::End(BytesEnd::new("permissions")))?;

        writer.write_event(Event::End(BytesEnd::new("adminTools")))?;
        let bytes = writer.into_inner();
        Ok(String::from_utf8(bytes)?)
    }
}

fn write_user_section<W: std::io::Write>(
    writer: &mut Writer<W>,
    section: &str,
    users: &[AdminUser],
    include_perm: bool,
) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new(section)))?;
    for u in users {
        let mut e = BytesStart::new("user");
        e.push_attribute(("platform", u.platform.as_deref().unwrap_or("Steam")));
        e.push_attribute(("userid", u.user_id.as_str()));
        if let Some(n) = &u.name {
            if !n.is_empty() { e.push_attribute(("name", n.as_str())); }
        }
        let lvl_str;
        if include_perm {
            lvl_str = u.permission_level.unwrap_or(1000).to_string();
            e.push_attribute(("permission_level", lvl_str.as_str()));
        }
        if let Some(r) = &u.reason {
            if !r.is_empty() { e.push_attribute(("reason", r.as_str())); }
        }
        writer.write_event(Event::Empty(e))?;
    }
    writer.write_event(Event::End(BytesEnd::new(section)))?;
    Ok(())
}

#[derive(Copy, Clone)]
enum Section { Admins, Whitelist, Blacklist, Permissions }

fn collect_attrs(e: &BytesStart<'_>, reader: &Reader<&[u8]>) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for attr in e.attributes().flatten() {
        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        let val = attr
            .decode_and_unescape_value(reader)
            .map(|c| c.into_owned())
            .unwrap_or_default();
        map.insert(key, val);
    }
    map
}

fn user_from_attrs(a: &std::collections::HashMap<String, String>) -> AdminUser {
    let user_id = a
        .get("userid")
        .or_else(|| a.get("steamID"))
        .cloned()
        .unwrap_or_default();
    AdminUser {
        platform: a.get("platform").cloned(),
        user_id,
        name: a.get("name").cloned().filter(|s| !s.is_empty()),
        permission_level: a.get("permission_level").and_then(|s| s.parse().ok()),
        reason: a.get("reason").cloned().filter(|s| !s.is_empty()),
    }
}
