import OverviewPage from '@/views/projects/OverviewPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <OverviewPage /> }
