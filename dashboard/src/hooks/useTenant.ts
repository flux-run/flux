'use client'

import { useStore } from "@/state/tenantStore";
import { useQueryClient } from "@tanstack/react-query";

export function useTenant() {
  const { tenantId, tenantName, setTenant } = useStore();
  const queryClient = useQueryClient();

  const switchTenant = (id: string, name: string) => {
    setTenant(id, name);
    // Invalidate all queries so they re-fetch with new tenant header
    queryClient.invalidateQueries();
  };

  return { tenantId, tenantName, switchTenant };
}
