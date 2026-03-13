/**
 * Internal credential-based auth.
 *
 * No Firebase / third-party auth.  The Flux API issues a plain HS256 JWT
 * that we store in localStorage under the key `flux_token`.
 */

const TOKEN_KEY = "flux_token";

export function getToken(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken() {
  localStorage.removeItem(TOKEN_KEY);
}

export interface TokenUser {
  id: string;
  username: string;
  email: string;
  role: "admin" | "viewer" | "readonly";
  tenant_id: string | null;
}

export interface LoginResult {
  token: string;
  user: TokenUser;
}

const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:4000";

export async function signIn(email: string, password: string): Promise<LoginResult> {
  const res = await fetch(`${API_BASE}/flux/api/auth/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, password }),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({ message: "Login failed" }));
    throw new Error(err.message ?? "Invalid credentials");
  }
  const data: LoginResult = await res.json();
  setToken(data.token);
  return data;
}

export async function signOut() {
  clearToken();
}

export async function fetchMe(): Promise<TokenUser | null> {
  const token = getToken();
  if (!token) return null;
  const res = await fetch(`${API_BASE}/flux/api/auth/me`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!res.ok) {
    clearToken();
    return null;
  }
  const data = await res.json();
  return data.user as TokenUser;
}

