# wxbtrv-web — local UI for wxbtrv.db

`wxbtrv-web` is a single-binary HTTP server with the React UI embedded.
It exposes the same surface as `db-config` and `btr-import`, plus a
schema diff tool and a live row preview, against any `wxbtrv.db` you
point it at. It binds to `127.0.0.1` only — there's no auth because
there's no remote attack surface.

## Running it

```bash
# Linux
tar xf wxbtrv-web-vX.Y.Z-linux-x86_64.tar.gz
./wxbtrv-web

# Windows
unzip wxbtrv-web-vX.Y.Z-windows-x86_64.zip
wxbtrv-web.exe
```

It picks a free localhost port, prints the URL, and opens your default
browser. To override:

```
wxbtrv-web --db /path/to/wxbtrv.db   # pre-open a project
wxbtrv-web --port 7777               # bind a specific port
wxbtrv-web --no-browser              # don't auto-launch a browser
```

Recent projects are remembered at
`~/.config/wxbtrv-web/recent.json` (Linux) /
`%APPDATA%\wxbtrv-web\recent.json` (Windows).

## Pages

### Health

Project-wide cockpit. One section across the top reports the global
connection test (green if reachable, red if not, with the underlying
error). Five summary cards: total tables, in-sync, drifted, missing
on the backend, migrated. The body is a per-table grid showing each
table's status (✓ in sync, ⚠ drift, ✗ missing on backend, ○ no
fields), the resolved section, drift counts as `-N +N ~N` (missing /
extra / type mismatches), row counts, and migration state. Every
table name is a link straight to its TableDetail.

### Workbench

Open an existing `wxbtrv.db` or create a fresh one. Native open/save
dialogs come up via `rfd` — same dialogs your other desktop apps use.
The recent-projects list is one click per entry; the trash icon
forgets one.

### Connections

`wxbtrv.db` holds a single global connection plus optional
per-directory overrides (e.g. `[PACIFIC]` for files opened from
`G:\PACIFIC`). The Connections page shows them as a tree:

- **Global defaults** — backend, server, database, user, password,
  TLS flags. Used for any directory that has no override.
- **Per-directory entries** — only the keys you set; everything
  else inherits from global. The form shows the inherited value as
  a placeholder so you know what would happen if you leave it blank.
- **Test** — opens a connection using either the saved values or the
  current draft (so you can validate before saving).
- **Add** — type the section name (`PACIFIC`, `WAREHOUSE`, etc.) and
  override only the keys that differ. If other entries already exist,
  Add can clone fields from one of them as a starting point — passwords
  aren't echoed by the server, so you'll re-enter that one.

### Tables

Lists every table in the project with field/index counts. Click a row
to drill in.

### Table detail

For one table:

- **Edit** — add/remove fields, add/remove indexes, set table
  properties (page size, schema name, etc.), delete the table.
- **Schema diff** — compares the project's column list against the
  actual columns in the configured backend (resolved through the
  per-directory section). Reports matched, missing, extra, and type
  mismatches. The "Add N missing column(s) to backend" button fires
  off `ALTER TABLE ADD COLUMN` on the live backend for every column
  the project has but the backend lacks.
- **Data preview** — runs `SELECT ... LIMIT N` against the backend
  and renders the rows. Prev / Next pages and a CSV download are one
  click each. The WHERE input lets you filter — the expression is
  appended verbatim to the query (localhost-only, no sanitization).

### Tools

Forms over the existing db-config / btr-import operations, with native
file pickers for every input:

- Import `.INT` files (streams progress live)
- Import `MDS.INI`
- Analyze a `.B` file (FCR header, key layout)
- Export tables back as `.INT` files
- Export `MDS.INI`
- Generate DDL (`CREATE TABLE` for every table)

### .B Import

Bulk import Btrieve flat files into the configured backend
(MSSQL / Postgres / SQLite). File-list and directory-tree variants;
both stream progress over Server-Sent Events with a Cancel button.
Options: `CREATE TABLE` first, `TRUNCATE` first, dry-run, batch size,
auto-derive schema from the FCR, persist that derived schema back
into `wxbtrv.db`. Inspect mode shows the FCR header without loading
anything.

### Migration

Per-table migrated / not-migrated toggle, backed by `wxbtrv.db`'s
`btr_tables.migrated` flag. Useful for tracking what you've moved
when the data side and the schema side run on different cadences.

## API surface

Every page is a thin layer over `/api/*`. Useful endpoints if you
script against it:

| Method | Path | Notes |
|---|---|---|
| `GET`  | `/api/project` | currently-open `wxbtrv.db` path |
| `POST` | `/api/project/open` | `{path}` — open existing |
| `POST` | `/api/project/init` | `{path,overwrite?}` — create new |
| `GET`  | `/api/connections` | global + per-dir entries |
| `POST` | `/api/connections/:name` | upsert (`""` password clears it) |
| `DELETE` | `/api/connections/:name` | refuses `global` |
| `POST` | `/api/connections/test` | `{name, draft?}` |
| `GET`  | `/api/tables` / `/api/tables/:name` | list / detail |
| `GET`  | `/api/tables/:name/diff` | schema diff vs backend |
| `POST` | `/api/tables/:name/diff/apply` | `{add_missing?, drop_extra?}` ALTER TABLE |
| `GET`  | `/api/tables/:name/rows?limit&offset&section&where` | row preview |
| `GET`  | `/api/health/overview` | per-table drift + connection check |
| `POST` | `/api/import/int/stream` | SSE: log lines + done event |
| `POST` | `/api/bimport/files/stream`, `/api/bimport/dir/stream` | SSE |
| `GET`  | `/api/migration/status` | per-table migrated flag |

The streaming endpoints emit `event: log` (one line each) and a single
`event: done` with the final stats, or `event: error` on hard failure.
Clients that only want the buffered result can hit the non-`/stream`
sibling endpoint instead.
