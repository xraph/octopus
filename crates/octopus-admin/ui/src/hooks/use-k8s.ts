"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { K8sResourceSummary, K8sStatus } from "@/lib/types";

export function useK8sStatus() {
  return useQuery({
    queryKey: ["k8s-status"],
    queryFn: () => apiGet<K8sStatus>("/k8s/status"),
    refetchInterval: 30_000,
  });
}

export function useK8sGateways(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["k8s-gateways"],
    queryFn: () => apiGet<K8sResourceSummary[]>("/k8s/gateways"),
    ...options,
  });
}

export function useK8sRoutes(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["k8s-routes"],
    queryFn: () => apiGet<K8sResourceSummary[]>("/k8s/routes"),
    ...options,
  });
}

export function useK8sPolicies(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["k8s-policies"],
    queryFn: () => apiGet<K8sResourceSummary[]>("/k8s/policies"),
    ...options,
  });
}

export function useK8sUpstreams(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["k8s-upstreams"],
    queryFn: () => apiGet<K8sResourceSummary[]>("/k8s/upstreams"),
    ...options,
  });
}
