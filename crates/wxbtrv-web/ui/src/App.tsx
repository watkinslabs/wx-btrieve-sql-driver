import { NavLink, Outlet } from "react-router-dom";
import { Database, ListTree, Settings } from "lucide-react";

const navLinkClass = ({ isActive }: { isActive: boolean }) =>
  [
    "flex items-center gap-2 rounded-md px-3 py-2 text-sm font-medium",
    isActive
      ? "bg-zinc-900 text-zinc-50 dark:bg-zinc-100 dark:text-zinc-900"
      : "text-zinc-700 hover:bg-zinc-200 dark:text-zinc-300 dark:hover:bg-zinc-800",
  ].join(" ");

export default function App() {
  return (
    <div className="min-h-screen flex">
      <aside className="w-56 border-r border-zinc-200 dark:border-zinc-800 p-4 flex flex-col gap-1">
        <div className="px-3 pb-4 text-lg font-semibold flex items-center gap-2">
          <Database className="h-5 w-5" /> wxbtrv
        </div>
        <NavLink to="/connection" className={navLinkClass}>
          <Settings className="h-4 w-4" /> Connection
        </NavLink>
        <NavLink to="/tables" className={navLinkClass}>
          <ListTree className="h-4 w-4" /> Tables
        </NavLink>
      </aside>
      <main className="flex-1 p-6 overflow-y-auto">
        <Outlet />
      </main>
    </div>
  );
}
