# Testing

wlbtr ships with an integration test harness (`crates/btr-test-harness`) that
exercises every Btrieve opcode against a real SQL Server instance. Tests run
on any host (Linux, macOS, Windows) — the harness depends on `wxbtrv-core` as
an rlib and calls `btrcall_internal` directly, no DLL loading required.

The same suite runs on every push as part of CI
([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)) on an
`ubuntu-22.04` runner with `msodbcsql17` and a
`mcr.microsoft.com/mssql/server:2022-latest` service container.

## Prerequisites

- `cargo` (stable toolchain)
- `docker` or `podman` — for the local SQL Server container
- `/opt/microsoft/msodbcsql17/` (or equivalent) — ODBC Driver 17 for SQL Server
- `unixODBC` (Linux) — driver manager
- `/opt/mssql-tools/bin/sqlcmd` (optional, for manual inspection)

On Fedora:
```bash
sudo dnf install -y unixODBC
# msodbcsql17 via Microsoft repo (https://learn.microsoft.com/sql/connect/odbc/linux-mac/installing-the-microsoft-odbc-driver-for-sql-server)
```

## One-time setup

The bootstrap script starts a disposable SQL Server container and creates the
fixture database. Safe to rerun — it's idempotent.

```bash
bash scripts/test-setup.sh
```

What it does:

1. Ensures the `wxbtrv-test-sql` docker container is running (creates it on
   first run: `mcr.microsoft.com/mssql/server:2022-latest`, port `1433`,
   SA password `WxTest!2024`).
2. Waits for SQL Server to accept connections.
3. Drops and recreates the `WXBTRV_TEST` database from
   `crates/btr-test-harness/fixtures/schema.sql`.
4. Seeds the fixture table `TEST_CUST` with 10 deterministic rows.

If the container doesn't exist yet, the script tells you the exact
`docker run` command to create it.

## Running the tests

```bash
cargo test -p btr-test-harness -- --test-threads=1
```

- `--test-threads=1` is required because every test calls `reset_fixture()`,
  which drops and recreates the database. Parallel tests would race on the
  shared SQL Server.
- Each test file under `crates/btr-test-harness/tests/` is compiled as its
  own binary, so global state in `wxbtrv-core` (ODBC connection, handle
  table, txn flag) is isolated per test.

Expected result: **56 passed, 0 failed**.

## Configuration

The harness honors these environment variables (all optional):

| Variable | Default | Purpose |
|---|---|---|
| `BTR_TEST_SERVER` | `localhost,1433` | SQL Server host:port |
| `BTR_TEST_USER` | `sa` | SQL login |
| `BTR_TEST_PASS` | `WxTest!2024` | SQL password |

For CI, set these explicitly and point at a dedicated test instance.

## Test layout

```
crates/btr-test-harness/
├── Cargo.toml                    depends on wxbtrv-core + odbc-api + rusqlite
├── fixtures/
│   └── schema.sql                CREATE DATABASE + TEST_CUST + 10 seed rows
├── src/
│   └── lib.rs                    reset_fixture(), fixture_open(), btrcall(), ...
└── tests/
    ├── op_00_open.rs             one file per opcode (or group)
    ├── op_01_close.rs
    ├── op_01b_close_invalid.rs   negative test
    ├── op_02_insert.rs
    ├── ...
    ├── op_55_63_get_key.rs       9 tests for the +50 bias variants
    └── op_1019_concurrent_txn.rs
```

## Coverage

Every Btrieve opcode in the 1998 Programmer's Reference has at least one
integration test:

| Category | Ops | Tests |
|---|---|---|
| File | 0, 1, 14, 15, 16, 17, 18, 25, 26, 42, 65 | 12 |
| Record insert/modify | 2, 3, 4, 40, 53 | 5 |
| Get (key-ordered) | 5, 6, 7, 8, 9, 10, 11, 12, 13 | 10 |
| Step (physical) | 24, 33, 34, 35 | 4 |
| Extended get/step | 36, 37, 38, 39 | 4 |
| Position / direct | 22, 23, 44, 45 | 4 |
| Transaction | 19+20, 19+21, 1019 | 3 |
| Concurrency | 27, 28 | 2 |
| Owner / Index | 29+30, 31+32 | 2 |
| Get Key (+50 bias) | 55-63 | 9 |
| Directory | 17, 18 | (in file test) |

**Total: 56 tests, 44 test binaries.**

## Writing a new test

Template:

```rust
use btr_test_harness as h;
use wxbtrv_core::constants::*;

#[test]
fn op_NN_description() {
    h::reset_fixture();

    // Open the fixture table
    let (mut posblk, rc) = h::fixture_open("TEST_CUST.B");
    assert_eq!(rc, 0, "open failed");

    // Build buffers
    let mut data = [0u8; 256];
    let mut key = [0u8; 80];
    let mut dlen: u32 = data.len() as u32;

    // Call the op
    let rc = h::btrcall(OP_CODE, &mut posblk[..], &mut data[..], &mut dlen, &mut key[..], 0);

    // Assert result
    assert_eq!(rc, 0);
}
```

Helpers available in `btr_test_harness`:

- `reset_fixture()` — drop+recreate the test database, reseed, reset `wxbtrv-core` state
- `fixture_open(path)` — convenience wrapper for op 0 with a fresh posblk
- `btrcall(op, posblk, data, dlen, key, key_num)` — thin wrapper around `btrcall_internal`
- `new_posblk()` — zeroed 128-byte position block
- `test_connection_string()` — ODBC connection string to the fixture database

Keep each test in its own `#[test]` fn inside its own file unless you're
grouping closely-related variants (e.g. the 9 Get Key +50 tests). One file
per op is the convention because cargo runs them as separate processes,
which gives the cleanest isolation.

## Adding a fixture table

Fixture tables live in `crates/btr-test-harness/fixtures/schema.sql`. After
adding a table there, also add its metadata to the SQLite config that
`reset_fixture()` builds — see `src/lib.rs::build_fixture_wxbtrv_db()`.

Good candidate tables for future expansion:

- `TEST_MULTI` — multi-segment index (exercises `build_order_by_cols` + compound keys)
- `TEST_AUTOINC` — AUTOINC/IDENTITY column (exercises field type 14/15)
- `TEST_TYPES` — every field type (FLOAT, DECIMAL, DATE, TIME, LOGICAL, ZSTRING) in one record
- `TEST_DESC` — descending index

## Troubleshooting

**Tests flaky with "database in transition" errors.**
The harness retries SQL errors in `reset_sql_fixture()`. If you see repeated
failures, the container may be hung — `docker restart wxbtrv-test-sql` and
rerun.

**"ODBC driver manager not available".**
Install `unixODBC` and make sure `odbcinst.ini` references `msodbcsql17`. Run
`odbcinst -q -d` to list installed drivers.

**Tests hang.**
Kill any stuck cargo test processes and restart the container:
`docker restart wxbtrv-test-sql`. A stale ODBC session inside `wxbtrv-core`
can block `DROP DATABASE`; the harness calls `reset_connection()` between
tests but the cache is per-test-binary.

**"Database WXBTRV_TEST is already in use".**
This can happen if another connection is holding the DB open. The fixture
uses `SET SINGLE_USER WITH ROLLBACK IMMEDIATE` before the drop, which usually
handles it. If it persists, manually connect with `sqlcmd` and kill
sessions: `USE master; ALTER DATABASE WXBTRV_TEST SET OFFLINE WITH ROLLBACK IMMEDIATE; DROP DATABASE WXBTRV_TEST;`.

## Benchmarks

Hot-path microbenches for `wxbtrv-core` live in
`crates/wxbtrv-core/benches/hot_paths.rs` and use Criterion. They cover the
WHERE/ORDER-BY builders (`build_key_where`, `build_continuation_where`,
`build_order_by_cols`), `col_to_sql_literal` (string/int/decimal), the
extended-op filter parser (`parse_filter_terms`) and the record codec
(`pack_row`/`unpack_row`) against a 10-field, 200-byte TEST_CUST-like fixture.

Run:
```bash
cargo bench -p wxbtrv-core
```

HTML reports land in `target/criterion/`. Use `--no-run` to verify the bench
binary compiles without spending minutes on measurements.

## Fuzzing

The record codec (`crates/wxbtrv-core/src/record.rs`) is fed by a DOS V86
caller, so a panic would trap NTVDM. Two libfuzzer targets exercise the
pack/unpack paths against arbitrary input. They live in
`crates/wxbtrv-core/fuzz/` and are deliberately excluded from the workspace
because `libfuzzer-sys` requires nightly + sanitizers.

One-time setup:
```bash
rustup toolchain install nightly
cargo install cargo-fuzz
```

Run the targets (from the repo root):
```bash
cd crates/wxbtrv-core
cargo +nightly fuzz run unpack_row
cargo +nightly fuzz run pack_row
```

The contract is "no panic, ever". Any crash file in
`crates/wxbtrv-core/fuzz/artifacts/` is a regression — minimize with
`cargo +nightly fuzz tmin <target> <crash>` and add a regression test to
`record.rs::tests`.
