"use client";

import { usePathname } from "next/navigation";
import { Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { SidebarTrigger } from "@/components/ui/sidebar";
import { Separator } from "@/components/ui/separator";
import { useTheme } from "@/components/layout/theme-provider";
import { UserMenu } from "@/components/layout/user-menu";

/** Friendly labels for path segments that don't title-case cleanly. */
const SEGMENT_LABELS: Record<string, string> = {
  "api-explorer": "API Explorer",
  tls: "TLS Certificates",
  grpc: "gRPC Services",
  kubernetes: "Kubernetes",
  auth: "Authentication",
  events: "Security Events",
  k8s: "Kubernetes",
};

function titleCase(segment: string): string {
  return (
    SEGMENT_LABELS[segment] ??
    segment
      .split("-")
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(" ")
  );
}

export function Header() {
  const pathname = usePathname() ?? "/";
  const { theme, setTheme } = useTheme();

  // Strip a possible basePath prefix, then take the last segment.
  const normalized = pathname.replace(/^\/admin\/ui/, "").replace(/\/$/, "");
  const segments = normalized.split("/").filter(Boolean);
  const pageName =
    segments.length === 0 ? "Overview" : titleCase(segments[segments.length - 1]);

  return (
    <header className="flex h-14 items-center gap-4 border-b px-6">
      <SidebarTrigger />
      <Separator orientation="vertical" className="h-6" />
      <h1 className="text-lg font-semibold">{pageName}</h1>
      <div className="ml-auto flex items-center gap-1">
        <Button
          variant="ghost"
          size="icon"
          onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
        >
          {theme === "dark" ? (
            <Sun className="h-4 w-4" />
          ) : (
            <Moon className="h-4 w-4" />
          )}
        </Button>
        <UserMenu />
      </div>
    </header>
  );
}
