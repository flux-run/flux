import ProjectSettingsPage from '@/views/projects/ProjectSettingsPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <ProjectSettingsPage /> }
