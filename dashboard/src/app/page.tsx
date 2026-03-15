'use client'
import { useEffect } from 'react'
import { useRouter } from 'next/navigation'

// SPA entry point: redirect the root URL to /dashboard.
// Next.js App Router automatically prepends basePath ("/flux") to all
// router.replace/push calls, so paths here must NOT include the basePath.
export default function RootPage() {
  const router = useRouter()
  useEffect(() => {
    router.replace('/dashboard')
  }, [router])
  return null
}
