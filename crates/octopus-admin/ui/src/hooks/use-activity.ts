"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { ActivityLogEntry } from "@/lib/types";

export function useActivity(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["activity"],
    queryFn: () => apiGet<ActivityLogEntry[]>("/activity"),
    refetchInterval: 15000,
    ...options,
  });
}
