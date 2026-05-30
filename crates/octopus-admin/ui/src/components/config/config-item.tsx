"use client";

import { useState, useEffect } from "react";
import { useUpdateConfig } from "@/hooks/use-config";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import type { ConfigItem as ConfigItemType } from "@/lib/types";
import { Save } from "lucide-react";

interface ConfigItemProps {
  item: ConfigItemType;
}

export function ConfigItem({ item }: ConfigItemProps) {
  const updateConfig = useUpdateConfig();
  const [value, setValue] = useState<unknown>(item.value);
  const [dirty, setDirty] = useState(false);

  useEffect(() => {
    setValue(item.value);
    setDirty(false);
  }, [item.value]);

  const handleSave = () => {
    updateConfig.mutate(
      { key: item.key, value },
      { onSuccess: () => setDirty(false) }
    );
  };

  const isBoolean = typeof item.value === "boolean";
  const isNumber = typeof item.value === "number";

  return (
    <div className="flex items-center justify-between gap-4 rounded-lg border p-3">
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium font-mono truncate">{item.key}</p>
        {item.description && (
          <p className="text-xs text-muted-foreground mt-0.5">
            {item.description}
          </p>
        )}
      </div>

      <div className="flex items-center gap-2 shrink-0">
        {isBoolean ? (
          <Switch
            checked={value as boolean}
            onCheckedChange={(checked: boolean) => {
              setValue(checked);
              setDirty(checked !== item.value);
            }}
            disabled={!item.editable}
          />
        ) : isNumber ? (
          <Input
            type="number"
            value={String(value ?? "")}
            onChange={(e) => {
              const num = parseFloat(e.target.value);
              setValue(isNaN(num) ? e.target.value : num);
              setDirty(true);
            }}
            disabled={!item.editable}
            className="w-32"
          />
        ) : (
          <Input
            type="text"
            value={String(value ?? "")}
            onChange={(e) => {
              setValue(e.target.value);
              setDirty(true);
            }}
            disabled={!item.editable}
            className="w-48"
          />
        )}

        {item.editable && dirty && (
          <Button
            size="sm"
            onClick={handleSave}
            disabled={updateConfig.isPending}
          >
            <Save className="h-3.5 w-3.5" />
          </Button>
        )}
      </div>
    </div>
  );
}
