# Deploying Octopus on Kubernetes

Octopus is designed to run Kubernetes-first: it exposes dedicated health probes,
drains gracefully on rolling updates, and discovers upstreams from the
Kubernetes API via EndpointSlices.

## Options

| Path | Use when |
|------|----------|
| [`helm/octopus`](helm/octopus) | You want a parameterized, upgradable install (recommended). |
| [`kubernetes/octopus.yaml`](kubernetes/octopus.yaml) | You want a quick, Helm-free `kubectl apply`. |

## Helm

```sh
helm install octopus deploy/helm/octopus \
  --namespace octopus --create-namespace
```

Common overrides:

```sh
helm install octopus deploy/helm/octopus -n octopus --create-namespace \
  --set image.tag=0.1.0 \
  --set replicaCount=3 \
  --set autoscaling.enabled=true \
  --set metrics.serviceMonitor.enabled=true \
  --set-string discovery.namespace=default \
  --set rbac.clusterScoped=false
```

Routes, upstreams, plugins, and auth go under `.Values.config` (rendered into a
ConfigMap and mounted at `/etc/octopus/config.yaml`).

## Health probes

Served on the gateway port (`8080`):

| Probe | Path | Meaning |
|-------|------|---------|
| startup | `/startupz` | 200 once the listener has bound. |
| liveness | `/livez` | 200 while the process is alive (stays 200 while draining). |
| readiness | `/readyz` | 200 only when ready to serve; flips to 503 the instant SIGTERM is received. |

Prometheus metrics are served at `/metrics` on the gateway port.

## Graceful shutdown

On `SIGTERM` the gateway:

1. flips `/readyz` to **503** immediately (Kubernetes removes the pod from the
   Service EndpointSlice);
2. waits `gateway.pre_stop_delay` (default `5s`) so deregistration propagates;
3. stops accepting and drains in-flight requests up to `gateway.shutdown_timeout`.

Keep `terminationGracePeriodSeconds >= pre_stop_delay + shutdown_timeout`
(the chart defaults to `45s`).

## RBAC

The gateway needs `get/list/watch` on `services`, `endpoints`, and
`discovery.k8s.io/endpointslices`. Watching all namespaces requires the
cluster-scoped role (`rbac.clusterScoped=true`, the default). To restrict it to
one namespace, set `rbac.clusterScoped=false` and `discovery.namespace=<ns>`.

```sh
kubectl auth can-i list endpointslices \
  --as=system:serviceaccount:octopus:octopus
```
