"use client";

import { useSystemInfo } from "@/hooks/use-system-info";
import { PageHeader } from "@/components/shared/page-header";
import { ConfigTabs } from "@/components/config/config-tabs";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  CardDescription,
} from "@/components/ui/card";
import { Server } from "lucide-react";

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h ${mins}m`;
  if (hours > 0) return `${hours}h ${mins}m`;
  return `${mins}m`;
}

function formatBytes(bytes: number): string {
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(1)} GB`;
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}

export default function ConfigPage() {
  const { data: sysInfo } = useSystemInfo();

  return (
    <div className="space-y-6">
      <PageHeader
        title="Configuration"
        description="Gateway configuration settings"
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
            <div className="grid gap-3 text-sm sm:grid-cols-2 lg:grid-cols-3">
              <div>
                <span className="text-muted-foreground">Version:</span>{" "}
                <span className="font-medium">{sysInfo.version}</span>
              </div>
              <div>
                <span className="text-muted-foreground">Hostname:</span>{" "}
                <span className="font-medium">{sysInfo.hostname}</span>
              </div>
              <div>
                <span className="text-muted-foreground">OS / Arch:</span>{" "}
                <span className="font-medium">
                  {sysInfo.os} / {sysInfo.arch}
                </span>
              </div>
              <div>
                <span className="text-muted-foreground">CPUs:</span>{" "}
                <span className="font-medium">{sysInfo.num_cpus}</span>
              </div>
              <div>
                <span className="text-muted-foreground">Total Memory:</span>{" "}
                <span className="font-medium">
                  {formatBytes(sysInfo.total_memory)}
                </span>
              </div>
              <div>
                <span className="text-muted-foreground">Uptime:</span>{" "}
                <span className="font-medium">
                  {formatUptime(sysInfo.uptime_seconds)}
                </span>
              </div>
            </div>
          </CardContent>
        </Card>
      )}

      <ConfigTabs />
    </div>
  );
}
