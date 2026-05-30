"use client";

import { useHealth } from "@/hooks/use-health";
import { PageHeader } from "@/components/shared/page-header";
import { HealthTable } from "@/components/health/health-table";
import { Badge } from "@/components/ui/badge";
import { useMemo } from "react";

export default function HealthPage() {
  const { data: checks } = useHealth();

  const summary = useMemo(() => {
    if (!checks) return { passing: 0, warning: 0, critical: 0 };
    return checks.reduce(
      (acc, check) => {
        if (check.status === "passing" || check.status === "healthy") {
          acc.passing++;
        } else if (check.status === "warning") {
          acc.warning++;
        } else {
          acc.critical++;
        }
        return acc;
      },
      { passing: 0, warning: 0, critical: 0 }
    );
  }, [checks]);

  return (
    <div className="space-y-6">
      <PageHeader
        title="Health Checks"
        description="Monitor upstream service health"
      />

      <div className="flex items-center gap-3">
        <Badge variant="default">{summary.passing} passing</Badge>
        <Badge variant="secondary">{summary.warning} warning</Badge>
        <Badge variant="destructive">{summary.critical} critical</Badge>
      </div>

      <HealthTable />
    </div>
  );
}
