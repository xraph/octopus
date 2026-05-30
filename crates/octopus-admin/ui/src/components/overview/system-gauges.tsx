"use client";

import { usePerformance } from "@/hooks/use-performance";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Progress, ProgressLabel, ProgressValue } from "@/components/ui/progress";
import { Skeleton } from "@/components/ui/skeleton";
import { CpuIcon, MemoryStickIcon } from "lucide-react";

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
}

export function SystemGauges() {
  const { data, isLoading } = usePerformance();

  if (isLoading) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>System Resources</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid gap-6 sm:grid-cols-2">
            {Array.from({ length: 2 }).map((_, i) => (
              <div key={i} className="space-y-2">
                <Skeleton className="h-4 w-20" />
                <Skeleton className="h-2 w-full" />
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    );
  }

  const cpuUsage = data ? Math.min(Math.round(data.cpu_usage * 100), 100) : 0;
  const memoryUsage = data
    ? Math.min(Math.round(data.memory_usage * 100), 100)
    : 0;

  return (
    <Card>
      <CardHeader>
        <CardTitle>System Resources</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid gap-6 sm:grid-cols-2">
          <div className="space-y-2">
            <div className="flex items-center gap-2">
              <CpuIcon className="size-4 text-muted-foreground" />
              <Progress value={cpuUsage}>
                <ProgressLabel>CPU Usage</ProgressLabel>
                <ProgressValue>
                  {(formattedValue: string | null) =>
                    formattedValue ? `${formattedValue}` : `${cpuUsage}%`
                  }
                </ProgressValue>
              </Progress>
            </div>
          </div>
          <div className="space-y-2">
            <div className="flex items-center gap-2">
              <MemoryStickIcon className="size-4 text-muted-foreground" />
              <Progress value={memoryUsage}>
                <ProgressLabel>Memory Usage</ProgressLabel>
                <ProgressValue>
                  {(formattedValue: string | null) =>
                    `${formattedValue ?? `${memoryUsage}%`}${
                      data
                        ? ` (${formatBytes(data.memory_total - data.memory_available)} / ${formatBytes(data.memory_total)})`
                        : ""
                    }`
                  }
                </ProgressValue>
              </Progress>
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
