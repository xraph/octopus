"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import type { GrpcConfigInfo } from "@/lib/types";

export function useGrpcConfig() {
  return useQuery({
    queryKey: ["grpc"],
    queryFn: () => apiGet<GrpcConfigInfo>("/grpc/services"),
    refetchInterval: 30_000,
  });
}
