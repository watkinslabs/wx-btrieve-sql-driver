import { NavLink, Outlet } from "react-router-dom";
import { Activity, Database, FileInput, GitMerge, Hammer, ListTree, Plug, Wrench } from "lucide-react";
import { ProjectProvider, useProject } from "@/project";

const navLinkClass = ({ isActive }: { isActive: boolean }) =>
  [
    "flex items-center gap-2 rounded-md px-3 py-2 text-sm font-medium",
    isActive
      ? "bg-zinc-900 text-zinc-50 dark:bg-zinc-100 dark:text-zinc-900"
      : "text-zinc-700 hover:bg-zinc-200 dark:text-zinc-300 dark:hover:bg-zinc-800",
  ].join(" ");

export default function App() {
  return (
    <ProjectProvider>
      <Shell />
    </ProjectProvider>
  );
}

function Shell() {
  const { project } = useProject();
  return (
    <div className="min-h-screen flex">
      <aside className="w-60 border-r border-zinc-200 dark:border-zinc-800 p-4 flex flex-col gap-1">
        <div className="px-3 pb-2 text-lg font-semibold flex items-center gap-2">
          <Database className="h-5 w-5" /> wxbtrv
        </div>
        <div className="px-3 pb-3 text-xs text-zinc-500 break-all">
          {project?.path ? project.path : "no project open"}
        </div>
        <NavLink to="/workbench" className={navLinkClass}>
          <Wrench className="h-4 w-4" /> Workbench
        </NavLink>
        <NavLink to="/health" className={navLinkClass}>
          <Activity className="h-4 w-4" /> Health
        </NavLink>
        <NavLink to="/connections" className={navLinkClass}>
          <Plug className="h-4 w-4" /> Connections
        </NavLink>
        <NavLink to="/tables" className={navLinkClass}>
          <ListTree className="h-4 w-4" /> Tables
        </NavLink>
        <NavLink to="/tools" className={navLinkClass}>
          <Hammer className="h-4 w-4" /> Tools
        </NavLink>
        <NavLink to="/bimport" className={navLinkClass}>
          <FileInput className="h-4 w-4" /> .B Import
        </NavLink>
        <NavLink to="/migration" className={navLinkClass}>
          <GitMerge className="h-4 w-4" /> Migration
        </NavLink>
      </aside>
      <main className="flex-1 p-6 overflow-y-auto">
        <Outlet />
      </main>
    </div>
  );
}
