import { getAuth, signOut } from "firebase/auth";
import { useStore } from "@/state/tenantStore";

const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:8080";

interface FetchOptions extends RequestInit {
  skipTenant?: boolean;
  skipProject?: boolean;
  projectId?: string;
}

export async function apiFetch<T = unknown>(
  path: string,
  options: FetchOptions = {},
): Promise<T> {
  const auth = getAuth();
  await auth.authStateReady();
  const user = auth.currentUser;
  const token = user ? await user.getIdToken() : null;

  const state = useStore.getState();

  const finalProjectId = options.projectId || state.projectId;

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
    ...(!options.skipTenant && state.tenantId
      ? { "X-Fluxbase-Tenant": state.tenantId }
      : {}),
    ...(!options.skipProject && finalProjectId
      ? { "X-Fluxbase-Project": finalProjectId }
      : {}),
    ...(options.headers as Record<string, string>),
  };

  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers,
  });

  if (res.status === 401) {
    try {
      await signOut(getAuth());
    } catch (_) {}
    window.location.replace("/dashboard/login");
    throw new Error("session_expired");
  }

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: "unknown_error" }));
    // API standardised error format: { success: false, error: "..." }
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
// Routes execution traffic (POST /db/query, cron triggers, file ops) through
// the gateway, which proxies internally to the Data Engine.
// CONFIGURATION calls (CRUD) go via apiFetch → API service instead.

const GATEWAY_BASE = process.env.NEXT_PUBLIC_GATEWAY_URL ?? "http://localhost:8081";

export async function gatewayFetch<T = unknown>(
  path: string,
  options: FetchOptions = {},
): Promise<T> {
  const auth = getAuth();
  await auth.authStateReady();
  const user = auth.currentUser;
  const token = user ? await user.getIdToken() : null;

  const state = useStore.getState();
  const finalProjectId = options.projectId || state.projectId;

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
    ...(!options.skipTenant && state.tenantId
      ? { "X-Fluxbase-Tenant": state.tenantId }
      : {}),
    ...(!options.skipProject && finalProjectId
      ? { "X-Fluxbase-Project": finalProjectId }
      : {}),
    ...(options.headers as Record<string, string>),
  };

  const res = await fetch(`${GATEWAY_BASE}${path}`, { ...options, headers });

  if (res.status === 401) {
    try {
      await signOut(getAuth());
    } catch (_) {}
    window.location.replace("/dashboard/login");
    throw new Error("session_expired");
  }

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: "unknown_error" }));
    throw new Error((err as { error?: string }).error ?? `HTTP ${res.status}`);
  }

  const text = await res.text();
  if (!text) return {} as T;
  return JSON.parse(text) as T;
}
