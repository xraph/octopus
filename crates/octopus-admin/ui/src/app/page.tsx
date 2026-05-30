"use client";

import { PageHeader } from "@/components/shared/page-header";
import { StatsCards } from "@/components/overview/stats-cards";
import { RequestChart } from "@/components/overview/request-chart";
import { ActivityFeed } from "@/components/overview/activity-feed";
import { SystemGauges } from "@/components/overview/system-gauges";

export default function OverviewPage() {
  return (
    <div className="flex flex-col gap-6 p-6">
      <PageHeader
        title="Overview"
        description="Monitor your API gateway at a glance."
      />
      <StatsCards />
      <div className="grid gap-6 lg:grid-cols-3">
        <div className="lg:col-span-2">
          <RequestChart />
        </div>
        <div className="lg:col-span-1">
          <ActivityFeed />
        </div>
      </div>
      <SystemGauges />
    </div>
  );
}
