"use client";

import { useState } from "react";
import {
  useRoutes,
  useCreateRoute,
  useUpdateRoute,
  useDeleteRoute,
} from "@/hooks/use-routes";
import type { RouteInfo, RouteConfig } from "@/lib/types";
import { PageHeader } from "@/components/shared/page-header";
import { RoutesTable } from "@/components/routes/routes-table";
import { RouteDialog } from "@/components/routes/route-dialog";
import { Button } from "@/components/ui/button";
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
import { PlusIcon } from "lucide-react";
import { toast } from "sonner";

export default function RoutesPage() {
  const { data: routes, isLoading } = useRoutes();
  const createRoute = useCreateRoute();
  const updateRoute = useUpdateRoute();
  const deleteRoute = useDeleteRoute();

  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingRoute, setEditingRoute] = useState<RouteInfo | undefined>();
  const [deletingRoute, setDeletingRoute] = useState<RouteInfo | undefined>();

  function handleCreate() {
    setEditingRoute(undefined);
    setDialogOpen(true);
  }

  function handleEdit(route: RouteInfo) {
    setEditingRoute(route);
    setDialogOpen(true);
  }

  function handleDelete(route: RouteInfo) {
    setDeletingRoute(route);
  }

  function handleSubmit(config: RouteConfig) {
    if (editingRoute) {
      updateRoute.mutate(
        { id: editingRoute.id, route: config },
        {
          onSuccess: () => {
            toast.success("Route updated successfully.");
            setDialogOpen(false);
            setEditingRoute(undefined);
          },
          onError: (err) => {
            toast.error(`Failed to update route: ${err.message}`);
          },
        }
      );
    } else {
      createRoute.mutate(config, {
        onSuccess: () => {
          toast.success("Route created successfully.");
          setDialogOpen(false);
        },
        onError: (err) => {
          toast.error(`Failed to create route: ${err.message}`);
        },
      });
    }
  }

  function confirmDelete() {
    if (!deletingRoute) return;
    deleteRoute.mutate(deletingRoute.id, {
      onSuccess: () => {
        toast.success("Route deleted successfully.");
        setDeletingRoute(undefined);
      },
      onError: (err) => {
        toast.error(`Failed to delete route: ${err.message}`);
      },
    });
  }

  return (
    <div className="flex flex-col gap-6 p-6">
      <PageHeader
        title="Routes"
        description="Manage API gateway route configurations."
        action={
          <Button onClick={handleCreate}>
            <PlusIcon className="size-4" />
            Add Route
          </Button>
        }
      />

      <RoutesTable
        data={routes ?? []}
        isLoading={isLoading}
        onEdit={handleEdit}
        onDelete={handleDelete}
      />

      <RouteDialog
        route={editingRoute}
        open={dialogOpen}
        onOpenChange={(open) => {
          setDialogOpen(open);
          if (!open) setEditingRoute(undefined);
        }}
        onSubmit={handleSubmit}
      />

      <AlertDialog
        open={!!deletingRoute}
        onOpenChange={(open) => {
          if (!open) setDeletingRoute(undefined);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete Route</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to delete the route{" "}
              <span className="font-mono font-medium">
                {deletingRoute?.method} {deletingRoute?.path}
              </span>
              ? This action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={confirmDelete}
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
