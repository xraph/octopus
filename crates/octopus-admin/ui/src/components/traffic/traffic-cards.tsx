"use client";

import { useStats } from "@/hooks/use-stats";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Activity, AlertTriangle, Radio, Zap } from "lucide-react";

export function TrafficCards() {
  const { data: stats } = useStats();

  const cards = [
    {
      title: "Total Requests",
      value: stats?.total_requests?.toLocaleString() ?? "0",
      icon: Activity,
    },
    {
      title: "Error Rate",
      value: stats ? `${(stats.error_rate * 100).toFixed(2)}%` : "0%",
      icon: AlertTriangle,
    },
    {
      title: "Active Connections",
      value: stats?.active_connections?.toLocaleString() ?? "0",
      icon: Radio,
    },
    {
      title: "Requests/sec",
      value: stats?.requests_per_second?.toFixed(1) ?? "0",
      icon: Zap,
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
            <card.icon className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{card.value}</div>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
