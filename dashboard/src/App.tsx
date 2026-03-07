import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import AppShell from '@/components/layout/AppShell'
import LoginPage from '@/pages/LoginPage'
import ProjectsPage from '@/pages/projects/ProjectsPage'
import OverviewPage from '@/pages/projects/OverviewPage'
import FunctionsPage from '@/pages/functions/FunctionsPage'
import FunctionDetailPage from '@/pages/functions/FunctionDetailPage'
import SecretsPage from '@/pages/secrets/SecretsPage'
import ApiKeysPage from '@/pages/api-keys/ApiKeysPage'
import TenantSettingsPage from '@/pages/tenants/TenantSettingsPage'
import LogsPage from '@/pages/logs/LogsPage'
import ProjectSettingsPage from '@/pages/projects/ProjectSettingsPage'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      staleTime: 15_000,
    },
  },
})

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <Routes>
          {/* Public */}
          <Route path="/login" element={<LoginPage />} />

          {/* Protected — all under /dashboard */}
          <Route path="/dashboard" element={<AppShell />}>
            {/* Workspace level */}
            <Route index element={<ProjectsPage />} />
            <Route path="tenants" element={<TenantSettingsPage />} />

            {/* Project level */}
            <Route path="projects/:projectId">
              <Route path="overview" element={<OverviewPage />} />
              <Route path="functions" element={<FunctionsPage />} />
              <Route path="functions/:functionId" element={<FunctionDetailPage />} />
              <Route path="secrets" element={<SecretsPage />} />
              <Route path="api-keys" element={<ApiKeysPage />} />
              <Route path="logs" element={<LogsPage />} />
              <Route path="settings" element={<ProjectSettingsPage />} />
            </Route>
          </Route>

          {/* Fallback */}
          <Route path="*" element={<Navigate to="/dashboard" replace />} />
        </Routes>
      </BrowserRouter>
    </QueryClientProvider>
  )
}
