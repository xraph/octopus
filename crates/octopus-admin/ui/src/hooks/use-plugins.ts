"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost, apiPut } from "@/lib/api-client";
import type { PluginInfo } from "@/lib/types";

export function usePlugins(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["plugins"],
    queryFn: () => apiGet<PluginInfo[]>("/plugins"),
    refetchInterval: 30000,
    ...options,
  });
}

export function usePlugin(id: string) {
  return useQuery({
    queryKey: ["plugins", id],
    queryFn: () => apiGet<PluginInfo>(`/plugins/${id}`),
    enabled: !!id,
  });
}

export function useTogglePlugin() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      apiPost<PluginInfo>(`/plugins/${id}/toggle`, { enabled }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["plugins"] });
    },
  });
}

export function useUpdatePluginConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, config }: { id: string; config: unknown }) =>
      apiPut<{ success: boolean; message?: string }>(
        `/plugins/${id}/config`,
        config,
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["plugins"] });
    },
  });
}
