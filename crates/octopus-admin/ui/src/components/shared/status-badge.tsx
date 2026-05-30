import { Badge } from "@/components/ui/badge";

export function StatusBadge({ status }: { status: string }) {
  const variant =
    status === "passing" || status === "healthy" || status === "closed"
      ? "default"
      : status === "warning" || status === "half-open"
        ? "secondary"
        : "destructive";

  return <Badge variant={variant}>{status}</Badge>;
}
