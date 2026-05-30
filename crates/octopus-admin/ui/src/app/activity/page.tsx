"use client";

import { useActivity } from "@/hooks/use-activity";
import { PageHeader } from "@/components/shared/page-header";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ActivityIcon, AlertCircleIcon } from "lucide-react";

export default function ActivityPage() {
  const { data: activity, isLoading } = useActivity();

  return (
    <div className="space-y-6">
      <PageHeader
        title="Activity"
        description="A live stream of the gateway's most recent requests."
      />

      <Card>
        <CardContent className="p-0">
          {isLoading ? (
            <div className="space-y-2 p-4">
              {Array.from({ length: 8 }).map((_, i) => (
                <Skeleton key={i} className="h-10 w-full" />
              ))}
            </div>
          ) : !activity || activity.length === 0 ? (
            <div className="py-10 text-center text-muted-foreground">
              No recent activity.
            </div>
          ) : (
            <ScrollArea className="h-[65vh]">
              <ol className="relative space-y-0">
                {activity.map((entry, i) => {
                  const isError = entry.level === "error";
                  return (
                    <li
                      key={i}
                      className="flex items-start gap-3 border-b px-4 py-3 last:border-b-0"
                    >
                      <div
                        className={`mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-full ${
                          isError
                            ? "bg-destructive/10 text-destructive"
                            : "bg-primary/10 text-primary"
                        }`}
                      >
                        {isError ? (
                          <AlertCircleIcon className="size-4" />
                        ) : (
                          <ActivityIcon className="size-4" />
                        )}
                      </div>
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium">
                          {entry.message}
                        </p>
                        {entry.details && (
                          <p className="text-xs text-muted-foreground">
                            {entry.details}
                          </p>
                        )}
                      </div>
                      <div className="flex shrink-0 items-center gap-2">
                        {entry.source && (
                          <Badge variant="outline" className="text-xs">
                            {entry.source}
                          </Badge>
                        )}
                        <span className="text-xs text-muted-foreground">
                          {entry.timestamp}
                        </span>
                      </div>
                    </li>
                  );
                })}
              </ol>
            </ScrollArea>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
