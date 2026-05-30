"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost } from "@/lib/api-client";
import type { TlsCertInfo, TlsCertUpload } from "@/lib/types";

export function useTlsCerts() {
  return useQuery({
    queryKey: ["tls-certs"],
    queryFn: () => apiGet<TlsCertInfo[]>("/tls/certs"),
    refetchInterval: 60_000,
  });
}

export function useUploadTlsCert() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (cert: TlsCertUpload) =>
      apiPost<{ success: boolean; message?: string }>("/tls/certs", cert),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["tls-certs"] });
    },
  });
}

export function useReloadTls() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () =>
      apiPost<{ success: boolean; message?: string }>("/tls/reload"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["tls-certs"] });
    },
  });
}
