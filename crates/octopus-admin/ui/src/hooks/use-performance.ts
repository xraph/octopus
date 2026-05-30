"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { PerformanceMetrics } from "@/lib/types";

export function usePerformance(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["performance"],
    queryFn: () => apiGet<PerformanceMetrics>("/metrics/performance"),
    refetchInterval: 5000,
    ...options,
  });
}
