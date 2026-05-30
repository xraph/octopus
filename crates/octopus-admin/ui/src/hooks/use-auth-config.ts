"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { AuthConfigInfo } from "@/lib/types";

export function useAuthConfig() {
  return useQuery({
    queryKey: ["auth-config"],
    queryFn: () => apiGet<AuthConfigInfo>("/auth/config"),
    refetchInterval: 60_000,
  });
}
