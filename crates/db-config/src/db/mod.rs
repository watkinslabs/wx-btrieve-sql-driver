use rusqlite::{Connection, Result};
use std::path::Path;

pub mod config;
pub mod migration;
pub mod schema;
pub mod tables;

pub use config::*;
pub use migration::*;
pub use schema::*;
pub use tables::*;

pub fn init(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}

pub fn open(path: &Path) -> Result<Connection> {
    if !path.exists() {
        return Err(rusqlite::Error::InvalidPath(path.to_path_buf()));
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}
