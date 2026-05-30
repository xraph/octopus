"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost, apiPut, apiDelete } from "@/lib/api-client";
import type { RouteInfo, RouteConfig } from "@/lib/types";

export function useRoutes(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["routes"],
    queryFn: () => apiGet<RouteInfo[]>("/routes"),
    refetchInterval: 30000,
    ...options,
  });
}

export function useCreateRoute() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (route: RouteConfig) =>
      apiPost<RouteInfo>("/routes", route),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["routes"] });
    },
  });
}

export function useUpdateRoute() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, route }: { id: string; route: RouteConfig }) =>
      apiPut<RouteInfo>(`/routes/${id}`, route),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["routes"] });
    },
  });
}

export function useDeleteRoute() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => apiDelete(`/routes/${id}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["routes"] });
    },
  });
}
