"use client";

import { useMemo } from "react";
import { useAnalytics } from "@/hooks/use-analytics";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  CardDescription,
} from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

export function RouteStatsTable() {
  const { data: analytics } = useAnalytics();

  const sortedRoutes = useMemo(() => {
    if (!analytics?.top_routes) return [];
    return [...analytics.top_routes].sort((a, b) => b.requests - a.requests);
  }, [analytics?.top_routes]);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Top Routes</CardTitle>
        <CardDescription>Per-route traffic statistics</CardDescription>
      </CardHeader>
      <CardContent>
        {sortedRoutes.length === 0 ? (
          <p className="text-sm text-muted-foreground py-4 text-center">
            No route data available yet.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Path</TableHead>
                <TableHead className="text-right">Requests</TableHead>
                <TableHead className="text-right">Avg Latency (ms)</TableHead>
                <TableHead className="text-right">Error Rate (%)</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {sortedRoutes.map((route) => (
                <TableRow key={route.path}>
                  <TableCell className="font-mono text-sm">
                    {route.path}
                  </TableCell>
                  <TableCell className="text-right">
                    {route.requests.toLocaleString()}
                  </TableCell>
                  <TableCell className="text-right">
                    {route.avg_latency.toFixed(1)}
                  </TableCell>
                  <TableCell className="text-right">
                    {(route.error_rate * 100).toFixed(2)}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}
