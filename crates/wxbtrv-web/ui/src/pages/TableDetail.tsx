import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { AlertTriangle, CheckCircle2, ChevronLeft, Loader2, Plus, RefreshCw, Trash2, XCircle } from "lucide-react";
import { api, type BrowseResult, type DiffResult, type TableDetail } from "@/api";

const TYPE_NAMES: Record<number, string> = {
  0: "STRING",
  1: "INT",
  2: "FLOAT",
  3: "DATE",
  4: "TIME",
  5: "DECIMAL",
  6: "MONEY",
  7: "LOGICAL",
  8: "NUMERIC",
  11: "BFLOAT",
  14: "AUTOINC",
  15: "AUTOINCREMENT",
  16: "ZSTRING",
};

const TYPE_OPTIONS: { value: number; label: string }[] = Object.entries(TYPE_NAMES).map(
  ([v, l]) => ({ value: Number(v), label: `${v} — ${l}` }),
);

export function TableDetailPage() {
  const { name = "" } = useParams<{ name: string }>();
  const navigate = useNavigate();
  const [detail, setDetail] = useState<TableDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  async function reload() {
    setError(null);
    try {
      setDetail(await api.showTable(name));
    } catch (e: any) {
      setError(String(e?.message ?? e));
    }
  }
  useEffect(() => {
    setDetail(null);
    reload();
  }, [name]);

  async function deleteTable() {
    if (!confirm(`Delete table ${name}? This drops the schema row only — backend data is untouched.`))
      return;
    try {
      await api.deleteTable(name);
      navigate("/tables");
    } catch (e: any) {
      setMsg(`Error: ${e?.message ?? e}`);
    }
  }

  async function deleteField(num: number) {
    if (!confirm(`Remove field #${num}?`)) return;
    try {
      await api.removeField(name, num);
      await reload();
    } catch (e: any) {
      setMsg(`Error: ${e?.message ?? e}`);
    }
  }

  async function deleteIndex(num: number) {
    if (!confirm(`Remove index #${num}?`)) return;
    try {
      await api.removeIndex(name, num);
      await reload();
    } catch (e: any) {
      setMsg(`Error: ${e?.message ?? e}`);
    }
  }

  if (error) {
    return (
      <div className="space-y-4">
        <Link to="/tables" className="inline-flex items-center text-sm text-blue-700 hover:underline dark:text-blue-400">
          <ChevronLeft className="h-4 w-4" /> Back to tables
        </Link>
        <div className="text-red-700 dark:text-red-300 text-sm">Could not load table: {error}</div>
      </div>
    );
  }
  if (!detail) return <div className="text-zinc-500">Loading…</div>;
  const s = detail.summary;

  return (
    <div className="space-y-6 max-w-4xl">
      <Link to="/tables" className="inline-flex items-center text-sm text-blue-700 hover:underline dark:text-blue-400">
        <ChevronLeft className="h-4 w-4" /> Back to tables
      </Link>

      <header className="flex items-start justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">{s.table_name}</h1>
          <p className="text-sm text-zinc-600 dark:text-zinc-400">
            {[s.schema_name, s.db_name].filter(Boolean).join(".")}
            {s.source_dir && ` · ${s.source_dir}`}
          </p>
        </div>
        <button
          className="inline-flex items-center gap-1 rounded-md border border-red-300 px-3 py-1.5 text-xs text-red-700 hover:bg-red-50 dark:border-red-800 dark:text-red-300 dark:hover:bg-red-950"
          onClick={deleteTable}
        >
          <Trash2 className="h-3.5 w-3.5" /> Delete table
        </button>
      </header>

      <section className="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <Stat label="Record length" value={s.record_length} />
        <Stat label="Fields" value={s.field_count} />
        <Stat label="Indexes" value={s.index_count} />
        <Stat label="Primary index" value={detail.primary_index ?? "—"} />
      </section>

      <section className="flex flex-wrap gap-x-6 gap-y-2 text-sm text-zinc-700 dark:text-zinc-300">
        <Flag on={detail.ignore_null_values} label="ignore null values" />
        <Flag on={detail.trim_string_fields} label="trim string fields" />
        <Flag on={detail.translate_oem_to_ansi} label="OEM→ANSI" />
        <Flag on={detail.local_cache} label="local cache" />
      </section>

      {msg && (
        <div className="rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200">
          {msg}
        </div>
      )}

      <section>
        <h2 className="text-sm font-semibold uppercase tracking-wide pb-2">Fields</h2>
        <div className="overflow-x-auto rounded-md border border-zinc-200 dark:border-zinc-800">
          <table className="min-w-full text-sm">
            <thead className="bg-zinc-100 text-left dark:bg-zinc-900">
              <tr>
                <Th className="text-right w-12">#</Th>
                <Th>Name</Th>
                <Th>Type</Th>
                <Th className="text-right">Length</Th>
                <Th className="text-right">Offset</Th>
                <Th>Default</Th>
                <Th className="w-8">{""}</Th>
              </tr>
            </thead>
            <tbody>
              {detail.fields.map((f) => (
                <tr key={f.num} className="border-t border-zinc-200 dark:border-zinc-800">
                  <Td className="text-right tabular-nums">{f.num}</Td>
                  <Td className="font-medium">{f.name}</Td>
                  <Td>
                    <span className="inline-block rounded bg-zinc-200 px-1.5 py-0.5 text-xs dark:bg-zinc-800">
                      {TYPE_NAMES[f.native_type] ?? f.native_type}
                    </span>
                  </Td>
                  <Td className="text-right tabular-nums">{f.length}</Td>
                  <Td className="text-right tabular-nums">{f.offset}</Td>
                  <Td className="text-zinc-500">{f.default_value ?? ""}</Td>
                  <Td>
                    <button
                      className="text-zinc-400 hover:text-red-600"
                      onClick={() => deleteField(f.num)}
                      title="Remove field"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </button>
                  </Td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <AddFieldForm tableName={name} onAdded={reload} />
      </section>

      <section>
        <h2 className="text-sm font-semibold uppercase tracking-wide pb-2">Indexes</h2>
        <ul className="space-y-2 text-sm">
          {detail.indexes.map((ix) => (
            <li
              key={ix.num}
              className="flex items-center justify-between gap-3 rounded-md border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-950"
            >
              <div>
                <div className="font-medium">#{ix.num}</div>
                <div className="text-zinc-600 dark:text-zinc-400">
                  {ix.segments.map((seg, i) => {
                    const f = detail.fields.find((f) => f.num === seg.field_num);
                    return (
                      <span key={i}>
                        {i > 0 && ", "}
                        {f?.name ?? `field#${seg.field_num}`}
                        {seg.descending && " ↓"}
                      </span>
                    );
                  })}
                </div>
              </div>
              <button
                className="text-zinc-400 hover:text-red-600"
                onClick={() => deleteIndex(ix.num)}
                title="Remove index"
              >
                <Trash2 className="h-4 w-4" />
              </button>
            </li>
          ))}
          {detail.indexes.length === 0 && (
            <li className="text-zinc-500">No indexes defined.</li>
          )}
        </ul>
        <AddIndexForm tableName={name} onAdded={reload} />
      </section>

      <SchemaDiffPanel tableName={name} />
      <RowBrowser tableName={name} />
    </div>
  );
}

function SchemaDiffPanel({ tableName }: { tableName: string }) {
  const [data, setData] = useState<DiffResult | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [section, setSection] = useState("");
  const [applyMsg, setApplyMsg] = useState<string | null>(null);

  async function load() {
    setBusy(true);
    setErr(null);
    setApplyMsg(null);
    try {
      setData(await api.diffTable(tableName, section || undefined));
    } catch (e: any) {
      setErr(String(e?.message ?? e));
      setData(null);
    } finally {
      setBusy(false);
    }
  }

  async function applyMissing() {
    if (!data) return;
    if (!confirm(`Run ALTER TABLE ADD COLUMN for ${data.missing_in_backend.length} column(s)?`))
      return;
    setBusy(true);
    setApplyMsg(null);
    try {
      const r = await api.applyDiff(tableName, {
        section: section || undefined,
        add_missing: true,
        drop_extra: false,
      });
      setApplyMsg(
        r.errors.length === 0
          ? `Ran ${r.statements.length} statement(s) successfully.`
          : `Ran ${r.statements.length} statement(s) — ${r.errors.length} error(s):\n${r.errors.join("\n")}`,
      );
      await load();
    } catch (e: any) {
      setApplyMsg(`Error: ${e?.message ?? e}`);
    } finally {
      setBusy(false);
    }
  }

  const diffOk =
    data &&
    data.backend_table_exists &&
    data.missing_in_backend.length === 0 &&
    data.extra_in_backend.length === 0 &&
    data.type_mismatches.length === 0;

  return (
    <section>
      <h2 className="text-sm font-semibold uppercase tracking-wide pb-2">Schema diff</h2>
      <div className="rounded-md border border-zinc-200 dark:border-zinc-800 p-3 space-y-3">
        <div className="flex flex-wrap items-end gap-3 text-sm">
          <label>
            <span className="block pb-1 text-xs text-zinc-500">Section override</span>
            <input
              className="rounded-md border border-zinc-300 bg-white p-1.5 text-sm dark:bg-zinc-900 dark:border-zinc-700"
              placeholder="(auto from source_dir)"
              value={section}
              onChange={(e) => setSection(e.target.value)}
            />
          </label>
          <button
            className="inline-flex items-center gap-1 rounded-md bg-zinc-900 px-3 py-1.5 text-xs font-medium text-zinc-50 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900"
            onClick={load}
            disabled={busy}
          >
            {busy ? <Loader2 className="h-3 w-3 animate-spin" /> : <RefreshCw className="h-3 w-3" />}
            {data ? "Refresh" : "Compare"}
          </button>
          {data && (
            <span className="text-xs text-zinc-500">
              {data.backend} · {data.table_ref}
            </span>
          )}
        </div>

        {err && (
          <div className="rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200">
            <XCircle className="mr-1 inline h-4 w-4" />
            {err}
          </div>
        )}

        {data && !data.backend_table_exists && (
          <div className="rounded-md border border-amber-300 bg-amber-50 p-3 text-sm text-amber-900 dark:bg-amber-950 dark:border-amber-800 dark:text-amber-200">
            <AlertTriangle className="mr-1 inline h-4 w-4" />
            Table doesn't exist on the backend yet. Generate DDL on the Tools page or
            run a .B import with <code>CREATE TABLE</code> to create it.
          </div>
        )}

        {data && data.backend_table_exists && diffOk && (
          <div className="rounded-md border border-emerald-300 bg-emerald-50 p-3 text-sm text-emerald-900 dark:bg-emerald-950 dark:border-emerald-800 dark:text-emerald-200">
            <CheckCircle2 className="mr-1 inline h-4 w-4" />
            All {data.matched.length} project columns match the backend schema.
          </div>
        )}

        {applyMsg && (
          <pre className="rounded-md border border-zinc-200 bg-zinc-50 p-3 text-xs text-zinc-700 dark:border-zinc-800 dark:bg-zinc-900 dark:text-zinc-300 whitespace-pre-wrap">
            {applyMsg}
          </pre>
        )}

        {data && data.backend_table_exists && !diffOk && (
          <div className="space-y-3 text-sm">
            {data.missing_in_backend.length > 0 && (
              <button
                className="inline-flex items-center gap-1 rounded-md bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-500 disabled:opacity-50"
                onClick={applyMissing}
                disabled={busy}
              >
                Add {data.missing_in_backend.length} missing column
                {data.missing_in_backend.length === 1 ? "" : "s"} to backend
              </button>
            )}
            <DiffList
              title="Missing in backend"
              items={data.missing_in_backend}
              tone="red"
              hint="These columns are in the project schema but not in the backend table. Click the button above to ALTER TABLE on the live backend."
            />
            <DiffList
              title="Extra in backend"
              items={data.extra_in_backend}
              tone="amber"
              hint="The backend has columns the project doesn't. Either add them to the project schema or drop them on the server."
            />
            {data.type_mismatches.length > 0 && (
              <div>
                <div className="font-medium text-amber-700 dark:text-amber-300">Type mismatches</div>
                <ul className="mt-1 rounded-md border border-amber-300 dark:border-amber-800 divide-y divide-amber-200 dark:divide-amber-900">
                  {data.type_mismatches.map((m) => (
                    <li key={m.name} className="p-2">
                      <code className="font-medium">{m.name}</code> — expected{" "}
                      <code>{m.expected}</code>, got <code>{m.actual}</code>
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {data.matched.length > 0 && (
              <DiffList title={`Matched (${data.matched.length})`} items={data.matched} tone="emerald" hint="" />
            )}
          </div>
        )}
      </div>
    </section>
  );
}

function DiffList({
  title,
  items,
  tone,
  hint,
}: {
  title: string;
  items: string[];
  tone: "red" | "amber" | "emerald";
  hint: string;
}) {
  if (items.length === 0) return null;
  const colors = {
    red: "border-red-300 dark:border-red-800 text-red-700 dark:text-red-300",
    amber: "border-amber-300 dark:border-amber-800 text-amber-700 dark:text-amber-300",
    emerald: "border-emerald-300 dark:border-emerald-800 text-emerald-700 dark:text-emerald-300",
  }[tone];
  return (
    <div>
      <div className={`font-medium ${colors.split(" ").slice(2).join(" ")}`}>{title}</div>
      {hint && <div className="text-xs text-zinc-500">{hint}</div>}
      <ul className={`mt-1 rounded-md border ${colors} text-sm`}>
        {items.map((n) => (
          <li key={n} className="p-1.5 px-2 border-b last:border-0 border-inherit">
            <code>{n}</code>
          </li>
        ))}
      </ul>
    </div>
  );
}

function RowBrowser({ tableName }: { tableName: string }) {
  const [data, setData] = useState<BrowseResult | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [limit, setLimit] = useState(50);
  const [offset, setOffset] = useState(0);
  const [section, setSection] = useState("");
  const [where, setWhere] = useState("");

  async function load(at?: number) {
    setBusy(true);
    setErr(null);
    const useOffset = at ?? offset;
    try {
      const result = await api.browseRows(tableName, {
        limit,
        offset: useOffset,
        section: section || undefined,
        where: where || undefined,
      });
      setData(result);
      if (at !== undefined) setOffset(at);
    } catch (e: any) {
      setErr(String(e?.message ?? e));
      setData(null);
    } finally {
      setBusy(false);
    }
  }
  async function next() {
    await load(offset + limit);
  }
  async function prev() {
    await load(Math.max(0, offset - limit));
  }
  function downloadCsv() {
    if (!data) return;
    const escape = (v: string | null) => {
      if (v === null) return "";
      const needsQuote = /[",\r\n]/.test(v);
      const escaped = v.replace(/"/g, '""');
      return needsQuote ? `"${escaped}"` : escaped;
    };
    const lines: string[] = [];
    lines.push(data.columns.map((c) => escape(c)).join(","));
    for (const row of data.rows) {
      lines.push(row.map(escape).join(","));
    }
    const blob = new Blob([lines.join("\n")], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${tableName}_${offset}-${offset + data.rows.length}.csv`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }

  return (
    <section>
      <h2 className="text-sm font-semibold uppercase tracking-wide pb-2">Data preview</h2>
      <div className="rounded-md border border-zinc-200 dark:border-zinc-800 p-3 space-y-3">
        <div className="flex flex-wrap items-end gap-3 text-sm">
          <label>
            <span className="block pb-1 text-xs text-zinc-500">Limit</span>
            <input
              type="number"
              className="w-20 rounded-md border border-zinc-300 bg-white p-1.5 text-sm dark:bg-zinc-900 dark:border-zinc-700"
              value={limit}
              onChange={(e) => setLimit(Math.max(1, Math.min(1000, Number(e.target.value) || 50)))}
            />
          </label>
          <label>
            <span className="block pb-1 text-xs text-zinc-500">Offset</span>
            <input
              type="number"
              className="w-20 rounded-md border border-zinc-300 bg-white p-1.5 text-sm dark:bg-zinc-900 dark:border-zinc-700"
              value={offset}
              onChange={(e) => setOffset(Math.max(0, Number(e.target.value) || 0))}
            />
          </label>
          <label>
            <span className="block pb-1 text-xs text-zinc-500">Section override</span>
            <input
              className="rounded-md border border-zinc-300 bg-white p-1.5 text-sm dark:bg-zinc-900 dark:border-zinc-700"
              placeholder="(auto from source_dir)"
              value={section}
              onChange={(e) => setSection(e.target.value)}
            />
          </label>
          <label className="flex-1 min-w-[16rem]">
            <span className="block pb-1 text-xs text-zinc-500">WHERE</span>
            <input
              className="w-full rounded-md border border-zinc-300 bg-white p-1.5 text-sm font-mono dark:bg-zinc-900 dark:border-zinc-700"
              placeholder='e.g. "ID" > 100'
              value={where}
              onChange={(e) => setWhere(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") load(0);
              }}
            />
          </label>
          <button
            className="inline-flex items-center gap-1 rounded-md bg-zinc-900 px-3 py-1.5 text-xs font-medium text-zinc-50 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900"
            onClick={() => load(0)}
            disabled={busy}
          >
            {busy ? <Loader2 className="h-3 w-3 animate-spin" /> : <RefreshCw className="h-3 w-3" />}
            {data ? "Refresh" : "Load"}
          </button>
          {data && (
            <>
              <button
                className="rounded-md border border-zinc-300 px-2 py-1.5 text-xs disabled:opacity-50 dark:border-zinc-700"
                onClick={prev}
                disabled={busy || offset === 0}
              >
                ← Prev
              </button>
              <button
                className="rounded-md border border-zinc-300 px-2 py-1.5 text-xs disabled:opacity-50 dark:border-zinc-700"
                onClick={next}
                disabled={busy || !data.truncated}
              >
                Next →
              </button>
              <button
                className="rounded-md border border-zinc-300 px-2 py-1.5 text-xs hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
                onClick={downloadCsv}
              >
                Download CSV
              </button>
              <span className="text-xs text-zinc-500">
                {data.backend} · {data.table_ref}
                {` · rows ${offset + 1}–${offset + data.rows.length}`}
                {data.truncated && " (more available)"}
              </span>
            </>
          )}
        </div>

        {err && (
          <div className="rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200">
            {err}
          </div>
        )}

        {data && (
          <div className="overflow-x-auto rounded-md border border-zinc-200 dark:border-zinc-800">
            <table className="min-w-full text-sm">
              <thead className="bg-zinc-100 text-left dark:bg-zinc-900">
                <tr>
                  {data.columns.map((c) => (
                    <Th key={c}>{c}</Th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {data.rows.map((r, i) => (
                  <tr key={i} className="border-t border-zinc-200 dark:border-zinc-800">
                    {r.map((cell, j) => (
                      <Td key={j} className={cell === null ? "text-zinc-400 italic" : ""}>
                        {cell === null ? "NULL" : cell}
                      </Td>
                    ))}
                  </tr>
                ))}
                {data.rows.length === 0 && (
                  <tr>
                    <td className="p-3 text-zinc-500" colSpan={data.columns.length || 1}>
                      No rows.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </section>
  );
}

function AddFieldForm({ tableName, onAdded }: { tableName: string; onAdded: () => Promise<void> }) {
  const [open, setOpen] = useState(false);
  const [num, setNum] = useState("");
  const [fname, setFname] = useState("");
  const [nt, setNt] = useState(0);
  const [length, setLength] = useState("");
  const [offset, setOffset] = useState("");
  const [err, setErr] = useState<string | null>(null);

  async function submit() {
    setErr(null);
    try {
      await api.addField(tableName, {
        num: Number(num),
        name: fname,
        native_type: nt,
        length: Number(length),
        offset: Number(offset),
      });
      setOpen(false);
      setNum("");
      setFname("");
      setNt(0);
      setLength("");
      setOffset("");
      await onAdded();
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    }
  }

  if (!open) {
    return (
      <button
        className="mt-3 inline-flex items-center gap-1 rounded-md border border-zinc-300 px-3 py-1.5 text-xs hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
        onClick={() => setOpen(true)}
      >
        <Plus className="h-3 w-3" /> Add field
      </button>
    );
  }
  return (
    <div className="mt-3 rounded-md border border-zinc-200 dark:border-zinc-800 p-3 space-y-2 text-sm">
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        <Tiny label="Num">
          <input className={inputCls} type="number" value={num} onChange={(e) => setNum(e.target.value)} />
        </Tiny>
        <Tiny label="Name">
          <input className={inputCls} value={fname} onChange={(e) => setFname(e.target.value)} />
        </Tiny>
        <Tiny label="Type">
          <select
            className={inputCls}
            value={nt}
            onChange={(e) => setNt(Number(e.target.value))}
          >
            {TYPE_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </Tiny>
        <Tiny label="Length">
          <input className={inputCls} type="number" value={length} onChange={(e) => setLength(e.target.value)} />
        </Tiny>
        <Tiny label="Offset">
          <input className={inputCls} type="number" value={offset} onChange={(e) => setOffset(e.target.value)} />
        </Tiny>
      </div>
      {err && <div className="text-xs text-red-600">{err}</div>}
      <div className="flex gap-2">
        <button
          className="rounded-md bg-zinc-900 px-3 py-1.5 text-xs font-medium text-zinc-50 dark:bg-zinc-100 dark:text-zinc-900"
          onClick={submit}
        >
          Save
        </button>
        <button
          className="rounded-md border border-zinc-300 px-3 py-1.5 text-xs dark:border-zinc-700"
          onClick={() => setOpen(false)}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

function AddIndexForm({ tableName, onAdded }: { tableName: string; onAdded: () => Promise<void> }) {
  const [open, setOpen] = useState(false);
  const [num, setNum] = useState("");
  const [fields, setFields] = useState("");
  const [attrs, setAttrs] = useState("0");
  const [desc, setDesc] = useState("0");
  const [err, setErr] = useState<string | null>(null);

  async function submit() {
    setErr(null);
    try {
      await api.addIndex(tableName, { num: Number(num), fields, attrs, desc });
      setOpen(false);
      setNum("");
      setFields("");
      setAttrs("0");
      setDesc("0");
      await onAdded();
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    }
  }

  if (!open) {
    return (
      <button
        className="mt-3 inline-flex items-center gap-1 rounded-md border border-zinc-300 px-3 py-1.5 text-xs hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
        onClick={() => setOpen(true)}
      >
        <Plus className="h-3 w-3" /> Add index
      </button>
    );
  }
  return (
    <div className="mt-3 rounded-md border border-zinc-200 dark:border-zinc-800 p-3 space-y-2 text-sm">
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        <Tiny label="Num">
          <input className={inputCls} type="number" value={num} onChange={(e) => setNum(e.target.value)} />
        </Tiny>
        <Tiny label="Field nums (comma)">
          <input className={inputCls} value={fields} onChange={(e) => setFields(e.target.value)} placeholder="1,3" />
        </Tiny>
        <Tiny label="Attrs">
          <input className={inputCls} value={attrs} onChange={(e) => setAttrs(e.target.value)} />
        </Tiny>
        <Tiny label="Desc (0/1)">
          <input className={inputCls} value={desc} onChange={(e) => setDesc(e.target.value)} />
        </Tiny>
      </div>
      {err && <div className="text-xs text-red-600">{err}</div>}
      <div className="flex gap-2">
        <button
          className="rounded-md bg-zinc-900 px-3 py-1.5 text-xs font-medium text-zinc-50 dark:bg-zinc-100 dark:text-zinc-900"
          onClick={submit}
        >
          Save
        </button>
        <button
          className="rounded-md border border-zinc-300 px-3 py-1.5 text-xs dark:border-zinc-700"
          onClick={() => setOpen(false)}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

const inputCls =
  "w-full rounded-md border border-zinc-300 bg-white p-1.5 text-xs dark:bg-zinc-900 dark:border-zinc-700";

function Tiny({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="block pb-0.5 text-xs text-zinc-500">{label}</span>
      {children}
    </label>
  );
}

function Stat({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="rounded-md border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-950">
      <div className="text-xs text-zinc-500 uppercase">{label}</div>
      <div className="text-lg font-semibold tabular-nums">{value}</div>
    </div>
  );
}
function Flag({ on, label }: { on: boolean; label: string }) {
  return (
    <span className={on ? "text-emerald-700 dark:text-emerald-400" : "text-zinc-400"}>
      {on ? "✓" : "·"} {label}
    </span>
  );
}
function Th({ children, className }: { children: React.ReactNode; className?: string }) {
  return <th className={`px-3 py-2 text-xs font-semibold uppercase tracking-wide ${className ?? ""}`}>{children}</th>;
}
function Td({ children, className }: { children: React.ReactNode; className?: string }) {
  return <td className={`px-3 py-2 ${className ?? ""}`}>{children}</td>;
}
