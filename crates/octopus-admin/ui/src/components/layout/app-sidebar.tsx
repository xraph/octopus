"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import {
  LayoutDashboard,
  Route,
  Server,
  Globe,
  Activity,
  HeartPulse,
  CircuitBoard,
  BookOpen,
  Settings,
  BarChart3,
  ScrollText,
  History,
  Network,
  KeyRound,
  ShieldCheck,
  ShieldAlert,
  Puzzle,
  Boxes,
  type LucideIcon,
} from "lucide-react";
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarHeader,
  SidebarFooter,
} from "@/components/ui/sidebar";
import { useWebSocket } from "@/providers/websocket-provider";

interface NavItem {
  title: string;
  url: string;
  icon: LucideIcon;
}

interface NavGroup {
  label: string;
  items: NavItem[];
}

// URLs are basePath-relative; Next prepends the configured `basePath`.
const navGroups: NavGroup[] = [
  {
    label: "Overview",
    items: [{ title: "Overview", url: "/", icon: LayoutDashboard }],
  },
  {
    label: "Observability",
    items: [
      { title: "Analytics", url: "/analytics", icon: BarChart3 },
      { title: "Traffic", url: "/traffic", icon: Activity },
      { title: "Logs", url: "/logs", icon: ScrollText },
      { title: "Activity", url: "/activity", icon: History },
      { title: "Health", url: "/health", icon: HeartPulse },
    ],
  },
  {
    label: "Traffic & Routing",
    items: [
      { title: "Routes", url: "/routes", icon: Route },
      { title: "Upstreams", url: "/upstreams", icon: Server },
      { title: "Services", url: "/services", icon: Globe },
      { title: "Circuits", url: "/circuits", icon: CircuitBoard },
      { title: "gRPC", url: "/grpc", icon: Network },
    ],
  },
  {
    label: "Security",
    items: [
      { title: "Auth Providers", url: "/security/auth", icon: KeyRound },
      { title: "TLS Certificates", url: "/security/tls", icon: ShieldCheck },
      { title: "Security Events", url: "/security/events", icon: ShieldAlert },
    ],
  },
  {
    label: "Platform",
    items: [
      { title: "Plugins", url: "/plugins", icon: Puzzle },
      { title: "Kubernetes", url: "/kubernetes", icon: Boxes },
      { title: "API Explorer", url: "/api-explorer", icon: BookOpen },
      { title: "Config", url: "/config", icon: Settings },
    ],
  },
];

export function AppSidebar() {
  const pathname = usePathname() ?? "/";
  const { isConnected } = useWebSocket();

  // Normalize away a possible basePath prefix so active-state matching works
  // regardless of how `usePathname()` reports the path.
  const current = pathname.replace(/^\/admin\/ui/, "").replace(/\/$/, "") || "/";

  const isActive = (url: string) =>
    url === "/" ? current === "/" : current === url || current.startsWith(`${url}/`);

  return (
    <Sidebar>
      <SidebarHeader className="border-b px-6 py-4">
        <div className="flex items-center gap-2">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary text-primary-foreground font-bold text-sm">
            O
          </div>
          <div>
            <h2 className="text-sm font-semibold">Octopus Gateway</h2>
            <p className="text-xs text-muted-foreground">Admin Dashboard</p>
          </div>
        </div>
      </SidebarHeader>
      <SidebarContent>
        {navGroups.map((group) => (
          <SidebarGroup key={group.label}>
            <SidebarGroupLabel>{group.label}</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {group.items.map((item) => (
                  <SidebarMenuItem key={item.title}>
                    <SidebarMenuButton
                      render={<Link href={item.url} />}
                      isActive={isActive(item.url)}
                    >
                      <item.icon className="h-4 w-4" />
                      <span>{item.title}</span>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        ))}
      </SidebarContent>
      <SidebarFooter className="border-t p-4">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <div
            className={`h-2 w-2 rounded-full ${isConnected ? "bg-green-500" : "bg-red-500"}`}
          />
          {isConnected ? "Connected" : "Disconnected"}
        </div>
      </SidebarFooter>
    </Sidebar>
  );
}
