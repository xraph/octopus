"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { ActivityLogEntry, LogQuery } from "@/lib/types";

export function useLogs(query?: LogQuery, options?: { enabled?: boolean }) {
  const params = new URLSearchParams();
  if (query?.level) params.set("level", query.level);
  if (query?.limit) params.set("limit", String(query.limit));
  if (query?.offset) params.set("offset", String(query.offset));
  if (query?.search) params.set("search", query.search);

  const qs = params.toString();
  const path = qs ? `/logs?${qs}` : "/logs";

  return useQuery({
    queryKey: ["logs", query],
    queryFn: () => apiGet<ActivityLogEntry[]>(path),
    ...options,
  });
}
