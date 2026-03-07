import { getAuth } from "firebase/auth";
import { useStore } from "@/state/tenantStore";

const API_BASE = import.meta.env.VITE_API_URL ?? "http://localhost:8080";

interface FetchOptions extends RequestInit {
  skipTenant?: boolean;
  skipProject?: boolean;
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

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
    ...(!options.skipTenant && state.tenantId
      ? { "X-Fluxbase-Tenant": state.tenantId }
      : {}),
    ...(!options.skipProject && state.projectId
      ? { "X-Fluxbase-Project": state.projectId }
      : {}),
    ...(options.headers as Record<string, string>),
  };

  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers,
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: "unknown_error" }));
    throw new Error((err as { error?: string }).error ?? `HTTP ${res.status}`);
  }

  const text = await res.text();
  return text ? (JSON.parse(text) as T) : ({} as T);
}
