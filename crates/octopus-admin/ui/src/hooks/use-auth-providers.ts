"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { AuthProviderInfo } from "@/lib/types";

export function useAuthProviders() {
  return useQuery({
    queryKey: ["auth-providers"],
    queryFn: () => apiGet<AuthProviderInfo[]>("/auth/providers"),
    refetchInterval: 60_000,
  });
}
