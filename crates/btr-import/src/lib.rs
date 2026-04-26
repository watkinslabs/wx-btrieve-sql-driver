//! btr-import — library half of the .B → SQL importer.
//!
//! The bin (`src/main.rs`) is a thin CLI wrapper. The web server
//! (`crates/wxbtrv-web`) consumes the same modules directly so it can
//! drive imports from a UI without shelling out.

pub mod bfile;
pub mod schema;
pub mod sqlsrv;
