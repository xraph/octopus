"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPut } from "@/lib/api-client";
import type { ConfigItem } from "@/lib/types";

export function useConfig(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["config"],
    queryFn: () => apiGet<ConfigItem[]>("/config"),
    refetchInterval: 60000,
    ...options,
  });
}

export function useUpdateConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ key, value }: { key: string; value: unknown }) =>
      apiPut<ConfigItem>(`/config/${key}`, { value }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["config"] });
    },
  });
}
