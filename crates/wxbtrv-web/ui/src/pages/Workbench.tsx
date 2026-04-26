import { useEffect, useState } from "react";
import { FolderOpen, FilePlus2, X, Loader2 } from "lucide-react";
import { api, WXBTRV_DB_FILTER, type RecentEntry } from "@/api";
import { useProject } from "@/project";

export function WorkbenchPage() {
  const { project, refresh } = useProject();
  const [recent, setRecent] = useState<RecentEntry[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  async function reloadRecent() {
    try {
      setRecent(await api.recent());
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    }
  }
  useEffect(() => {
    reloadRecent();
  }, []);

  async function openExisting() {
    setErr(null);
    setBusy("open");
    try {
      const picked = await api.pickOpen({
        title: "Open wxbtrv.db",
        filters: [WXBTRV_DB_FILTER],
      });
      if (picked.path) {
        await api.openProject(picked.path);
        await refresh();
        await reloadRecent();
      }
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setBusy(null);
    }
  }

  async function createNew() {
    setErr(null);
    setBusy("create");
    try {
      const picked = await api.pickSave({
        title: "Create wxbtrv.db",
        filters: [WXBTRV_DB_FILTER],
        suggest_name: "wxbtrv.db",
      });
      if (picked.path) {
        await api.initProject(picked.path, true);
        await refresh();
        await reloadRecent();
      }
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setBusy(null);
    }
  }

  async function openRecent(path: string) {
    setErr(null);
    setBusy(`open:${path}`);
    try {
      await api.openProject(path);
      await refresh();
      await reloadRecent();
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setBusy(null);
    }
  }

  async function closeProject() {
    setErr(null);
    setBusy("close");
    try {
      await api.closeProject();
      await refresh();
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setBusy(null);
    }
  }

  async function forget(path: string) {
    await api.forgetRecent(path);
    await reloadRecent();
  }

  return (
    <div className="max-w-3xl space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Workbench</h1>
        <p className="text-sm text-zinc-600 dark:text-zinc-400">
          A wxbtrv project is a single <code>wxbtrv.db</code> SQLite file holding your
          connection settings and table schemas. Open one to start working.
        </p>
      </header>

      <section className="rounded-md border border-zinc-200 dark:border-zinc-800 p-4">
        <div className="text-sm font-medium text-zinc-700 dark:text-zinc-300">Current project</div>
        {project?.path ? (
          <div className="mt-2 flex items-center justify-between gap-3">
            <code className="break-all text-sm">{project.path}</code>
            <button
              className="rounded-md border border-zinc-300 px-3 py-1.5 text-xs hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
              onClick={closeProject}
              disabled={busy === "close"}
            >
              Close
            </button>
          </div>
        ) : (
          <div className="mt-2 text-sm text-zinc-500">No project open.</div>
        )}
      </section>

      <div className="flex gap-3">
        <button
          className="inline-flex items-center gap-2 rounded-md bg-zinc-900 px-4 py-2 text-sm font-medium text-zinc-50 hover:bg-zinc-800 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200"
          onClick={openExisting}
          disabled={busy === "open"}
        >
          {busy === "open" ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <FolderOpen className="h-4 w-4" />
          )}
          Open existing…
        </button>
        <button
          className="inline-flex items-center gap-2 rounded-md border border-zinc-300 px-4 py-2 text-sm font-medium hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
          onClick={createNew}
          disabled={busy === "create"}
        >
          {busy === "create" ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <FilePlus2 className="h-4 w-4" />
          )}
          Create new…
        </button>
      </div>

      {err && (
        <div className="rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-900 dark:bg-red-950 dark:border-red-800 dark:text-red-200">
          {err}
        </div>
      )}

      <section>
        <div className="text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-2">Recent</div>
        {recent.length === 0 ? (
          <div className="text-sm text-zinc-500">No recently opened projects.</div>
        ) : (
          <ul className="divide-y divide-zinc-200 dark:divide-zinc-800 rounded-md border border-zinc-200 dark:border-zinc-800">
            {recent.map((r) => (
              <li key={r.path} className="flex items-center gap-3 p-3">
                <button
                  className="flex-1 text-left text-sm hover:underline disabled:opacity-50"
                  onClick={() => openRecent(r.path)}
                  disabled={busy === `open:${r.path}`}
                >
                  <code className="break-all">{r.path}</code>
                  <div className="text-xs text-zinc-500">
                    opened {new Date(r.opened_at * 1000).toLocaleString()}
                  </div>
                </button>
                <button
                  className="rounded-md p-1.5 text-zinc-400 hover:text-zinc-700 hover:bg-zinc-100 dark:hover:bg-zinc-800"
                  onClick={() => forget(r.path)}
                  title="Forget"
                >
                  <X className="h-4 w-4" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
