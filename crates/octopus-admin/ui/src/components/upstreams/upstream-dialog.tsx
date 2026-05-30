"use client";

import { useEffect, useState } from "react";
import type {
  UpstreamClusterInfo,
  UpstreamConfig,
  UpstreamInstanceConfig,
} from "@/lib/types";
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
import { PlusIcon, Trash2Icon } from "lucide-react";

const STRATEGIES = [
  { value: "round_robin", label: "Round Robin" },
  { value: "least_connections", label: "Least Connections" },
  { value: "weighted_round_robin", label: "Weighted Round Robin" },
  { value: "random", label: "Random" },
  { value: "ip_hash", label: "IP Hash" },
] as const;

/** Map the Rust `Debug` strategy form (e.g. "RoundRobin") to a select value. */
function strategyToValue(debug: string): string {
  switch (debug) {
    case "LeastConnections":
      return "least_connections";
    case "WeightedRoundRobin":
      return "weighted_round_robin";
    case "Random":
      return "random";
    case "IpHash":
      return "ip_hash";
    default:
      return "round_robin";
  }
}

interface InstanceRow {
  id?: string;
  address: string;
  port: string;
  weight: string;
}

interface UpstreamDialogProps {
  cluster?: UpstreamClusterInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (config: UpstreamConfig) => void;
}

const emptyRow = (): InstanceRow => ({ address: "", port: "", weight: "1" });

export function UpstreamDialog({
  cluster,
  open,
  onOpenChange,
  onSubmit,
}: UpstreamDialogProps) {
  const isEdit = !!cluster;
  const [name, setName] = useState("");
  const [strategy, setStrategy] = useState("round_robin");
  const [rows, setRows] = useState<InstanceRow[]>([emptyRow()]);

  useEffect(() => {
    if (cluster) {
      setName(cluster.name);
      setStrategy(strategyToValue(cluster.strategy));
      setRows(
        cluster.instances.length
          ? cluster.instances.map((i) => ({
              id: i.id,
              address: i.address,
              port: String(i.port),
              weight: String(i.weight),
            }))
          : [emptyRow()],
      );
    } else {
      setName("");
      setStrategy("round_robin");
      setRows([emptyRow()]);
    }
  }, [cluster, open]);

  function updateRow(index: number, patch: Partial<InstanceRow>) {
    setRows((prev) =>
      prev.map((r, i) => (i === index ? { ...r, ...patch } : r)),
    );
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const instances: UpstreamInstanceConfig[] = rows
      .filter((r) => r.address.trim())
      .map((r) => ({
        id: r.id,
        address: r.address.trim(),
        port: Number(r.port) || 0,
        weight: Number(r.weight) || 1,
      }));
    onSubmit({ name: name.trim(), strategy, instances });
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit Upstream" : "Add Upstream"}</DialogTitle>
          <DialogDescription>
            {isEdit
              ? "Update the cluster's load-balancing strategy and instances."
              : "Define a new upstream cluster and its backend instances."}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="up-name">Cluster Name</Label>
            <Input
              id="up-name"
              placeholder="user-service"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
              disabled={isEdit}
            />
          </div>

          <div className="grid gap-2">
            <Label htmlFor="up-strategy">Load Balancing</Label>
            <Select
              value={strategy}
              onValueChange={(val) => {
                if (val) setStrategy(val);
              }}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Select strategy" />
              </SelectTrigger>
              <SelectContent>
                {STRATEGIES.map((s) => (
                  <SelectItem key={s.value} value={s.value}>
                    {s.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="grid gap-2">
            <div className="flex items-center justify-between">
              <Label>Instances</Label>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => setRows((prev) => [...prev, emptyRow()])}
              >
                <PlusIcon className="size-4" />
                Add
              </Button>
            </div>
            <div className="space-y-2">
              {rows.map((row, i) => (
                <div key={i} className="flex items-center gap-2">
                  <Input
                    placeholder="host / IP"
                    value={row.address}
                    onChange={(e) => updateRow(i, { address: e.target.value })}
                    className="flex-1"
                  />
                  <Input
                    type="number"
                    placeholder="port"
                    value={row.port}
                    onChange={(e) => updateRow(i, { port: e.target.value })}
                    className="w-24"
                    min={0}
                  />
                  <Input
                    type="number"
                    placeholder="wt"
                    value={row.weight}
                    onChange={(e) => updateRow(i, { weight: e.target.value })}
                    className="w-16"
                    min={1}
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() =>
                      setRows((prev) =>
                        prev.length > 1 ? prev.filter((_, idx) => idx !== i) : prev,
                      )
                    }
                    aria-label="Remove instance"
                  >
                    <Trash2Icon className="size-4" />
                  </Button>
                </div>
              ))}
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
            <Button type="submit">
              {isEdit ? "Save Changes" : "Create Upstream"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
