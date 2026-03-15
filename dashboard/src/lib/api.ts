import { getToken, clearToken } from "@/lib/auth";
import { useStore } from "@/state/tenantStore";

const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? "";

interface FetchOptions extends RequestInit {
  skipTenant?: boolean;
  skipProject?: boolean;
  projectId?: string;
}

function handleUnauthorized() {
  clearToken();
  if (typeof window !== "undefined") {
    window.location.replace("/flux/login");
  }
  throw new Error("session_expired");
}

export async function apiFetch<T = unknown>(
  path: string,
  options: FetchOptions = {},
): Promise<T> {
  const token = getToken();
  const state = useStore.getState();
  const finalProjectId = options.projectId || state.projectId;

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
    ...(!options.skipTenant && state.tenantId
      ? { "X-Flux-Tenant": state.tenantId }
      : {}),
    ...(!options.skipProject && finalProjectId
      ? { "X-Flux-Project": finalProjectId }
      : {}),
    ...(options.headers as Record<string, string>),
  };

  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers,
  });

  if (res.status === 401) {
    handleUnauthorized();
  }

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: "unknown_error" }));
    throw new Error((err as { error?: string }).error ?? `HTTP ${res.status}`);
  }

  const text = await res.text();
  if (!text) return {} as T;

  const json = JSON.parse(text);
  // Support standard ApiResponse<T> where `{ success: true, data: T }`
  if ("success" in json && "data" in json) {
    return json.data as T;
  }
  return json as T;
}

// ─── Gateway client (execution plane) ────────────────────────────────────────
// Routes execution traffic through the gateway (same origin, different path).

const GATEWAY_BASE = process.env.NEXT_PUBLIC_GATEWAY_URL ?? "";

export async function gatewayFetch<T = unknown>(
  path: string,
  options: FetchOptions = {},
): Promise<T> {
  const token = getToken();
  const state = useStore.getState();
  const finalProjectId = options.projectId || state.projectId;

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
    ...(!options.skipTenant && state.tenantId
      ? { "X-Flux-Tenant": state.tenantId }
      : {}),
    ...(!options.skipProject && finalProjectId
      ? { "X-Flux-Project": finalProjectId }
      : {}),
    ...(options.headers as Record<string, string>),
  };

  const res = await fetch(`${GATEWAY_BASE}${path}`, { ...options, headers });

  if (res.status === 401) {
    handleUnauthorized();
  }

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: "unknown_error" }));
    throw new Error((err as { error?: string }).error ?? `HTTP ${res.status}`);
  }

  const text = await res.text();
  if (!text) return {} as T;
  return JSON.parse(text) as T;
}
