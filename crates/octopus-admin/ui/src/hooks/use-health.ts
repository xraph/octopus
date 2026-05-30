"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { HealthCheckInfo } from "@/lib/types";

export function useHealth(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["health"],
    queryFn: () => apiGet<HealthCheckInfo[]>("/health"),
    refetchInterval: 10000,
    ...options,
  });
}
