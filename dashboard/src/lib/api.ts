import { getAuth } from "firebase/auth";
import { useStore } from "@/state/tenantStore";

const API_BASE = import.meta.env.VITE_API_URL ?? "http://localhost:8080";

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
