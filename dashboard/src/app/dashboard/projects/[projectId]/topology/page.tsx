import TopologyPage from '@/views/topology/TopologyPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <TopologyPage /> }
