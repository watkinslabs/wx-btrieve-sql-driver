# Migrating a Btrieve database to SQL

End-to-end walkthrough: take a DOS application's Btrieve files (`.B`
flat files plus their `.INT` metadata) and stand them up on SQL Server,
PostgreSQL, or SQLite — then point the runtime DLL at them so the DOS
app keeps working unchanged.

The web UI ([docs/web.md](web.md)) is the recommended path; the
equivalent `db-config` / `btr-import` CLI invocations are listed under
each step for scripts and headless boxes.

---

## Before you start

You need:

- the **`.B` files** — the actual Btrieve flat-file data
- the **`.INT` files** that describe their schemas (one per `.B`)
- optionally an **`MDS.INI`** with the legacy connection settings
- a **target backend** ready to accept connections:
  - SQL Server: a database, a login that can `CREATE TABLE`, an ODBC
    driver installed on whichever machine runs the migration tool
  - PostgreSQL: a role that can create tables in a chosen schema
  - SQLite: a writable file path

Run `wxbtrv-web` (or `db_config`) on a machine that can reach all of
the above and where the `.B` / `.INT` files are mounted. The runtime
DLL will be deployed to the DOS box separately, after the migration is
done.

---

## 1. Open or create the project

The "project" is a single SQLite file called `wxbtrv.db`. It holds the
backend connection, per-directory overrides, every table's schema, and
migration state.

**Web UI** — Workbench page → **Create new…** → save to e.g.
`C:\WatkinsX\bin\wxbtrv.db`.

**CLI**
```
db_config --db wxbtrv.db init
```

You can keep multiple `wxbtrv.db` files (one per environment, customer,
etc.) and switch between them via Workbench → Open existing.

---

## 2. Configure the connection

`wxbtrv.db` stores a global connection and optional per-directory
overrides. The runtime, when the DOS app opens a file from
`G:\PACIFIC\BOOK.B`, looks up an override under section `PACIFIC` and
falls back to global for any unset keys.

**Web UI** — Connections page:

1. Click **Global defaults**, pick the backend, fill server / database /
   user / password (or browse to a SQLite file), click **Save**, then
   **Test connection**. Don't move on until Test is green.
2. If you have legacy MDS.INI from the old install, you can shortcut
   step 1: Tools → **Import MDS.INI**.
3. For each directory the DOS app uses (e.g. `PACIFIC`, `WAREHOUSE`)
   that needs different credentials, click **Add** in the connections
   sidebar, type the directory name, and override only the keys that
   differ. Empty fields inherit from global — the form shows the
   inherited value as a placeholder.

**CLI**
```
db_config --db wxbtrv.db set-connection \
  --backend mssql --server SQLBOX,1433 \
  --database WatkinsX --user wxsvc --password '...'
db_config --db wxbtrv.db test-connection
```

---

## 3. Import the table schemas

Schema definitions go from your `.INT` files into `wxbtrv.db`. After
this step the project knows about every table, its fields, and its
indexes.

**Web UI** — Tools → **Import .INT files** → browse to the directory
containing the INTs, tick **Recursive** if subdirectories matter, click
**Import**. Progress streams live; the result line says how many
tables landed.

**CLI**
```
db_config --db wxbtrv.db import-int /path/to/INTs -r
```

If some `.B` files have no matching `.INT`, you can derive a partial
schema from the `.B` itself:

**Web UI** — Tools → **Analyze .B file** (single-file). To do many at
once, use the .B Import page below with **Auto-schema** and **Save
derived schema** both ticked.

**CLI**
```
db_config --db wxbtrv.db analyze-b /path/to/SOMETHING.B
```

After this step, the **Tables** page lists every table the project
knows about. **Health** still shows them as "missing" because the
backend tables haven't been created yet.

---

## 4. Create the backend tables and load the data

Two paths — pick whichever fits.

### Option A — DDL first, data second

Generate `CREATE TABLE` statements for every table in the project and
hand them to your DBA or run them yourself. Then load the data.

**Web UI** — Tools → **Generate DDL** → save to a `.sql` file. Apply it
on the target. Then `.B Import` page → **Import a directory** with
*CREATE TABLE* unticked (tables already exist).

**CLI**
```
db_config --db wxbtrv.db gen-ddl --out schema.sql
sqlcmd -S ... -d ... -i schema.sql           # or psql / sqlite3
btr-import --db wxbtrv.db import-dir /path/to/Bs -r
```

### Option B — `btr-import --create` (one shot)

Let `btr-import` create each table the first time it sees its `.B`.
Faster on a green-field migration; doesn't help if your DBA needs to
review DDL first.

**Web UI** — `.B Import` page → **Import a directory** → tick
**CREATE TABLE** and **Auto-schema** if some `.B` files lack INT
schemas. **Save derived schema** persists those back to `wxbtrv.db` so
later runs see the same layout. Click **Import**; logs stream live.

**CLI**
```
btr-import --db wxbtrv.db import-dir /path/to/Bs -r --create --auto-schema --save-schema
```

Use **Dry run** first if you want to count records without writing
anything.

---

## 5. Verify

The Health page is the single dashboard for "is this ready?":

- The status banner reports the global connection (green = reachable).
- The summary cards count total tables, in-sync, drifted, missing,
  migrated.
- The per-table grid shows each table's status. ✓ in sync, ⚠ drift,
  ✗ no backend table, ○ no project schema. The drift column reads
  `-N +N ~N` (missing-on-backend / extra-on-backend / type
  mismatches).

If a table reads ⚠ drift with `-N` only, click it → **Schema diff** →
**Add N missing column(s) to backend**. That runs `ALTER TABLE ADD
COLUMN` on the live backend.

If a table reads ✗ missing, the backend needs the DDL applied (Option A
above) or another `.B import` pass with CREATE on (Option B).

For data sanity-check, on any table click **Data preview** → Load. The
WHERE input filters; CSV download exports the current page.

---

## 6. Track migration progress

When you've copied a table's data over and you're satisfied with it,
mark it migrated. This doesn't change the backend — it just records
that the table is "done" so the dashboard's `migrated` count is
meaningful when you stage a multi-environment cutover.

**Web UI** — Migration page → click the circle next to a row.

**CLI**
```
db_config --db wxbtrv.db mark-migrated SOMETABLE \
    --rows 12345 --target-server SQLBOX --target-db WatkinsX
```

---

## 7. Deploy the runtime

The runtime is a Windows DLL that reads `wxbtrv.db` at startup and
serves Btrieve calls against the configured backend.

1. Build or download `wxbtrv-installer-vX.Y.Z-windows-i686.exe`
   (releases page or `cargo build -p installer --release --target i686-pc-windows-gnu`).
2. Copy your `wxbtrv.db` to the target Windows machine — the installer
   doesn't ship it; only your environment knows the right values.
3. Run the installer. It drops `wxbtrv.dll` into `C:\Windows\System32`
   (required — see [docs/installation.md](installation.md)),
   `wxbtrv.sys` into `C:\WatkinsX\bin`, and patches `config.nt`.
4. Place `wxbtrv.db` somewhere the runtime will find it. The search
   order is:
   1. `WXBTRV_CONFIG_DIR` env var
   2. CWD
   3. Directory containing `wxbtrv.dll`
   4. Compile-time `WXBTRV_CONFIG_DIR`
   5. `C:\WatkinsX\bin`

   The standard install drops it at `C:\WatkinsX\bin\wxbtrv.db`.
5. Reboot or restart NTVDM, then run the DOS app. Btrieve calls now
   land in your SQL backend.

If the DOS app errors on startup, check
`C:\WatkinsX\logs\wxbtrv_<timestamp>.log` (debug builds only) — the
runtime logs every Btrieve call plus its translation. Most early
failures are "table not found" (re-check Health on the project),
"backend unreachable" (firewall / credentials), or a path
discrepancy in the per-directory section name.

---

## Common pitfalls

- **Section names are uppercased basenames.** If the DOS app opens
  `G:\Pacific\book.b`, the runtime looks under section `PACIFIC`.
  When you add a per-directory override, use that exact name. The
  Connections page uppercases what you type.
- **MSSQL drivers must be installed on the migration host.** The
  ODBC driver name in your config has to match an actually-installed
  driver. Test connection will tell you fast.
- **`.B` files with no `.INT`.** Either run analyze-b to derive a
  partial schema, or do the .B Import with auto-schema on. Auto-derived
  schemas only know about *key* fields — non-key bytes are dropped.
  Get the real INT in if you can.
- **Production firewalls.** Test connection from the migration host
  first; the runtime will use the same credentials and will fail the
  same way.
- **Two `wxbtrv.db` paths fighting.** If the runtime finds an old copy
  in CWD, it'll use that, not the one in `C:\WatkinsX\bin`. The debug
  log shows which file was opened.
