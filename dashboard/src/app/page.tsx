'use client'
import { useEffect } from 'react'
import { useRouter } from 'next/navigation'

// SPA entry point. When the Rust server serves index.html as a fallback for an
// unmatched /flux/* path (e.g. after a hard refresh on a deep link), this page
// reads window.location and navigates the App Router to the real destination.
export default function RootPage() {
  const router = useRouter()
  useEffect(() => {
    const path = window.location.pathname
    const dest = path === '/flux' || path === '/flux/' ? '/flux/dashboard' : path
    router.replace(dest)
  }, [router])
  return null
}
