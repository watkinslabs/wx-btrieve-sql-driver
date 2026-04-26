// Typed JSON client for the embedded wxbtrv-web server.

export type Backend = "mssql" | "postgres" | "sqlite";

// ── Project (open wxbtrv.db) ──────────────────────────────────────────

export interface CurrentProject {
  path: string | null;
}

export interface RecentEntry {
  path: string;
  opened_at: number;
}

// ── Connections ───────────────────────────────────────────────────────

export interface ConnectionFields {
  backend?: Backend | null;
  server?: string | null;
  database?: string | null;
  schema?: string | null;
  driver?: string | null;
  network?: string | null;
  user?: string | null;
  password?: string | null;
  has_password?: boolean | null;
  trusted_connection?: boolean | null;
  encrypt?: boolean | null;
  trust_server_certificate?: boolean | null;
  recnum_column?: string | null;
}

export interface ConnectionEntry {
  name: string;
  is_global: boolean;
  fields: ConnectionFields;
}

export interface ResolvedConnection {
  name: string;
  fields: ConnectionFields;
}

export interface TestConnectionResult {
  ok: boolean;
  backend: string;
  message: string;
}

// ── Tables ────────────────────────────────────────────────────────────

export interface TableSummary {
  table_name: string;
  schema_name: string;
  db_name: string;
  source_dir: string;
  record_length: number;
  field_count: number;
  index_count: number;
}

export interface FieldRow {
  num: number;
  name: string;
  native_type: number;
  length: number;
  offset: number;
  field_index: number | null;
  default_value: string | null;
}

export interface IndexSegmentRow {
  field_num: number;
  attrs: number;
  descending: boolean;
}

export interface IndexRow {
  num: number;
  segments: IndexSegmentRow[];
}

export interface TableDetail {
  summary: TableSummary;
  primary_index: number | null;
  ignore_null_values: boolean;
  trim_string_fields: boolean;
  translate_oem_to_ansi: boolean;
  local_cache: boolean;
  fields: FieldRow[];
  indexes: IndexRow[];
}

// ── File pickers ──────────────────────────────────────────────────────

export interface FilterSpec {
  name: string;
  ext: string[];
}

export interface PickerOptions {
  title?: string;
  filters?: FilterSpec[];
  suggest_name?: string;
  start_dir?: string;
}

export interface PickedPath {
  path: string | null;
}

// ── Fetch wrapper ─────────────────────────────────────────────────────

async function jsonFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
  if (!res.ok) {
    let detail = `${res.status} ${res.statusText}`;
    try {
      const body = await res.json();
      if (body && typeof body === "object" && "error" in body) {
        detail = String((body as { error: string }).error);
      }
    } catch {
      // body wasn't JSON; stick with the status text
    }
    throw new Error(detail);
  }
  return (await res.json()) as T;
}

const post = <T,>(path: string, body?: unknown) =>
  jsonFetch<T>(path, { method: "POST", body: body ? JSON.stringify(body) : "{}" });

export const api = {
  health: () => fetch("/api/health").then((r) => r.text()),

  // Project
  currentProject: () => jsonFetch<CurrentProject>("/api/project"),
  openProject: (path: string) => post<CurrentProject>("/api/project/open", { path }),
  closeProject: () => post<CurrentProject>("/api/project/close"),
  initProject: (path: string, overwrite = false) =>
    post<CurrentProject>("/api/project/init", { path, overwrite }),
  recent: () => jsonFetch<RecentEntry[]>("/api/project/recent"),
  forgetRecent: (path: string) => post<{ ok: boolean }>("/api/project/forget", { path }),

  // File pickers
  pickOpen: (opts: PickerOptions = {}) => post<PickedPath>("/api/fs/pick-open", opts),
  pickSave: (opts: PickerOptions = {}) => post<PickedPath>("/api/fs/pick-save", opts),
  pickDir: (opts: PickerOptions = {}) => post<PickedPath>("/api/fs/pick-dir", opts),

  // Connections
  listConnections: () => jsonFetch<ConnectionEntry[]>("/api/connections"),
  getConnection: (name: string) =>
    jsonFetch<ConnectionEntry>(`/api/connections/${encodeURIComponent(name)}`),
  resolvedConnection: (name: string) =>
    jsonFetch<ResolvedConnection>(`/api/connections/${encodeURIComponent(name)}/resolved`),
  upsertConnection: (name: string, fields: ConnectionFields) =>
    post<{ ok: boolean }>(`/api/connections/${encodeURIComponent(name)}`, fields),
  deleteConnection: (name: string) =>
    jsonFetch<{ ok: boolean }>(`/api/connections/${encodeURIComponent(name)}`, {
      method: "DELETE",
    }),
  testConnection: (name: string, draft?: ConnectionFields) =>
    post<TestConnectionResult>("/api/connections/test", { name, draft }),

  // Tables (read)
  listTables: () => jsonFetch<TableSummary[]>("/api/tables"),
  showTable: (name: string) =>
    jsonFetch<TableDetail>(`/api/tables/${encodeURIComponent(name)}`),

  // Tables (mutations)
  deleteTable: (name: string) =>
    jsonFetch<{ ok: boolean }>(`/api/tables/${encodeURIComponent(name)}`, {
      method: "DELETE",
    }),
  setTableProp: (name: string, key: string, value: string) =>
    post<{ ok: boolean }>(`/api/tables/${encodeURIComponent(name)}/prop`, { key, value }),
  addField: (
    table: string,
    body: {
      num: number;
      name: string;
      native_type: number;
      length: number;
      offset: number;
      index?: number | null;
      default?: string | null;
    },
  ) => post<{ ok: boolean }>(`/api/tables/${encodeURIComponent(table)}/fields`, body),
  removeField: (table: string, num: number) =>
    jsonFetch<{ ok: boolean }>(
      `/api/tables/${encodeURIComponent(table)}/fields/${num}`,
      { method: "DELETE" },
    ),
  addIndex: (
    table: string,
    body: { num: number; fields: string; attrs: string; desc: string },
  ) => post<{ ok: boolean }>(`/api/tables/${encodeURIComponent(table)}/indexes`, body),
  removeIndex: (table: string, num: number) =>
    jsonFetch<{ ok: boolean }>(
      `/api/tables/${encodeURIComponent(table)}/indexes/${num}`,
      { method: "DELETE" },
    ),
};

// ── Workflow (imports, exports, migration) ────────────────────────────

export interface OpResult {
  ok: boolean;
  message: string;
}

export interface MigrationRow {
  table_name: string;
  source_dir: string;
  field_count: number;
  migrated: boolean;
  migrated_at: string | null;
  row_count: number | null;
  target_db: string;
}

export const workflow = {
  importInt: (dirs: string[], opts: { recursive?: boolean; db?: string; schema?: string } = {}) =>
    post<OpResult>("/api/import/int", { dirs, ...opts }),
  importMds: (path: string) => post<OpResult>("/api/import/mds", { path }),
  analyzeB: (path: string, table_name?: string) =>
    post<OpResult>("/api/import/analyze-b", { path, table_name }),
  exportInt: (out_dir: string, tables: string[] = []) =>
    post<OpResult>("/api/export/int", { out_dir, tables }),
  exportMds: (path: string) => post<OpResult>("/api/export/mds", { path }),
  exportDdl: (out: string, opts: { add_recnum?: boolean; tables?: string[] } = {}) =>
    post<OpResult>("/api/export/ddl", { out, tables: [], ...opts }),
  migrationStatus: () => jsonFetch<MigrationRow[]>("/api/migration/status"),
  markMigrated: (body: {
    table: string;
    rows?: number;
    server?: string;
    target_db?: string;
  }) => post<OpResult>("/api/migration/mark", body),
  clearMigrated: (table: string) => post<OpResult>("/api/migration/clear", { table }),
};

// ── .B → backend bulk import ──────────────────────────────────────────

export interface BImportOptions {
  create?: boolean;
  truncate?: boolean;
  dry_run?: boolean;
  batch?: number;
  auto_schema?: boolean;
  save_schema?: boolean;
  collation?: string;
  table?: string;
}

export interface BImportStats {
  files_ok: number;
  files_skipped: number;
  records: number;
  log: string[];
}

export interface BInfoSegment {
  offset: number;
  length: number;
  data_type: number;
  null_value: number;
  descending: boolean;
  allows_dups: boolean;
}

export interface BInfoKey {
  number: number;
  segments: BInfoSegment[];
}

export interface BInfo {
  path: string;
  version: number;
  page_size: number;
  logical_rec_len: number;
  physical_rec_len: number;
  key_count: number;
  declared_records: number;
  page_count: number;
  file_size: number;
  active_records: number;
  uncovered_bytes: number | null;
  record_kind: string | null;
  keys: BInfoKey[];
}

export const bimport = {
  info: (path: string) => post<BInfo>("/api/bimport/info", { path }),
  importFiles: (files: string[], options: BImportOptions = {}) =>
    post<BImportStats>("/api/bimport/files", { files, ...options }),
  importDir: (dir: string, recursive: boolean, options: BImportOptions = {}) =>
    post<BImportStats>("/api/bimport/dir", { dir, recursive, ...options }),
};

// ── SSE: streaming variants of the .B importers ───────────────────────

export interface StreamHandlers {
  onLog?: (line: string) => void;
  onDone?: (stats: BImportStats) => void;
  onError?: (msg: string) => void;
}

async function streamPost(
  path: string,
  body: unknown,
  handlers: StreamHandlers,
  signal?: AbortSignal,
): Promise<void> {
  const res = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "text/event-stream" },
    body: JSON.stringify(body),
    signal,
  });
  if (!res.ok || !res.body) {
    let detail = `${res.status} ${res.statusText}`;
    try {
      const j = await res.json();
      if (j?.error) detail = String(j.error);
    } catch {
      // not JSON
    }
    handlers.onError?.(detail);
    return;
  }
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    let idx: number;
    while ((idx = buf.indexOf("\n\n")) !== -1) {
      const frame = buf.slice(0, idx);
      buf = buf.slice(idx + 2);
      const ev = parseSseFrame(frame);
      if (!ev) continue;
      if (ev.event === "log") {
        handlers.onLog?.(ev.data);
      } else if (ev.event === "done") {
        try {
          handlers.onDone?.(JSON.parse(ev.data) as BImportStats);
        } catch {
          handlers.onError?.("malformed done payload");
        }
      } else if (ev.event === "error") {
        try {
          handlers.onError?.((JSON.parse(ev.data) as { error: string }).error);
        } catch {
          handlers.onError?.(ev.data);
        }
      }
    }
  }
}

function parseSseFrame(frame: string): { event: string; data: string } | null {
  let event = "message";
  const data: string[] = [];
  for (const line of frame.split("\n")) {
    if (line.startsWith("event:")) {
      event = line.slice(6).trim();
    } else if (line.startsWith("data:")) {
      data.push(line.slice(5).replace(/^ /, ""));
    }
  }
  if (data.length === 0) return null;
  return { event, data: data.join("\n") };
}

export const bimportStream = {
  importFiles: (files: string[], options: BImportOptions, handlers: StreamHandlers, signal?: AbortSignal) =>
    streamPost("/api/bimport/files/stream", { files, ...options }, handlers, signal),
  importDir: (
    dir: string,
    recursive: boolean,
    options: BImportOptions,
    handlers: StreamHandlers,
    signal?: AbortSignal,
  ) => streamPost("/api/bimport/dir/stream", { dir, recursive, ...options }, handlers, signal),
};

// SQLite filter shared by Open/Create wxbtrv.db pickers.
export const WXBTRV_DB_FILTER: FilterSpec = {
  name: "wxbtrv config",
  ext: ["db", "sqlite", "sqlite3"],
};
