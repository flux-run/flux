'use client'

import { useState } from 'react'
import { useRouter } from 'next/navigation'
import { Zap } from 'lucide-react'
import { signIn } from '@/lib/auth'
import { Button } from '@/components/ui/button'

export default function LoginPage() {
  const router = useRouter()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoading(true)
    setError(null)
    try {
      const result = await signIn(email, password)
      if (result.user.tenant_id) {
      }
      router.push('/dashboard')
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Sign in failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-background relative overflow-hidden">
      {/* Background glow */}
      <div className="absolute inset-0 pointer-events-none">
        <div className="absolute top-1/3 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] rounded-full bg-primary/10 blur-[120px]" />
        <div className="absolute top-2/3 left-1/4 w-[300px] h-[300px] rounded-full bg-violet-500/5 blur-[80px]" />
      </div>

      <div className="relative z-10 w-full max-w-sm mx-auto px-6">
        {/* Logo */}
        <div className="flex flex-col items-center mb-10">
          <div className="flex items-center justify-center w-14 h-14 rounded-2xl bg-primary/15 border border-primary/30 mb-4 shadow-lg shadow-primary/10">
            <Zap className="w-7 h-7 text-primary" />
          </div>
          <h1 className="text-2xl font-bold tracking-tight">Flux</h1>
          <p className="text-sm text-muted-foreground mt-1">Control Plane</p>
          <p className="text-xs text-muted-foreground/70 mt-2 text-center leading-relaxed">
            Deploy functions. Query data. Ship faster.
          </p>
        </div>

        {/* Card */}
        <div className="rounded-2xl border bg-card p-8 shadow-xl shadow-black/20">
          <h2 className="text-lg font-semibold mb-1">Sign in</h2>
          <p className="text-sm text-muted-foreground mb-6">
            Internal access only. Contact your admin to get an account.
          </p>

          {error && (
            <div className="mb-4 rounded-lg bg-destructive/10 border border-destructive/20 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}

          <form onSubmit={handleSubmit} className="flex flex-col gap-3">
            <div className="flex flex-col gap-1.5">
              <label htmlFor="email" className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
                Email
              </label>
              <input
                id="email"
                type="email"
                autoComplete="email"
                required
                value={email}
                onChange={e => setEmail(e.target.value)}
                placeholder="admin@example.com"
                className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <label htmlFor="password" className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
                Password
              </label>
              <input
                id="password"
                type="password"
                autoComplete="current-password"
                required
                value={password}
                onChange={e => setPassword(e.target.value)}
                placeholder="••••••••"
                className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              />
            </div>

            <Button type="submit" className="w-full mt-1 gap-2 h-10" disabled={loading}>
              {loading && (
                <span className="w-4 h-4 border-2 border-white/40 border-t-white rounded-full animate-spin" />
              )}
              {loading ? 'Signing in…' : 'Sign in'}
            </Button>
          </form>

          {/* CLI hint */}
          <div className="mt-5 rounded-lg border border-border/50 bg-muted/50 px-4 py-3">
            <p className="text-xs text-muted-foreground mb-1.5">First time? Create the initial admin account</p>
            <code className="text-xs font-mono text-foreground/80">flux login</code>
          </div>
        </div>
      </div>
    </div>
  )
}


