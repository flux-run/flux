'use client'

import { useState, useEffect } from "react";
import { type User, onAuthStateChanged } from "firebase/auth";
import { auth, signInWithGoogle, signInWithGitHub, signOut } from "@/lib/auth";

export function useAuth() {
  const [user, setUser] = useState<User | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const unsubscribe = onAuthStateChanged(auth, (u) => {
      setUser(u);
      setLoading(false);
    });
    return unsubscribe;
  }, []);

  return { user, loading, signInWithGoogle, signInWithGitHub, signOut };
}
