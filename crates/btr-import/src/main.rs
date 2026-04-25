/// btr-import — import Btrieve .B flat files into SQL Server.
///
/// Usage:
///   btr-import [--db <wxbtrv.db>] info <file.B>
///   btr-import [--db <wxbtrv.db>] import [options] <file.B> [<file.B>...]
///   btr-import [--db <wxbtrv.db>] import-dir [options] <dir> [-r]
///
/// The tool reads table schemas and SQL Server credentials from wxbtrv.db
/// (built with db_config).  When --auto-schema is set, missing schemas are
/// derived from the .B file's FCR (File Control Record) and optionally saved
/// back to wxbtrv.db with --save-schema.
use clap::{Parser, Subcommand};
use sqlsrv::SqlConnection;
use std::path::{Path, PathBuf};

mod bfile;
mod schema;
mod sqlsrv;

use btr_types::unpack_row;

// ── CLI ────────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "btr-import", about = "Import Btrieve .B files into SQL Server")]
struct Cli {
    /// Path to wxbtrv.db (default: wxbtrv.db in CWD or C:\\WatkinsX\\bin)
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show file header info: page size, record length, record count, etc.
    Info { file: PathBuf },

    /// Import one or more .B files into SQL Server
    Import {
        #[arg(required = true)]
        files: Vec<PathBuf>,

        /// Override table name (only valid when a single file is given)
        #[arg(long)]
        table: Option<String>,

        /// CREATE TABLE if it doesn't exist
        #[arg(long)]
        create: bool,

        /// TRUNCATE the target table before importing
        #[arg(long)]
        truncate: bool,

        /// Parse and count records without inserting
        #[arg(long)]
        dry_run: bool,

        /// Records per INSERT batch
        #[arg(long, default_value = "200")]
        batch: usize,

        /// Derive schema from the .B FCR if not found in wxbtrv.db
        #[arg(long)]
        auto_schema: bool,

        /// Persist auto-derived schemas back to wxbtrv.db for future use
        #[arg(long)]
        save_schema: bool,

        /// SQL Server collation for VARCHAR columns (e.g. Latin1_General_CI_AS).
        /// Overrides the IMPORT.COLLATION setting in wxbtrv.db.
        /// Leave blank to use the database's default collation.
        #[arg(long)]
        collation: Option<String>,
    },

    /// Import all .B files in a directory
    ImportDir {
        dir: PathBuf,

        #[arg(short = 'r', long)]
        recursive: bool,

        #[arg(long)]
        create: bool,

        #[arg(long)]
        truncate: bool,

        #[arg(long)]
        dry_run: bool,

        #[arg(long, default_value = "200")]
        batch: usize,

        /// Derive schema from the .B FCR if not found in wxbtrv.db
        #[arg(long)]
        auto_schema: bool,

        /// Persist auto-derived schemas back to wxbtrv.db for future use
        #[arg(long)]
        save_schema: bool,

        /// SQL Server collation for VARCHAR columns.
        /// Overrides the IMPORT.COLLATION setting in wxbtrv.db.
        #[arg(long)]
        collation: Option<String>,
    },
}

// ── Entry point ────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.cmd {
        Cmd::Info { file } => cmd_info(&file),

        Cmd::Import {
            files,
            table,
            create,
            truncate,
            dry_run,
            batch,
            auto_schema,
            save_schema,
            collation,
        } => {
            if table.is_some() && files.len() > 1 {
                return Err("--table can only be used with a single .B file".to_string());
            }
            let db_path = schema::find_db(cli.db.as_deref())?;
            let conn = if save_schema {
                schema::open_db_rw(&db_path)?
            } else {
                schema::open_db(&db_path)?
            };
            let cfg = schema::load_config(&conn)?;
            let coll = resolve_collation(&conn, collation.as_deref());

            let mut sql_conn: Option<SqlConnection> = if dry_run {
                None
            } else {
                Some(sqlsrv::connect(&cfg)?)
            };

            for file in &files {
                let tname = table
                    .clone()
                    .unwrap_or_else(|| schema::table_name_from_path(file));
                let int_file = load_schema_for_file(&conn, &tname, file, auto_schema, save_schema)?;
                import_file(
                    file,
                    &int_file,
                    &mut sql_conn,
                    cfg.backend.as_str(),
                    create,
                    truncate,
                    dry_run,
                    batch,
                    &coll,
                )?;
            }
            Ok(())
        }

        Cmd::ImportDir {
            dir,
            recursive,
            create,
            truncate,
            dry_run,
            batch,
            auto_schema,
            save_schema,
            collation,
        } => {
            let files = collect_b_files(&dir, recursive);
            if files.is_empty() {
                return Err(format!("no .B files found in {}", dir.display()));
            }

            let db_path = schema::find_db(cli.db.as_deref())?;
            let conn = if save_schema {
                schema::open_db_rw(&db_path)?
            } else {
                schema::open_db(&db_path)?
            };
            let cfg = schema::load_config(&conn)?;
            let coll = resolve_collation(&conn, collation.as_deref());

            let mut sql_conn: Option<SqlConnection> = if dry_run {
                None
            } else {
                Some(sqlsrv::connect(&cfg)?)
            };

            let mut ok = 0usize;
            let mut skipped = 0usize;
            for file in &files {
                let tname = schema::table_name_from_path(file);
                match load_schema_for_file(&conn, &tname, file, auto_schema, save_schema) {
                    Ok(int_file) => {
                        import_file(
                            file,
                            &int_file,
                            &mut sql_conn,
                            cfg.backend.as_str(),
                            create,
                            truncate,
                            dry_run,
                            batch,
                            &coll,
                        )?;
                        ok += 1;
                    }
                    Err(e) => {
                        eprintln!("skip {} — {}", file.display(), e);
                        skipped += 1;
                    }
                }
            }
            println!("imported {} files, skipped {}", ok, skipped);
            Ok(())
        }
    }
}

// ── Schema resolution ──────────────────────────────────────────────────────────

/// Load schema for a table: first tries wxbtrv.db; if not found and auto_schema
/// is set, derives from the .B file's FCR.  If save_schema is also set and a
/// schema was auto-derived, it is persisted back to wxbtrv.db.
fn load_schema_for_file(
    conn: &rusqlite::Connection,
    table_name: &str,
    file: &Path,
    auto_schema: bool,
    save_schema: bool,
) -> Result<btr_types::IntFile, String> {
    match schema::load_table(conn, table_name) {
        Ok(s) => Ok(s),
        Err(_) if auto_schema => {
            eprintln!(
                "note: '{}' not in wxbtrv.db — deriving schema from FCR",
                table_name
            );
            let b_schema = btr_types::parse_schema(file)?;
            let source_file = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let int_file = btr_types::schema_to_int(&b_schema, table_name, &source_file);

            let uncovered = b_schema.record_length as u32 - btr_types::covered_bytes(&b_schema);
            if uncovered > 0 {
                eprintln!(
                    "  warning: {} bytes in the record are not covered by key fields.",
                    uncovered
                );
                eprintln!("  Non-key fields will be missing from the import.");
                eprintln!(
                    "  Use 'db_config add-field {} ...' to define them, then re-import.",
                    table_name
                );
            }

            if save_schema {
                schema::upsert_table(conn, &int_file)?;
                eprintln!("  schema saved to wxbtrv.db as '{}'", table_name);
            }

            Ok(int_file)
        }
        Err(e) => Err(e),
    }
}

/// Resolve the collation to use: CLI flag overrides, then db config, then empty (database default).
fn resolve_collation(conn: &rusqlite::Connection, cli_override: Option<&str>) -> String {
    if let Some(c) = cli_override {
        return c.to_string();
    }
    schema::get_config(conn, "IMPORT", "COLLATION")
}

// ── info command ───────────────────────────────────────────────────────────────

fn cmd_info(path: &Path) -> Result<(), String> {
    let bf = bfile::BtrieveFile::open(path)?;
    let h = &bf.header;
    println!("File:              {}", path.display());
    println!("Version:           Btrieve {}", h.version);
    println!("Page size:         {} bytes", h.page_size);
    println!("Logical rec len:   {} bytes", h.logical_rec_len);
    println!("Physical rec len:  {} bytes", h.physical_rec_len);
    println!("Key count:         {}", h.key_count);
    println!("Declared records:  {}", h.record_count);
    println!("Page count:        {}", h.page_count);
    println!(
        "File size:         {} bytes",
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    );

    // Full FCR schema parse for key details
    match btr_types::parse_schema(path) {
        Ok(s) => {
            println!("Record kind:       {:?}", s.record_kind);
            for key in &s.keys {
                for (i, seg) in key.segments.iter().enumerate() {
                    println!(
                        "  Key {} seg {}: offset={} len={} type={} null_value={} desc={} dups={}",
                        key.number + 1,
                        i + 1,
                        seg.offset,
                        seg.length,
                        seg.data_type,
                        seg.null_value,
                        seg.descending,
                        seg.allows_dups
                    );
                }
            }
            let uncovered = s.record_length as u32 - btr_types::covered_bytes(&s);
            if uncovered > 0 {
                println!(
                    "Uncovered bytes:   {} (non-key fields unknown without INT file)",
                    uncovered
                );
            }
        }
        Err(e) => eprintln!("  (FCR key parse failed: {})", e),
    }

    let active: u32 = bf.records().count() as u32;
    println!("Active records:    {}", active);
    Ok(())
}

// ── import one file ────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn import_file(
    path: &Path,
    int_file: &btr_types::IntFile,
    sql_conn: &mut Option<SqlConnection>,
    backend: &str,
    create: bool,
    truncate: bool,
    dry_run: bool,
    batch_sz: usize,
    collation: &str,
) -> Result<(), String> {
    let bf = bfile::BtrieveFile::open(path)?;
    let h = &bf.header;

    if h.logical_rec_len > 0 && h.logical_rec_len != int_file.record_length {
        eprintln!(
            "warning: {} FCR rec_len={} but schema record_length={} — using schema",
            path.display(),
            h.logical_rec_len,
            int_file.record_length
        );
    }

    if let Some(conn) = sql_conn.as_mut() {
        if create {
            let ddl = sqlsrv::gen_create_table(backend, int_file, collation);
            sqlsrv::execute(conn, &ddl)
                .map_err(|e| format!("CREATE TABLE failed for {}: {}", int_file.table_name, e))?;
        }
        if truncate {
            let tref = sqlsrv::table_ref(backend, int_file);
            // SQLite has no TRUNCATE; use DELETE which is equivalent for
            // empty-tables semantics. Postgres + MSSQL accept TRUNCATE.
            let stmt = if backend == "sqlite" {
                format!("DELETE FROM {}", tref)
            } else {
                format!("TRUNCATE TABLE {}", tref)
            };
            sqlsrv::execute(conn, &stmt)
                .map_err(|e| format!("TRUNCATE failed for {}: {}", int_file.table_name, e))?;
        }
    }

    let mut total = 0u64;
    let mut batch: Vec<Vec<(String, String)>> = Vec::with_capacity(batch_sz);

    for record in bf.records() {
        let row = unpack_row(&int_file.fields, &record);
        batch.push(row);

        if batch.len() >= batch_sz {
            if let Some(conn) = sql_conn.as_mut() {
                sqlsrv::batch_insert(conn, int_file, &batch, dry_run)?;
            }
            total += batch.len() as u64;
            batch.clear();
        }
    }

    if !batch.is_empty() {
        if let Some(conn) = sql_conn.as_mut() {
            sqlsrv::batch_insert(conn, int_file, &batch, dry_run)?;
        }
        total += batch.len() as u64;
    }

    let mode = if dry_run { " (dry-run)" } else { "" };
    println!(
        "{}: {} records → {}{}",
        path.display(),
        total,
        sqlsrv::table_ref(backend, int_file),
        mode
    );
    Ok(())
}

// ── directory collector ────────────────────────────────────────────────────────

fn collect_b_files(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && recursive {
                stack.push(path);
            } else if path.is_file()
                && path
                    .extension()
                    .map(|e| e.to_string_lossy().eq_ignore_ascii_case("b"))
                    .unwrap_or(false)
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
