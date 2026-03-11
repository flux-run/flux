import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Zap } from 'lucide-react'
import { useAuth } from '@/hooks/useAuth'
import { apiFetch } from '@/lib/api'
import { useStore } from '@/state/tenantStore'
import { Button } from '@/components/ui/button'

export default function LoginPage() {
  const { signInWithGoogle, signInWithGitHub } = useAuth()
  const navigate = useNavigate()
  const { setTenant } = useStore()
  const [loadingGoogle, setLoadingGoogle] = useState(false)
  const [loadingGitHub, setLoadingGitHub] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleSignIn = async (provider: 'google' | 'github') => {
    const setLoading = provider === 'google' ? setLoadingGoogle : setLoadingGitHub
    setLoading(true)
    setError(null)
    try {
      if (provider === 'google') {
        await signInWithGoogle()
      } else {
        await signInWithGitHub()
      }
      // Call /auth/me to bootstrap user in DB and get tenant list
      const me = await apiFetch<{ tenants: Array<{ tenant_id: string; name: string }> }>(
        '/auth/me'
      )
      if (me.tenants.length > 0) {
        setTenant(me.tenants[0].tenant_id, me.tenants[0].name)
      }
      navigate('/dashboard')
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Sign in failed')
    } finally {
      setLoading(false)
    }
  }

  const loading = loadingGoogle || loadingGitHub

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
          <h1 className="text-2xl font-bold tracking-tight">Fluxbase</h1>
          <p className="text-sm text-muted-foreground mt-1">Control Plane</p>
          <p className="text-xs text-muted-foreground/70 mt-2 text-center leading-relaxed">
            Deploy functions. Query data. Ship faster.
          </p>
        </div>

        {/* Card */}
        <div className="rounded-2xl border bg-card p-8 shadow-xl shadow-black/20">
          <h2 className="text-lg font-semibold mb-1">Welcome back</h2>
          <p className="text-sm text-muted-foreground mb-6">
            Sign in to manage your functions, secrets, and deployments.
          </p>

          {error && (
            <div className="mb-4 rounded-lg bg-destructive/10 border border-destructive/20 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}

          <div className="flex flex-col gap-3">
            {/* GitHub — primary for developer tools */}
            <Button
              variant="secondary"
              className="w-full gap-2 h-10 bg-[#24292e] hover:bg-[#2f363d] text-white border-0"
              onClick={() => handleSignIn('github')}
              disabled={loading}
            >
              {loadingGitHub ? (
                <span className="w-4 h-4 border-2 border-white/40 border-t-white rounded-full animate-spin" />
              ) : (
                <svg viewBox="0 0 24 24" className="w-4 h-4 fill-white" aria-hidden="true">
                  <path d="M12 2C6.477 2 2 6.484 2 12.017c0 4.425 2.865 8.18 6.839 9.504.5.092.682-.217.682-.483 0-.237-.008-.868-.013-1.703-2.782.605-3.369-1.343-3.369-1.343-.454-1.158-1.11-1.466-1.11-1.466-.908-.62.069-.608.069-.608 1.003.07 1.531 1.032 1.531 1.032.892 1.53 2.341 1.088 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.113-4.555-4.951 0-1.093.39-1.988 1.029-2.688-.103-.253-.446-1.272.098-2.65 0 0 .84-.27 2.75 1.026A9.564 9.564 0 0112 6.844a9.59 9.59 0 012.504.337c1.909-1.296 2.747-1.027 2.747-1.027.546 1.379.202 2.398.1 2.651.64.7 1.028 1.595 1.028 2.688 0 3.848-2.339 4.695-4.566 4.943.359.309.678.92.678 1.855 0 1.338-.012 2.419-.012 2.747 0 .268.18.58.688.482A10.019 10.019 0 0022 12.017C22 6.484 17.522 2 12 2z" />
                </svg>
              )}
              {loadingGitHub ? 'Signing in…' : 'Continue with GitHub'}
            </Button>

            {/* Google */}
            <Button
              variant="outline"
              className="w-full gap-2 h-10"
              onClick={() => handleSignIn('google')}
              disabled={loading}
            >
              {loadingGoogle ? (
                <span className="w-4 h-4 border-2 border-foreground/30 border-t-foreground rounded-full animate-spin" />
              ) : (
                <svg viewBox="0 0 24 24" className="w-4 h-4" aria-hidden="true">
                  <path d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z" fill="#4285F4" />
                  <path d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z" fill="#34A853" />
                  <path d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z" fill="#FBBC05" />
                  <path d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z" fill="#EA4335" />
                </svg>
              )}
              {loadingGoogle ? 'Signing in…' : 'Continue with Google'}
            </Button>
          </div>

          {/* CLI hint */}
          <div className="mt-5 rounded-lg border border-border/50 bg-muted/50 px-4 py-3">
            <p className="text-xs text-muted-foreground mb-1.5">Or authenticate from the CLI</p>
            <code className="text-xs font-mono text-foreground/80">flux login</code>
          </div>

          <p className="text-xs text-muted-foreground text-center mt-4">
            By signing in you agree to the Fluxbase Terms of Service.
          </p>
        </div>
      </div>
    </div>
  )
}
