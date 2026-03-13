import SecretsPage from '@/views/secrets/SecretsPage'

export function generateStaticParams() { return [{ projectId: "_projectId_" }] }
export default function Page() { return <SecretsPage /> }
