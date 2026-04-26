import { useState } from "react";
import { CheckCircle2, Info, Loader2, XCircle } from "lucide-react";
import { api, bimport, type BInfo, type BImportStats } from "@/api";
import { useProject } from "@/project";

export function BImportPage() {
  const { project } = useProject();
  if (!project?.path) {
    return (
      <div className="max-w-xl rounded-md border border-zinc-200 dark:border-zinc-800 p-6">
        <h2 className="font-medium">No project open</h2>
        <p className="mt-1 text-sm text-zinc-600 dark:text-zinc-400">
          Open a wxbtrv.db on the Workbench page first.
        </p>
      </div>
    );
  }
  return (
    <div className="max-w-3xl space-y-8">
      <header>
        <h1 className="text-xl font-semibold">.B Bulk Import</h1>
        <p className="text-sm text-zinc-600 dark:text-zinc-400">
          Pump Btrieve flat files (<code>.B</code>) directly into the configured
          backend. Schemas are loaded from this project; missing ones can be
          auto-derived from the FCR.
        </p>
      </header>
      <FileImporter />
      <DirImporter />
      <InfoBox />
    </div>
  );
}

function OptionsBlock({
  opts,
  setOpts,
}: {
  opts: ImportState;
  setOpts: React.Dispatch<React.SetStateAction<ImportState>>;
}) {
  const set = <K extends keyof ImportState>(k: K, v: ImportState[K]) =>
    setOpts((s) => ({ ...s, [k]: v }));
  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
      <Toggle label="CREATE TABLE" checked={opts.create} onChange={(v) => set("create", v)} />
      <Toggle label="TRUNCATE first" checked={opts.truncate} onChange={(v) => set("truncate", v)} />
      <Toggle label="Dry run" checked={opts.dry_run} onChange={(v) => set("dry_run", v)} />
      <Toggle label="Auto-schema (FCR)" checked={opts.auto_schema} onChange={(v) => set("auto_schema", v)} />
      <Toggle label="Save derived schema" checked={opts.save_schema} onChange={(v) => set("save_schema", v)} />
      <label className="block text-sm">
        <span className="block pb-1 text-xs text-zinc-500">Batch size</span>
        <input
          type="number"
          className="w-full rounded-md border border-zinc-300 bg-white p-2 text-sm dark:bg-zinc-900 dark:border-zinc-700"
          value={opts.batch}
          onChange={(e) => set("batch", Number(e.target.value || 0))}
        />
      </label>
    </div>
  );
}

interface ImportState {
  create: boolean;
  truncate: boolean;
  dry_run: boolean;
  auto_schema: boolean;
  save_schema: boolean;
  batch: number;
}
const defaults: ImportState = {
  create: false,
  truncate: false,
  dry_run: false,
  auto_schema: false,
  save_schema: false,
  batch: 200,
};

function FileImporter() {
  const [files, setFiles] = useState<string[]>([]);
  const [opts, setOpts] = useState<ImportState>(defaults);
  const [busy, setBusy] = useState(false);
  const [stats, setStats] = useState<BImportStats | null>(null);
  const [err, setErr] = useState<string | null>(null);

  async function pick() {
    const r = await api.pickOpen({
      title: "Select a .B file",
      filters: [{ name: "Btrieve", ext: ["b", "B"] }],
    });
    if (r.path) setFiles((cur) => Array.from(new Set([...cur, r.path!])));
  }

  async function run() {
    setBusy(true);
    setStats(null);
    setErr(null);
    try {
      setStats(await bimport.importFiles(files, opts));
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title="Import individual files">
      <button
        type="button"
        className="rounded-md border border-zinc-300 px-3 py-2 text-sm hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
        onClick={pick}
      >
        Add file…
      </button>
      {files.length > 0 && (
        <ul className="rounded-md border border-zinc-200 dark:border-zinc-800 divide-y divide-zinc-200 dark:divide-zinc-800 text-sm">
          {files.map((f) => (
            <li key={f} className="flex items-center justify-between p-2">
              <code className="break-all">{f}</code>
              <button
                className="text-xs text-zinc-500 hover:text-red-600"
                onClick={() => setFiles((cur) => cur.filter((x) => x !== f))}
              >
                remove
              </button>
            </li>
          ))}
        </ul>
      )}
      <OptionsBlock opts={opts} setOpts={setOpts} />
      <RunButton busy={busy} label="Import" onClick={run} disabled={files.length === 0} />
      {err && <ErrBox msg={err} />}
      {stats && <StatsBox stats={stats} />}
    </Card>
  );
}

function DirImporter() {
  const [dir, setDir] = useState("");
  const [recursive, setRecursive] = useState(true);
  const [opts, setOpts] = useState<ImportState>(defaults);
  const [busy, setBusy] = useState(false);
  const [stats, setStats] = useState<BImportStats | null>(null);
  const [err, setErr] = useState<string | null>(null);

  async function pick() {
    const r = await api.pickDir({ title: "Select a directory of .B files" });
    if (r.path) setDir(r.path);
  }
  async function run() {
    setBusy(true);
    setStats(null);
    setErr(null);
    try {
      setStats(await bimport.importDir(dir, recursive, opts));
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title="Import a directory">
      <label className="block text-sm">
        <span className="block pb-1 font-medium text-zinc-700 dark:text-zinc-300">Directory</span>
        <div className="flex gap-2">
          <input
            className="flex-1 rounded-md border border-zinc-300 bg-white p-2 text-sm dark:bg-zinc-900 dark:border-zinc-700"
            value={dir}
            onChange={(e) => setDir(e.target.value)}
            placeholder="/path/to/btrieve/files"
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
      <Toggle label="Recursive" checked={recursive} onChange={setRecursive} />
      <OptionsBlock opts={opts} setOpts={setOpts} />
      <RunButton busy={busy} label="Import directory" onClick={run} disabled={!dir} />
      {err && <ErrBox msg={err} />}
      {stats && <StatsBox stats={stats} />}
    </Card>
  );
}

function InfoBox() {
  const [path, setPath] = useState("");
  const [info, setInfo] = useState<BInfo | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function pick() {
    const r = await api.pickOpen({
      title: "Select a .B file",
      filters: [{ name: "Btrieve", ext: ["b", "B"] }],
    });
    if (r.path) setPath(r.path);
  }
  async function run() {
    setBusy(true);
    setErr(null);
    setInfo(null);
    try {
      setInfo(await bimport.info(path));
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title="Inspect .B file">
      <label className="block text-sm">
        <span className="block pb-1 font-medium text-zinc-700 dark:text-zinc-300">File</span>
        <div className="flex gap-2">
          <input
            className="flex-1 rounded-md border border-zinc-300 bg-white p-2 text-sm dark:bg-zinc-900 dark:border-zinc-700"
            value={path}
            onChange={(e) => setPath(e.target.value)}
            placeholder="/path/to/SOMETHING.B"
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
      <RunButton busy={busy} label="Inspect" onClick={run} disabled={!path} />
      {err && <ErrBox msg={err} />}
      {info && (
        <div className="rounded-md border border-zinc-200 dark:border-zinc-800 p-3 text-sm space-y-1">
          <div className="flex items-center gap-2 text-zinc-600 dark:text-zinc-400">
            <Info className="h-4 w-4" /> Btrieve v{info.version} · {info.record_kind ?? "?"}
          </div>
          <Row k="Page size" v={`${info.page_size} bytes`} />
          <Row k="Logical rec len" v={`${info.logical_rec_len} bytes`} />
          <Row k="Physical rec len" v={`${info.physical_rec_len} bytes`} />
          <Row k="Pages" v={`${info.page_count}`} />
          <Row k="Records (declared / active)" v={`${info.declared_records} / ${info.active_records}`} />
          <Row k="File size" v={`${info.file_size} bytes`} />
          {info.uncovered_bytes !== null && info.uncovered_bytes > 0 && (
            <div className="text-amber-700 dark:text-amber-400">
              {info.uncovered_bytes} bytes uncovered by key fields — non-key fields
              are unknown without an INT file.
            </div>
          )}
          {info.keys.length > 0 && (
            <div className="pt-2">
              <div className="text-xs font-semibold uppercase tracking-wide text-zinc-500">Keys</div>
              <ul className="text-xs">
                {info.keys.map((k) => (
                  <li key={k.number}>
                    Key {k.number + 1}:{" "}
                    {k.segments
                      .map((s) => `off=${s.offset} len=${s.length} type=${s.data_type}${s.descending ? " ↓" : ""}`)
                      .join(", ")}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </Card>
  );
}

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-md border border-zinc-200 dark:border-zinc-800 p-4 space-y-3">
      <h2 className="font-medium">{title}</h2>
      {children}
    </section>
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

function RunButton({
  busy,
  label,
  onClick,
  disabled,
}: {
  busy: boolean;
  label: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      className="inline-flex items-center gap-2 rounded-md bg-zinc-900 px-4 py-2 text-sm font-medium text-zinc-50 hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
      onClick={onClick}
      disabled={busy || disabled}
    >
      {busy && <Loader2 className="h-4 w-4 animate-spin" />}
      {busy ? "Running…" : label}
    </button>
  );
}

function ErrBox({ msg }: { msg: string }) {
  return (
    <div className="rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200">
      <XCircle className="mr-1 inline h-4 w-4" />
      {msg}
    </div>
  );
}

function StatsBox({ stats }: { stats: BImportStats }) {
  return (
    <div className="rounded-md border border-emerald-300 bg-emerald-50 p-3 text-sm dark:bg-emerald-950 dark:border-emerald-800">
      <div className="flex items-center gap-2 text-emerald-900 dark:text-emerald-200">
        <CheckCircle2 className="h-4 w-4" />
        <strong>{stats.records}</strong> records imported across {stats.files_ok} files
        {stats.files_skipped > 0 && ` (${stats.files_skipped} skipped)`}
      </div>
      {stats.log.length > 0 && (
        <pre className="mt-2 max-h-64 overflow-auto rounded bg-white/50 p-2 text-xs text-zinc-700 dark:bg-zinc-900/50 dark:text-zinc-300">
          {stats.log.join("\n")}
        </pre>
      )}
    </div>
  );
}

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex justify-between gap-4">
      <span className="text-zinc-500">{k}</span>
      <span className="tabular-nums">{v}</span>
    </div>
  );
}
