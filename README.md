# wlbtr — WatkinsX Btrieve Replacement Stack

Full Rust replacement for the DOS Btrieve stack. DOS applications that call
the Btrieve API continue to work unchanged; all database I/O is served by our
components running against SQL Server. No legacy vendor drivers are required
on the target machine.

---

## Documentation

- [Architecture](docs/architecture.md) — call chain, crate layout, implementation status
- [Installation](docs/installation.md) — step-by-step setup on a Windows target
- [Building](docs/building.md) — build instructions and crate reference
- [Testing](docs/testing.md) — how to run the integration harness against SQL Server

---

## Components at a glance

| Component | Crate | Role |
|---|---|---|
| `wxbtrv.sys` | `wxbtrv-sys` | 16-bit DOS device driver. Loaded by `config.nt`, hooks INT 7B, calls into the DLL via NTVDM BOP. |
| `wxbtrv.dll` | `wxbtrv` (+ `wxbtrv-core`) | 32-bit Rust DLL. Acts as both NTVDM VDD and Btrieve engine. Every Btrieve call is translated into SQL Server operations. |
| `wxbtrv.db` | — | SQLite database holding SQL Server credentials, per-directory config, and every table schema. Read at startup by the DLL; populated once via `db-config`. |
| `db-config` | `db-config` | CLI for building and managing `wxbtrv.db`: set connection, import legacy `.INT` files, inspect schemas. |
| `btr-import` | `btr-import` | CLI for migrating legacy Btrieve `.B` flat files into SQL Server in one pass. |
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
