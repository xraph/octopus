"use client";

import { PageHeader } from "@/components/shared/page-header";
import { TrafficCards } from "@/components/traffic/traffic-cards";
import { TrafficChart } from "@/components/traffic/traffic-chart";
import { RouteStatsTable } from "@/components/traffic/route-stats-table";

export default function TrafficPage() {
  return (
    <div className="space-y-6">
      <PageHeader
        title="Traffic"
        description="Real-time traffic metrics and per-route statistics"
      />
      <TrafficCards />
      <TrafficChart />
      <RouteStatsTable />
    </div>
  );
}
