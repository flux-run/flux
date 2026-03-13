import EventsPage from '@/views/events/EventsPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <EventsPage /> }
