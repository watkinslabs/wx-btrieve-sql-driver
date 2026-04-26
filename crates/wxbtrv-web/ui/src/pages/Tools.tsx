import { useRef, useState } from "react";
import { CheckCircle2, Loader2, XCircle } from "lucide-react";
import { api, workflow, type ImportIntStats, type OpResult } from "@/api";
import { useProject } from "@/project";

export function ToolsPage() {
  const { project } = useProject();
  if (!project?.path) {
    return (
      <div className="max-w-xl rounded-md border border-zinc-200 dark:border-zinc-800 p-6">
        <h2 className="font-medium">No project open</h2>
        <p className="mt-1 text-sm text-zinc-600 dark:text-zinc-400">
          Open or create a wxbtrv.db on the Workbench page first.
        </p>
      </div>
    );
  }

  return (
    <div className="max-w-3xl space-y-8">
      <header>
        <h1 className="text-xl font-semibold">Tools</h1>
        <p className="text-sm text-zinc-600 dark:text-zinc-400">
          Import legacy <code>.INT</code> / <code>MDS.INI</code> files, generate
          DDL, or export your current schema.
        </p>
      </header>

      <ImportInt />
      <ImportMds />
      <AnalyzeB />
      <ExportInt />
      <ExportMds />
      <ExportDdl />
    </div>
  );
}

function Card({
  title,
  desc,
  children,
}: {
  title: string;
  desc: string;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-md border border-zinc-200 dark:border-zinc-800 p-4 space-y-3">
      <div>
        <h2 className="font-medium">{title}</h2>
        <p className="text-sm text-zinc-600 dark:text-zinc-400">{desc}</p>
      </div>
      {children}
    </section>
  );
}

function PathRow({
  label,
  value,
  onChange,
  pick,
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  pick: () => Promise<void>;
  placeholder?: string;
}) {
  return (
    <label className="block text-sm">
      <span className="block pb-1 font-medium text-zinc-700 dark:text-zinc-300">{label}</span>
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
          onClick={pick}
        >
          Browse…
        </button>
      </div>
    </label>
  );
}

function RunButton({
  busy,
  label,
  onClick,
}: {
  busy: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      className="inline-flex items-center gap-2 rounded-md bg-zinc-900 px-4 py-2 text-sm font-medium text-zinc-50 hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
      onClick={onClick}
      disabled={busy}
    >
      {busy && <Loader2 className="h-4 w-4 animate-spin" />}
      {busy ? "Running…" : label}
    </button>
  );
}

function ResultBox({ result }: { result: OpResult | null }) {
  if (!result) return null;
  return (
    <div
      className={`rounded-md border p-3 text-sm ${
        result.ok
          ? "border-emerald-300 bg-emerald-50 text-emerald-900 dark:bg-emerald-950 dark:border-emerald-800 dark:text-emerald-200"
          : "border-red-300 bg-red-50 text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200"
      }`}
    >
      {result.ok ? (
        <CheckCircle2 className="mr-1 inline h-4 w-4" />
      ) : (
        <XCircle className="mr-1 inline h-4 w-4" />
      )}
      {result.message}
    </div>
  );
}

function useRunner() {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<OpResult | null>(null);
  async function run(p: Promise<OpResult>) {
    setBusy(true);
    setResult(null);
    try {
      setResult(await p);
    } catch (e: any) {
      setResult({ ok: false, message: String(e?.message ?? e) });
    } finally {
      setBusy(false);
    }
  }
  return { busy, result, run };
}

function ImportInt() {
  const [dir, setDir] = useState("");
  const [recursive, setRecursive] = useState(true);
  const [db, setDb] = useState("");
  const [schema, setSchema] = useState("");
  const [busy, setBusy] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [stats, setStats] = useState<ImportIntStats | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  async function pick() {
    const r = await api.pickDir({ title: "Select INT directory" });
    if (r.path) setDir(r.path);
  }

  async function run() {
    setBusy(true);
    setLogs([]);
    setStats(null);
    setErr(null);
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    try {
      await workflow.importIntStream(
        [dir],
        { recursive, db: db || undefined, schema: schema || undefined },
        {
          onLog: (l) => setLogs((cur) => [...cur, l]),
          onDone: setStats,
          onError: setErr,
        },
        ctrl.signal,
      );
    } catch (e: any) {
      if (e?.name !== "AbortError") setErr(String(e?.message ?? e));
    } finally {
      setBusy(false);
      abortRef.current = null;
    }
  }
  function cancel() {
    abortRef.current?.abort();
  }

  return (
    <Card title="Import .INT files" desc="Scan a directory for Btrieve metadata files and load every table into the project.">
      <PathRow label="Directory" value={dir} onChange={setDir} pick={pick} placeholder="/path/to/INTs" />
      <div className="grid grid-cols-2 gap-4">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={recursive}
            onChange={(e) => setRecursive(e.target.checked)}
            className="h-4 w-4 rounded border-zinc-300 dark:border-zinc-700"
          />
          Recursive
        </label>
        <div />
        <label className="block text-sm">
          <span className="block pb-1 font-medium text-zinc-700 dark:text-zinc-300">DB override</span>
          <input
            className="w-full rounded-md border border-zinc-300 bg-white p-2 text-sm dark:bg-zinc-900 dark:border-zinc-700"
            value={db}
            onChange={(e) => setDb(e.target.value)}
            placeholder="(optional)"
          />
        </label>
        <label className="block text-sm">
          <span className="block pb-1 font-medium text-zinc-700 dark:text-zinc-300">Schema override</span>
          <input
            className="w-full rounded-md border border-zinc-300 bg-white p-2 text-sm dark:bg-zinc-900 dark:border-zinc-700"
            value={schema}
            onChange={(e) => setSchema(e.target.value)}
            placeholder="(optional)"
          />
        </label>
      </div>
      <div className="flex items-center gap-2">
        <RunButton busy={busy} label="Import" onClick={run} />
        {busy && (
          <button
            className="rounded-md border border-zinc-300 px-3 py-2 text-sm hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
            onClick={cancel}
          >
            Cancel
          </button>
        )}
      </div>
      {err && <ResultBox result={{ ok: false, message: err }} />}
      {(busy || logs.length > 0) && (
        <pre className="max-h-72 overflow-auto rounded-md border border-zinc-200 bg-zinc-50 p-3 text-xs text-zinc-700 dark:border-zinc-800 dark:bg-zinc-900 dark:text-zinc-300">
          {logs.length === 0 ? "waiting…" : logs.join("\n")}
        </pre>
      )}
      {stats && (
        <ResultBox
          result={{
            ok: true,
            message: `Imported ${stats.imported}, skipped ${stats.skipped}`,
          }}
        />
      )}
    </Card>
  );
}

function ImportMds() {
  const [path, setPath] = useState("");
  const { busy, result, run } = useRunner();
  async function pick() {
    const r = await api.pickOpen({
      title: "Select MDS.INI",
      filters: [{ name: "INI", ext: ["ini"] }],
    });
    if (r.path) setPath(r.path);
  }
  return (
    <Card title="Import MDS.INI" desc="Pull legacy connection settings from an MDS.INI file.">
      <PathRow label="File" value={path} onChange={setPath} pick={pick} placeholder="/path/to/MDS.INI" />
      <RunButton busy={busy} label="Import" onClick={() => run(workflow.importMds(path))} />
      <ResultBox result={result} />
    </Card>
  );
}

function AnalyzeB() {
  const [path, setPath] = useState("");
  const [table, setTable] = useState("");
  const { busy, result, run } = useRunner();
  async function pick() {
    const r = await api.pickOpen({
      title: "Select .B file",
      filters: [{ name: "Btrieve", ext: ["b", "B"] }],
    });
    if (r.path) setPath(r.path);
  }
  return (
    <Card title="Analyze .B file" desc="Parse a Btrieve flat file's header and emit a synthesized INT layout (logged on the server).">
      <PathRow label="File" value={path} onChange={setPath} pick={pick} placeholder="/path/to/SOMETHING.B" />
      <label className="block text-sm">
        <span className="block pb-1 font-medium text-zinc-700 dark:text-zinc-300">Table name override</span>
        <input
          className="w-full rounded-md border border-zinc-300 bg-white p-2 text-sm dark:bg-zinc-900 dark:border-zinc-700"
          value={table}
          onChange={(e) => setTable(e.target.value)}
          placeholder="(optional — defaults to filename)"
        />
      </label>
      <RunButton busy={busy} label="Analyze" onClick={() => run(workflow.analyzeB(path, table || undefined))} />
      <ResultBox result={result} />
    </Card>
  );
}

function ExportInt() {
  const [dir, setDir] = useState("");
  const { busy, result, run } = useRunner();
  async function pick() {
    const r = await api.pickDir({ title: "Choose output directory" });
    if (r.path) setDir(r.path);
  }
  return (
    <Card title="Export .INT files" desc="Render the project's tables back out as Btrieve-compatible .INT files.">
      <PathRow label="Output directory" value={dir} onChange={setDir} pick={pick} />
      <RunButton busy={busy} label="Export" onClick={() => run(workflow.exportInt(dir))} />
      <ResultBox result={result} />
    </Card>
  );
}

function ExportMds() {
  const [path, setPath] = useState("");
  const { busy, result, run } = useRunner();
  async function pick() {
    const r = await api.pickSave({
      title: "Save MDS.INI",
      filters: [{ name: "INI", ext: ["ini"] }],
      suggest_name: "MDS.INI",
    });
    if (r.path) setPath(r.path);
  }
  return (
    <Card title="Export MDS.INI" desc="Render the project's connection config as an MDS.INI file.">
      <PathRow label="Output file" value={path} onChange={setPath} pick={pick} />
      <RunButton busy={busy} label="Export" onClick={() => run(workflow.exportMds(path))} />
      <ResultBox result={result} />
    </Card>
  );
}

function ExportDdl() {
  const [path, setPath] = useState("");
  const [recnum, setRecnum] = useState(false);
  const { busy, result, run } = useRunner();
  async function pick() {
    const r = await api.pickSave({
      title: "Save DDL",
      filters: [{ name: "SQL", ext: ["sql"] }],
      suggest_name: "schema.sql",
    });
    if (r.path) setPath(r.path);
  }
  return (
    <Card title="Generate DDL" desc="Emit CREATE TABLE statements for every table in the project.">
      <PathRow label="Output file" value={path} onChange={setPath} pick={pick} />
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={recnum}
          onChange={(e) => setRecnum(e.target.checked)}
          className="h-4 w-4 rounded border-zinc-300 dark:border-zinc-700"
        />
        Add a synthetic <code>recnum</code> identity column
      </label>
      <RunButton
        busy={busy}
        label="Generate"
        onClick={() => run(workflow.exportDdl(path, { add_recnum: recnum }))}
      />
      <ResultBox result={result} />
    </Card>
  );
}
