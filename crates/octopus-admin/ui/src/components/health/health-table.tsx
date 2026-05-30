"use client";

import { useHealth } from "@/hooks/use-health";
import { StatusBadge } from "@/components/shared/status-badge";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { formatDistanceToNow } from "date-fns";

export function HealthTable() {
  const { data: checks, isLoading } = useHealth();

  if (isLoading) {
    return (
      <div className="space-y-3">
        {Array.from({ length: 5 }).map((_, i) => (
          <Skeleton key={i} className="h-12 w-full" />
        ))}
      </div>
    );
  }

  if (!checks || checks.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
        <p className="text-lg font-medium">No health checks configured</p>
        <p className="text-sm">Health checks will appear here once configured.</p>
      </div>
    );
  }

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Name</TableHead>
          <TableHead>Endpoint</TableHead>
          <TableHead>Status</TableHead>
          <TableHead>Response Time</TableHead>
          <TableHead>Consecutive Failures</TableHead>
          <TableHead>Last Check</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {checks.map((check) => (
          <TableRow key={check.name}>
            <TableCell className="font-medium">{check.name}</TableCell>
            <TableCell className="font-mono text-xs">
              {check.endpoint ?? "-"}
            </TableCell>
            <TableCell>
              <StatusBadge status={check.status} />
            </TableCell>
            <TableCell>{check.response_time_ms} ms</TableCell>
            <TableCell>{check.consecutive_failures}</TableCell>
            <TableCell>
              {formatDistanceToNow(new Date(check.last_check), {
                addSuffix: true,
              })}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
