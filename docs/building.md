# Building

## Prerequisites

- Rust toolchain with `i686-pc-windows-gnu` target:
  ```bash
  rustup target add i686-pc-windows-gnu
  ```
- MinGW i686 cross-compiler (for the DLL builds):
  ```bash
  # Debian/Ubuntu
  apt install gcc-mingw-w64-i686
  ```
- NASM (for the DOS driver):
  ```bash
  apt install nasm
  ```

---

## wxbtrv-core (portable rlib)

All op logic lives here and builds host-native. No Windows dependencies, so
it's the fastest way to check a change compiles.

```bash
cargo build -p wxbtrv-core
cargo test  -p wxbtrv-core
```

## wxbtrv.dll

The thin Windows cdylib shim that wraps `wxbtrv-core`. Provides `DllMain`,
the VDD glue, and the exported C ABI (`BTRCALL`, `BTRCALLID`, `WB*`, `DBU*`,
`Mds*`). Requires the MinGW i686 cross-compiler.

```bash
cargo build -p wxbtrv --release --target i686-pc-windows-gnu
# Output: target/i686-pc-windows-gnu/release/wxbtrv.dll
```

Deploy target: **`C:\Windows\System32\wxbtrv.dll`** — see
[Installation](installation.md) for why System32 is required.

## wxbtrv.sys

The 16-bit DOS device driver. Requires NASM. The build script assembles it
automatically.

```bash
cargo build -p wxbtrv-sys
# Output: target/debug/build/wxbtrv-sys-*/out/wxbtrv.sys
```

Or assemble directly:

```bash
nasm -f bin -o wxbtrv.sys crates/wxbtrv-sys/src/wxbtrv.asm
```

## db-config

The config database CLI. Builds natively on Linux or Windows.

```bash
cargo build -p db-config --release
# Output: target/release/db-config  (or db-config.exe on Windows)
```

## btr-import (Linux or Windows native)

```bash
cargo build -p btr-import --release
# Output: target/release/btr-import  (or btr-import.exe on Windows)
```

## wxbtrv-web (Linux or Windows native)

Single-binary local web UI. The build script bundles the React UI via
`npm install` + `npm run build` before cargo embeds it with rust-embed.

```bash
# Linux (needs node + libdbus-1-dev for rfd's xdg-portal feature)
sudo apt install -y nodejs npm libdbus-1-dev pkg-config unixodbc-dev
cargo build -p wxbtrv-web --release
# Output: target/release/wxbtrv-web

# Windows (PowerShell, with node + Rust toolchain)
cargo build -p wxbtrv-web --release
# Output: target\release\wxbtrv-web.exe
```

If `npm` is missing the build still succeeds but the UI is replaced
with a placeholder page (the `/api/*` endpoints still work). Set
`WXBTRV_WEB_SKIP_UI=1` to skip the UI build deliberately.

## btr-test-harness

Host-native integration test suite. 56 tests, one per opcode, running
against a real SQL Server. See [Testing](testing.md) for setup and
execution; the short version:

```bash
bash scripts/test-setup.sh
cargo test -p btr-test-harness -- --test-threads=1
```

---

## Crate Layout

```
crates/
  btr-types/          Shared library: INT file parser, MDS.INI parser,
                      Btrieve type definitions, record codec.
                      Used by db-config and btr-import — NOT by
                      wxbtrv-core or the runtime DLL.

  wxbtrv-core/        Portable rlib. All op logic, SQL translation, SQLite
                      config loader, state. Host-native, unit-testable.
  wxbtrv/             wxbtrv.dll — thin Windows cdylib shim (DllMain + VDD
                      glue + C ABI exports). Depends on wxbtrv-core.
  wxbtrv-sys/         wxbtrv.sys — DOS device driver (NASM, 16-bit).

  db-config/          CLI for managing wxbtrv.db: import INT/MDS files, set
                      connection, inspect schemas, generate DDL.
  btr-import/         CLI for importing .B flat files directly into SQL Server.
  btr-test-harness/   Integration test harness (56 opcode tests against SQL
                      Server). Links wxbtrv-core directly, no DLL needed.
  installer/          Self-bundling Windows installer. Embeds wxbtrv.dll,
                      wxbtrv.sys, db_config.exe, and btr-import.exe via
                      include_bytes! when WXBTRV_DLL / WXBTRV_SYS /
                      INT_TOOL_EXE / BTR_IMPORT_EXE point at prebuilt
                      binaries (the release workflow and Makefile both do).
```

Internal development tools (proxy DLL for diff runs against a reference
Pervasive install, differential exerciser, NTVDM-driving MCP server) live
in a separate sibling repo,
[`watkinslabs/wlbtr_testing`](https://github.com/watkinslabs/wlbtr_testing),
and are not required to build or run wlbtr.

---

## Building the installer locally

The simplest path is `make installer`, which sets the payload env vars
correctly:

```bash
make dll sys db-config btr-import installer
# Output: target/i686-pc-windows-gnu/release/installer.exe
```

If you invoke cargo directly, set the env vars yourself so the build script
embeds the binaries:

```bash
WIN=target/i686-pc-windows-gnu/release \
WXBTRV_DLL="$PWD/$WIN/wxbtrv.dll"     \
WXBTRV_SYS="$PWD/$WIN/wxbtrv.sys"     \
INT_TOOL_EXE="$PWD/$WIN/db_config.exe" \
BTR_IMPORT_EXE="$PWD/$WIN/btr-import.exe" \
cargo build -p installer --release --target i686-pc-windows-gnu
```

If those env vars are unset, `cargo build` still succeeds but the resulting
`installer.exe` is hollow — every "drop file X" step will warn at runtime.
The build script prints a `cargo:warning=Payload not set ...` line so the
state is obvious in build output.

---

## Debugging

Trace output in `wxbtrv` is only compiled into debug builds. Release builds
of `wxbtrv.dll` have all trace calls compiled out via
`#[cfg(debug_assertions)]`, so there is no release-mode overhead or log file.

Debug builds of `wxbtrv.dll` log to `C:\WatkinsX\logs\wxbtrv_<timestamp>.log`
(with fallbacks to `C:\WatkinsX\bin\`, `C:\Windows\Temp\`, or CWD).

The trace level is also runtime-configurable via the `WXBTRV_TRACE_LEVEL`
environment variable (`off`, `error`, `info`, `debug`) — useful for turning
verbose logging on in a release build without rebuilding.

---

## db-config Reference

```
db-config [--db <path>] <command>

  init                              Create a new empty wxbtrv.db
  import-int [-r] <dir> [<dir>...]  Import .INT files (use -r for recursive)
  import-mds <file>                 Import SQL Server config from an mds.ini file
  set-connection                    Set SQL Server connection details directly
    --server <host>
    --database <name>
    --schema <name>
    --driver <odbc-driver>
    --network <DBMSSOCN|tcp:>
    --user <login>
    --password <pass>
    --trusted-connection <bool>
    --encrypt <bool>
    --trust-server-certificate <bool>
    --recnum-column <name>
  test-connection                   Try each configured ODBC driver against the server
  list                              List all imported tables
  show <table>                      Show full schema for a table
  show-config                       Show all config values (mds.ini equivalents)
  show-table-config <table>         Show resolved table config as YAML (optionally --discover from SQL Server)
  export-int [<table>...]           Export INT files back to disk
  export-mds                        Export mds.ini from config
  set-config <section> <key> <value>   Set a raw config value
  set-table <table> <key> <value>   Update a table property (db_name, schema_name, ...)
  add-field <table> ...             Add or replace a field definition
  rm-field <table> <num>            Remove a field
  add-index <table> ...             Add or replace an index
  rm-index <table> <num>            Remove an index
  rm-table <table>                  Delete a table and all its fields/indexes
  analyze-b <file.B>                Parse a .B FCR and bootstrap a schema
  gen-ddl [<table>...]              Generate SQL Server CREATE TABLE DDL
  migration-status                  Show which tables have been migrated
  mark-migrated <table>             Mark a table as migrated
  clear-migrated <table>            Clear migration flag

Default --db path: wxbtrv.db next to the binary, or C:\WatkinsX\bin\wxbtrv.db on Windows.
```

---

## btr-import Reference

```
btr-import [--db <wxbtrv.db>] <command>

  info <file.B>
      Show file header: Btrieve version, page size, record length, key structure,
      uncovered bytes (non-key fields not in FCR).

  import [options] <file.B> [<file.B>...]
      Import one or more .B files into SQL Server.
      --table <name>      Override table name (single-file only)
      --create            CREATE TABLE if it does not exist
      --truncate          TRUNCATE the target table before importing
      --dry-run           Parse and count without inserting
      --batch <n>         Records per INSERT batch (default 200)
      --auto-schema       Derive schema from FCR if not found in wxbtrv.db
      --save-schema       Persist auto-derived schema back to wxbtrv.db
      --collation <name>  VARCHAR collation (overrides IMPORT.COLLATION in wxbtrv.db)

  import-dir [options] <dir>
      Import all .B files in a directory.
      -r, --recursive     Scan subdirectories recursively
      --create / --truncate / --dry-run / --batch   (same as import)
      --auto-schema / --save-schema / --collation   (same as import)

Default --db path: wxbtrv.db in CWD or C:\WatkinsX\bin.
```

### Collation

SQL Server collation controls how VARCHAR data is sorted and compared.
Set a project-wide default in wxbtrv.db so you don't have to repeat it:

```bat
db-config set-config IMPORT COLLATION Latin1_General_CI_AS
```

Common collation values:

| Collation | Description |
|-----------|-------------|
| *(empty)*  | Inherit from database (SQL Server default) |
| `Latin1_General_CI_AS` | Latin, case-insensitive, accent-sensitive |
| `Latin1_General_CS_AS` | Latin, case-sensitive, accent-sensitive |
| `Latin1_General_BIN` | Binary (byte-by-byte comparison) |
| `SQL_Latin1_General_CP1_CI_AS` | SQL Server legacy default |
| `Latin1_General_100_CI_AS_SC_UTF8` | UTF-8 capable |

The `--collation` flag on the import command overrides the db setting for that run.

### Typical migration workflows

**With INT files:**
```bat
db-config import-int -r G:\pacific G:\canada J:\advdata
db-config set-connection --server 10.0.0.231 --database GCanada --user sa --password ...
db-config set-config IMPORT COLLATION Latin1_General_CI_AS
btr-import import-dir --create G:\pacific
```

**Without INT files (bootstrap from .B FCR):**
```bat
db-config init
db-config set-connection --server 10.0.0.231 --database GCanada --user sa --password ...
db-config set-config IMPORT COLLATION Latin1_General_CI_AS
btr-import import-dir --create --auto-schema --save-schema G:\pacific
rem Schemas saved; add non-key fields manually:
db-config show BKGLTRAN           ← see what key fields were found
db-config add-field BKGLTRAN --num 5 --name DESCRIPTION --type 0 --length 30 --offset 12
rem Re-import with full schema:
btr-import import --truncate G:\pacific\BKGLTRAN.B
```
