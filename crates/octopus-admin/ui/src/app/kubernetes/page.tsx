"use client";

import {
  useK8sStatus,
  useK8sGateways,
  useK8sRoutes,
  useK8sPolicies,
  useK8sUpstreams,
} from "@/hooks/use-k8s";
import type { K8sResourceSummary } from "@/lib/types";
import { PageHeader } from "@/components/shared/page-header";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { AlertTriangleIcon, CheckCircle2Icon } from "lucide-react";
import type { UseQueryResult } from "@tanstack/react-query";

function specSummary(spec: unknown): string {
  if (!spec || typeof spec !== "object") return "—";
  const s = spec as Record<string, unknown>;
  const keys = ["path", "listen", "upstream", "targetRef", "parentRefs"];
  const parts: string[] = [];
  for (const k of keys) {
    if (k in s) {
      const v = s[k];
      parts.push(`${k}: ${typeof v === "object" ? JSON.stringify(v) : String(v)}`);
    }
  }
  return parts.length ? parts.join("  ·  ") : JSON.stringify(s).slice(0, 80);
}

function ResourceTable({
  query,
  enabled,
}: {
  query: UseQueryResult<K8sResourceSummary[]>;
  enabled: boolean;
}) {
  if (!enabled) {
    return (
      <div className="py-10 text-center text-muted-foreground">
        Kubernetes integration is not available.
      </div>
    );
  }
  if (query.isLoading) return <Skeleton className="h-32 w-full" />;
  const items = query.data ?? [];
  if (items.length === 0) {
    return (
      <div className="py-10 text-center text-muted-foreground">
        No resources found.
      </div>
    );
  }
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Name</TableHead>
          <TableHead>Namespace</TableHead>
          <TableHead>Spec</TableHead>
          <TableHead>Created</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {items.map((item) => (
          <TableRow key={`${item.namespace ?? ""}/${item.name}`}>
            <TableCell className="font-medium">{item.name}</TableCell>
            <TableCell>
              <Badge variant="outline">{item.namespace ?? "—"}</Badge>
            </TableCell>
            <TableCell className="max-w-md truncate font-mono text-xs text-muted-foreground">
              {specSummary(item.spec)}
            </TableCell>
            <TableCell className="text-xs text-muted-foreground">
              {item.created_at
                ? new Date(item.created_at).toLocaleString()
                : "—"}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

export default function KubernetesPage() {
  const status = useK8sStatus();
  const enabled = !!status.data?.feature_enabled && !!status.data?.connected;

  const gateways = useK8sGateways({ enabled });
  const routes = useK8sRoutes({ enabled });
  const policies = useK8sPolicies({ enabled });
  const upstreams = useK8sUpstreams({ enabled });

  const featureEnabled = status.data?.feature_enabled ?? false;

  return (
    <div className="space-y-6">
      <PageHeader
        title="Kubernetes"
        description="Octopus custom resources reconciled by the in-cluster operator."
      />

      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            {status.data?.connected ? (
              <CheckCircle2Icon className="size-5 text-green-600" />
            ) : (
              <AlertTriangleIcon className="size-5 text-amber-500" />
            )}
            <CardTitle className="text-base">
              {status.data?.connected
                ? "Connected to cluster"
                : featureEnabled
                  ? "Not connected"
                  : "Kubernetes feature disabled"}
            </CardTitle>
          </div>
          {status.data?.detail && (
            <CardDescription>{status.data.detail}</CardDescription>
          )}
        </CardHeader>
        {status.data?.connected && (
          <CardContent className="flex flex-wrap gap-4">
            {Object.entries(status.data.counts).map(([kind, count]) => (
              <Badge key={kind} variant="secondary">
                {kind}: {count}
              </Badge>
            ))}
          </CardContent>
        )}
      </Card>

      <Tabs defaultValue="gateways">
        <TabsList>
          <TabsTrigger value="gateways">Gateways</TabsTrigger>
          <TabsTrigger value="routes">Routes</TabsTrigger>
          <TabsTrigger value="policies">Policies</TabsTrigger>
          <TabsTrigger value="upstreams">Upstreams</TabsTrigger>
        </TabsList>
        <TabsContent value="gateways" className="mt-4">
          <Card>
            <CardContent className="p-0">
              <ResourceTable query={gateways} enabled={enabled} />
            </CardContent>
          </Card>
        </TabsContent>
        <TabsContent value="routes" className="mt-4">
          <Card>
            <CardContent className="p-0">
              <ResourceTable query={routes} enabled={enabled} />
            </CardContent>
          </Card>
        </TabsContent>
        <TabsContent value="policies" className="mt-4">
          <Card>
            <CardContent className="p-0">
              <ResourceTable query={policies} enabled={enabled} />
            </CardContent>
          </Card>
        </TabsContent>
        <TabsContent value="upstreams" className="mt-4">
          <Card>
            <CardContent className="p-0">
              <ResourceTable query={upstreams} enabled={enabled} />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
