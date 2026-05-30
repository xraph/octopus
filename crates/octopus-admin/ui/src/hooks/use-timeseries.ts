"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { TimeSeriesPoint } from "@/lib/types";

export function useTimeseries(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["timeseries"],
    queryFn: () => apiGet<TimeSeriesPoint[]>("/metrics/timeseries"),
    refetchInterval: 10000,
    ...options,
  });
}
