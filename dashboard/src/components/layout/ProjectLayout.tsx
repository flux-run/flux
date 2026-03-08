import { useEffect, useState } from 'react'
import { Outlet, useParams } from 'react-router-dom'
import { useStore } from '@/state/tenantStore'

export default function ProjectLayout() {
  const { projectId } = useParams<{ projectId: string }>()
  const { projectId: storeId, setProject } = useStore()
  const [ready, setReady] = useState(false)

  useEffect(() => {
    if (projectId && projectId !== storeId) {
      setProject(projectId, 'Project')
    }
    setReady(true)
  }, [projectId, storeId, setProject])

  // Don't render outlet until the Zustand store is definitely populated.
  // This ensures apiFetch correctly sends the X-Fluxbase-Project header.
  if (!ready || (projectId && projectId !== useStore.getState().projectId)) {
    return null
  }

  return <Outlet />
}
