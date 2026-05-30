"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { ServiceInfo, FarpServiceInfo } from "@/lib/types";

export function useServices() {
  return useQuery<ServiceInfo[]>({
    queryKey: ["services"],
    queryFn: () => apiGet<ServiceInfo[]>("/services"),
    refetchInterval: 15_000,
  });
}

export function useFarpServices() {
  return useQuery<FarpServiceInfo[]>({
    queryKey: ["farp-services"],
    queryFn: () => apiGet<FarpServiceInfo[]>("/farp/services"),
    refetchInterval: 30_000,
  });
}
