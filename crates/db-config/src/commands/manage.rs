use crate::db;
use btr_types::type_name;
use btr_types::{IndexSegment, IntField, IntIndex};
use odbc_api::{ConnectionOptions, Environment};
use std::path::Path;

pub fn do_init(db_path: &Path) -> Result<(), String> {
    let conn = db::init(db_path).map_err(|e| e.to_string())?;
    db::create_schema(&conn).map_err(|e| e.to_string())?;
    println!("initialized: {}", db_path.display());
    Ok(())
}

pub fn do_list(db_path: &Path) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    let rows = db::list_tables(&conn).map_err(|e| e.to_string())?;
    if rows.is_empty() {
        println!("(no tables)");
        return Ok(());
    }
    let mut name_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for r in &rows {
        *name_counts.entry(r.table_name.as_str()).or_default() += 1;
    }
    println!("TABLES ({}):", rows.len());
    for r in &rows {
        let multi = name_counts[r.table_name.as_str()] > 1;
        print!(
            "  {:<30}  [{}.{}]  rec={:<5} fields={} idx={}",
            r.table_name, r.schema_name, r.db_name, r.record_length, r.field_count, r.index_count
        );
        if multi || !r.source_dir.is_empty() {
            print!("  dir={}", r.source_dir);
        }
        println!();
    }
    Ok(())
}

pub fn do_show(db_path: &Path, name: &str, source_dir: Option<&str>) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    let f = db::get_table(&conn, name, source_dir)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("table '{name}' not found"))?;

    println!("TABLE: {}", f.table_name);
    println!("  schema:        {}", f.schema_name);
    if !f.db_name.is_empty() {
        println!("  database:      {}", f.db_name);
    }
    println!("  record_length: {}", f.record_length);
    println!("  page_size:     {}", f.page_size);
    println!("  file_flags:    {}", f.file_flags);
    println!(
        "  local_cache:   {}",
        if f.local_cache { "yes" } else { "no" }
    );
    println!(
        "  ignore_nulls:  {}",
        if f.ignore_null_values { "yes" } else { "no" }
    );
    println!(
        "  trim_strings:  {}",
        if f.trim_string_fields { "yes" } else { "no" }
    );
    println!(
        "  oem_to_ansi:   {}",
        if f.translate_oem_to_ansi { "yes" } else { "no" }
    );
    if let Some(pi) = f.primary_index {
        println!("  primary_index: {pi}");
    }
    if !f.source_file.is_empty() {
        println!("  source_file:   {}", f.source_file);
    }
    if !f.source_path.is_empty() {
        println!("  source_path:   {}", f.source_path);
    }
    if !f.source_dir.is_empty() {
        println!("  source_dir:    {}", f.source_dir);
    }

    println!("\nFIELDS ({}):", f.fields.len());
    for field in &f.fields {
        let type_str = format!("{}({})", type_name(field.native_type), field.native_type);
        print!(
            "  #{:<3} {:<30} {:<16} len={:<5} off={}",
            field.num, field.name, type_str, field.length, field.offset
        );
        if let Some(fi) = field.field_index {
            print!("  FIELD_INDEX={fi}");
        }
        if let Some(ref d) = field.default_value {
            print!("  default={d}");
        }
        println!();
    }

    println!("\nINDEXES ({}):", f.indexes.len());
    for idx in &f.indexes {
        let segs: Vec<String> = idx
            .segments
            .iter()
            .map(|s| {
                let dir = if s.descending { "DESC" } else { "ASC" };
                format!("f{}({},flags={})", s.field_num, dir, s.attrs)
            })
            .collect();
        println!("  #{:<3} [{}]", idx.num, segs.join(", "));
    }
    Ok(())
}

pub fn do_show_config(db_path: &Path) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    let rows = db::get_all_config(&conn).map_err(|e| e.to_string())?;
    if rows.is_empty() {
        println!("(no config)");
        return Ok(());
    }
    let mut cur = String::new();
    for (section, key, value) in &rows {
        if *section != cur {
            println!("\n[{section}]");
            cur = section.clone();
        }
        println!("  {key:<12} = {value}");
    }
    Ok(())
}

pub fn do_test_connection(db_path: &Path) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;

    let get = |key: &str| -> String {
        db::get_config(&conn, "MDS", key)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let get_bool = |key: &str| -> bool {
        matches!(get(key).to_ascii_lowercase().as_str(), "yes" | "true" | "1")
    };

    let server = get("SERVER");
    let database = get("DATABASE");
    let driver = get("DRIVER");
    let network = get("NETWORK");
    let user = get("USER");
    let password = get("PASSWORD");
    let trusted = get_bool("TRUSTED_CONNECTION");
    let encrypt = get_bool("ENCRYPT");
    let trust_cert = get_bool("TRUST_SERVER_CERTIFICATE");

    println!("=== Connection config from {} ===", db_path.display());
    println!(
        "  SERVER                   = {}",
        if server.is_empty() {
            "(not set)"
        } else {
            &server
        }
    );
    println!(
        "  DATABASE                 = {}",
        if database.is_empty() {
            "(not set)"
        } else {
            &database
        }
    );
    println!(
        "  DRIVER                   = {}",
        if driver.is_empty() {
            "(not set)"
        } else {
            &driver
        }
    );
    println!(
        "  NETWORK                  = {}",
        if network.is_empty() {
            "(not set — driver default)"
        } else {
            &network
        }
    );
    println!(
        "  USER                     = {}",
        if user.is_empty() {
            "(not set — Windows auth)"
        } else {
            &user
        }
    );
    println!(
        "  PASSWORD                 = {}",
        if password.is_empty() {
            "(not set)"
        } else {
            "(set)"
        }
    );
    println!(
        "  TRUSTED_CONNECTION       = {}",
        if trusted { "yes" } else { "no" }
    );
    println!(
        "  ENCRYPT                  = {}",
        if encrypt { "yes" } else { "no" }
    );
    println!(
        "  TRUST_SERVER_CERTIFICATE = {}",
        if trust_cert { "yes" } else { "no" }
    );
    println!();

    let auth_part = if trusted {
        "Trusted_Connection=Yes;".to_string()
    } else if !user.is_empty() {
        format!("Uid={};Pwd={};", user, password)
    } else {
        "Trusted_Connection=No;".to_string()
    };
    let tls_part = format!(
        "Encrypt={};TrustServerCertificate={};",
        if encrypt { "Yes" } else { "No" },
        if trust_cert { "Yes" } else { "No" },
    );
    let db_part = if database.is_empty() {
        String::new()
    } else {
        format!("Database={};", database)
    };

    // Try each driver with both plain server name AND explicit tcp: prefix.
    // Plain name uses whatever default protocol the driver picks (usually Named Pipes
    // for old drivers, TCP for new ones). tcp: forces TCP/IP explicitly.
    let server_variants: &[(&str, String)] = &[
        ("plain", format!("Server={};", server)),
        ("tcp", format!("Server=tcp:{},1433;", server)),
        ("DBMSSOCN", format!("Server={};Network=DBMSSOCN;", server)),
    ];

    let drivers = [
        "SQL Server Native Client 10.0",
        "SQL Server Native Client 11.0",
        "SQL Server",
        "ODBC Driver 17 for SQL Server",
        "ODBC Driver 18 for SQL Server",
    ];

    let env = Environment::new().map_err(|e| format!("ODBC init failed: {e}"))?;

    // List installed ODBC drivers so we know which ones to try
    println!("=== Installed ODBC drivers ===");
    match env.drivers() {
        Ok(drivers_list) => {
            for d in &drivers_list {
                println!("  {}", d.description);
            }
        }
        Err(e) => println!("  (could not enumerate: {})", e),
    }
    println!();

    // Also test with Windows auth (trusted connection) regardless of stored setting —
    // the DLL runs as the NTVDM Windows user which may have Windows auth access
    // even if the stored credentials use SQL auth.
    let windows_auth_part = "Trusted_Connection=Yes;";

    println!("=== Testing connection strings ===");
    let mut any_ok = false;
    for driver in &drivers {
        for (variant_name, server_part) in server_variants {
            for (auth_label, auth) in &[
                ("sql-auth", auth_part.as_str()),
                ("windows-auth", windows_auth_part),
            ] {
                let cs = format!(
                    "Driver={{{}}};{}{}{}{}",
                    driver, server_part, db_part, auth, tls_part
                );
                let display = cs.replace(&password, "***");
                println!();
                println!("  Driver:  {} / {} / {}", driver, variant_name, auth_label);
                println!("  ConnStr: {}", display);
                print!("  Result:  ");
                match env.connect_with_connection_string(&cs, ConnectionOptions::default()) {
                    Ok(_) => {
                        println!("OK -- CONNECTED");
                        any_ok = true;
                    }
                    Err(e) => {
                        println!("FAILED");
                        println!("  Error:   {}", e);
                    }
                }
            }
        }
    }

    println!();
    if any_ok {
        println!("=== SUCCESS: at least one driver connected ===");
        Ok(())
    } else {
        println!("=== FAILED: no driver could connect ===");
        Err("all connection attempts failed".into())
    }
}

pub fn do_set_config(db_path: &Path, section: &str, key: &str, value: &str) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    db::upsert_config(
        &conn,
        &section.to_ascii_uppercase(),
        &key.to_ascii_uppercase(),
        value,
    )
    .map_err(|e| e.to_string())?;
    println!("set [{section}] {key} = {value}");
    Ok(())
}

pub fn do_set_table(db_path: &Path, table: &str, key: &str, value: &str) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    db::update_table_prop(&conn, table, key, value)?;
    println!("updated {table}.{key} = {value}");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn do_add_field(
    db_path: &Path,
    table: &str,
    num: u32,
    name: &str,
    native_type: i32,
    length: u32,
    offset: u32,
    index: Option<u32>,
    default: Option<&str>,
) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    let f = IntField {
        num,
        name: name.to_string(),
        native_type,
        length,
        offset,
        field_index: index,
        default_value: default.map(|s| s.to_string()),
    };
    db::upsert_field_by_name(&conn, table, &f)?;
    println!("field #{num} ({name}) saved to {table}");
    Ok(())
}

pub fn do_rm_field(db_path: &Path, table: &str, num: u32) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    db::delete_field_by_name(&conn, table, num)?;
    println!("removed field #{num} from {table}");
    Ok(())
}

pub fn do_add_index(
    db_path: &Path,
    table: &str,
    num: u32,
    fields_str: &str,
    attrs_str: &str,
    desc_str: &str,
) -> Result<(), String> {
    let field_nums: Vec<u32> = fields_str
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<u32>()
                .map_err(|_| format!("invalid field number '{s}'"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if field_nums.is_empty() {
        return Err("--fields cannot be empty".into());
    }

    let attrs: Vec<u16> = {
        let parts: Vec<&str> = attrs_str.split(',').collect();
        if parts.len() == 1 {
            let v: u16 = parts[0]
                .trim()
                .parse()
                .map_err(|_| format!("invalid attrs '{attrs_str}'"))?;
            vec![v; field_nums.len()]
        } else {
            parts
                .iter()
                .map(|s| {
                    s.trim()
                        .parse::<u16>()
                        .map_err(|_| format!("invalid attrs value '{s}'"))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    let descs: Vec<bool> = {
        let parts: Vec<&str> = desc_str.split(',').collect();
        if parts.len() == 1 {
            let v = parts[0].trim() == "1";
            vec![v; field_nums.len()]
        } else {
            parts
                .iter()
                .map(|s| Ok::<bool, String>(s.trim() == "1"))
                .collect::<Result<Vec<_>, _>>()?
        }
    };

    if attrs.len() != field_nums.len() {
        return Err(format!(
            "--attrs has {} values but --fields has {}",
            attrs.len(),
            field_nums.len()
        ));
    }
    if descs.len() != field_nums.len() {
        return Err(format!(
            "--desc has {} values but --fields has {}",
            descs.len(),
            field_nums.len()
        ));
    }

    let segments: Vec<IndexSegment> = field_nums
        .iter()
        .enumerate()
        .map(|(i, &fn_)| IndexSegment {
            field_num: fn_,
            attrs: attrs[i],
            descending: descs[i],
            null_value: 0,
        })
        .collect();
    let num_segments = segments.len() as u32;

    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    db::upsert_index_by_name(
        &conn,
        table,
        &IntIndex {
            num,
            num_segments,
            segments,
        },
    )?;
    println!("index #{num} saved to {table}");
    Ok(())
}

pub fn do_rm_index(db_path: &Path, table: &str, num: u32) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    db::delete_index_by_name(&conn, table, num)?;
    println!("removed index #{num} from {table}");
    Ok(())
}

pub fn do_rm_table(db_path: &Path, table: &str) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    let found = db::delete_table(&conn, table).map_err(|e| e.to_string())?;
    if found {
        println!("deleted {table}");
    } else {
        return Err(format!("table '{table}' not found"));
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub fn do_set_connection(
    db_path: &Path,
    server: Option<&str>,
    database: Option<&str>,
    schema: Option<&str>,
    driver: Option<&str>,
    network: Option<&str>,
    user: Option<&str>,
    password: Option<&str>,
    trusted_connection: Option<bool>,
    encrypt: Option<bool>,
    trust_server_certificate: Option<bool>,
    recnum_column: Option<&str>,
) -> Result<(), String> {
    if server.is_none()
        && database.is_none()
        && schema.is_none()
        && driver.is_none()
        && network.is_none()
        && user.is_none()
        && password.is_none()
        && trusted_connection.is_none()
        && encrypt.is_none()
        && trust_server_certificate.is_none()
        && recnum_column.is_none()
    {
        return Err("at least one connection option required".into());
    }
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    let set = |key: &str, val: &str| -> Result<(), String> {
        db::upsert_config(&conn, "MDS", key, val).map_err(|e| e.to_string())
    };
    if let Some(v) = server {
        set("SERVER", v)?;
        println!("  SERVER   = {v}");
    }
    if let Some(v) = database {
        set("DATABASE", v)?;
        println!("  DATABASE = {v}");
    }
    if let Some(v) = schema {
        set("SCHEMA", v)?;
        println!("  SCHEMA   = {v}");
    }
    if let Some(v) = driver {
        set("DRIVER", v)?;
        println!("  DRIVER   = {v}");
    }
    if let Some(v) = network {
        set("NETWORK", v)?;
        println!("  NETWORK  = {v}");
    }
    if let Some(v) = user {
        set("USER", v)?;
        println!("  USER     = {v}");
    }
    if let Some(v) = password {
        set("PASSWORD", v)?;
        println!("  PASSWORD = (set)");
    }
    if let Some(v) = trusted_connection {
        set("TRUSTED_CONNECTION", if v { "yes" } else { "no" })?;
        println!("  TRUSTED_CONNECTION = {}", if v { "yes" } else { "no" });
    }
    if let Some(v) = encrypt {
        set("ENCRYPT", if v { "yes" } else { "no" })?;
        println!("  ENCRYPT = {}", if v { "yes" } else { "no" });
    }
    if let Some(v) = trust_server_certificate {
        set("TRUST_SERVER_CERTIFICATE", if v { "yes" } else { "no" })?;
        println!(
            "  TRUST_SERVER_CERTIFICATE = {}",
            if v { "yes" } else { "no" }
        );
    }
    if let Some(v) = recnum_column {
        // Format: "ColName" (global default) or "DBName:ColName" (per-db override)
        if let Some((db, col)) = v.split_once(':') {
            let key = format!("RECNUM_COLUMN.{}", db.to_ascii_uppercase());
            set(&key, col)?;
            println!("  RECNUM_COLUMN.{} = {col}", db.to_ascii_uppercase());
        } else {
            set("RECNUM_COLUMN", v)?;
            println!("  RECNUM_COLUMN = {v}");
        }
    }
    Ok(())
}
