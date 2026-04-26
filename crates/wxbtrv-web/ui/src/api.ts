// Typed JSON client for the embedded wxbtrv-web server.

export type Backend = "mssql" | "postgres" | "sqlite";

export interface ConnectionConfig {
  backend: Backend;
  server: string;
  database: string;
  schema: string;
  driver: string;
  network: string;
  user: string;
  password: string;
  has_password: boolean;
  trusted_connection: boolean;
  encrypt: boolean;
  trust_server_certificate: boolean;
  recnum_column: string;
}

export interface UpdateConnection extends Partial<Omit<ConnectionConfig, "has_password">> {}

export interface TestConnectionResult {
  ok: boolean;
  backend: string;
  message: string;
}

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

export const api = {
  health: () => fetch("/api/health").then((r) => r.text()),
  getConfig: () => jsonFetch<ConnectionConfig>("/api/config"),
  setBackend: (body: UpdateConnection) =>
    jsonFetch<{ ok: boolean }>("/api/config/backend", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  testConnection: () =>
    jsonFetch<TestConnectionResult>("/api/test-connection", { method: "POST" }),
  listTables: () => jsonFetch<TableSummary[]>("/api/tables"),
  showTable: (name: string) => jsonFetch<TableDetail>(`/api/tables/${encodeURIComponent(name)}`),
};
