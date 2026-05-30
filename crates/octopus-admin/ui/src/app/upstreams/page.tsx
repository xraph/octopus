"use client";

import { useState } from "react";
import { PageHeader } from "@/components/shared/page-header";
import {
  useUpstreams,
  useCreateUpstream,
  useUpdateUpstream,
  useDeleteUpstream,
} from "@/hooks/use-upstreams";
import { UpstreamDialog } from "@/components/upstreams/upstream-dialog";
import type { UpstreamClusterInfo, UpstreamConfig } from "@/lib/types";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { PlusIcon, PencilIcon, Trash2Icon } from "lucide-react";
import { toast } from "sonner";

export default function UpstreamsPage() {
  const { data: clusters, isLoading } = useUpstreams();
  const createUpstream = useCreateUpstream();
  const updateUpstream = useUpdateUpstream();
  const deleteUpstream = useDeleteUpstream();

  const [dialogOpen, setDialogOpen] = useState(false);
  const [editing, setEditing] = useState<UpstreamClusterInfo | undefined>();
  const [deleting, setDeleting] = useState<UpstreamClusterInfo | undefined>();

  const totalInstances =
    clusters?.reduce((acc, c) => acc + c.instance_count, 0) ?? 0;
  const totalHealthy =
    clusters?.reduce((acc, c) => acc + c.healthy_count, 0) ?? 0;
  const totalUnhealthy = totalInstances - totalHealthy;

  function handleSubmit(config: UpstreamConfig) {
    if (editing) {
      updateUpstream.mutate(
        { name: editing.name, config },
        {
          onSuccess: () => {
            toast.success("Upstream updated.");
            setDialogOpen(false);
            setEditing(undefined);
          },
          onError: (e) => toast.error(`Failed to update: ${e.message}`),
        },
      );
    } else {
      createUpstream.mutate(config, {
        onSuccess: () => {
          toast.success("Upstream created.");
          setDialogOpen(false);
        },
        onError: (e) => toast.error(`Failed to create: ${e.message}`),
      });
    }
  }

  function confirmDelete() {
    if (!deleting) return;
    deleteUpstream.mutate(deleting.name, {
      onSuccess: () => {
        toast.success("Upstream deleted.");
        setDeleting(undefined);
      },
      onError: (e) => toast.error(`Failed to delete: ${e.message}`),
    });
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="Upstreams"
        description="Upstream service clusters, load balancing, and instance health."
        action={
          <Button
            onClick={() => {
              setEditing(undefined);
              setDialogOpen(true);
            }}
          >
            <PlusIcon className="size-4" />
            Add Upstream
          </Button>
        }
      />

      <div className="grid grid-cols-2 gap-4 md:grid-cols-4">
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Clusters</CardDescription>
            <CardTitle className="text-2xl">{clusters?.length ?? 0}</CardTitle>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Total Instances</CardDescription>
            <CardTitle className="text-2xl">{totalInstances}</CardTitle>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Healthy</CardDescription>
            <CardTitle className="text-2xl text-green-600">
              {totalHealthy}
            </CardTitle>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Unhealthy</CardDescription>
            <CardTitle className="text-2xl text-red-600">
              {totalUnhealthy}
            </CardTitle>
          </CardHeader>
        </Card>
      </div>

      {isLoading && (
        <div className="text-muted-foreground text-sm">Loading upstreams…</div>
      )}

      {!isLoading && (!clusters || clusters.length === 0) && (
        <Card>
          <CardContent className="py-10 text-center text-muted-foreground">
            No upstream clusters yet. Click “Add Upstream” to create one.
          </CardContent>
        </Card>
      )}

      {clusters?.map((cluster) => (
        <Card key={cluster.name}>
          <CardHeader>
            <div className="flex items-center justify-between gap-4">
              <div>
                <CardTitle className="text-lg">{cluster.name}</CardTitle>
                <CardDescription>
                  Strategy: {cluster.strategy} &middot; {cluster.healthy_count}/
                  {cluster.instance_count} healthy
                </CardDescription>
              </div>
              <div className="flex items-center gap-2">
                <Badge
                  variant={
                    cluster.healthy_count === cluster.instance_count
                      ? "default"
                      : "destructive"
                  }
                >
                  {cluster.healthy_count === cluster.instance_count
                    ? "Healthy"
                    : `${cluster.instance_count - cluster.healthy_count} Unhealthy`}
                </Badge>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => {
                    setEditing(cluster);
                    setDialogOpen(true);
                  }}
                  aria-label="Edit upstream"
                >
                  <PencilIcon className="size-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => setDeleting(cluster)}
                  aria-label="Delete upstream"
                >
                  <Trash2Icon className="size-4" />
                </Button>
              </div>
            </div>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>ID</TableHead>
                  <TableHead>URL</TableHead>
                  <TableHead>Weight</TableHead>
                  <TableHead>Health</TableHead>
                  <TableHead className="text-right">Connections</TableHead>
                  <TableHead className="text-right">Latency (ms)</TableHead>
                  <TableHead className="text-right">Error Rate</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {cluster.instances.map((inst) => (
                  <TableRow key={inst.id}>
                    <TableCell className="font-mono text-sm">{inst.id}</TableCell>
                    <TableCell className="font-mono text-sm">{inst.url}</TableCell>
                    <TableCell>{inst.weight}</TableCell>
                    <TableCell>
                      <Badge
                        variant={inst.healthy ? "default" : "destructive"}
                        className="text-xs"
                      >
                        {inst.healthy ? "Healthy" : "Unhealthy"}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      {inst.active_connections}
                    </TableCell>
                    <TableCell className="text-right">
                      {inst.avg_latency_ms.toFixed(1)}
                    </TableCell>
                    <TableCell className="text-right">
                      {(inst.error_rate * 100).toFixed(1)}%
                    </TableCell>
                  </TableRow>
                ))}
                {cluster.instances.length === 0 && (
                  <TableRow>
                    <TableCell
                      colSpan={7}
                      className="text-center text-muted-foreground"
                    >
                      No instances
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      ))}

      <UpstreamDialog
        cluster={editing}
        open={dialogOpen}
        onOpenChange={(open) => {
          setDialogOpen(open);
          if (!open) setEditing(undefined);
        }}
        onSubmit={handleSubmit}
      />

      <AlertDialog
        open={!!deleting}
        onOpenChange={(open) => {
          if (!open) setDeleting(undefined);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete Upstream</AlertDialogTitle>
            <AlertDialogDescription>
              Delete the upstream cluster{" "}
              <span className="font-mono font-medium">{deleting?.name}</span>?
              Routes pointing at it will fail until reassigned. This cannot be
              undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={confirmDelete}>
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
