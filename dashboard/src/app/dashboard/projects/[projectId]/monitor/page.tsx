import MonitorPage from '@/views/monitor/MonitorPage'

export function generateStaticParams() { return [{ projectId: '_projectId_' }] }
export default function Page() { return <MonitorPage /> }
