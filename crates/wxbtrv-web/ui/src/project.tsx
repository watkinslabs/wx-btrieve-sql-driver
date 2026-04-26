import { createContext, useCallback, useContext, useEffect, useState } from "react";
import { api, type CurrentProject } from "@/api";

interface ProjectCtx {
  project: CurrentProject | null;
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

const Ctx = createContext<ProjectCtx>({
  project: null,
  loading: true,
  error: null,
  refresh: async () => {},
});

export function ProjectProvider({ children }: { children: React.ReactNode }) {
  const [project, setProject] = useState<CurrentProject | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setProject(await api.currentProject());
    } catch (e: any) {
      setError(String(e?.message ?? e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return <Ctx.Provider value={{ project, loading, error, refresh }}>{children}</Ctx.Provider>;
}

export const useProject = () => useContext(Ctx);
