"use client";

import { useEffect, useState } from "react";
import type { RouteInfo, RouteConfig } from "@/lib/types";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const HTTP_METHODS = ["GET", "POST", "PUT", "DELETE", "PATCH"] as const;

interface RouteDialogProps {
  route?: RouteInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (config: RouteConfig) => void;
}

export function RouteDialog({
  route,
  open,
  onOpenChange,
  onSubmit,
}: RouteDialogProps) {
  const isEdit = !!route;

  const [path, setPath] = useState("");
  const [method, setMethod] = useState<string>("GET");
  const [upstream, setUpstream] = useState("");
  const [timeout, setTimeout] = useState("");
  const [retryCount, setRetryCount] = useState("");

  useEffect(() => {
    if (route) {
      setPath(route.path);
      setMethod(route.method);
      setUpstream(route.upstream);
      setTimeout("");
      setRetryCount("");
    } else {
      setPath("");
      setMethod("GET");
      setUpstream("");
      setTimeout("");
      setRetryCount("");
    }
  }, [route, open]);

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const config: RouteConfig = {
      path,
      method,
      upstream,
    };
    if (route?.id) {
      config.id = route.id;
    }
    if (timeout) {
      config.timeout_ms = Number(timeout);
    }
    if (retryCount) {
      config.retry_count = Number(retryCount);
    }
    onSubmit(config);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit Route" : "Add Route"}</DialogTitle>
          <DialogDescription>
            {isEdit
              ? "Update the route configuration."
              : "Configure a new route for the gateway."}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="route-path">Path</Label>
            <Input
              id="route-path"
              placeholder="/api/v1/resource"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              required
            />
          </div>

          <div className="grid gap-2">
            <Label htmlFor="route-method">Method</Label>
            <Select value={method} onValueChange={(val) => { if (val) setMethod(val); }}>
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Select method" />
              </SelectTrigger>
              <SelectContent>
                {HTTP_METHODS.map((m) => (
                  <SelectItem key={m} value={m}>
                    {m}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="grid gap-2">
            <Label htmlFor="route-upstream">Upstream URL</Label>
            <Input
              id="route-upstream"
              placeholder="http://localhost:3000"
              value={upstream}
              onChange={(e) => setUpstream(e.target.value)}
              required
            />
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="grid gap-2">
              <Label htmlFor="route-timeout">Timeout (ms)</Label>
              <Input
                id="route-timeout"
                type="number"
                placeholder="30000"
                value={timeout}
                onChange={(e) => setTimeout(e.target.value)}
                min={0}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="route-retry">Retry Count</Label>
              <Input
                id="route-retry"
                type="number"
                placeholder="3"
                value={retryCount}
                onChange={(e) => setRetryCount(e.target.value)}
                min={0}
              />
            </div>
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
            >
              Cancel
            </Button>
            <Button type="submit">{isEdit ? "Save Changes" : "Create Route"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
