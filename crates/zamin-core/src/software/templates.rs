//! Creation templates (Phase 6 deliverable): the files a freshly created
//! server needs so its first start is an honest, guarded experience — a
//! `server.properties` with sane visible defaults and an `eula.txt` with
//! `eula=false`. The EULA is *not* pre-accepted: acceptance is the user's
//! click, surfaced by the preflight's typed `NEEDS_EULA`.
//!
//! Templates are daemon-side data identified by `templateId`. The V1
//! table has exactly one honest entry; more land when a second one
//! actually exists (the protocol's stability rule about growing only for
//! real clients applies to data too).

use std::path::Path;

use crate::error::CoreError;
use crate::fsops::atomic_write;

#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub id: &'static str,
    pub description: &'static str,
    /// Server-root-relative path → content, stamped verbatim.
    pub files: &'static [(&'static str, &'static str)],
}

pub const DEFAULT_TEMPLATE_ID: &str = "survival";

const EULA_TXT: &str = "\
# Accepting the Minecraft EULA is a user action, never a default.
# The panel offers this acceptance before the first start.
eula=false
";

const SERVER_PROPERTIES: &str = "\
#Minecraft server properties (written by ZIM's creation template)
#The daemon reconciles the port with the server's configured setting; edit either side honestly.
server-port=25565
motd=A Zamin-managed server
max-players=20
view-distance=10
simulation-distance=10
enable-status=true
online-mode=true
";

pub const TEMPLATES: &[Template] = &[Template {
    id: DEFAULT_TEMPLATE_ID,
    description: "Vanilla-honest defaults; EULA awaits acceptance.",
    files: &[
        ("eula.txt", EULA_TXT),
        ("server.properties", SERVER_PROPERTIES),
    ],
}];

pub fn templates() -> &'static [Template] {
    TEMPLATES
}

pub fn template(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

/// Stamp the template's files into `dir`. Never overwrites: creation is
/// for fresh directories, and clobbering existing files is how tools
/// destroy things. Returns the stamped paths.
pub fn stamp_template(dir: &Path, template_id: &str) -> Result<Vec<std::path::PathBuf>, CoreError> {
    let template = template(template_id).ok_or_else(|| CoreError::NotFound {
        path: dir.join(format!("<template:{template_id}>")),
    })?;
    let mut stamped = Vec::new();
    for (rel, content) in template.files {
        let path = dir.join(rel);
        if path.exists() {
            return Err(CoreError::Io {
                path,
                source: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "refusing to overwrite an existing file while stamping a template",
                ),
            });
        }
        atomic_write(&path, content.as_bytes())?;
        stamped.push(path);
    }
    Ok(stamped)
}
