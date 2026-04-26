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

// SQLite filter shared by Open/Create wxbtrv.db pickers.
export const WXBTRV_DB_FILTER: FilterSpec = {
  name: "wxbtrv config",
  ext: ["db", "sqlite", "sqlite3"],
};
