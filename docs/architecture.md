# Architecture

## What this is

A drop-in replacement for the DOS Btrieve runtime. The DOS application keeps
calling the Btrieve API; underneath, every call is served by Rust against
SQL Server. No Pervasive/Btrieve runtime needs to be present on the target
machine.

## The replacement, end-to-end

```
DOS application (16-bit, NTVDM)
        │  Btrieve API call
        ▼
wxbtrv.sys              ← our 16-bit DOS device driver, hooks INT 7B
        │
        ▼
wxbtrv.dll              ← our 32-bit Rust DLL: NTVDM VDD + Btrieve engine
        │
        ▼
SQL Server              ← via ODBC
```

## Crates

The runtime splits in two so the op logic can be unit-tested without a
Windows build:

- **`wxbtrv-core`** — portable rlib. Every Btrieve op, the SQL translation,
  the SQLite config loader, and the state. Host-native. The integration
  test harness links this crate directly.
- **`wxbtrv`** — the Windows `cdylib` that ships as `wxbtrv.dll`. Contains
  `DllMain`, the VDD glue, and the C ABI exports. Thin wrapper over
  `wxbtrv-core`.

Plus the support crates:

- **`wxbtrv-sys`** — the 16-bit DOS driver (NASM).
- **`btr-types`** — INT/MDS parser used only by the import/config tools.
- **`db-config`** — CLI that builds and edits `wxbtrv.db`.
- **`btr-import`** — CLI that migrates legacy `.B` flat files into SQL Server.
- **`installer`** — self-bundling Windows installer that ships in releases.
- **`btr-test-harness`** — 56 integration tests against a real SQL Server.

## Configuration

`wxbtrv.db` (SQLite) holds the SQL Server connection, per-directory
overrides, and every table's schema. The DLL reads it at startup and never
touches `.INT` files at runtime. Schemas get into `wxbtrv.db` once, via
`db-config import-int` or `db-config analyze-b`.

The DLL searches for `wxbtrv.db` in this order:

1. `WXBTRV_CONFIG_DIR` environment variable
2. Current working directory
3. The directory containing `wxbtrv.dll`
4. The compile-time `WXBTRV_CONFIG_DIR`
5. `C:\WatkinsX\bin`

A standard install drops it at `C:\WatkinsX\bin\wxbtrv.db` and uses no env
vars.

## Implementation Status

| Feature | Status |
|---------|--------|
| DOS device driver (`wxbtrv.sys`) | Complete |
| VDD + Btrieve engine (`wxbtrv.dll`) | Complete |
| Btrieve ops via SQL Server | Complete (Open, Close, Get, Step, Insert, Update, Delete, transactions) |
| Portable core (`wxbtrv-core`) | Complete |
| Integration test harness | Complete (56 opcode tests) |
| `db-config` (schemas, connection, DDL) | Complete |
| `.B` flat-file importer (`btr-import`) | Complete |
| Installer | Complete (shipped as `wlbtr-installer-vX.Y.Z-windows-i686.exe`) |
