"use client";

import { useGrpcConfig } from "@/hooks/use-grpc";
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

function FeatureBadge({ on }: { on?: boolean }) {
  return (
    <Badge variant={on ? "default" : "outline"}>{on ? "enabled" : "off"}</Badge>
  );
}

export default function GrpcPage() {
  const { data: config, isLoading } = useGrpcConfig();

  return (
    <div className="space-y-6">
      <PageHeader
        title="gRPC Services"
        description="gRPC proxying configuration and the services routed through the gateway."
      />

      {isLoading && <Skeleton className="h-40 w-full" />}

      {!isLoading && config && (
        <>
          <div className="grid grid-cols-2 gap-4 md:grid-cols-4">
            <Card>
              <CardHeader className="pb-2">
                <CardDescription>gRPC Proxy</CardDescription>
                <CardTitle className="text-lg">
                  <FeatureBadge on={config.enabled} />
                </CardTitle>
              </CardHeader>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardDescription>Reflection</CardDescription>
                <CardTitle className="text-lg">
                  <FeatureBadge on={config.enable_reflection} />
                </CardTitle>
              </CardHeader>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardDescription>gRPC-Web</CardDescription>
                <CardTitle className="text-lg">
                  <FeatureBadge on={config.enable_grpc_web} />
                </CardTitle>
              </CardHeader>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardDescription>Deadline Propagation</CardDescription>
                <CardTitle className="text-lg">
                  <FeatureBadge on={config.deadline_propagation} />
                </CardTitle>
              </CardHeader>
            </Card>
          </div>

          <Card>
            <CardHeader>
              <CardTitle className="text-base">Services</CardTitle>
              <CardDescription>
                {config.services.length} service
                {config.services.length === 1 ? "" : "s"} mapped to upstreams.
              </CardDescription>
            </CardHeader>
            <CardContent className="p-0">
              {config.services.length === 0 ? (
                <div className="py-10 text-center text-muted-foreground">
                  No gRPC services are configured.
                </div>
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Service</TableHead>
                      <TableHead>Upstream</TableHead>
                      <TableHead>Status</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {config.services.map((svc) => (
                      <TableRow key={svc.service}>
                        <TableCell className="font-mono text-sm">
                          {svc.service}
                        </TableCell>
                        <TableCell className="font-mono text-sm">
                          {svc.upstream}
                        </TableCell>
                        <TableCell>
                          <FeatureBadge on={svc.enabled} />
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}
