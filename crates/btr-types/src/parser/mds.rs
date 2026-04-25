#[derive(Debug, Default)]
pub struct MdsConfig {
    pub server: String,
    pub database: String,
    pub schema: String,
    pub user: String,
    /// Password as stored in the INI file — may be obfuscated hex or plaintext.
    /// Callers that need the live credential must decrypt it themselves.
    pub password: String,
    pub int_path: String,
}

pub fn parse(text: &str) -> MdsConfig {
    let mut cfg = MdsConfig::default();
    let mut section = String::new();
    let clean = text.replace('\u{feff}', ""); // strip BOM

    for raw in clean.lines() {
        let line = raw.trim().trim_end_matches('\r');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_uppercase();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim().to_ascii_uppercase();
        let val = v.trim();

        match section.as_str() {
            "MDS" | "SQL_BTR" | "SQL" => match key.as_str() {
                "SERVER" => {
                    if cfg.server.is_empty() {
                        cfg.server = val.to_string()
                    }
                }
                "DATABASE" => {
                    if cfg.database.is_empty() {
                        cfg.database = val.to_string()
                    }
                }
                "SCHEMA" => {
                    if cfg.schema.is_empty() {
                        cfg.schema = val.to_string()
                    }
                }
                "USER" => {
                    if cfg.user.is_empty() {
                        cfg.user = val.to_string()
                    }
                }
                "PASSWORD" => {
                    if cfg.password.is_empty() {
                        cfg.password = val.to_string()
                    }
                }
                _ => {}
            },
            "INT" => {
                if key == "PATH" {
                    cfg.int_path = val.to_string();
                }
            }
            _ => {}
        }
    }
    cfg
}
