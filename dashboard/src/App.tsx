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
import RoutesPage from '@/pages/routes/RoutesPage'
import ProjectLayout from '@/components/layout/ProjectLayout'
// Data platform
import DatabasesPage from '@/pages/data/DatabasesPage'
import TablesPage from '@/pages/data/TablesPage'
import TableWorkspacePage from '@/pages/data/TableWorkspacePage'
import StoragePage from '@/pages/storage/StoragePage'
import EventsPage from '@/pages/events/EventsPage'
import WorkflowsPage from '@/pages/workflows/WorkflowsPage'
import CronPage from '@/pages/cron/CronPage'

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
            <Route path="projects/:projectId" element={<ProjectLayout />}>
              <Route path="overview" element={<OverviewPage />} />
              {/* Data platform */}
              <Route path="data" element={<DatabasesPage />} />
              <Route path="data/:database" element={<TablesPage />} />
              <Route path="data/:database/:table" element={<TableWorkspacePage />} />
              <Route path="storage" element={<StoragePage />} />
              <Route path="events" element={<EventsPage />} />
              <Route path="workflows" element={<WorkflowsPage />} />
              <Route path="cron" element={<CronPage />} />
              {/* Serverless */}
              <Route path="functions" element={<FunctionsPage />} />
              <Route path="functions/:functionId" element={<FunctionDetailPage />} />
              <Route path="routes" element={<RoutesPage />} />
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
