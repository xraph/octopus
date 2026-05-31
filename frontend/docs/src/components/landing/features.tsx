'use client'

import { Boxes, Gauge, Layers, Network, Route, Shield } from 'lucide-react'
import { cn } from '@/lib/utils'

const features = [
  {
    title: 'FARP Service Discovery',
    label: 'Zero manual wiring',
    description:
      'Upstreams register themselves over FARP, so the gateway always routes to healthy, up-to-date backends.',
    color: 'from-violet-500/25',
    icon: Network,
  },
  {
    title: 'Dynamic Routing & Proxy',
    label: 'Radix-trie matching',
    description:
      'Host- and path-based matching reverse-proxying HTTP, gRPC, WebSocket, SSE, and GraphQL.',
    color: 'from-fuchsia-500/25',
    icon: Route,
  },
  {
    title: 'Load Balancing & Health',
    label: 'Active health checks',
    description:
      'Weighted and round-robin balancing with health checks and circuit breaking to shed failing upstreams.',
    color: 'from-purple-500/25',
    icon: Gauge,
  },
  {
    title: 'TLS, SNI & mTLS',
    label: 'Hot certificate reload',
    description:
      'Terminate TLS with SNI-based certificate selection, mutual TLS, and hot reload — no restarts.',
    color: 'from-violet-500/25',
    icon: Shield,
  },
  {
    title: 'Middleware & Rate Limiting',
    label: 'Programmable with Rhai',
    description:
      'Composable middleware for rate limiting, CORS, compression, and auth — plus custom Rhai scripts.',
    color: 'from-fuchsia-500/25',
    icon: Layers,
  },
  {
    title: 'Kubernetes Native',
    label: 'CRDs + Gateway API',
    description:
      'Run as an operator with native CRDs and Gateway API support, installable straight from a Helm chart.',
    color: 'from-purple-500/25',
    icon: Boxes,
  },
]

export const Features = () => {
  return (
    <section className="relative bg-black px-6 py-28 md:py-32">
      <div className="pointer-events-none absolute inset-0 bg-[repeating-linear-gradient(45deg,#1a1a1a_0px_1px,transparent_1px_8px)] opacity-60 [mask-image:radial-gradient(ellipse_80%_50%_at_50%_0%,#000_60%,transparent_110%)]" />
      <div className="relative z-10 mx-auto max-w-7xl space-y-20">
        <div className="flex flex-col justify-between gap-10 border-b border-neutral-800 pb-12 md:flex-row md:items-end">
          <h2 className="max-w-2xl text-4xl font-black uppercase leading-none tracking-tighter text-white md:text-6xl">
            Everything you need to
            <br />
            <span className="bg-gradient-to-r from-violet-300 to-fuchsia-400 bg-clip-text text-transparent">
              route production traffic.
            </span>
          </h2>
          <p className="max-w-xs font-mono text-sm uppercase leading-relaxed tracking-widest text-neutral-500">
            From service discovery to TLS termination — a complete, programmable
            edge for your services.
          </p>
        </div>

        <div className="grid grid-cols-1 gap-6 md:grid-cols-2 lg:grid-cols-3">
          {features.map((card, i) => (
            <div
              key={i}
              className="group relative overflow-hidden rounded-2xl border border-neutral-800 bg-neutral-950 p-10 transition-all duration-500 hover:border-violet-500/40"
            >
              <div
                className={cn(
                  'absolute inset-0 bg-gradient-to-br to-transparent opacity-0 transition-opacity duration-700 group-hover:opacity-100',
                  card.color
                )}
              />
              <div className="relative z-10 space-y-10">
                <div className="flex size-14 items-center justify-center rounded-2xl border border-white/10 bg-white/[0.06]">
                  <card.icon className="size-6 text-violet-300" />
                </div>
                <div className="space-y-3">
                  <span className="text-[10px] font-mono uppercase tracking-[0.3em] text-neutral-500">
                    {card.label}
                  </span>
                  <h3 className="text-2xl font-black uppercase tracking-tighter text-white">
                    {card.title}
                  </h3>
                  <p className="text-sm font-light leading-relaxed text-neutral-400">
                    {card.description}
                  </p>
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}
