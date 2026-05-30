"use client";

import { useEffect } from "react";
import { usePathname, useRouter } from "next/navigation";
import { Loader2Icon } from "lucide-react";
import { SidebarProvider, SidebarInset } from "@/components/ui/sidebar";
import { QueryProvider } from "@/providers/query-provider";
import { WebSocketProvider } from "@/providers/websocket-provider";
import { AuthProvider, useAuth } from "@/providers/auth-provider";
import { ThemeProvider } from "@/components/layout/theme-provider";
import { AppSidebar } from "@/components/layout/app-sidebar";
import { Header } from "@/components/layout/header";
import { Toaster } from "@/components/ui/sonner";

function FullScreenLoader() {
  return (
    <div className="flex min-h-screen items-center justify-center">
      <Loader2Icon className="size-6 animate-spin text-muted-foreground" />
    </div>
  );
}

/** The authenticated dashboard chrome (sidebar + header + live socket). */
function Chrome({ children }: { children: React.ReactNode }) {
  return (
    <WebSocketProvider>
      <SidebarProvider>
        <AppSidebar />
        <SidebarInset>
          <Header />
          <main className="flex-1 p-6 space-y-6">{children}</main>
        </SidebarInset>
      </SidebarProvider>
    </WebSocketProvider>
  );
}

/**
 * Client-side auth gate. Real enforcement lives on the API; this only controls
 * what the SPA renders. The login route is rendered bare (no chrome); every
 * other route requires an authenticated session.
 *
 * Path checks strip a leading `/admin/ui` so they work whether or not
 * `usePathname()` includes the configured `basePath`.
 */
function AuthGate({ children }: { children: React.ReactNode }) {
  const { status } = useAuth();
  const router = useRouter();
  const pathname = usePathname() ?? "/";
  const normalized = pathname.replace(/^\/admin\/ui/, "").replace(/\/$/, "");
  const isLogin = normalized === "/login" || normalized.endsWith("/login");

  useEffect(() => {
    if (status === "unauthenticated" && !isLogin) {
      router.replace("/login");
    } else if (status === "authenticated" && isLogin) {
      router.replace("/");
    }
  }, [status, isLogin, router]);

  if (status === "loading") return <FullScreenLoader />;
  if (isLogin) return <>{children}</>;
  if (status !== "authenticated") return <FullScreenLoader />;
  return <Chrome>{children}</Chrome>;
}

export function AppShell({ children }: { children: React.ReactNode }) {
  return (
    <ThemeProvider>
      <QueryProvider>
        <AuthProvider>
          <AuthGate>{children}</AuthGate>
          <Toaster />
        </AuthProvider>
      </QueryProvider>
    </ThemeProvider>
  );
}
