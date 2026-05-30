"use client";

import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api-client";
import { PageHeader } from "@/components/shared/page-header";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  CardDescription,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { BookOpen, ChevronRight } from "lucide-react";
import { useState } from "react";

interface OpenApiSpec {
  openapi?: string;
  info?: { title?: string; version?: string; description?: string };
  paths?: Record<
    string,
    Record<
      string,
      {
        summary?: string;
        description?: string;
        operationId?: string;
        tags?: string[];
        parameters?: Array<{
          name: string;
          in: string;
          required?: boolean;
          description?: string;
        }>;
        responses?: Record<string, { description?: string }>;
      }
    >
  >;
}

const METHOD_COLORS: Record<string, string> = {
  get: "bg-blue-500/10 text-blue-700 border-blue-200",
  post: "bg-green-500/10 text-green-700 border-green-200",
  put: "bg-amber-500/10 text-amber-700 border-amber-200",
  delete: "bg-red-500/10 text-red-700 border-red-200",
  patch: "bg-purple-500/10 text-purple-700 border-purple-200",
};

function EndpointItem({
  method,
  path,
  op,
}: {
  method: string;
  path: string;
  op: { summary?: string; description?: string; tags?: string[] };
}) {
  const [open, setOpen] = useState(false);
  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="flex items-center gap-3 w-full px-4 py-2.5 hover:bg-muted/50 rounded-lg transition-colors text-left"
      >
        <ChevronRight
          className={`size-4 text-muted-foreground transition-transform ${open ? "rotate-90" : ""}`}
        />
        <Badge
          variant="outline"
          className={`uppercase text-xs font-bold min-w-[60px] justify-center ${METHOD_COLORS[method] ?? ""}`}
        >
          {method}
        </Badge>
        <code className="text-sm font-mono">{path}</code>
        {op.summary && (
          <span className="text-sm text-muted-foreground ml-auto truncate max-w-[300px]">
            {op.summary}
          </span>
        )}
      </button>
      {open && (
        <div className="px-4 pb-3 pt-1 ml-[76px]">
          {op.description && (
            <p className="text-sm text-muted-foreground mb-2">
              {op.description}
            </p>
          )}
          {op.tags && op.tags.length > 0 && (
            <div className="flex gap-1">
              {op.tags.map((t) => (
                <Badge key={t} variant="outline" className="text-xs">
                  {t}
                </Badge>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default function ApiExplorerPage() {
  const { data: spec, isLoading } = useQuery<OpenApiSpec>({
    queryKey: ["openapi-spec"],
    queryFn: () => apiGet<OpenApiSpec>("/farp/schema/openapi"),
    refetchInterval: 60_000,
  });

  const paths = spec?.paths ?? {};
  const pathEntries = Object.entries(paths);
  const hasEndpoints = pathEntries.some(
    ([, methods]) => Object.keys(methods).length > 0,
  );

  // Group by tag
  const grouped = new Map<string, Array<{ method: string; path: string; op: Record<string, unknown> }>>();
  for (const [path, methods] of pathEntries) {
    for (const [method, op] of Object.entries(methods)) {
      const tags = (op as { tags?: string[] }).tags ?? ["default"];
      for (const tag of tags) {
        if (!grouped.has(tag)) grouped.set(tag, []);
        grouped.get(tag)!.push({ method, path, op: op as Record<string, unknown> });
      }
    }
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="API Explorer"
        description="Interactive API documentation from federated FARP schemas"
      />

      {spec?.info && (
        <Card>
          <CardHeader>
            <CardTitle>{spec.info.title ?? "API"}</CardTitle>
            <CardDescription>
              {spec.info.description ?? "Federated API specification"}{" "}
              {spec.info.version && (
                <Badge variant="outline" className="ml-2">
                  v{spec.info.version}
                </Badge>
              )}
            </CardDescription>
          </CardHeader>
        </Card>
      )}

      {isLoading && (
        <div className="text-muted-foreground text-sm">
          Loading API specification...
        </div>
      )}

      {!isLoading && hasEndpoints && (
        <>
          {Array.from(grouped.entries()).map(([tag, endpoints]) => (
            <Card key={tag}>
              <CardHeader className="pb-2">
                <CardTitle className="text-base capitalize">{tag}</CardTitle>
                <CardDescription>
                  {endpoints.length} endpoint{endpoints.length !== 1 ? "s" : ""}
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-1">
                {endpoints.map((ep, i) => (
                  <EndpointItem
                    key={`${ep.method}-${ep.path}-${i}`}
                    method={ep.method}
                    path={ep.path}
                    op={ep.op as { summary?: string; description?: string; tags?: string[] }}
                  />
                ))}
              </CardContent>
            </Card>
          ))}
        </>
      )}

      {!isLoading && !hasEndpoints && (
        <Card>
          <CardContent className="flex flex-col items-center justify-center py-16">
            <div className="flex h-16 w-16 items-center justify-center rounded-full bg-muted mb-6">
              <BookOpen className="h-8 w-8 text-muted-foreground" />
            </div>
            <h3 className="text-lg font-semibold mb-2">
              No API Specs Available
            </h3>
            <p className="text-muted-foreground text-center max-w-md">
              API documentation will appear when services register their OpenAPI
              specs via FARP service discovery
            </p>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
