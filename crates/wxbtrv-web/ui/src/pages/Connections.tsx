import { useEffect, useMemo, useState } from "react";
import { CheckCircle2, Loader2, Plus, Trash2, XCircle } from "lucide-react";
import {
  api,
  type Backend,
  type ConnectionEntry,
  type ConnectionFields,
  type TestConnectionResult,
} from "@/api";
import { useProject } from "@/project";

export function ConnectionsPage() {
  const { project } = useProject();
  const [list, setList] = useState<ConnectionEntry[] | null>(null);
  const [selected, setSelected] = useState<string>("global");
  const [loadErr, setLoadErr] = useState<string | null>(null);

  async function reload() {
    if (!project?.path) {
      setList(null);
      return;
    }
    setLoadErr(null);
    try {
      setList(await api.listConnections());
    } catch (e: any) {
      setLoadErr(String(e?.message ?? e));
    }
  }
  useEffect(() => {
    reload();
  }, [project?.path]);

  if (!project?.path) {
    return (
      <EmptyState
        title="No project open"
        body="Open or create a wxbtrv.db on the Workbench page first."
      />
    );
  }
  if (loadErr) {
    return <ErrorBox msg={loadErr} />;
  }
  if (!list) {
    return <div className="text-zinc-500">Loading…</div>;
  }

  const current = list.find((e) => e.name === selected) ?? list[0];

  return (
    <div className="flex gap-6">
      <aside className="w-64 shrink-0">
        <div className="flex items-center justify-between mb-2">
          <div className="text-sm font-medium text-zinc-700 dark:text-zinc-300">Connections</div>
          <AddDirectoryButton
            existing={list}
            onAdded={async (name) => {
              await reload();
              setSelected(name);
            }}
          />
        </div>
        <ul className="rounded-md border border-zinc-200 dark:border-zinc-800 divide-y divide-zinc-200 dark:divide-zinc-800">
          {list.map((e) => (
            <li key={e.name}>
              <button
                className={`w-full text-left p-2 text-sm ${
                  e.name === current?.name
                    ? "bg-zinc-100 dark:bg-zinc-800"
                    : "hover:bg-zinc-50 dark:hover:bg-zinc-900"
                }`}
                onClick={() => setSelected(e.name)}
              >
                <div className="font-medium">{e.is_global ? "Global defaults" : e.name}</div>
                <div className="text-xs text-zinc-500">
                  {e.fields.backend ?? (e.is_global ? "(unset)" : "inherits")}
                </div>
              </button>
            </li>
          ))}
        </ul>
      </aside>

      <main className="flex-1 min-w-0">
        {current && (
          <ConnectionEditor
            key={current.name}
            entry={current}
            onSaved={reload}
            onDeleted={async () => {
              setSelected("global");
              await reload();
            }}
          />
        )}
      </main>
    </div>
  );
}

function ConnectionEditor({
  entry,
  onSaved,
  onDeleted,
}: {
  entry: ConnectionEntry;
  onSaved: () => Promise<void>;
  onDeleted: () => Promise<void>;
}) {
  const [draft, setDraft] = useState<ConnectionFields>({});
  const [resolved, setResolved] = useState<ConnectionFields | null>(null);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [test, setTest] = useState<TestConnectionResult | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    setDraft({});
    setTest(null);
    setMsg(null);
    if (!entry.is_global) {
      api
        .resolvedConnection(entry.name)
        .then((r) => setResolved(r.fields))
        .catch(() => setResolved(null));
    } else {
      setResolved(null);
    }
  }, [entry.name, entry.is_global]);

  const fields = entry.fields;
  const set = <K extends keyof ConnectionFields>(k: K, v: ConnectionFields[K]) =>
    setDraft((d) => ({ ...d, [k]: v }));

  // Effective backend = draft override → entry value → (per-dir) resolved → "mssql"
  const backend: Backend = (draft.backend ??
    fields.backend ??
    resolved?.backend ??
    "mssql") as Backend;

  const placeholderFor = (k: keyof ConnectionFields): string => {
    if (entry.is_global) return "";
    const v = resolved?.[k];
    if (v === null || v === undefined || v === "") return "";
    return `inherits: ${String(v)}`;
  };

  async function save() {
    setSaving(true);
    setMsg(null);
    try {
      const body: ConnectionFields = { ...draft };
      // Don't echo password if user didn't set one.
      if (body.password === undefined) delete body.password;
      if (Object.keys(body).length === 0) {
        setMsg("No changes");
        return;
      }
      await api.upsertConnection(entry.name, body);
      setDraft({});
      setMsg("Saved");
      await onSaved();
    } catch (e: any) {
      setMsg(`Error: ${e?.message ?? e}`);
    } finally {
      setSaving(false);
    }
  }

  async function runTest() {
    setTesting(true);
    setTest(null);
    try {
      const r = await api.testConnection(entry.name, draft);
      setTest(r);
    } catch (e: any) {
      setTest({ ok: false, backend, message: String(e?.message ?? e) });
    } finally {
      setTesting(false);
    }
  }

  async function del() {
    if (!confirm(`Delete per-directory overrides for ${entry.name}?`)) return;
    try {
      await api.deleteConnection(entry.name);
      await onDeleted();
    } catch (e: any) {
      setMsg(`Error: ${e?.message ?? e}`);
    }
  }

  const valOr = <K extends keyof ConnectionFields>(k: K): string => {
    const d = draft[k];
    if (d !== undefined && d !== null) return String(d);
    const v = fields[k];
    return v === undefined || v === null ? "" : String(v);
  };
  const boolOr = (k: keyof ConnectionFields): boolean => {
    const d = draft[k];
    if (typeof d === "boolean") return d;
    const v = fields[k];
    return typeof v === "boolean" ? v : false;
  };

  return (
    <div className="space-y-6 max-w-2xl">
      <header className="flex items-start justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">
            {entry.is_global ? "Global defaults" : entry.name}
          </h1>
          <p className="text-sm text-zinc-600 dark:text-zinc-400">
            {entry.is_global
              ? "Used as the fallback for any directory without overrides."
              : `Per-directory overrides. Empty fields inherit from global.`}
          </p>
        </div>
        {!entry.is_global && (
          <button
            className="inline-flex items-center gap-1 rounded-md border border-red-300 px-3 py-1.5 text-xs text-red-700 hover:bg-red-50 dark:border-red-800 dark:text-red-300 dark:hover:bg-red-950"
            onClick={del}
          >
            <Trash2 className="h-3.5 w-3.5" /> Delete
          </button>
        )}
      </header>

      <section className="space-y-4">
        <Field label="Backend">
          <select
            className="w-full rounded-md border border-zinc-300 bg-white p-2 dark:bg-zinc-900 dark:border-zinc-700"
            value={backend}
            onChange={(e) => set("backend", e.target.value as Backend)}
          >
            <option value="mssql">SQL Server (MSSQL)</option>
            <option value="postgres">PostgreSQL</option>
            <option value="sqlite">SQLite</option>
          </select>
        </Field>

        {backend !== "sqlite" && (
          <>
            <Field label="Server">
              <Input
                value={valOr("server")}
                placeholder={placeholderFor("server") || (backend === "postgres" ? "host:5432" : "host[,port]")}
                onChange={(v) => set("server", v)}
              />
            </Field>
            <Field label="Database">
              <Input
                value={valOr("database")}
                placeholder={placeholderFor("database")}
                onChange={(v) => set("database", v)}
              />
            </Field>
            <Field label="User">
              <Input
                value={valOr("user")}
                placeholder={placeholderFor("user")}
                onChange={(v) => set("user", v)}
              />
            </Field>
            <Field
              label={`Password${
                fields.has_password ? " (set; leave blank to keep)" : ""
              }`}
            >
              <Input
                type="password"
                value={draft.password ?? ""}
                placeholder={fields.has_password ? "•••••••••" : ""}
                onChange={(v) => set("password", v)}
              />
            </Field>
          </>
        )}

        {backend === "sqlite" && (
          <Field label="Database file path">
            <FilePathInput
              value={valOr("database")}
              placeholder={placeholderFor("database") || "/path/to/data.sqlite"}
              onChange={(v) => set("database", v)}
            />
          </Field>
        )}

        {backend === "mssql" && (
          <>
            <Field label="ODBC Driver">
              <Input
                value={valOr("driver")}
                placeholder={placeholderFor("driver") || "ODBC Driver 17 for SQL Server"}
                onChange={(v) => set("driver", v)}
              />
            </Field>
            <Field label="Schema">
              <Input
                value={valOr("schema")}
                placeholder={placeholderFor("schema") || "dbo"}
                onChange={(v) => set("schema", v)}
              />
            </Field>
          </>
        )}

        {backend !== "sqlite" && (
          <div className="grid grid-cols-2 gap-4">
            <Toggle
              label="Encrypt connection (TLS)"
              checked={boolOr("encrypt")}
              onChange={(v) => set("encrypt", v)}
            />
            <Toggle
              label="Trust server certificate"
              checked={boolOr("trust_server_certificate")}
              onChange={(v) => set("trust_server_certificate", v)}
            />
          </div>
        )}
      </section>

      <div className="flex items-center gap-3 flex-wrap">
        <button
          className="rounded-md bg-zinc-900 px-4 py-2 text-sm font-medium text-zinc-50 hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
          onClick={save}
          disabled={saving}
        >
          {saving ? "Saving…" : "Save changes"}
        </button>
        <button
          className="rounded-md border border-zinc-300 px-4 py-2 text-sm font-medium hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
          onClick={runTest}
          disabled={testing}
        >
          {testing ? (
            <>
              <Loader2 className="mr-1 inline h-3 w-3 animate-spin" /> Testing…
            </>
          ) : (
            "Test connection"
          )}
        </button>
        {msg && <span className="text-sm text-zinc-600 dark:text-zinc-400">{msg}</span>}
      </div>

      {test && (
        <div
          className={`rounded-md border p-4 text-sm ${
            test.ok
              ? "border-emerald-300 bg-emerald-50 text-emerald-900 dark:bg-emerald-950 dark:border-emerald-800 dark:text-emerald-200"
              : "border-red-300 bg-red-50 text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200"
          }`}
        >
          {test.ok ? (
            <CheckCircle2 className="mr-1 inline h-4 w-4" />
          ) : (
            <XCircle className="mr-1 inline h-4 w-4" />
          )}
          {test.message}
        </div>
      )}
    </div>
  );
}

function AddDirectoryButton({
  existing,
  onAdded,
}: {
  existing: ConnectionEntry[];
  onAdded: (name: string) => void | Promise<void>;
}) {
  const taken = useMemo(
    () => new Set(existing.map((e) => e.name.toUpperCase())),
    [existing],
  );
  const cloneable = useMemo(() => existing.map((e) => e.name), [existing]);

  async function add() {
    const raw = prompt(
      "Directory name (e.g. PACIFIC) — overrides apply when DOS opens files in this directory.",
    );
    if (!raw) return;
    const name = raw.trim().toUpperCase();
    if (!name || name === "GLOBAL" || name === "CONFIG") {
      alert("invalid name");
      return;
    }
    if (taken.has(name)) {
      onAdded(name);
      return;
    }
    const cloneFrom = cloneable.length
      ? prompt(
          `Optional: clone overrides from existing entry?\nLeave blank to start empty.\nKnown: ${cloneable.join(", ")}`,
        )
      : null;
    try {
      if (cloneFrom && cloneFrom.trim()) {
        const src = await api
          .getConnection(cloneFrom.trim())
          .catch(() => null);
        if (src) {
          // Copy every set field (passwords are not echoed by the
          // server; user will need to re-enter them).
          const fields: ConnectionFields = { ...src.fields, password: undefined };
          await api.upsertConnection(name, fields);
        } else {
          await api.upsertConnection(name, { schema: "" });
        }
      } else {
        await api.upsertConnection(name, { schema: "" });
      }
      await onAdded(name);
    } catch (e: any) {
      alert(`Error: ${e?.message ?? e}`);
    }
  }
  return (
    <button
      className="inline-flex items-center gap-1 rounded-md border border-zinc-300 px-2 py-1 text-xs hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
      onClick={add}
      title="Add per-directory overrides (optionally clone from existing)"
    >
      <Plus className="h-3 w-3" /> Add
    </button>
  );
}

function FilePathInput({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
}) {
  async function browse() {
    try {
      const r = await api.pickOpen({ title: "Choose SQLite file" });
      if (r.path) onChange(r.path);
    } catch {
      // ignore
    }
  }
  return (
    <div className="flex gap-2">
      <input
        className="flex-1 rounded-md border border-zinc-300 bg-white p-2 text-sm dark:bg-zinc-900 dark:border-zinc-700"
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
      <button
        type="button"
        className="rounded-md border border-zinc-300 px-3 py-2 text-sm hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
        onClick={browse}
      >
        Browse…
      </button>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block text-sm">
      <span className="block pb-1 font-medium text-zinc-700 dark:text-zinc-300">{label}</span>
      {children}
    </label>
  );
}

function Input({
  value,
  onChange,
  placeholder,
  type,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: string;
}) {
  return (
    <input
      className="w-full rounded-md border border-zinc-300 bg-white p-2 text-sm dark:bg-zinc-900 dark:border-zinc-700"
      type={type ?? "text"}
      value={value}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-2 text-sm">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="h-4 w-4 rounded border-zinc-300 dark:border-zinc-700"
      />
      <span>{label}</span>
    </label>
  );
}

function EmptyState({ title, body }: { title: string; body: string }) {
  return (
    <div className="max-w-xl rounded-md border border-zinc-200 dark:border-zinc-800 p-6">
      <h2 className="font-medium">{title}</h2>
      <p className="mt-1 text-sm text-zinc-600 dark:text-zinc-400">{body}</p>
    </div>
  );
}

function ErrorBox({ msg }: { msg: string }) {
  return (
    <div className="rounded-md border border-red-300 bg-red-50 p-4 text-sm text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200">
      {msg}
    </div>
  );
}
