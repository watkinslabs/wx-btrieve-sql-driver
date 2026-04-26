import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { api, type TableSummary } from "@/api";

export function TablesPage() {
  const [rows, setRows] = useState<TableSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

  useEffect(() => {
    api.listTables().then(setRows).catch((e) => setError(String(e.message ?? e)));
  }, []);

  const filtered = useMemo(() => {
    if (!rows) return [];
    const q = filter.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter(
      (r) =>
        r.table_name.toLowerCase().includes(q) ||
        r.db_name.toLowerCase().includes(q) ||
        r.source_dir.toLowerCase().includes(q),
    );
  }, [rows, filter]);

  if (error) {
    return (
      <div className="text-red-700 dark:text-red-300 text-sm">
        Could not load tables: {error}
      </div>
    );
  }
  if (!rows) return <div className="text-zinc-500">Loading…</div>;

  return (
    <div className="space-y-4">
      <header className="flex items-end justify-between">
        <div>
          <h1 className="text-xl font-semibold">Tables</h1>
          <p className="text-sm text-zinc-600 dark:text-zinc-400">
            {rows.length} tables in this <code>wxbtrv.db</code>
          </p>
        </div>
        <input
          type="search"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter…"
          className="rounded-md border border-zinc-300 bg-white p-2 text-sm dark:bg-zinc-900 dark:border-zinc-700"
        />
      </header>

      <div className="overflow-x-auto rounded-md border border-zinc-200 dark:border-zinc-800">
        <table className="min-w-full text-sm">
          <thead className="bg-zinc-100 text-left dark:bg-zinc-900">
            <tr>
              <Th>Table</Th>
              <Th>Schema</Th>
              <Th>Database</Th>
              <Th>Source dir</Th>
              <Th className="text-right">Rec len</Th>
              <Th className="text-right">Fields</Th>
              <Th className="text-right">Indexes</Th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((r) => (
              <tr
                key={`${r.table_name}::${r.source_dir}`}
                className="border-t border-zinc-200 hover:bg-zinc-50 dark:border-zinc-800 dark:hover:bg-zinc-900"
              >
                <Td>
                  <Link
                    className="font-medium text-blue-700 hover:underline dark:text-blue-400"
                    to={`/tables/${encodeURIComponent(r.table_name)}`}
                  >
                    {r.table_name}
                  </Link>
                </Td>
                <Td>{r.schema_name || "—"}</Td>
                <Td>{r.db_name || "—"}</Td>
                <Td className="text-zinc-600 dark:text-zinc-400">{r.source_dir || "—"}</Td>
                <Td className="text-right tabular-nums">{r.record_length}</Td>
                <Td className="text-right tabular-nums">{r.field_count}</Td>
                <Td className="text-right tabular-nums">{r.index_count}</Td>
              </tr>
            ))}
            {filtered.length === 0 && (
              <tr>
                <td colSpan={7} className="p-8 text-center text-zinc-500">
                  No tables match the filter.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
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
