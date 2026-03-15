import { getToken, clearToken } from "@/lib/auth";

// In dev (monolith on one port), the management API is mounted at /flux/api.
// In production, NEXT_PUBLIC_API_URL points directly to the API service host
// where routes are at root level (e.g. https://api.example.com/functions).
const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? "/flux/api";

interface FetchOptions extends RequestInit {
  skipAuth?: boolean;
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

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
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
  if ("success" in json && "data" in json) {
    return json.data as T;
  }
  return json as T;
}

const GATEWAY_BASE = process.env.NEXT_PUBLIC_GATEWAY_URL ?? "";

export async function gatewayFetch<T = unknown>(
  path: string,
  options: FetchOptions = {},
): Promise<T> {
  const token = getToken();

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
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
