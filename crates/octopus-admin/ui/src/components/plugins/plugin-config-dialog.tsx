"use client";

import { useEffect, useState } from "react";
import type { PluginInfo } from "@/lib/types";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

interface PluginConfigDialogProps {
  plugin?: PluginInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (config: unknown) => void;
}

export function PluginConfigDialog({
  plugin,
  open,
  onOpenChange,
  onSubmit,
}: PluginConfigDialogProps) {
  const [text, setText] = useState("{}");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setText(JSON.stringify(plugin?.config ?? {}, null, 2));
    setError(null);
  }, [plugin, open]);

  function handleSave() {
    try {
      const parsed = JSON.parse(text);
      setError(null);
      onSubmit(parsed);
    } catch {
      setError("Configuration must be valid JSON.");
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Configure {plugin?.name}</DialogTitle>
          <DialogDescription>
            Edit the plugin configuration as JSON. Saving hot-reloads the plugin.
          </DialogDescription>
        </DialogHeader>
        <Textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          rows={14}
          className="font-mono text-xs"
          spellCheck={false}
        />
        {error && <p className="text-sm text-destructive">{error}</p>}
        <DialogFooter>
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="button" onClick={handleSave}>
            Save Configuration
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
