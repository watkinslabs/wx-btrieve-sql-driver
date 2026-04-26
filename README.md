# wlbtr — WatkinsX Btrieve Replacement Stack

Full Rust replacement for the DOS Btrieve stack. DOS applications that call
the Btrieve API continue to work unchanged; all database I/O is served by our
components running against SQL Server. No legacy vendor drivers are required
on the target machine.

---

## Install (end users)

Download the latest release from
[**Releases**](https://github.com/watkinslabs/wx-btrieve-mssql-driver/releases/latest)
and run the Windows installer.

| Platform | File | What it contains |
|---|---|---|
| Windows i686 (32-bit) | `wlbtr-installer-vX.Y.Z-windows-i686.exe` | Self-contained installer that drops `wxbtrv.dll`, `wxbtrv.sys`, `db_config.exe`, and `btr-import.exe` into place and patches `config.nt`. |
| Linux x86_64 | `wlbtr-tools-vX.Y.Z-linux-x86_64.tar.gz` | `db-config` and `btr-import` for managing `wxbtrv.db` from a Linux dev box. There is no Linux runtime — the runtime is the Windows DLL. |
| Linux x86_64 | `wxbtrv-web-vX.Y.Z-linux-x86_64.tar.gz` | `wxbtrv-web` — a single-binary local web UI for managing `wxbtrv.db`. Runs on `127.0.0.1`, embeds the React UI. |
| Windows x86_64 | `wxbtrv-web-vX.Y.Z-windows-x86_64.zip` | Same web UI for Windows. |

Detailed walkthroughs: [Installation](docs/installation.md), [Web UI](docs/web.md).

---

## Documentation

- [Migrate](docs/migrate.md) — end-to-end walkthrough: Btrieve → SQL backend → DLL up
- [Architecture](docs/architecture.md) — call chain, crate layout, implementation status
- [Installation](docs/installation.md) — step-by-step setup on a Windows target
- [Web UI](docs/web.md) — the `wxbtrv-web` local server: project file picker, connection editor, schema diff, data browser, .B import
- [Building](docs/building.md) — build instructions and crate reference
- [Testing](docs/testing.md) — how to run the integration harness against SQL Server

---

## Components

| Component | Crate | Role |
|---|---|---|
| `wxbtrv.sys` | `wxbtrv-sys` | 16-bit DOS device driver. Loaded by `config.nt`, hooks INT 7B, calls into the DLL via NTVDM BOP. |
| `wxbtrv.dll` | `wxbtrv` (+ `wxbtrv-core`) | 32-bit Rust DLL. Acts as both NTVDM VDD and Btrieve engine. Every Btrieve call is translated into SQL Server operations. |
| `wxbtrv.db` | — | SQLite database holding SQL Server credentials, per-directory config, and every table schema. Read at startup by the DLL; populated once via `db-config`. |
| `db-config` | `db-config` | CLI for building and managing `wxbtrv.db`: set connection, import legacy `.INT` files, inspect schemas. |
| `btr-import` | `btr-import` | CLI for migrating legacy Btrieve `.B` flat files into SQL Server / Postgres / SQLite in one pass. |
| `wxbtrv-web` | `wxbtrv-web` | Self-serving local web UI (Rust HTTP server + embedded React) covering everything `db-config` and `btr-import` do, plus schema diff and live row preview. Localhost-only. |
| `installer` | `installer` | Self-bundling Windows installer that ships in the release. |
| `btr-test-harness` | `btr-test-harness` | Host-native integration test suite. 56 tests, one per opcode, running against a real SQL Server. |

---

## Quick start (developer)

```bash
# Build the runtime core (host-native)
cargo build -p wxbtrv-core

# Build the production DLL (32-bit Windows)
cargo build -p wxbtrv --release --target i686-pc-windows-gnu

# Run the integration tests against a docker SQL Server
bash scripts/test-setup.sh
cargo test -p btr-test-harness -- --test-threads=1
```

Cutting a release: trigger the `Release` workflow from the GitHub Actions
tab. It bumps the workspace version, builds everything, tags, and publishes
the two-file release described above. See
[`.github/workflows/release.yml`](.github/workflows/release.yml).

---

## License

TBD.
