"use client";

import { useEffect, useState } from "react";
import type { TlsCertUpload } from "@/lib/types";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";

interface CertUploadDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (cert: TlsCertUpload) => void;
}

export function CertUploadDialog({
  open,
  onOpenChange,
  onSubmit,
}: CertUploadDialogProps) {
  const [name, setName] = useState("");
  const [certPem, setCertPem] = useState("");
  const [keyPem, setKeyPem] = useState("");

  useEffect(() => {
    if (open) {
      setName("");
      setCertPem("");
      setKeyPem("");
    }
  }, [open]);

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    onSubmit({ name: name.trim(), cert_pem: certPem, key_pem: keyPem });
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Upload Certificate</DialogTitle>
          <DialogDescription>
            Paste a PEM certificate chain and private key. They are written to
            the configured certificate paths and hot-reloaded when enabled.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="cert-name">Name / Primary Host</Label>
            <Input
              id="cert-name"
              placeholder="api.example.com"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="cert-pem">Certificate (PEM)</Label>
            <Textarea
              id="cert-pem"
              rows={6}
              placeholder="-----BEGIN CERTIFICATE-----"
              value={certPem}
              onChange={(e) => setCertPem(e.target.value)}
              className="font-mono text-xs"
              spellCheck={false}
              required
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="key-pem">Private Key (PEM)</Label>
            <Textarea
              id="key-pem"
              rows={4}
              placeholder="-----BEGIN PRIVATE KEY-----"
              value={keyPem}
              onChange={(e) => setKeyPem(e.target.value)}
              className="font-mono text-xs"
              spellCheck={false}
              required
            />
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit">Upload</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
