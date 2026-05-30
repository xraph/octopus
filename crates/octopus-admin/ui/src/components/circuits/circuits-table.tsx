"use client";

import { useMemo } from "react";
import { useHealth } from "@/hooks/use-health";
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
import { formatDistanceToNow } from "date-fns";

interface CircuitEntry {
  target_url: string;
  route: string;
  state: "closed" | "half-open" | "open";
  failures: number;
  last_check: string;
}

function deriveCircuitState(
  consecutiveFailures: number
): "closed" | "half-open" | "open" {
  if (consecutiveFailures === 0) return "closed";
  if (consecutiveFailures <= 2) return "half-open";
  return "open";
}

const stateVariant: Record<string, "default" | "secondary" | "destructive"> = {
  closed: "default",
  "half-open": "secondary",
  open: "destructive",
};

export function CircuitsTable() {
  const { data: checks, isLoading } = useHealth();

  const circuits: CircuitEntry[] = useMemo(() => {
    if (!checks) return [];
    return checks.map((check) => ({
      target_url: check.endpoint ?? check.name,
      route: check.name,
      state: deriveCircuitState(check.consecutive_failures),
      failures: check.consecutive_failures,
      last_check: check.last_check,
    }));
  }, [checks]);

  if (isLoading) {
    return (
      <div className="space-y-3">
        {Array.from({ length: 5 }).map((_, i) => (
          <Skeleton key={i} className="h-12 w-full" />
        ))}
      </div>
    );
  }

  if (circuits.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
        <p className="text-lg font-medium">No circuit breakers active</p>
        <p className="text-sm">
          Circuit breaker states will appear here once health checks are
          configured.
        </p>
      </div>
    );
  }

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Target URL</TableHead>
          <TableHead>Route</TableHead>
          <TableHead>State</TableHead>
          <TableHead>Failures</TableHead>
          <TableHead>Last Check</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {circuits.map((circuit, index) => (
          <TableRow key={`${circuit.target_url}-${index}`}>
            <TableCell className="font-mono text-xs">
              {circuit.target_url}
            </TableCell>
            <TableCell className="font-mono text-xs">{circuit.route}</TableCell>
            <TableCell>
              <Badge variant={stateVariant[circuit.state]}>
                {circuit.state}
              </Badge>
            </TableCell>
            <TableCell>{circuit.failures}</TableCell>
            <TableCell>
              {formatDistanceToNow(new Date(circuit.last_check), {
                addSuffix: true,
              })}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
