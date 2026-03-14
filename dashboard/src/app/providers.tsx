'use client'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useState } from 'react'
import { ToastRoot } from '@/components/ui/toast'

export function Providers({ children }: { children: React.ReactNode }) {
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            retry: 1,
            staleTime: 15_000,
          },
        },
      })
  )

  return (
    <QueryClientProvider client={queryClient}>
      <ToastRoot>
        {children}
      </ToastRoot>
    </QueryClientProvider>
  )
}
