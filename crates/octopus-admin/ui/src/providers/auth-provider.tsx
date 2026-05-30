"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react";
import { apiGet, apiPost, setUnauthorizedHandler } from "@/lib/api-client";
import type { LoginResponse, MeResponse } from "@/lib/types";

type AuthStatus = "loading" | "authenticated" | "unauthenticated";

interface AuthUser {
  username: string | null;
  role: string | null;
}

interface AuthContextValue {
  status: AuthStatus;
  user: AuthUser | null;
  /** Whether the dashboard requires authentication at all. */
  authRequired: boolean;
  login: (username: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  refresh: () => Promise<void>;
}

const AuthContext = createContext<AuthContextValue>({
  status: "loading",
  user: null,
  authRequired: true,
  login: async () => {},
  logout: async () => {},
  refresh: async () => {},
});

export function useAuth() {
  return useContext(AuthContext);
}

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [status, setStatus] = useState<AuthStatus>("loading");
  const [user, setUser] = useState<AuthUser | null>(null);
  const [authRequired, setAuthRequired] = useState(true);

  const refresh = useCallback(async () => {
    try {
      const me = await apiGet<MeResponse>("/auth/me");
      setAuthRequired(me.auth_required);
      if (me.authenticated) {
        setUser({ username: me.username, role: me.role });
        setStatus("authenticated");
      } else {
        setUser(null);
        setStatus("unauthenticated");
      }
    } catch {
      // 401 / network error → treat as signed out.
      setUser(null);
      setStatus("unauthenticated");
    }
  }, []);

  useEffect(() => {
    // Any 401 from any request flips the app to the signed-out state, which
    // makes the gate redirect to /login.
    setUnauthorizedHandler(() => {
      setUser(null);
      setStatus("unauthenticated");
    });
    void refresh();
    return () => setUnauthorizedHandler(null);
  }, [refresh]);

  const login = useCallback(
    async (username: string, password: string) => {
      await apiPost<LoginResponse>("/auth/login", { username, password });
      await refresh();
    },
    [refresh],
  );

  const logout = useCallback(async () => {
    try {
      await apiPost("/auth/logout");
    } catch {
      // ignore — we clear local state regardless.
    }
    setUser(null);
    setStatus("unauthenticated");
  }, []);

  return (
    <AuthContext.Provider
      value={{ status, user, authRequired, login, logout, refresh }}
    >
      {children}
    </AuthContext.Provider>
  );
}
