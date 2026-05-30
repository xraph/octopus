"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { PerformanceMetrics } from "@/lib/types";

export function useRealtimeMetrics(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["realtime-metrics"],
    queryFn: () => apiGet<PerformanceMetrics>("/metrics/realtime"),
    refetchInterval: 3000,
    ...options,
  });
}
