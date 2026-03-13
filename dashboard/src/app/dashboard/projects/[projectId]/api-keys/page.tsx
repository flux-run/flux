import ApiKeysPage from '@/views/api-keys/ApiKeysPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <ApiKeysPage /> }
