"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { DashboardStats } from "@/lib/types";

export function useStats(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["stats"],
    queryFn: () => apiGet<DashboardStats>("/stats"),
    refetchInterval: 5000,
    ...options,
  });
}
