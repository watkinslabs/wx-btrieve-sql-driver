import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import {
  AlertTriangle,
  CheckCircle2,
  Circle,
  Loader2,
  RefreshCw,
  XCircle,
} from "lucide-react";
import { api, type ProjectHealth, type TableHealthStatus } from "@/api";
import { useProject } from "@/project";

export function HealthPage() {
  const { project } = useProject();
  const [data, setData] = useState<ProjectHealth | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function load() {
    setBusy(true);
    setErr(null);
    try {
      setData(await api.healthOverview());
    } catch (e: any) {
      setErr(String(e?.message ?? e));
      setData(null);
    } finally {
      setBusy(false);
    }
  }
  useEffect(() => {
    if (project?.path) load();
    else setData(null);
  }, [project?.path]);

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
    <div className="space-y-6 max-w-5xl">
      <header className="flex items-start justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Project health</h1>
          <p className="text-sm text-zinc-600 dark:text-zinc-400">
            Connection check + per-table schema drift + migration progress.
          </p>
        </div>
        <button
          className="inline-flex items-center gap-1 rounded-md border border-zinc-300 px-3 py-1.5 text-xs hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
          onClick={load}
          disabled={busy}
        >
          {busy ? <Loader2 className="h-3 w-3 animate-spin" /> : <RefreshCw className="h-3 w-3" />}
          Refresh
        </button>
      </header>

      {err && (
        <div className="rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200">
          {err}
        </div>
      )}

      {data && (
        <>
          <section
            className={`rounded-md border p-4 text-sm ${
              data.connection_ok
                ? "border-emerald-300 bg-emerald-50 dark:bg-emerald-950 dark:border-emerald-800"
                : "border-red-300 bg-red-50 dark:bg-red-950 dark:border-red-800"
            }`}
          >
            <div className="flex items-center gap-2 font-medium">
              {data.connection_ok ? (
                <CheckCircle2 className="h-4 w-4 text-emerald-700 dark:text-emerald-300" />
              ) : (
                <XCircle className="h-4 w-4 text-red-700 dark:text-red-300" />
              )}
              {data.backend} · {data.connection_message}
            </div>
          </section>

          <section className="grid grid-cols-2 gap-3 sm:grid-cols-5">
            <Stat label="Tables" value={data.summary.total} />
            <Stat label="In sync" value={data.summary.in_sync} tone="emerald" />
            <Stat label="Drifted" value={data.summary.drifted} tone="amber" />
            <Stat label="Missing" value={data.summary.missing_table} tone="red" />
            <Stat label="Migrated" value={data.summary.migrated} tone="zinc" />
          </section>

          <section>
            <div className="overflow-x-auto rounded-md border border-zinc-200 dark:border-zinc-800">
              <table className="min-w-full text-sm">
                <thead className="bg-zinc-100 text-left dark:bg-zinc-900">
                  <tr>
                    <Th className="w-8">{""}</Th>
                    <Th>Table</Th>
                    <Th>Section</Th>
                    <Th className="text-right">Fields</Th>
                    <Th className="text-right">Drift</Th>
                    <Th className="text-right">Rows</Th>
                    <Th>Migrated</Th>
                  </tr>
                </thead>
                <tbody>
                  {data.tables.map((t) => (
                    <tr
                      key={`${t.table_name}::${t.source_dir}`}
                      className="border-t border-zinc-200 dark:border-zinc-800"
                    >
                      <Td>
                        <StatusIcon status={t.status} />
                      </Td>
                      <Td className="font-medium">
                        <Link
                          to={`/tables/${encodeURIComponent(t.table_name)}`}
                          className="hover:underline"
                        >
                          {t.table_name}
                        </Link>
                      </Td>
                      <Td className="text-zinc-500">{t.section || "(global)"}</Td>
                      <Td className="text-right tabular-nums">{t.field_count}</Td>
                      <Td className="text-right tabular-nums">
                        <DriftCell t={t} />
                      </Td>
                      <Td className="text-right tabular-nums">
                        {t.row_count ?? "—"}
                      </Td>
                      <Td>
                        {t.migrated ? (
                          <CheckCircle2 className="h-4 w-4 text-emerald-600" />
                        ) : (
                          <Circle className="h-4 w-4 text-zinc-400" />
                        )}
                      </Td>
                    </tr>
                  ))}
                  {data.tables.length === 0 && (
                    <tr>
                      <td className="p-3 text-zinc-500" colSpan={7}>
                        No tables in this project yet.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </section>
        </>
      )}
    </div>
  );
}

function StatusIcon({ status }: { status: TableHealthStatus }) {
  switch (status) {
    case "ok":
      return <CheckCircle2 className="h-4 w-4 text-emerald-600" />;
    case "drift":
      return <AlertTriangle className="h-4 w-4 text-amber-600" />;
    case "missing":
      return <XCircle className="h-4 w-4 text-red-600" />;
    case "no_schema":
      return <Circle className="h-4 w-4 text-zinc-400" />;
    case "no_connection":
      return <XCircle className="h-4 w-4 text-zinc-400" />;
  }
}

function DriftCell({
  t,
}: {
  t: { missing_in_backend: number; extra_in_backend: number; type_mismatches: number; status: TableHealthStatus };
}) {
  if (t.status === "ok") return <span className="text-emerald-600">·</span>;
  if (t.status === "missing") return <span className="text-red-600">no table</span>;
  if (t.status === "no_schema") return <span className="text-zinc-400">no fields</span>;
  if (t.status === "no_connection") return <span className="text-zinc-400">·</span>;
  const parts: string[] = [];
  if (t.missing_in_backend) parts.push(`-${t.missing_in_backend}`);
  if (t.extra_in_backend) parts.push(`+${t.extra_in_backend}`);
  if (t.type_mismatches) parts.push(`~${t.type_mismatches}`);
  return <span className="text-amber-700 dark:text-amber-400">{parts.join(" ")}</span>;
}

function Stat({
  label,
  value,
  tone,
}: {
  label: string;
  value: number;
  tone?: "emerald" | "amber" | "red" | "zinc";
}) {
  const colors: Record<string, string> = {
    emerald: "text-emerald-700 dark:text-emerald-300",
    amber: "text-amber-700 dark:text-amber-300",
    red: "text-red-700 dark:text-red-300",
    zinc: "text-zinc-700 dark:text-zinc-300",
  };
  const cls = tone ? colors[tone] : "";
  return (
    <div className="rounded-md border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-950">
      <div className="text-xs text-zinc-500 uppercase">{label}</div>
      <div className={`text-2xl font-semibold tabular-nums ${cls}`}>{value}</div>
    </div>
  );
}

function Th({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <th className={`px-3 py-2 text-xs font-semibold uppercase tracking-wide ${className ?? ""}`}>
      {children}
    </th>
  );
}
function Td({ children, className }: { children: React.ReactNode; className?: string }) {
  return <td className={`px-3 py-2 ${className ?? ""}`}>{children}</td>;
}
