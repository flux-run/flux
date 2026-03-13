import CronPage from '@/views/cron/CronPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <CronPage /> }
