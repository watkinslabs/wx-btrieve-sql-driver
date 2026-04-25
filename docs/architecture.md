# Architecture

## Btrieve Paths

Three distinct stacks can sit underneath a DOS Btrieve application. This
project replaces one of them end-to-end and provides tooling to migrate off
the other two.

### Path 1 — Native Btrieve (flat files)

```
DOS application (16-bit, running in NTVDM)
      │
      │  calls INT 7B
      ▼
BTRDRVR.SYS  — Pervasive DOS device driver
      │  BOP → NTVDM
      ▼
BTRVDD.DLL   — Pervasive VDD shim
      │
      ▼
w3btrv7.dll  — Pervasive Btrieve engine
      │
      ▼
.B files     — Btrieve flat-file database on local/network disk
```

The original Btrieve stack. Data lives in proprietary `.B` flat files.
No SQL Server. The runtime path is not replaced by this project, but
**`btr-import`** handles one-time migration of the data to SQL Server.

Two schema-sourcing options exist for migration (detailed in
[Installation](installation.md#migrating-from-btrieve-flat-files-b)):

| Situation | How to get schemas into wxbtrv.db |
|-----------|-----------------------------------|
| Have `.INT` sidecar files | `db-config import-int` — complete field definitions |
| Only `.B` files, no INT | `db-config analyze-b` — bootstraps from FCR; key fields auto-extracted, non-key fields added manually |

After schemas are in `wxbtrv.db`, `btr-import import-dir --create` creates the
SQL Server tables and loads all records. The DOS app can then be switched to
Path 3.

### Path 2 — Pervasive PSQL

```
DOS application (16-bit, running in NTVDM)
      │
      │  calls INT 7B
      ▼
BTRDRVR.SYS  — Pervasive DOS device driver
      │  BOP → NTVDM
      ▼
BTRVDD.DLL   — Pervasive VDD shim
      │
      ▼
w3btrv7.dll  — Pervasive Btrieve engine (PSQL variant)
      │
      ▼
Pervasive PSQL Server  — proprietary SQL engine bundled with Pervasive
```

Pervasive PSQL layers SQL on top of Btrieve using their own server.
Internal protocol is proprietary. **Not targeted by this project.**

### Path 3 — wxbtrv → SQL Server (our target)

```
DOS application (16-bit, running in NTVDM)
      │
      │  calls INT 7B
      ▼
BTRDRVR.SYS  — Pervasive DOS device driver  ← replaced by wxbtrv.sys
      │  BOP → NTVDM
      ▼
BTRVDD.DLL   — Pervasive VDD shim           ← eliminated (merged into wxbtrv.dll)
      │
      ▼
w3btrv7.dll  — Btrieve engine DLL           ← replaced by wxbtrv.dll
      │
      ▼
SQL Server
```

Our stack eliminates every third-party file in the runtime path:

- `BTRDRVR.SYS` → replaced by `wxbtrv.sys` (16-bit NASM DOS driver)
- `BTRVDD.DLL` → eliminated; VDD glue merged directly into `wxbtrv.dll`
- `w3btrv7.dll` → replaced by `wxbtrv.dll` (32-bit Rust Btrieve engine)

No vendor Btrieve runtime is required on the target machine. Legacy `.INT`
files are read only once, at migration time, by `db-config import-int` — never
by the DLL at runtime.

---

## Crate Split

The runtime is split into two crates so the op logic can be unit-tested on
any host without pulling in Windows APIs:

- **`wxbtrv-core`** — portable rlib. Contains every Btrieve op, the SQL
  Server translation layer, the SQLite config loader, and all state
  management. No Windows dependencies, no `btr-types`, no INT file parser.
  Unit-testable on Linux/macOS/Windows. The integration test harness
  (`btr-test-harness`) links this crate directly and calls
  `btrcall_internal`.
- **`wxbtrv`** — thin Windows cdylib shim. Contains `DllMain`, the VDD glue
  (`vdd.rs`), and the exported C ABI wrappers (`BTRCALL`, `BTRCALLID`, `WB*`,
  `DBU*`, `Mds*`). Depends on `wxbtrv-core`.

---

## Our Call Chain

```
DOS application (16-bit, running in NTVDM)
      │
      │  calls INT 7B  (the Btrieve software interrupt)
      ▼
wxbtrv.sys  — our DOS character device driver (16-bit, NASM)
      │
      │  BOP 58h  — NTVDM magic that switches V86 → Win32
      ▼
wxbtrv.dll  — our Win32 VDD + Btrieve engine (32-bit Rust)
      │
      │  VDDDispatch: reads DS:DX register from NTVDM,
      │  calls MGetVdmPointer to convert DOS seg:off → 32-bit ptr,
      │  unpacks the 28-byte BtrCallBlock, calls wxbtrv-core's btrcall_internal
      ▼
SQL Server  — via ODBC, using connection config in wxbtrv.db
```

---

## NTVDM BOP Mechanism

NTVDM uses a 3-byte instruction sequence as a trap door between 16-bit V86 mode
and Win32:

```
C4 C4 <code>
```

`wxbtrv.sys` uses two BOP codes:

- **BOP 59h** — `RegisterModule`: tells NTVDM to load `wxbtrv.dll` as a VDD.
  Registers: DS:SI = DLL filename, DS:DI = init export name, DS:BX = dispatch export name.

- **BOP 58h** — `CALL_VDD`: fires on every INT 7B. NTVDM pauses the V86 CPU,
  calls `VDDDispatch` in `wxbtrv.dll`, then resumes. DS:DX at the time of the
  BOP points to the caller's `BtrCallBlock`.

NTVDM loads VDD DLLs via `LoadLibraryExA` with flag
`LOAD_LIBRARY_SEARCH_SYSTEM32` (0x800). That flag ignores absolute paths and
only searches `C:\Windows\System32\`, which is why `wxbtrv.dll` must be
deployed there. The `.sys` driver passes the bare filename `wxbtrv.dll` (no
path) in its RegisterModule call for this reason.

---

## BtrCallBlock Layout

28-byte structure at DS:DX when INT 7B fires. The DOS driver fills this in
before firing BOP 58h; `VDDDispatch` reads it via `MGetVdmPointer`.

```
offset  size  field
  0x00    4   data_buf_ptr  DOS seg:off → data buffer
  0x04    2   data_len      data buffer length (in/out)
  0x06    4   posblk_ptr    DOS seg:off → 128-byte position block
  0x0a    2   acs           ACS value (usually 0)
  0x0e    2   op_code       Btrieve operation (0=Open, 1=Close, 2=Insert, ...)
  0x10    4   key_buf_ptr   DOS seg:off → key buffer
  0x14    1   key_num       key number (signed)
  0x15    1   key_length    key buffer length
  0x16    4   client_id_ptr DOS seg:off → 16-byte client ID (BTRCALLID only)
```

---

## Runtime Config Lookup

At DLL startup `wxbtrv-core::sqlite_meta::find_sqlite_db` searches for
`wxbtrv.db` in this order and uses the first match:

1. `WXBTRV_CONFIG_DIR` environment variable (runtime override)
2. Current working directory
3. Directory containing `wxbtrv.dll` (Windows only)
4. Compile-time `WXBTRV_CONFIG_DIR` baked into the binary
5. `C:\WatkinsX\bin` (production default)

---

## Implementation Status

| Feature | Status | Notes |
|---------|--------|-------|
| DOS device driver (`wxbtrv.sys`) | **Complete** | 16-bit NASM, hooks INT 7B, loads VDD via BOP 59h |
| VDD interface (`wxbtrv.dll`) | **Complete** | VDDInitialize/VDDDispatch merged into DLL |
| Btrieve ops via SQL Server | **Complete** | Open, Close, Get, Step, Insert, Update, Delete, transactions |
| Table schema from SQLite | **Complete** | `wxbtrv.db` holds all schemas and connection details |
| Portable core (`wxbtrv-core`) | **Complete** | All op logic host-native testable |
| Integration test harness (`btr-test-harness`) | **Complete** | 56 tests, one per opcode, against real SQL Server |
| `db-config` schema import | **Complete** | Multi-dir recursive INT file import, set-connection command |
| DDL generator | **Partial** | `db-config gen-ddl` emits CREATE TABLE DDL from imported schemas |
| `.B` flat-file importer (`btr-import`) | **Complete** | Reads Btrieve v5/v6 files, decodes records via INT schema, bulk-inserts into SQL Server |
| Installer (`installer` crate) | **Complete** | Self-bundling Windows installer; embeds DLL + sys + CLIs via `include_bytes!`. Shipped as `wlbtr-installer-vX.Y.Z-windows-i686.exe` in each [release](https://github.com/watkinslabs/wx-btrieve-mssql-driver/releases). |
