'use client'

import { Activity, ArrowDownRight, ArrowUpRight } from 'lucide-react'
import React, { useRef } from 'react'
import { cn } from '@/lib/utils'
import { TimelineAnimation } from '@/components/ui/timeline-animation'
import { ClippedAreaChart } from './charts'

const kpis = [
  { label: 'Throughput', value: '128K req/s', change: '+18.4%', trend: 'up' },
  { label: 'p99 Latency', value: '0.9 ms', change: '-12.3%', trend: 'down' },
  { label: 'TLS Handshakes', value: '42K/s', change: '+9.1%', trend: 'up' },
  { label: 'Active Upstreams', value: '1,280', change: '+24', trend: 'up' },
]

export const Stats = () => {
  const timelineRef = useRef<HTMLDivElement>(null)

  return (
    <section
      ref={timelineRef}
      className="flex min-h-screen flex-col justify-center gap-12 bg-black py-28 text-white"
    >
      <div className="mx-auto w-full max-w-7xl px-6 text-center">
        <h2 className="text-4xl font-black tracking-tighter md:text-5xl">
          Built for{' '}
          <span className="bg-gradient-to-r from-violet-300 to-fuchsia-400 bg-clip-text text-transparent">
            production traffic.
          </span>
        </h2>
        <p className="mt-4 text-lg font-light text-neutral-400">
          A Rust data plane that stays fast under load — measured, not promised.
        </p>
      </div>

      <div className="mx-auto w-full max-w-7xl px-6">
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
          {/* Main chart */}
          <TimelineAnimation
            animationNum={1}
            timelineRef={timelineRef}
            className="rounded-3xl border border-zinc-800 bg-zinc-950 p-8 lg:col-span-2"
          >
            <ClippedAreaChart />
          </TimelineAnimation>

          {/* Breakdown */}
          <div className="flex flex-col gap-6">
            <TimelineAnimation
              animationNum={2}
              timelineRef={timelineRef}
              className="flex h-full flex-col justify-between rounded-3xl border border-violet-500/20 bg-gradient-to-br from-violet-600/20 to-zinc-950 p-6 shadow-lg"
            >
              <div>
                <p className="mb-2 text-[10px] font-bold uppercase tracking-[0.2em] text-violet-300/70">
                  30-day availability
                </p>
                <h4 className="text-xl font-bold tracking-tight">Uptime SLA</h4>
              </div>
              <div className="mt-8">
                <div className="mb-2 flex items-end justify-between">
                  <span className="text-3xl font-semibold tracking-tighter">
                    99.99%
                  </span>
                  <span className="mb-1 text-xs font-medium text-zinc-400">
                    Target: 99.95%
                  </span>
                </div>
                <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
                  <div className="h-full w-[99%] rounded-full bg-gradient-to-r from-violet-400 to-fuchsia-400" />
                </div>
              </div>
            </TimelineAnimation>

            <TimelineAnimation
              animationNum={3}
              timelineRef={timelineRef}
              className="h-full rounded-3xl border border-zinc-800 bg-zinc-950 p-6"
            >
              <div className="mb-4 flex items-center gap-3">
                <div className="flex size-8 items-center justify-center rounded-lg border border-zinc-800 bg-zinc-900">
                  <Activity className="size-4 text-violet-300" />
                </div>
                <h4 className="font-bold text-white">Traffic served</h4>
              </div>
              <p className="text-sm text-zinc-400">
                Octopus proxied{' '}
                <span className="font-semibold text-white">4.2B requests</span>{' '}
                last month — up{' '}
                <span className="font-semibold text-violet-300">31%</span> over
                the prior period.
              </p>
            </TimelineAnimation>
          </div>
        </div>

        {/* KPI row */}
        <div className="grid grid-cols-2 gap-4 pt-6 md:grid-cols-4">
          {kpis.map((kpi, index) => (
            <TimelineAnimation
              animationNum={4 + index}
              timelineRef={timelineRef}
              key={kpi.label}
              className="rounded-2xl border border-zinc-800 bg-zinc-950 p-6 transition-colors hover:border-violet-500/50 hover:bg-violet-500/5"
            >
              <p className="mb-2 text-xs font-bold uppercase tracking-widest text-zinc-500">
                {kpi.label}
              </p>
              <div className="flex items-baseline justify-between">
                <p className="text-2xl font-black tracking-tighter text-white">
                  {kpi.value}
                </p>
                <span className="inline-flex items-center gap-0.5 rounded bg-violet-500/15 px-1.5 py-0.5 text-xs font-bold text-violet-300">
                  {kpi.trend === 'up' ? (
                    <ArrowUpRight className="size-3" />
                  ) : (
                    <ArrowDownRight className="size-3" />
                  )}
                  {kpi.change}
                </span>
              </div>
            </TimelineAnimation>
          ))}
        </div>

        <p className="pt-6 text-center text-xs text-zinc-600">
          Representative figures from internal benchmarks — your numbers depend
          on hardware and workload.
        </p>
      </div>
    </section>
  )
}
