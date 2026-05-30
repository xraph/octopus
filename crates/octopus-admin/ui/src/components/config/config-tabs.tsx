"use client";

import { useMemo } from "react";
import { useConfig } from "@/hooks/use-config";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { ConfigItem } from "@/components/config/config-item";
import type { ConfigItem as ConfigItemType } from "@/lib/types";

const TAB_DEFINITIONS = [
  { value: "general", label: "General", prefixes: ["gateway", "server", "log", "host", "port"] },
  { value: "resilience", label: "Resilience", prefixes: ["timeout", "retry", "circuit", "rate_limit", "backoff"] },
  { value: "security", label: "Security", prefixes: ["tls", "ssl", "auth", "cors", "security", "token", "jwt"] },
  { value: "features", label: "Features", prefixes: ["plugin", "feature", "enable", "disable", "farp", "discovery"] },
];

function categorizeItem(item: ConfigItemType): string {
  const key = item.key.toLowerCase();
  for (const tab of TAB_DEFINITIONS) {
    if (tab.prefixes.some((prefix) => key.startsWith(prefix) || key.includes(prefix))) {
      return tab.value;
    }
  }
  return "general";
}

export function ConfigTabs() {
  const { data: configItems } = useConfig();

  const grouped = useMemo(() => {
    const groups: Record<string, ConfigItemType[]> = {
      general: [],
      resilience: [],
      security: [],
      features: [],
    };
    if (!configItems) return groups;
    for (const item of configItems) {
      const category = categorizeItem(item);
      groups[category].push(item);
    }
    return groups;
  }, [configItems]);

  return (
    <Tabs defaultValue="general">
      <TabsList>
        {TAB_DEFINITIONS.map((tab) => (
          <TabsTrigger key={tab.value} value={tab.value}>
            {tab.label}
            {grouped[tab.value].length > 0 && (
              <span className="ml-1.5 text-xs text-muted-foreground">
                ({grouped[tab.value].length})
              </span>
            )}
          </TabsTrigger>
        ))}
      </TabsList>

      {TAB_DEFINITIONS.map((tab) => (
        <TabsContent key={tab.value} value={tab.value}>
          <div className="space-y-2 mt-4">
            {grouped[tab.value].length === 0 ? (
              <p className="text-sm text-muted-foreground py-8 text-center">
                No configuration items in this category.
              </p>
            ) : (
              grouped[tab.value].map((item) => (
                <ConfigItem key={item.key} item={item} />
              ))
            )}
          </div>
        </TabsContent>
      ))}
    </Tabs>
  );
}
