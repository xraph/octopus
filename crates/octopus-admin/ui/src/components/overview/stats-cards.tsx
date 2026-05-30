"use client";

import { useStats } from "@/hooks/use-stats";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import {
  ActivityIcon,
  RouteIcon,
  TimerIcon,
  HeartPulseIcon,
  TrendingUpIcon,
  TrendingDownIcon,
} from "lucide-react";

function formatNumber(n: number): string {
  return n.toLocaleString();
}

export function StatsCards() {
  const { data, isLoading } = useStats();

  if (isLoading) {
    return (
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {Array.from({ length: 4 }).map((_, i) => (
          <Card key={i}>
            <CardHeader className="flex flex-row items-center justify-between pb-2">
              <Skeleton className="h-4 w-24" />
              <Skeleton className="h-4 w-4" />
            </CardHeader>
            <CardContent>
              <Skeleton className="h-7 w-20 mb-1" />
              <Skeleton className="h-3 w-28" />
            </CardContent>
          </Card>
        ))}
      </div>
    );
  }

  const stats = data;

  const cards = [
    {
      title: "Total Requests",
      value: stats ? formatNumber(stats.total_requests) : "0",
      description: stats
        ? `${stats.requests_per_second.toFixed(1)} req/s`
        : "No data",
      icon: ActivityIcon,
      trend: stats ? (stats.requests_per_second > 0 ? "up" : "neutral") : "neutral",
    },
    {
      title: "Active Routes",
      value: stats ? String(stats.active_routes) : "0",
      description: "Configured routes",
      icon: RouteIcon,
      trend: "neutral" as const,
    },
    {
      title: "Avg Latency",
      value: stats ? `${stats.avg_latency_ms.toFixed(1)}ms` : "0ms",
      description: `${stats ? (stats.error_rate * 100).toFixed(2) : "0"}% error rate`,
      icon: TimerIcon,
      trend: stats
        ? stats.avg_latency_ms < 100
          ? "up"
          : "down"
        : "neutral",
    },
    {
      title: "Health Status",
      value: stats?.health_status ?? "unknown",
      description: `${stats?.active_connections ?? 0} active connections`,
      icon: HeartPulseIcon,
      trend: stats?.health_status === "healthy" ? "up" : "down",
    },
  ];

  return (
    <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
      {cards.map((card) => (
        <Card key={card.title}>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">
              {card.title}
            </CardTitle>
            <card.icon className="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="flex items-center gap-2">
              {card.title === "Health Status" ? (
                <Badge
                  variant={
                    card.value === "healthy"
                      ? "default"
                      : card.value === "degraded"
                        ? "secondary"
                        : "destructive"
                  }
                >
                  {card.value}
                </Badge>
              ) : (
                <div className="text-2xl font-bold">{card.value}</div>
              )}
              {card.trend === "up" && (
                <TrendingUpIcon className="size-4 text-green-500" />
              )}
              {card.trend === "down" && (
                <TrendingDownIcon className="size-4 text-red-500" />
              )}
            </div>
            <p className="text-xs text-muted-foreground mt-1">
              {card.description}
            </p>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
