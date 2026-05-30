"use client";

import { useState } from "react";
import { useTlsCerts, useUploadTlsCert, useReloadTls } from "@/hooks/use-tls";
import type { TlsCertInfo, TlsCertUpload } from "@/lib/types";
import { PageHeader } from "@/components/shared/page-header";
import { CertUploadDialog } from "@/components/tls/cert-upload-dialog";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { UploadIcon, RefreshCwIcon } from "lucide-react";
import { toast } from "sonner";

function ExpiryBadge({ cert }: { cert: TlsCertInfo }) {
  const variant =
    cert.status === "valid"
      ? "default"
      : cert.status === "expiring"
        ? "secondary"
        : cert.status === "expired"
          ? "destructive"
          : "outline";
  const label =
    cert.days_until_expiry == null
      ? "Unknown"
      : cert.days_until_expiry < 0
        ? `Expired ${Math.abs(cert.days_until_expiry)}d ago`
        : `${cert.days_until_expiry}d left`;
  return <Badge variant={variant}>{label}</Badge>;
}

function fmtDate(iso: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString();
}

export default function TlsPage() {
  const { data: certs, isLoading } = useTlsCerts();
  const upload = useUploadTlsCert();
  const reload = useReloadTls();
  const [uploadOpen, setUploadOpen] = useState(false);

  function handleUpload(cert: TlsCertUpload) {
    upload.mutate(cert, {
      onSuccess: (res) => {
        toast.success(res.message ?? "Certificate uploaded.");
        setUploadOpen(false);
      },
      onError: (e) => toast.error(`Upload failed: ${e.message}`),
    });
  }

  function handleReload() {
    reload.mutate(undefined, {
      onSuccess: (res) => toast.success(res.message ?? "Reload triggered."),
      onError: (e) => toast.error(`Reload failed: ${e.message}`),
    });
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="TLS Certificates"
        description="Inspect the gateway's TLS certificates and monitor expiry."
        action={
          <div className="flex items-center gap-2">
            <Button variant="outline" onClick={handleReload}>
              <RefreshCwIcon className="size-4" />
              Reload
            </Button>
            <Button onClick={() => setUploadOpen(true)}>
              <UploadIcon className="size-4" />
              Upload
            </Button>
          </div>
        }
      />

      {isLoading && <Skeleton className="h-40 w-full" />}

      {!isLoading && (!certs || certs.length === 0) && (
        <Card>
          <CardContent className="py-10 text-center text-muted-foreground">
            No TLS certificate is configured (no <code>gateway.tls</code> block).
          </CardContent>
        </Card>
      )}

      {certs && certs.length > 0 && (
        <Card>
          <CardContent className="p-0">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Hosts / SANs</TableHead>
                  <TableHead>Issuer</TableHead>
                  <TableHead>Expires</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Min TLS</TableHead>
                  <TableHead>mTLS</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {certs.map((cert) => (
                  <TableRow key={cert.name}>
                    <TableCell className="font-medium">
                      {cert.subject_cn ?? cert.name}
                    </TableCell>
                    <TableCell className="max-w-xs">
                      <div className="flex flex-wrap gap-1">
                        {cert.sans.length === 0 ? (
                          <span className="text-muted-foreground">—</span>
                        ) : (
                          cert.sans.slice(0, 4).map((h) => (
                            <Badge key={h} variant="outline" className="text-xs">
                              {h}
                            </Badge>
                          ))
                        )}
                        {cert.sans.length > 4 && (
                          <span className="text-xs text-muted-foreground">
                            +{cert.sans.length - 4}
                          </span>
                        )}
                      </div>
                    </TableCell>
                    <TableCell className="max-w-xs truncate text-xs text-muted-foreground">
                      {cert.issuer ?? "—"}
                    </TableCell>
                    <TableCell>{fmtDate(cert.not_after)}</TableCell>
                    <TableCell>
                      <ExpiryBadge cert={cert} />
                    </TableCell>
                    <TableCell>{cert.min_tls_version ?? "—"}</TableCell>
                    <TableCell>
                      {cert.require_client_cert ? (
                        <Badge variant="secondary">required</Badge>
                      ) : (
                        <span className="text-muted-foreground">off</span>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      <CertUploadDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        onSubmit={handleUpload}
      />
    </div>
  );
}
