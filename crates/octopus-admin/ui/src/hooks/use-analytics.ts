"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { AnalyticsMetrics } from "@/lib/types";

export function useAnalytics(
  timeframe?: string,
  options?: { enabled?: boolean },
) {
  const path = timeframe
    ? `/analytics?timeframe=${encodeURIComponent(timeframe)}`
    : "/analytics";
  return useQuery({
    queryKey: ["analytics", timeframe ?? "default"],
    queryFn: () => apiGet<AnalyticsMetrics>(path),
    refetchInterval: 30000,
    ...options,
  });
}
