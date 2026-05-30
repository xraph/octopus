"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { SystemInfo } from "@/lib/types";

export function useSystemInfo(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["system-info"],
    queryFn: () => apiGet<SystemInfo>("/system/info"),
    refetchInterval: 30000,
    ...options,
  });
}
