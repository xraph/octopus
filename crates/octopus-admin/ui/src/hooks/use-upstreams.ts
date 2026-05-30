"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost, apiPut, apiDelete } from "@/lib/api-client";
import type { UpstreamClusterInfo, UpstreamConfig } from "@/lib/types";

export function useUpstreams() {
  return useQuery<UpstreamClusterInfo[]>({
    queryKey: ["upstreams"],
    queryFn: () => apiGet<UpstreamClusterInfo[]>("/upstreams"),
    refetchInterval: 10_000,
  });
}

export function useCreateUpstream() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (config: UpstreamConfig) =>
      apiPost<UpstreamClusterInfo>("/upstreams", config),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["upstreams"] });
    },
  });
}

export function useUpdateUpstream() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ name, config }: { name: string; config: UpstreamConfig }) =>
      apiPut<UpstreamClusterInfo>(
        `/upstreams/${encodeURIComponent(name)}`,
        config,
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["upstreams"] });
    },
  });
}

export function useDeleteUpstream() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) =>
      apiDelete(`/upstreams/${encodeURIComponent(name)}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["upstreams"] });
    },
  });
}
