import { useEffect, useState } from "react";
import { CheckCircle2, Circle, Loader2 } from "lucide-react";
import { workflow, type MigrationRow } from "@/api";
import { useProject } from "@/project";

export function MigrationPage() {
  const { project } = useProject();
  const [rows, setRows] = useState<MigrationRow[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  async function reload() {
    if (!project?.path) return;
    setErr(null);
    try {
      setRows(await workflow.migrationStatus());
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    }
  }
  useEffect(() => {
    reload();
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

  async function toggle(row: MigrationRow) {
    setBusy(row.table_name);
    try {
      if (row.migrated) {
        await workflow.clearMigrated(row.table_name);
      } else {
        await workflow.markMigrated({
          table: row.table_name,
          rows: row.row_count ?? undefined,
          target_db: row.target_db || undefined,
        });
      }
      await reload();
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setBusy(null);
    }
  }

  if (err) {
    return (
      <div className="rounded-md border border-red-300 bg-red-50 p-4 text-sm text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200">
        {err}
      </div>
    );
  }
  if (!rows) {
    return <div className="text-zinc-500">Loading…</div>;
  }

  const done = rows.filter((r) => r.migrated).length;

  return (
    <div className="space-y-4">
      <header>
        <h1 className="text-xl font-semibold">Migration tracking</h1>
        <p className="text-sm text-zinc-600 dark:text-zinc-400">
          {done}/{rows.length} tables marked migrated. Toggle a row once its
          data has been moved to the target server.
        </p>
      </header>
      <div className="overflow-x-auto rounded-md border border-zinc-200 dark:border-zinc-800">
        <table className="w-full text-sm">
          <thead className="bg-zinc-50 dark:bg-zinc-900">
            <tr className="text-left">
              <th className="p-2"></th>
              <th className="p-2">Table</th>
              <th className="p-2">Source dir</th>
              <th className="p-2 text-right">Rows</th>
              <th className="p-2 text-right">Fields</th>
              <th className="p-2">Target DB</th>
              <th className="p-2">Migrated at</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr
                key={`${r.table_name}::${r.source_dir}`}
                className="border-t border-zinc-200 dark:border-zinc-800"
              >
                <td className="p-2">
                  <button
                    className="inline-flex items-center"
                    onClick={() => toggle(r)}
                    disabled={busy === r.table_name}
                    title={r.migrated ? "Mark as not migrated" : "Mark migrated"}
                  >
                    {busy === r.table_name ? (
                      <Loader2 className="h-4 w-4 animate-spin text-zinc-400" />
                    ) : r.migrated ? (
                      <CheckCircle2 className="h-4 w-4 text-emerald-600" />
                    ) : (
                      <Circle className="h-4 w-4 text-zinc-400" />
                    )}
                  </button>
                </td>
                <td className="p-2 font-medium">{r.table_name}</td>
                <td className="p-2 text-zinc-600 dark:text-zinc-400">{r.source_dir}</td>
                <td className="p-2 text-right">{r.row_count ?? "—"}</td>
                <td className="p-2 text-right">{r.field_count}</td>
                <td className="p-2">{r.target_db || "—"}</td>
                <td className="p-2 text-zinc-600 dark:text-zinc-400">
                  {r.migrated_at ?? "—"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
