'use client'

import { useState, useEffect, useCallback } from "react";
import { fetchMe, signOut, type TokenUser } from "@/lib/auth";

export function useAuth() {
  const [user, setUser] = useState<TokenUser | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetchMe()
      .then(setUser)
      .finally(() => setLoading(false));
  }, []);

  const logout = useCallback(async () => {
    await signOut();
    setUser(null);
  }, []);

  return { user, loading, signOut: logout };
}

