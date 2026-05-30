"use client";

import { useAuthProviders } from "@/hooks/use-auth-providers";
import { useAuthConfig } from "@/hooks/use-auth-config";
import { PageHeader } from "@/components/shared/page-header";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { KeyRoundIcon, ShieldCheckIcon } from "lucide-react";

const PROVIDER_LABELS: Record<string, string> = {
  jwt: "JWT",
  oidc: "OIDC",
  api_key: "API Key",
  forward_auth: "Forward Auth",
  mtls: "Mutual TLS",
};

function ConfigRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between border-b py-2 last:border-b-0">
      <span className="text-sm text-muted-foreground">{label}</span>
      <span className="text-sm font-medium">{value}</span>
    </div>
  );
}

export default function AuthPage() {
  const { data: providers, isLoading: loadingProviders } = useAuthProviders();
  const { data: config, isLoading: loadingConfig } = useAuthConfig();

  return (
    <div className="space-y-6">
      <PageHeader
        title="Authentication"
        description="Configured auth providers and the global authorization policy."
      />

      <Tabs defaultValue="providers">
        <TabsList>
          <TabsTrigger value="providers">Providers</TabsTrigger>
          <TabsTrigger value="authorization">Authorization</TabsTrigger>
        </TabsList>

        <TabsContent value="providers" className="mt-4">
          {loadingProviders ? (
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
              {Array.from({ length: 3 }).map((_, i) => (
                <Skeleton key={i} className="h-28 w-full" />
              ))}
            </div>
          ) : !providers || providers.length === 0 ? (
            <Card>
              <CardContent className="py-10 text-center text-muted-foreground">
                No authentication providers are configured.
              </CardContent>
            </Card>
          ) : (
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
              {providers.map((p) => (
                <Card key={p.name}>
                  <CardHeader>
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <KeyRoundIcon className="size-4 text-muted-foreground" />
                        <CardTitle className="text-base">{p.name}</CardTitle>
                      </div>
                      <Badge variant="default">{p.status}</Badge>
                    </div>
                    <CardDescription>
                      {PROVIDER_LABELS[p.type] ?? p.type}
                    </CardDescription>
                  </CardHeader>
                </Card>
              ))}
            </div>
          )}
        </TabsContent>

        <TabsContent value="authorization" className="mt-4">
          <Card>
            <CardHeader>
              <div className="flex items-center gap-2">
                <ShieldCheckIcon className="size-4 text-muted-foreground" />
                <CardTitle className="text-base">
                  Global Authorization
                </CardTitle>
              </div>
            </CardHeader>
            <CardContent>
              {loadingConfig ? (
                <Skeleton className="h-40 w-full" />
              ) : config ? (
                <div className="divide-y">
                  <ConfigRow
                    label="Default provider"
                    value={config.default_provider ?? "none"}
                  />
                  <ConfigRow
                    label="Global enforcement"
                    value={
                      <Badge
                        variant={config.global_enforce ? "default" : "outline"}
                      >
                        {config.global_enforce ? "on" : "off"}
                      </Badge>
                    }
                  />
                  <ConfigRow
                    label="Authorization engine"
                    value={config.authz_engine ?? "—"}
                  />
                  <ConfigRow
                    label="Global rules"
                    value={config.global_rules_count ?? 0}
                  />
                  <ConfigRow
                    label="OPA configured"
                    value={config.opa_configured ? "yes" : "no"}
                  />
                  <ConfigRow
                    label="Token cache TTL"
                    value={
                      config.token_cache_ttl_secs != null
                        ? `${config.token_cache_ttl_secs}s`
                        : "—"
                    }
                  />
                  <ConfigRow
                    label="Providers configured"
                    value={config.providers_count}
                  />
                </div>
              ) : (
                <p className="text-muted-foreground">
                  Authorization configuration unavailable.
                </p>
              )}
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
