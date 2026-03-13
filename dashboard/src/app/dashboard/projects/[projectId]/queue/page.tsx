import QueuePage from '@/views/queue/QueuePage'

export function generateStaticParams() { return [{ projectId: '_projectId_' }] }
export default function Page() { return <QueuePage /> }
