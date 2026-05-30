"use client";

import { useSecurityEvents } from "@/hooks/use-security-events";
import { PageHeader } from "@/components/shared/page-header";
import { Card, CardContent } from "@/components/ui/card";
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

function severityVariant(
  s: string,
): "default" | "secondary" | "destructive" | "outline" {
  switch (s) {
    case "critical":
    case "high":
      return "destructive";
    case "medium":
      return "secondary";
    case "low":
      return "outline";
    default:
      return "default";
  }
}

export default function SecurityEventsPage() {
  const { data: events, isLoading } = useSecurityEvents();

  return (
    <div className="space-y-6">
      <PageHeader
        title="Security Events"
        description="Circuit-breaker trips, rate-limit hits, and other security signals."
      />

      {isLoading && <Skeleton className="h-40 w-full" />}

      {!isLoading && (!events || events.length === 0) && (
        <Card>
          <CardContent className="py-10 text-center text-muted-foreground">
            No security events recorded. All circuits are healthy.
          </CardContent>
        </Card>
      )}

      {events && events.length > 0 && (
        <Card>
          <CardContent className="p-0">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Time</TableHead>
                  <TableHead>Type</TableHead>
                  <TableHead>Severity</TableHead>
                  <TableHead>Source</TableHead>
                  <TableHead>Details</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {events.map((evt, i) => (
                  <TableRow key={i}>
                    <TableCell className="whitespace-nowrap text-xs text-muted-foreground">
                      {evt.timestamp}
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline">{evt.event_type}</Badge>
                    </TableCell>
                    <TableCell>
                      <Badge variant={severityVariant(evt.severity)}>
                        {evt.severity}
                      </Badge>
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {evt.source_ip}
                    </TableCell>
                    <TableCell className="text-sm">{evt.details}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
