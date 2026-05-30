"use client";

import { useState } from "react";
import { useAnalytics } from "@/hooks/use-analytics";
import { usePerformance } from "@/hooks/use-performance";
import { PageHeader } from "@/components/shared/page-header";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";

const TIMEFRAMES = [
  { value: "1h", label: "Last hour" },
  { value: "24h", label: "Last 24 hours" },
  { value: "7d", label: "Last 7 days" },
  { value: "30d", label: "Last 30 days" },
];

const latencyConfig = {
  value: { label: "Latency (ms)", color: "var(--chart-1)" },
} satisfies ChartConfig;

const methodConfig = {
  value: { label: "Requests", color: "var(--chart-2)" },
} satisfies ChartConfig;

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardDescription>{label}</CardDescription>
        <CardTitle className="text-2xl tabular-nums">{value}</CardTitle>
      </CardHeader>
    </Card>
  );
}

export default function AnalyticsPage() {
  const [timeframe, setTimeframe] = useState("24h");
  const { data: analytics, isLoading } = useAnalytics(timeframe);
  const { data: perf } = usePerformance();

  const p = analytics?.latency_percentiles;
  const latencyData = p
    ? [
        { name: "p50", value: p.p50 },
        { name: "p90", value: p.p90 },
        { name: "p95", value: p.p95 },
        { name: "p99", value: p.p99 },
      ]
    : [];

  const methodData = Object.entries(analytics?.traffic_by_method ?? {}).map(
    ([method, count]) => ({ method, value: count }),
  );

  const statusEntries = Object.entries(
    analytics?.status_code_distribution ?? {},
  );

  return (
    <div className="space-y-6">
      <PageHeader
        title="Analytics"
        description="Latency percentiles, traffic distribution, and top routes."
        action={
          <Select
            value={timeframe}
            onValueChange={(v) => {
              if (v) setTimeframe(v);
            }}
          >
            <SelectTrigger className="w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {TIMEFRAMES.map((t) => (
                <SelectItem key={t.value} value={t.value}>
                  {t.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        }
      />

      <div className="grid grid-cols-2 gap-4 md:grid-cols-4">
        <StatCard label="p50 Latency" value={`${(p?.p50 ?? 0).toFixed(1)} ms`} />
        <StatCard label="p95 Latency" value={`${(p?.p95 ?? 0).toFixed(1)} ms`} />
        <StatCard label="p99 Latency" value={`${(p?.p99 ?? 0).toFixed(1)} ms`} />
        <StatCard
          label="CPU / Memory"
          value={`${(perf?.cpu_usage ?? 0).toFixed(0)}% / ${(perf?.memory_usage ?? 0).toFixed(0)}%`}
        />
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">Latency Percentiles</CardTitle>
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <Skeleton className="h-[260px] w-full" />
            ) : latencyData.every((d) => d.value === 0) ? (
              <div className="flex h-[260px] items-center justify-center text-muted-foreground">
                No latency data yet.
              </div>
            ) : (
              <ChartContainer config={latencyConfig} className="h-[260px] w-full">
                <BarChart data={latencyData} accessibilityLayer>
                  <CartesianGrid vertical={false} />
                  <XAxis dataKey="name" tickLine={false} axisLine={false} />
                  <YAxis tickLine={false} axisLine={false} width={40} />
                  <ChartTooltip content={<ChartTooltipContent />} />
                  <Bar dataKey="value" fill="var(--color-value)" radius={4} />
                </BarChart>
              </ChartContainer>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Traffic by Method</CardTitle>
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <Skeleton className="h-[260px] w-full" />
            ) : methodData.length === 0 ? (
              <div className="flex h-[260px] items-center justify-center text-muted-foreground">
                No traffic recorded yet.
              </div>
            ) : (
              <ChartContainer config={methodConfig} className="h-[260px] w-full">
                <BarChart data={methodData} accessibilityLayer>
                  <CartesianGrid vertical={false} />
                  <XAxis dataKey="method" tickLine={false} axisLine={false} />
                  <YAxis tickLine={false} axisLine={false} width={40} />
                  <ChartTooltip content={<ChartTooltipContent />} />
                  <Bar dataKey="value" fill="var(--color-value)" radius={4} />
                </BarChart>
              </ChartContainer>
            )}
          </CardContent>
        </Card>
      </div>

      <div className="grid gap-4 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle className="text-base">Top Routes</CardTitle>
            <CardDescription>Busiest routes by request volume.</CardDescription>
          </CardHeader>
          <CardContent className="p-0">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Path</TableHead>
                  <TableHead className="text-right">Requests</TableHead>
                  <TableHead className="text-right">Avg Latency</TableHead>
                  <TableHead className="text-right">Error Rate</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {(analytics?.top_routes ?? []).length === 0 ? (
                  <TableRow>
                    <TableCell
                      colSpan={4}
                      className="py-8 text-center text-muted-foreground"
                    >
                      No route metrics yet.
                    </TableCell>
                  </TableRow>
                ) : (
                  analytics?.top_routes.map((r) => (
                    <TableRow key={r.path}>
                      <TableCell className="font-mono text-xs">{r.path}</TableCell>
                      <TableCell className="text-right tabular-nums">
                        {r.requests.toLocaleString()}
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {r.avg_latency.toFixed(1)} ms
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {(r.error_rate * 100).toFixed(1)}%
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Status Codes</CardTitle>
            <CardDescription>Response status distribution.</CardDescription>
          </CardHeader>
          <CardContent>
            {statusEntries.length === 0 ? (
              <p className="py-8 text-center text-sm text-muted-foreground">
                No status data yet.
              </p>
            ) : (
              <div className="space-y-2">
                {statusEntries.map(([code, count]) => (
                  <div
                    key={code}
                    className="flex items-center justify-between text-sm"
                  >
                    <span className="font-mono">{code}</span>
                    <span className="tabular-nums text-muted-foreground">
                      {count.toLocaleString()}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
