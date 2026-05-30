"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { SecurityEvent } from "@/lib/types";

export function useSecurityEvents(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["security-events"],
    queryFn: () => apiGet<SecurityEvent[]>("/security/events"),
    refetchInterval: 30000,
    ...options,
  });
}
