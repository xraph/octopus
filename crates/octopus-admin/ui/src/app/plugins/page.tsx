"use client";

import { useState } from "react";
import {
  usePlugins,
  useTogglePlugin,
  useUpdatePluginConfig,
} from "@/hooks/use-plugins";
import type { PluginInfo } from "@/lib/types";
import { PageHeader } from "@/components/shared/page-header";
import { PluginConfigDialog } from "@/components/plugins/plugin-config-dialog";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import { Settings2Icon } from "lucide-react";
import { toast } from "sonner";

export default function PluginsPage() {
  const { data: plugins, isLoading } = usePlugins();
  const togglePlugin = useTogglePlugin();
  const updateConfig = useUpdatePluginConfig();
  const [configuring, setConfiguring] = useState<PluginInfo | undefined>();

  const enabledCount = plugins?.filter((p) => p.enabled).length ?? 0;

  function handleToggle(plugin: PluginInfo) {
    togglePlugin.mutate(
      { id: plugin.id, enabled: !plugin.enabled },
      {
        onSuccess: () =>
          toast.success(
            `${plugin.name} ${plugin.enabled ? "disabled" : "enabled"}.`,
          ),
        onError: (e) => toast.error(`Toggle failed: ${e.message}`),
      },
    );
  }

  function handleSaveConfig(config: unknown) {
    if (!configuring) return;
    updateConfig.mutate(
      { id: configuring.id, config },
      {
        onSuccess: () => {
          toast.success("Plugin configuration updated.");
          setConfiguring(undefined);
        },
        onError: (e) => toast.error(`Update failed: ${e.message}`),
      },
    );
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="Plugins & Middleware"
        description="Enable, disable, and configure the gateway's middleware chain."
      />

      <div className="grid grid-cols-2 gap-4 md:grid-cols-3">
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Total Plugins</CardDescription>
            <CardTitle className="text-2xl">{plugins?.length ?? 0}</CardTitle>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Enabled</CardDescription>
            <CardTitle className="text-2xl text-green-600">
              {enabledCount}
            </CardTitle>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Disabled</CardDescription>
            <CardTitle className="text-2xl text-muted-foreground">
              {(plugins?.length ?? 0) - enabledCount}
            </CardTitle>
          </CardHeader>
        </Card>
      </div>

      {isLoading && (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={i} className="h-40 w-full" />
          ))}
        </div>
      )}

      {!isLoading && (!plugins || plugins.length === 0) && (
        <Card>
          <CardContent className="py-10 text-center text-muted-foreground">
            No plugins are registered with this gateway.
          </CardContent>
        </Card>
      )}

      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
        {plugins?.map((plugin) => (
          <Card key={plugin.id} className="flex flex-col">
            <CardHeader>
              <div className="flex items-start justify-between gap-2">
                <div className="space-y-1">
                  <CardTitle className="text-base">{plugin.name}</CardTitle>
                  <CardDescription className="flex items-center gap-2">
                    <Badge variant="outline" className="text-xs">
                      v{plugin.version}
                    </Badge>
                    {plugin.author && <span>{plugin.author}</span>}
                  </CardDescription>
                </div>
                <Switch
                  checked={plugin.enabled}
                  onCheckedChange={() => handleToggle(plugin)}
                />
              </div>
            </CardHeader>
            <CardContent className="flex flex-1 flex-col justify-between gap-4">
              <p className="text-sm text-muted-foreground">
                {plugin.description || "No description provided."}
              </p>
              <Button
                variant="outline"
                size="sm"
                className="w-fit"
                onClick={() => setConfiguring(plugin)}
              >
                <Settings2Icon className="size-4" />
                Configure
              </Button>
            </CardContent>
          </Card>
        ))}
      </div>

      <PluginConfigDialog
        plugin={configuring}
        open={!!configuring}
        onOpenChange={(open) => {
          if (!open) setConfiguring(undefined);
        }}
        onSubmit={handleSaveConfig}
      />
    </div>
  );
}
