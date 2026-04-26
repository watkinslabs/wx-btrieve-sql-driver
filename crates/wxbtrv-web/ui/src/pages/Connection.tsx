import { useEffect, useState } from "react";
import { CheckCircle2, XCircle, Loader2 } from "lucide-react";
import { api, type Backend, type ConnectionConfig, type TestConnectionResult } from "@/api";

export function ConnectionPage() {
  const [cfg, setCfg] = useState<ConnectionConfig | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [draft, setDraft] = useState<Partial<ConnectionConfig>>({});
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestConnectionResult | null>(null);

  useEffect(() => {
    api
      .getConfig()
      .then((c) => {
        setCfg(c);
        setDraft(c);
      })
      .catch((e) => setLoadError(String(e.message ?? e)));
  }, []);

  if (loadError) {
    return (
      <div className="max-w-xl">
        <h1 className="text-xl font-semibold mb-2">Connection</h1>
        <div className="rounded-md border border-red-300 bg-red-50 p-4 text-sm text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200">
          Could not load <code>wxbtrv.db</code>: {loadError}.
          <div className="mt-2">
            Run <code>wxbtrv-web --db /path/to/wxbtrv.db</code> or start it from
            a directory that contains the file.
          </div>
        </div>
      </div>
    );
  }
  if (!cfg) return <div className="text-zinc-500">Loading…</div>;

  const backend = (draft.backend ?? cfg.backend) as Backend;
  const setField = <K extends keyof ConnectionConfig>(k: K, v: ConnectionConfig[K]) =>
    setDraft((d) => ({ ...d, [k]: v }));

  async function save() {
    setSaving(true);
    setSaveMsg(null);
    try {
      // Only send changed fields; omit password unless user typed one
      // (so we don't blank an existing pw).
      const body: Partial<ConnectionConfig> = {};
      for (const k of Object.keys(draft) as (keyof ConnectionConfig)[]) {
        if (draft[k] !== cfg![k]) (body as any)[k] = draft[k];
      }
      if (body.password === "") delete (body as any).password;
      if (Object.keys(body).length === 0) {
        setSaveMsg("No changes to save");
        return;
      }
      await api.setBackend(body);
      const fresh = await api.getConfig();
      setCfg(fresh);
      setDraft(fresh);
      setSaveMsg("Saved");
    } catch (e: any) {
      setSaveMsg(`Error: ${e.message ?? e}`);
    } finally {
      setSaving(false);
    }
  }

  async function runTest() {
    setTesting(true);
    setTestResult(null);
    try {
      const r = await api.testConnection();
      setTestResult(r);
    } catch (e: any) {
      setTestResult({ ok: false, backend: backend, message: String(e.message ?? e) });
    } finally {
      setTesting(false);
    }
  }

  return (
    <div className="max-w-2xl space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Connection</h1>
        <p className="text-sm text-zinc-600 dark:text-zinc-400">
          The runtime reads these values from <code>wxbtrv.db</code> at startup.
        </p>
      </header>

      <section className="space-y-4">
        <Field label="Backend">
          <select
            className="w-full rounded-md border border-zinc-300 bg-white p-2 dark:bg-zinc-900 dark:border-zinc-700"
            value={backend}
            onChange={(e) => setField("backend", e.target.value as Backend)}
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
                value={draft.server ?? cfg.server}
                placeholder={backend === "postgres" ? "host:5432" : "host[,port]"}
                onChange={(v) => setField("server", v)}
              />
            </Field>
            <Field label="Database">
              <Input value={draft.database ?? cfg.database} onChange={(v) => setField("database", v)} />
            </Field>
            <Field label="User">
              <Input value={draft.user ?? cfg.user} onChange={(v) => setField("user", v)} />
            </Field>
            <Field label={`Password${cfg.has_password ? " (set; leave blank to keep)" : ""}`}>
              <Input
                type="password"
                value={draft.password ?? ""}
                placeholder={cfg.has_password ? "•••••••••" : ""}
                onChange={(v) => setField("password", v)}
              />
            </Field>
          </>
        )}

        {backend === "sqlite" && (
          <Field label="Database file path">
            <Input
              value={draft.database ?? cfg.database}
              placeholder="/path/to/data.sqlite"
              onChange={(v) => setField("database", v)}
            />
          </Field>
        )}

        {backend === "mssql" && (
          <>
            <Field label="ODBC Driver">
              <Input
                value={draft.driver ?? cfg.driver}
                placeholder="ODBC Driver 17 for SQL Server"
                onChange={(v) => setField("driver", v)}
              />
            </Field>
            <Field label="Schema">
              <Input
                value={draft.schema ?? cfg.schema}
                placeholder="dbo"
                onChange={(v) => setField("schema", v)}
              />
            </Field>
          </>
        )}

        {backend !== "sqlite" && (
          <div className="grid grid-cols-2 gap-4">
            <Toggle
              label="Encrypt connection (TLS)"
              checked={draft.encrypt ?? cfg.encrypt}
              onChange={(v) => setField("encrypt", v)}
            />
            <Toggle
              label="Trust server certificate"
              checked={draft.trust_server_certificate ?? cfg.trust_server_certificate}
              onChange={(v) => setField("trust_server_certificate", v)}
            />
          </div>
        )}
      </section>

      <div className="flex items-center gap-3">
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
        {saveMsg && <span className="text-sm text-zinc-600 dark:text-zinc-400">{saveMsg}</span>}
      </div>

      {testResult && (
        <div
          className={`rounded-md border p-4 text-sm ${
            testResult.ok
              ? "border-emerald-300 bg-emerald-50 text-emerald-900 dark:bg-emerald-950 dark:border-emerald-800 dark:text-emerald-200"
              : "border-red-300 bg-red-50 text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200"
          }`}
        >
          {testResult.ok ? (
            <CheckCircle2 className="mr-1 inline h-4 w-4" />
          ) : (
            <XCircle className="mr-1 inline h-4 w-4" />
          )}
          {testResult.message}
        </div>
      )}
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
