import { create } from "zustand";
import { persist } from "zustand/middleware";

interface TenantStore {
  tenantId: string | null;
  projectId: string | null;
  tenantName: string | null;
  projectName: string | null;
  setTenant: (id: string, name: string) => void;
  setProject: (id: string, name: string) => void;
  clearProject: () => void;
  clear: () => void;
}

export const useStore = create<TenantStore>()(
  persist(
    (set) => ({
      tenantId: null,
      projectId: null,
      tenantName: null,
      projectName: null,
      setTenant: (id, name) =>
        set({
          tenantId: id,
          tenantName: name,
          projectId: null,
          projectName: null,
        }),
      setProject: (id, name) => set({ projectId: id, projectName: name }),
      clearProject: () => set({ projectId: null, projectName: null }),
      clear: () =>
        set({
          tenantId: null,
          tenantName: null,
          projectId: null,
          projectName: null,
        }),
    }),
    {
      name: "fluxbase-workspace",
      partialize: (state) => ({
        tenantId: state.tenantId,
        tenantName: state.tenantName,
        projectId: state.projectId,
        projectName: state.projectName,
      }),
    },
  ),
);
