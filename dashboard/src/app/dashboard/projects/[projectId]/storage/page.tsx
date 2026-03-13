import StoragePage from '@/views/storage/StoragePage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <StoragePage /> }
