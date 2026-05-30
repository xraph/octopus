"use client";

import { useActivity } from "@/hooks/use-activity";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import { formatDistanceToNow } from "date-fns";

function levelVariant(level: string) {
  switch (level) {
    case "error":
      return "destructive" as const;
    case "warning":
      return "secondary" as const;
    default:
      return "default" as const;
  }
}

export function ActivityFeed() {
  const { data, isLoading } = useActivity();

  if (isLoading) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Recent Activity</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-4">
            {Array.from({ length: 5 }).map((_, i) => (
              <div key={i} className="flex items-start gap-3">
                <Skeleton className="h-5 w-14 rounded-full" />
                <div className="flex-1 space-y-1">
                  <Skeleton className="h-4 w-full" />
                  <Skeleton className="h-3 w-20" />
                </div>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    );
  }

  const entries = data ?? [];

  return (
    <Card>
      <CardHeader>
        <CardTitle>Recent Activity</CardTitle>
      </CardHeader>
      <CardContent>
        <ScrollArea className="h-[340px] pr-3">
          {entries.length === 0 ? (
            <div className="flex h-full items-center justify-center text-muted-foreground">
              No recent activity
            </div>
          ) : (
            <div className="space-y-4">
              {entries.map((entry, i) => (
                <div key={i} className="flex items-start gap-3">
                  <Badge
                    variant={levelVariant(entry.level)}
                    className="mt-0.5 shrink-0 text-xs"
                  >
                    {entry.level}
                  </Badge>
                  <div className="flex-1 min-w-0">
                    <p className="text-sm leading-snug break-words">
                      {entry.message}
                    </p>
                    <div className="mt-1 flex items-center gap-2 text-xs text-muted-foreground">
                      <span>
                        {formatDistanceToNow(new Date(entry.timestamp), {
                          addSuffix: true,
                        })}
                      </span>
                      {entry.source && (
                        <>
                          <span>&middot;</span>
                          <span>{entry.source}</span>
                        </>
                      )}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </ScrollArea>
      </CardContent>
    </Card>
  );
}
