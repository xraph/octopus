"use client";

import { ColumnDef } from "@tanstack/react-table";
import type { RouteInfo } from "@/lib/types";
import { MethodBadge } from "@/components/shared/method-badge";
import { StatusBadge } from "@/components/shared/status-badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  MoreHorizontalIcon,
  PencilIcon,
  TrashIcon,
  ArrowUpDownIcon,
} from "lucide-react";

export function getColumns(callbacks: {
  onEdit: (route: RouteInfo) => void;
  onDelete: (route: RouteInfo) => void;
}): ColumnDef<RouteInfo>[] {
  return [
    {
      accessorKey: "path",
      header: ({ column }) => (
        <Button
          variant="ghost"
          size="sm"
          onClick={() => column.toggleSorting(column.getIsSorted() === "asc")}
        >
          Path
          <ArrowUpDownIcon className="ml-1 size-3" />
        </Button>
      ),
      cell: ({ row }) => (
        <span className="font-mono text-sm">{row.getValue("path")}</span>
      ),
    },
    {
      accessorKey: "method",
      header: "Method",
      cell: ({ row }) => <MethodBadge method={row.getValue("method")} />,
    },
    {
      accessorKey: "upstream",
      header: "Upstream",
      cell: ({ row }) => (
        <span className="max-w-[200px] truncate text-sm text-muted-foreground">
          {row.getValue("upstream")}
        </span>
      ),
    },
    {
      accessorKey: "request_count",
      header: ({ column }) => (
        <Button
          variant="ghost"
          size="sm"
          onClick={() => column.toggleSorting(column.getIsSorted() === "asc")}
        >
          Requests
          <ArrowUpDownIcon className="ml-1 size-3" />
        </Button>
      ),
      cell: ({ row }) => (
        <span className="tabular-nums">
          {(row.getValue("request_count") as number).toLocaleString()}
        </span>
      ),
    },
    {
      accessorKey: "avg_latency_ms",
      header: ({ column }) => (
        <Button
          variant="ghost"
          size="sm"
          onClick={() => column.toggleSorting(column.getIsSorted() === "asc")}
        >
          Avg Latency
          <ArrowUpDownIcon className="ml-1 size-3" />
        </Button>
      ),
      cell: ({ row }) => (
        <span className="tabular-nums">
          {(row.getValue("avg_latency_ms") as number).toFixed(1)}ms
        </span>
      ),
    },
    {
      accessorKey: "is_healthy",
      header: "Health",
      cell: ({ row }) => (
        <StatusBadge
          status={row.getValue("is_healthy") ? "healthy" : "unhealthy"}
        />
      ),
    },
    {
      id: "actions",
      cell: ({ row }) => {
        const route = row.original;
        return (
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button variant="ghost" size="icon-sm" />
              }
            >
              <MoreHorizontalIcon className="size-4" />
              <span className="sr-only">Open menu</span>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={() => callbacks.onEdit(route)}>
                <PencilIcon className="size-4" />
                Edit
              </DropdownMenuItem>
              <DropdownMenuItem
                variant="destructive"
                onClick={() => callbacks.onDelete(route)}
              >
                <TrashIcon className="size-4" />
                Delete
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        );
      },
    },
  ];
}
