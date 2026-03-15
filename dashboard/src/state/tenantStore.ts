// Tenant/project concept removed — stub kept for backward compat during migration.
export const useStore = Object.assign(
  () => ({
    tenantId: null as string | null,
    projectId: null as string | null,
    tenantName: null as string | null,
    projectName: null as string | null,
    setTenant: (_id: string, _name: string) => {},
    setProject: (_id: string, _name: string) => {},
    clearProject: () => {},
    clear: () => {},
  }),
  {
    getState: () => ({
      tenantId: null as string | null,
      projectId: null as string | null,
      tenantName: null as string | null,
      projectName: null as string | null,
      setTenant: (_id: string, _name: string) => {},
      setProject: (_id: string, _name: string) => {},
      clearProject: () => {},
      clear: () => {},
    }),
  },
)
