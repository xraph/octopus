import { Badge } from "@/components/ui/badge";

const methodColors: Record<string, string> = {
  GET: "bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200",
  POST: "bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200",
  PUT: "bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-200",
  DELETE: "bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200",
  PATCH: "bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-200",
};

export function MethodBadge({ method }: { method: string }) {
  return (
    <Badge
      variant="outline"
      className={methodColors[method.toUpperCase()] || ""}
    >
      {method.toUpperCase()}
    </Badge>
  );
}
