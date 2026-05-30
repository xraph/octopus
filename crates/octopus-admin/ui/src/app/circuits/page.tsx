"use client";

import { useMemo } from "react";
import { useHealth } from "@/hooks/use-health";
import { PageHeader } from "@/components/shared/page-header";
import { CircuitsTable } from "@/components/circuits/circuits-table";
import { Badge } from "@/components/ui/badge";

function deriveState(consecutiveFailures: number): "closed" | "half-open" | "open" {
  if (consecutiveFailures === 0) return "closed";
  if (consecutiveFailures <= 2) return "half-open";
  return "open";
}

export default function CircuitsPage() {
  const { data: checks } = useHealth();

  const summary = useMemo(() => {
    if (!checks) return { closed: 0, halfOpen: 0, open: 0 };
    return checks.reduce(
      (acc, check) => {
        const state = deriveState(check.consecutive_failures);
        if (state === "closed") acc.closed++;
        else if (state === "half-open") acc.halfOpen++;
        else acc.open++;
        return acc;
      },
      { closed: 0, halfOpen: 0, open: 0 }
    );
  }, [checks]);

  return (
    <div className="space-y-6">
      <PageHeader
        title="Circuit Breakers"
        description="Circuit breaker states per upstream target"
      />

      <div className="flex items-center gap-3">
        <Badge variant="default">{summary.closed} closed</Badge>
        <Badge variant="secondary">{summary.halfOpen} half-open</Badge>
        <Badge variant="destructive">{summary.open} open</Badge>
      </div>

      <CircuitsTable />
    </div>
  );
}
