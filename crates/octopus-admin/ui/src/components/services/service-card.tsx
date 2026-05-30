import { Globe } from "lucide-react";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { StatusBadge } from "@/components/shared/status-badge";

interface ServiceCardProps {
  name: string;
  version: string;
  address: string;
  port: number;
  routeCount: number;
  healthy: boolean;
}

export function ServiceCard({
  name,
  version,
  address,
  port,
  routeCount,
  healthy,
}: ServiceCardProps) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-center gap-3">
        <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-primary/10">
          <Globe className="h-5 w-5 text-primary" />
        </div>
        <div className="flex-1">
          <div className="flex items-center gap-2">
            <CardTitle>{name}</CardTitle>
            <Badge variant="secondary" className="text-xs">
              v{version}
            </Badge>
          </div>
          <p className="text-xs text-muted-foreground">
            {address}:{port}
          </p>
        </div>
        <StatusBadge status={healthy ? "healthy" : "critical"} />
      </CardHeader>
      <CardContent>
        <div className="text-sm text-muted-foreground">
          {routeCount} {routeCount === 1 ? "route" : "routes"} registered
        </div>
      </CardContent>
    </Card>
  );
}
