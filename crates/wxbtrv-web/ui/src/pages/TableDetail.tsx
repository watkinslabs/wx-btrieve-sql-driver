import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ChevronLeft, Loader2, Plus, RefreshCw, Trash2 } from "lucide-react";
import { api, type BrowseResult, type TableDetail } from "@/api";

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

      <RowBrowser tableName={name} />
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

  async function load() {
    setBusy(true);
    setErr(null);
    try {
      setData(
        await api.browseRows(tableName, {
          limit,
          offset,
          section: section || undefined,
        }),
      );
    } catch (e: any) {
      setErr(String(e?.message ?? e));
      setData(null);
    } finally {
      setBusy(false);
    }
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
          <button
            className="inline-flex items-center gap-1 rounded-md bg-zinc-900 px-3 py-1.5 text-xs font-medium text-zinc-50 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900"
            onClick={load}
            disabled={busy}
          >
            {busy ? <Loader2 className="h-3 w-3 animate-spin" /> : <RefreshCw className="h-3 w-3" />}
            {data ? "Refresh" : "Load"}
          </button>
          {data && (
            <span className="text-xs text-zinc-500">
              {data.backend} · {data.table_ref}
              {data.truncated && ` · truncated to ${limit}`}
            </span>
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
