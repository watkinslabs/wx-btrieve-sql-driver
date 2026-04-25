mod btrieve;
mod commands;
mod db;
mod error;
mod export;

use btr_types::{IndexSegment, IntField, IntIndex};
use clap::{Parser, Subcommand};
use odbc_api::Cursor;
use std::path::PathBuf;

#[cfg(windows)]
const DEFAULT_DB: &str = concat!(env!("WXBTRV_CONFIG_DIR"), "\\wxbtrv.db");
#[cfg(not(windows))]
const DEFAULT_DB: &str = "wxbtrv.db";

#[derive(Parser)]
#[command(
    name = "db_config",
    about = "Manage wxbtrv.db: schemas, connection, config"
)]
struct Cli {
    /// SQLite database file
    #[arg(long, default_value = DEFAULT_DB, global = true)]
    db: PathBuf,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a new empty database
    Init,

    /// Import .INT files from one or more directories.
    /// Without -r only the top level of each directory is scanned.
    ImportInt {
        /// Directories containing .INT / .int files (one or more)
        #[arg(required = true)]
        dirs: Vec<PathBuf>,
        /// Scan directories recursively
        #[arg(long, short = 'r')]
        recursive: bool,
        /// Default database name for tables that have none in their INT file
        #[arg(long)]
        db_name: Option<String>,
        /// Default schema name for tables that have none in their INT file
        #[arg(long)]
        schema: Option<String>,
    },

    /// Import a mds.ini config file
    ImportMds {
        /// Path to mds.ini
        file: PathBuf,
    },

    /// List all tables
    List,

    /// Show full definition for a table
    Show {
        /// Table name (case-insensitive)
        table: String,
        /// Filter by import directory when the same table exists in multiple profiles
        #[arg(long)]
        source_dir: Option<String>,
    },

    /// Show all config (mds.ini) values
    ShowConfig,

    /// Export INT file(s) to a directory
    ExportInt {
        /// Output directory
        #[arg(long, default_value = ".")]
        out: PathBuf,
        /// Table name(s) to export; exports all if omitted
        tables: Vec<String>,
    },

    /// Export mds.ini from config
    ExportMds {
        /// Output file
        #[arg(long, default_value = "mds.ini")]
        out: PathBuf,
    },

    /// Set SQL Server connection details (overrides anything from INT files or mds.ini)
    SetConnection {
        /// SQL Server hostname or IP
        #[arg(long)]
        server: Option<String>,
        /// Database name (default session database)
        #[arg(long)]
        database: Option<String>,
        /// Default schema (default: dbo)
        #[arg(long)]
        schema: Option<String>,
        /// ODBC driver name, e.g. "SQL Server Native Client 10.0" or "ODBC Driver 17 for SQL Server"
        #[arg(long)]
        driver: Option<String>,
        /// Network library: "DBMSSOCN" (legacy TCP), "tcp:" (modern TCP), or omit for driver default
        #[arg(long)]
        network: Option<String>,
        /// SQL Server login username (omit for Windows/trusted auth)
        #[arg(long)]
        user: Option<String>,
        /// SQL Server login password
        #[arg(long)]
        password: Option<String>,
        /// Use Windows (trusted) authentication instead of SQL login
        #[arg(long)]
        trusted_connection: Option<bool>,
        /// Encrypt the connection (Encrypt=Yes/No)
        #[arg(long)]
        encrypt: Option<bool>,
        /// Trust the server TLS certificate without validation
        #[arg(long)]
        trust_server_certificate: Option<bool>,
        /// Identity column name for tables with no PRIMARY_INDEX.
        /// Global default: --recnum-column MDS_RECNUM
        /// Per-db override: --recnum-column GPacific:MDS_RECNUM
        #[arg(long)]
        recnum_column: Option<String>,
    },

    /// Test the SQL Server connection using credentials stored in the database.
    /// Tries each ODBC driver in order and reports which one succeeds (or all errors).
    TestConnection,

    /// Set a config value (e.g. set-config MDS SERVER 10.0.0.5)
    SetConfig {
        section: String,
        key: String,
        value: String,
    },

    /// Update a single property on a table
    /// Keys: schema_name, db_name, record_length, page_size, file_flags,
    ///       ignore_null_values, trim_string_fields, translate_oem_to_ansi,
    ///       primary_index, local_cache
    SetTable {
        table: String,
        key: String,
        value: String,
    },

    /// Add or replace a field on a table
    AddField {
        table: String,
        #[arg(long)]
        num: u32,
        #[arg(long)]
        name: String,
        #[arg(long = "type", default_value_t = 0)]
        native_type: i32,
        #[arg(long)]
        length: u32,
        #[arg(long)]
        offset: u32,
        /// FIELD_INDEX value (which index this field participates in)
        #[arg(long)]
        index: Option<u32>,
        /// FIELD_DEFAULT_VALUE
        #[arg(long)]
        default: Option<String>,
    },

    /// Remove a field from a table
    RmField {
        table: String,
        /// Field number
        num: u32,
    },

    /// Add or replace an index on a table
    AddIndex {
        table: String,
        #[arg(long)]
        num: u32,
        /// Comma-separated field numbers in segment order  (e.g. 1,3,4)
        #[arg(long)]
        fields: String,
        /// Comma-separated INDEX_SEGMENT_FLAG per segment, or single value for all  (e.g. 275,275,263)
        #[arg(long, default_value = "259")]
        attrs: String,
        /// Comma-separated descending flags (0/1) per segment, or single value
        #[arg(long, default_value = "0")]
        desc: String,
    },

    /// Remove an index from a table
    RmIndex {
        table: String,
        /// Index number
        num: u32,
    },

    /// Delete a table and all its fields/indexes
    RmTable { table: String },

    /// Parse a Btrieve .B file and create/update the table's INT schema in the DB.
    /// Key-segment fields are auto-generated; add remaining fields with add-field.
    AnalyzeB {
        /// Path to the .B file
        file: PathBuf,
        /// Override table name (default: derived from filename)
        #[arg(long)]
        table: Option<String>,
    },

    /// Generate SQL Server CREATE TABLE DDL for one or all tables
    GenDdl {
        /// Output SQL file (default: stdout)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Add MDS_RECNUM IDENTITY column (omit if table has PRIMARY_INDEX)
        #[arg(long)]
        recnum: bool,
        /// Table name(s) to generate (all if omitted)
        tables: Vec<String>,
    },

    /// Show migration status for all tables
    MigrationStatus,

    /// Mark a table as migrated
    MarkMigrated {
        table: String,
        /// Number of rows migrated
        #[arg(long)]
        rows: Option<i64>,
        /// Target SQL Server hostname
        #[arg(long, default_value = "")]
        server: String,
        /// Target database name
        #[arg(long = "target-db", default_value = "")]
        target_db: String,
    },

    /// Clear migrated flag on a table
    ClearMigrated { table: String },

    /// Show table config as YAML. Uses wxbtrv.db if available, otherwise discovers from SQL Server.
    /// The path determines which database config to use (tiered: directory → global).
    ShowTableConfig {
        /// Table name (e.g. BKARINVI)
        table: String,
        /// Full directory path (e.g. G:\PACIFIC) to resolve database config
        #[arg(long, default_value = "")]
        path: String,
        /// Also query SQL Server to discover schema if not in wxbtrv.db
        #[arg(long)]
        discover: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = run(&cli);
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<(), String> {
    use commands::export::*;
    use commands::import::*;
    use commands::manage::*;
    use commands::migrate::*;

    match &cli.cmd {
        Cmd::Init => do_init(&cli.db),
        Cmd::ImportInt {
            dirs,
            recursive,
            db_name,
            schema,
        } => do_import_int(
            &cli.db,
            dirs,
            *recursive,
            db_name.as_deref(),
            schema.as_deref(),
        ),
        Cmd::ImportMds { file } => do_import_mds(&cli.db, file),
        Cmd::List => do_list(&cli.db),
        Cmd::Show { table, source_dir } => do_show(&cli.db, table, source_dir.as_deref()),
        Cmd::ShowConfig => do_show_config(&cli.db),
        Cmd::ExportInt { out, tables } => do_export_int(&cli.db, out, tables),
        Cmd::ExportMds { out } => do_export_mds(&cli.db, out),
        Cmd::SetConnection {
            server,
            database,
            schema,
            driver,
            network,
            user,
            password,
            trusted_connection,
            encrypt,
            trust_server_certificate,
            recnum_column,
        } => do_set_connection(
            &cli.db,
            server.as_deref(),
            database.as_deref(),
            schema.as_deref(),
            driver.as_deref(),
            network.as_deref(),
            user.as_deref(),
            password.as_deref(),
            *trusted_connection,
            *encrypt,
            *trust_server_certificate,
            recnum_column.as_deref(),
        ),
        Cmd::TestConnection => commands::manage::do_test_connection(&cli.db),
        Cmd::SetConfig {
            section,
            key,
            value,
        } => do_set_config(&cli.db, section, key, value),
        Cmd::SetTable { table, key, value } => do_set_table(&cli.db, table, key, value),
        Cmd::AddField {
            table,
            num,
            name,
            native_type,
            length,
            offset,
            index,
            default,
        } => do_add_field(
            &cli.db,
            table,
            *num,
            name,
            *native_type,
            *length,
            *offset,
            *index,
            default.as_deref(),
        ),
        Cmd::RmField { table, num } => do_rm_field(&cli.db, table, *num),
        Cmd::AddIndex {
            table,
            num,
            fields,
            attrs,
            desc,
        } => do_add_index(&cli.db, table, *num, fields, attrs, desc),
        Cmd::RmIndex { table, num } => do_rm_index(&cli.db, table, *num),
        Cmd::RmTable { table } => do_rm_table(&cli.db, table),
        Cmd::AnalyzeB { file, table } => do_analyze_b(&cli.db, file, table.as_deref()),
        Cmd::GenDdl {
            out,
            recnum,
            tables,
        } => do_gen_ddl(&cli.db, out.as_ref(), *recnum, tables),
        Cmd::MigrationStatus => do_migration_status(&cli.db),
        Cmd::MarkMigrated {
            table,
            rows,
            server,
            target_db,
        } => do_mark_migrated(&cli.db, table, *rows, server, target_db),
        Cmd::ClearMigrated { table } => do_clear_migrated(&cli.db, table),
        Cmd::ShowTableConfig {
            table,
            path,
            discover,
        } => do_show_table_config(&cli.db, table, path, *discover),
    }
}

fn do_show_table_config(
    db_path: &PathBuf,
    table: &str,
    path: &str,
    discover: bool,
) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;

    // Show wxbtrv.db entry if it exists
    let int_file = db::get_table(&conn, table, None).map_err(|e| e.to_string())?;
    if let Some(ref f) = int_file {
        println!("# Source: wxbtrv.db");
        print_table_yaml(f);
        if !discover {
            return Ok(());
        }
        println!("\n# --- Also discovering from SQL Server ---");
    }

    if !discover && int_file.is_none() {
        println!("# Table '{}' not found in wxbtrv.db", table);
        println!("# Use --discover to query SQL Server for schema");
        return Ok(());
    }

    // Discover from SQL Server
    // Resolve database from path config
    let dir = if !path.is_empty() {
        path.to_ascii_uppercase()
    } else {
        String::new()
    };

    let db_name = {
        // Check for directory-specific DATABASE config
        let dir_db: Option<String> = if !dir.is_empty() {
            conn.query_row(
                "SELECT value FROM config WHERE UPPER(section) = ?1 AND UPPER(key) = 'DATABASE'",
                rusqlite::params![dir],
                |r| r.get::<_, String>(0),
            )
            .ok()
        } else {
            None
        };

        dir_db.unwrap_or_else(|| {
            conn.query_row(
                "SELECT value FROM config WHERE section = 'config' AND UPPER(key) = 'DATABASE'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap_or_else(|_| "JAdvdata".to_string())
        })
    };

    let schema = {
        let dir_sc: Option<String> = if !dir.is_empty() {
            conn.query_row(
                "SELECT value FROM config WHERE UPPER(section) = ?1 AND UPPER(key) = 'SCHEMA'",
                rusqlite::params![dir],
                |r| r.get::<_, String>(0),
            )
            .ok()
        } else {
            None
        };
        dir_sc.unwrap_or_else(|| "dbo".to_string())
    };

    // Build ODBC connection string from config
    let get = |key: &str| -> String {
        conn.query_row(
            "SELECT value FROM config WHERE section = 'config' AND key = ?1",
            rusqlite::params![key],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
    };

    let server = get("SERVER");
    let driver = get("DRIVER");
    let network = get("NETWORK");
    let user = get("USER");
    let pass = get("PASSWORD");

    let conn_str = format!(
        "Driver={{{driver}}};Server={server};{net}Database={db_name};Uid={user};Pwd={pass};",
        net = if network.is_empty() {
            String::new()
        } else {
            format!("Network={network};")
        },
    );

    println!("# Source: SQL Server (discovered)");
    println!("# Database: {db_name}.{schema}");

    // Query columns
    let env = odbc_api::Environment::new().map_err(|e| format!("ODBC env: {e}"))?;
    let odbc_conn = env
        .connect_with_connection_string(&conn_str, odbc_api::ConnectionOptions::default())
        .map_err(|e| format!("ODBC connect: {e}"))?;

    let col_sql = format!(
        "SELECT COLUMN_NAME, DATA_TYPE, COALESCE(CHARACTER_MAXIMUM_LENGTH,0), ORDINAL_POSITION \
         FROM [{db_name}].INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}' ORDER BY ORDINAL_POSITION"
    );

    let mut fields = Vec::new();
    let mut offset: u32 = 0;
    let mut fnum: u32 = 1;

    if let Some(mut cursor) = odbc_conn
        .execute(&col_sql, ())
        .map_err(|e| format!("SQL: {e}"))?
    {
        while let Some(mut row) = cursor.next_row().map_err(|e| format!("fetch: {e}"))? {
            let mut buf = vec![0u8; 256];
            let name = row.get_text(1, &mut buf).map_err(|e| format!("col: {e}"))?;
            let col_name = if name {
                String::from_utf8_lossy(&buf)
                    .trim_end_matches('\0')
                    .trim()
                    .to_string()
            } else {
                String::new()
            };

            buf.fill(0);
            row.get_text(2, &mut buf).ok();
            let dtype = String::from_utf8_lossy(&buf)
                .trim_end_matches('\0')
                .trim()
                .to_lowercase()
                .to_string();

            buf.fill(0);
            row.get_text(3, &mut buf).ok();
            let maxlen: u32 = String::from_utf8_lossy(&buf)
                .trim_end_matches('\0')
                .trim()
                .parse()
                .unwrap_or(0);

            if col_name.eq_ignore_ascii_case("MDS_RECNUM") {
                continue;
            }

            let (nt, len) = match dtype.as_str() {
                "char" | "nchar" => (0i32, maxlen.max(1)),
                "varchar" | "nvarchar" => (0, maxlen.max(1).min(255)),
                "int" => (1, 4),
                "smallint" => (1, 2),
                "tinyint" => (14, 1),
                "bigint" => (1, 8),
                "decimal" | "numeric" => (5, 8),
                "float" | "real" => (2, 8),
                "bit" => (7, 1),
                "datetime" | "datetime2" => (3, 8),
                "date" => (3, 4),
                _ => (0, maxlen.max(1).min(255)),
            };

            fields.push(IntField {
                num: fnum,
                name: col_name,
                native_type: nt,
                length: len,
                offset,
                field_index: None,
                default_value: None,
            });
            fnum += 1;
            offset += len;
        }
    }

    if fields.is_empty() {
        println!("# No columns found for {db_name}.{schema}.{table}");
        return Ok(());
    }

    // Indexes are NOT discovered from SQL Server — they come from table configs only.
    // SQL Server indexes are a different concept from Btrieve key paths.
    let indexes: Vec<IntIndex> = Vec::new();

    // Build config and print as YAML
    let int_file = btr_types::IntFile {
        table_name: table.to_string(),
        schema_name: schema.to_string(),
        db_name: db_name.to_string(),
        record_length: offset,
        page_size: 4096,
        file_flags: 0,
        fields,
        indexes,
        ignore_null_values: true,
        trim_string_fields: true,
        translate_oem_to_ansi: false,
        primary_index: None,
        local_cache: false,
        driver_name: String::new(),
        server_name: String::new(),
        permanent_int: false,
        number_df_fields: 0,
        source_file: String::new(),
        source_path: String::new(),
        source_dir: String::new(),
    };

    print_table_yaml(&int_file);
    Ok(())
}

fn print_table_yaml(f: &btr_types::IntFile) {
    println!("table: {}", f.table_name);
    println!("database: {}", f.db_name);
    println!("schema: {}", f.schema_name);
    println!("record_length: {}", f.record_length);
    println!("page_size: {}", f.page_size);
    println!("file_flags: {}", f.file_flags);
    println!("ignore_null_values: {}", f.ignore_null_values);
    println!("trim_string_fields: {}", f.trim_string_fields);
    println!("translate_oem_to_ansi: {}", f.translate_oem_to_ansi);
    if let Some(pi) = f.primary_index {
        println!("primary_index: {}", pi);
    }
    println!("local_cache: {}", f.local_cache);
    println!("fields:");
    for fld in &f.fields {
        println!("  - num: {}", fld.num);
        println!("    name: {}", fld.name);
        println!("    type: {}", fld.native_type);
        println!("    length: {}", fld.length);
        println!("    offset: {}", fld.offset);
        if let Some(idx) = fld.field_index {
            println!("    field_index: {}", idx);
        }
        if let Some(ref dv) = fld.default_value {
            println!("    default: \"{}\"", dv);
        }
    }
    if f.indexes.is_empty() {
        println!("indexes: [] # no index config — import table config for indexes");
    } else {
        println!("indexes:");
        for idx in &f.indexes {
            println!("  - num: {}", idx.num);
            println!("    segments:");
            for seg in &idx.segments {
                println!("      - field: {}", seg.field_num);
                println!("        attrs: {}", seg.attrs);
                println!("        descending: {}", seg.descending);
                println!("        null_value: {}", seg.null_value);
            }
        }
    }
}
