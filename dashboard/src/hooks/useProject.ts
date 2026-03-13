'use client'

import { useStore } from "@/state/tenantStore";
import { useQueryClient } from "@tanstack/react-query";

export function useProject() {
  const { projectId, projectName, setProject, clearProject } = useStore();
  const queryClient = useQueryClient();

  const switchProject = (id: string, name: string) => {
    setProject(id, name);
    queryClient.invalidateQueries({ queryKey: ["functions"] });
    queryClient.invalidateQueries({ queryKey: ["secrets"] });
    queryClient.invalidateQueries({ queryKey: ["api-keys"] });
  };

  return { projectId, projectName, switchProject, clearProject };
}
