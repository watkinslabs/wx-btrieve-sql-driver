import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ChevronLeft } from "lucide-react";
import { api, type TableDetail } from "@/api";

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

export function TableDetailPage() {
  const { name = "" } = useParams<{ name: string }>();
  const [detail, setDetail] = useState<TableDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDetail(null);
    setError(null);
    api.showTable(name).then(setDetail).catch((e) => setError(String(e.message ?? e)));
  }, [name]);

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

      <header>
        <h1 className="text-xl font-semibold">{s.table_name}</h1>
        <p className="text-sm text-zinc-600 dark:text-zinc-400">
          {[s.schema_name, s.db_name].filter(Boolean).join(".")}
          {s.source_dir && ` · ${s.source_dir}`}
        </p>
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
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section>
        <h2 className="text-sm font-semibold uppercase tracking-wide pb-2">Indexes</h2>
        <ul className="space-y-2 text-sm">
          {detail.indexes.map((ix) => (
            <li
              key={ix.num}
              className="rounded-md border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-950"
            >
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
            </li>
          ))}
          {detail.indexes.length === 0 && (
            <li className="text-zinc-500">No indexes defined.</li>
          )}
        </ul>
      </section>
    </div>
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
