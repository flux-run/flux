import FunctionDetailPage from '@/views/functions/FunctionDetailPage'

export function generateStaticParams() { return [{ projectId: "_projectId_", functionId: "_functionId_" }] }
export default function Page() { return <FunctionDetailPage /> }
