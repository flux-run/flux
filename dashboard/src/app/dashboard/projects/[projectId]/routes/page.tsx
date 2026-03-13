import RoutesPage from '@/views/routes/RoutesPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <RoutesPage /> }
