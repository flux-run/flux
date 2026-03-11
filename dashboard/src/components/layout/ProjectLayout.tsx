'use client'

import { useEffect, useState } from 'react'
import { useParams } from 'next/navigation'
import { useStore } from '@/state/tenantStore'

export default function ProjectLayout({ children }: { children: React.ReactNode }) {
  const params = useParams() as any
  const projectId = params?.projectId
  const { projectId: storeId, setProject } = useStore()
  const [ready, setReady] = useState(false)

  useEffect(() => {
    if (projectId && projectId !== storeId) {
      setProject(projectId, 'Project')
    }
    setReady(true)
  }, [projectId, storeId, setProject])

  if (!ready) return null

  return <>{children}</>
}
