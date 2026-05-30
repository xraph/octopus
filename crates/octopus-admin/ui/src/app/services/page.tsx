"use client";

import { useSystemInfo } from "@/hooks/use-system-info";
import { useServices, useFarpServices } from "@/hooks/use-services";
import { PageHeader } from "@/components/shared/page-header";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  CardDescription,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Globe, Server } from "lucide-react";

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h ${mins}m`;
  if (hours > 0) return `${hours}h ${mins}m`;
  return `${mins}m`;
}

export default function ServicesPage() {
  const { data: sysInfo } = useSystemInfo();
  const { data: services, isLoading: servicesLoading } = useServices();
  const { data: farpServices } = useFarpServices();

  const hasServices = (services && services.length > 0) || (farpServices && farpServices.length > 0);

  return (
    <div className="space-y-6">
      <PageHeader
        title="Services"
        description="Discovered upstream services and FARP-registered APIs"
      />

      {sysInfo && (
        <Card>
          <CardHeader className="flex flex-row items-center gap-3">
            <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-primary/10">
              <Server className="h-5 w-5 text-primary" />
            </div>
            <div>
              <CardTitle>System Info</CardTitle>
              <CardDescription>Gateway host details</CardDescription>
            </div>
          </CardHeader>
          <CardContent>
            <div className="grid gap-2 text-sm sm:grid-cols-2 lg:grid-cols-4">
              <div>
                <span className="text-muted-foreground">Hostname:</span>{" "}
                <span className="font-medium">{sysInfo.hostname}</span>
              </div>
              <div>
                <span className="text-muted-foreground">Version:</span>{" "}
                <span className="font-medium">{sysInfo.version}</span>
              </div>
              <div>
                <span className="text-muted-foreground">Uptime:</span>{" "}
                <span className="font-medium">
                  {formatUptime(sysInfo.uptime_seconds)}
                </span>
              </div>
              <div>
                <span className="text-muted-foreground">OS:</span>{" "}
                <span className="font-medium">
                  {sysInfo.os} / {sysInfo.arch}
                </span>
              </div>
            </div>
          </CardContent>
        </Card>
      )}

      {/* Services table */}
      {servicesLoading && (
        <div className="text-muted-foreground text-sm">Loading services...</div>
      )}

      {!servicesLoading && hasServices && (
        <Card>
          <CardHeader>
            <CardTitle>Registered Services</CardTitle>
            <CardDescription>
              {services?.length ?? 0} services discovered
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Source</TableHead>
                  <TableHead>Address</TableHead>
                  <TableHead>Routes</TableHead>
                  <TableHead>Instances</TableHead>
                  <TableHead>Health</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {services?.map((svc) => (
                  <TableRow key={svc.name}>
                    <TableCell className="font-medium">{svc.name}</TableCell>
                    <TableCell>
                      <Badge variant="outline" className="text-xs">
                        {svc.source ?? "upstream"}
                      </Badge>
                    </TableCell>
                    <TableCell className="font-mono text-sm">
                      {svc.address}:{svc.port}
                    </TableCell>
                    <TableCell>{svc.route_count}</TableCell>
                    <TableCell>
                      {svc.healthy_count ?? 0}/{svc.instance_count ?? 0}
                    </TableCell>
                    <TableCell>
                      <Badge
                        variant={svc.healthy ? "default" : "destructive"}
                        className="text-xs"
                      >
                        {svc.healthy ? "Healthy" : "Unhealthy"}
                      </Badge>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {/* FARP Services */}
      {farpServices && farpServices.length > 0 && (
        <Card>
          <CardHeader>
            <div className="flex items-center gap-2">
              <Globe className="h-5 w-5 text-primary" />
              <div>
                <CardTitle>FARP Services</CardTitle>
                <CardDescription>
                  Services registered via Federated API Registry Protocol
                </CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Service</TableHead>
                  <TableHead>Version</TableHead>
                  <TableHead>Instance ID</TableHead>
                  <TableHead>Schemas</TableHead>
                  <TableHead>Capabilities</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {farpServices.map((svc) => (
                  <TableRow key={svc.name}>
                    <TableCell className="font-medium">{svc.name}</TableCell>
                    <TableCell>{svc.version}</TableCell>
                    <TableCell className="font-mono text-sm">
                      {svc.instance_id ?? "-"}
                    </TableCell>
                    <TableCell>{svc.schemas_count}</TableCell>
                    <TableCell>
                      <div className="flex gap-1 flex-wrap">
                        {svc.capabilities.map((cap) => (
                          <Badge
                            key={cap}
                            variant="outline"
                            className="text-xs"
                          >
                            {cap}
                          </Badge>
                        ))}
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {!servicesLoading && !hasServices && (
        <Card>
          <CardContent className="flex flex-col items-center justify-center py-12">
            <Globe className="h-12 w-12 text-muted-foreground/40 mb-4" />
            <p className="text-muted-foreground text-center">
              No services discovered yet. Services will appear when upstreams are
              configured or FARP services register.
            </p>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
