import os

BASE = "/Users/shashisharma/code/self/flowbase/dashboard/src/app"

pages = [
  ("dashboard/login/page.tsx", "LoginPage", "@/pages/LoginPage"),
  ("dashboard/page.tsx", "ProjectsPage", "@/pages/projects/ProjectsPage"),
  ("dashboard/tenants/page.tsx", "TenantSettingsPage", "@/pages/tenants/TenantSettingsPage"),
  ("dashboard/projects/[projectId]/overview/page.tsx", "OverviewPage", "@/pages/projects/OverviewPage"),
  ("dashboard/projects/[projectId]/data/page.tsx", "DatabasesPage", "@/pages/data/DatabasesPage"),
  ("dashboard/projects/[projectId]/data/[database]/page.tsx", "TablesPage", "@/pages/data/TablesPage"),
  ("dashboard/projects/[projectId]/data/[database]/[table]/page.tsx", "TableWorkspacePage", "@/pages/data/TableWorkspacePage"),
  ("dashboard/projects/[projectId]/storage/page.tsx", "StoragePage", "@/pages/storage/StoragePage"),
  ("dashboard/projects/[projectId]/events/page.tsx", "EventsPage", "@/pages/events/EventsPage"),
  ("dashboard/projects/[projectId]/workflows/page.tsx", "WorkflowsPage", "@/pages/workflows/WorkflowsPage"),
  ("dashboard/projects/[projectId]/cron/page.tsx", "CronPage", "@/pages/cron/CronPage"),
  ("dashboard/projects/[projectId]/query/page.tsx", "QueryExplorerPage", "@/pages/query/QueryExplorerPage"),
  ("dashboard/projects/[projectId]/schema/page.tsx", "SchemaGraphPage", "@/pages/schema/SchemaGraphPage"),
  ("dashboard/projects/[projectId]/functions/page.tsx", "FunctionsPage", "@/pages/functions/FunctionsPage"),
  ("dashboard/projects/[projectId]/functions/[functionId]/page.tsx", "FunctionDetailPage", "@/pages/functions/FunctionDetailPage"),
  ("dashboard/projects/[projectId]/routes/page.tsx", "RoutesPage", "@/pages/routes/RoutesPage"),
  ("dashboard/projects/[projectId]/secrets/page.tsx", "SecretsPage", "@/pages/secrets/SecretsPage"),
  ("dashboard/projects/[projectId]/api-keys/page.tsx", "ApiKeysPage", "@/pages/api-keys/ApiKeysPage"),
  ("dashboard/projects/[projectId]/logs/page.tsx", "LogsPage", "@/pages/logs/LogsPage"),
  ("dashboard/projects/[projectId]/settings/page.tsx", "ProjectSettingsPage", "@/pages/projects/ProjectSettingsPage"),
  ("dashboard/projects/[projectId]/integrations/page.tsx", "IntegrationsPage", "@/pages/integrations/IntegrationsPage"),
]

layouts = [
  ("dashboard/layout.tsx",
   "import AppShell from '@/components/layout/AppShell'\nexport default function DashboardLayout({ children }: { children: React.ReactNode }) {\n  return <AppShell>{children}</AppShell>\n}\n"),
  ("dashboard/projects/[projectId]/layout.tsx",
   "import ProjectLayout from '@/components/layout/ProjectLayout'\nexport default function Layout({ children }: { children: React.ReactNode }) {\n  return <ProjectLayout>{children}</ProjectLayout>\n}\n"),
]

for rel, component, imp in pages:
    path = os.path.join(BASE, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(f"import {component} from '{imp}'\nexport default {component}\n")
    print(f"  created {rel}")

for rel, content in layouts:
    path = os.path.join(BASE, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(content)
    print(f"  created {rel}")

print("Done")
