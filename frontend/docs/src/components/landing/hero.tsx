'use client'

import React, { useRef } from 'react'
import Link from 'next/link'
import dynamic from 'next/dynamic'
import { ArrowRight, Boxes, Network, Route, Shield } from 'lucide-react'
import { TimelineAnimation } from '@/components/ui/timeline-animation'

// The shader is WebGL/three-based — load it client-only so it never runs during SSR.
const ShaderBg = dynamic(() => import('./shader-bg'), { ssr: false })

const capabilities = [
  {
    title: 'FARP Service Discovery',
    label: 'Self-registering upstreams',
    icon: Network,
  },
  {
    title: 'Dynamic Routing',
    label: 'Host & path matching',
    icon: Route,
  },
  {
    title: 'TLS / SNI / mTLS',
    label: 'Hot-reload certificates',
    icon: Shield,
  },
  {
    title: 'Kubernetes Native',
    label: 'CRDs + Gateway API',
    icon: Boxes,
  },
]

export const Hero = () => {
  const timelineRef = useRef<HTMLDivElement>(null)

  return (
    <section
      ref={timelineRef}
      className="relative min-h-screen bg-black text-white overflow-hidden flex flex-col"
    >
      <ShaderBg />

      {/* Legibility overlay over the shader */}
      <div className="pointer-events-none absolute inset-0 z-[1] bg-gradient-to-b from-black/50 via-black/20 to-black/80" />

      {/* Main hero content */}
      <div className="relative z-10 mx-auto flex w-full max-w-7xl grow flex-col justify-center px-6 pt-32 pb-12">
        <TimelineAnimation
          once
          animationNum={1}
          timelineRef={timelineRef}
          className="mb-8 inline-flex w-fit items-center gap-2 rounded-full border border-white/15 bg-white/5 px-4 py-1.5 text-xs font-medium uppercase tracking-[0.2em] text-violet-200 backdrop-blur-md"
        >
          <span className="size-1.5 rounded-full bg-violet-400" />
          Cloud-Native API Gateway · Written in Rust
        </TimelineAnimation>

        <TimelineAnimation
          once
          as="h1"
          animationNum={2}
          timelineRef={timelineRef}
          className="flex flex-col text-[12vw] md:text-[8vw] xl:text-[6.5vw] font-medium leading-[100%] tracking-tight pb-8"
        >
          <span>Ship a programmable</span>
          <span className="bg-clip-text text-transparent bg-gradient-to-r from-violet-200 via-violet-400 to-fuchsia-400">
            API gateway in minutes.
          </span>
        </TimelineAnimation>

        <div className="flex flex-col lg:flex-row items-start lg:items-center gap-8 lg:gap-12">
          <TimelineAnimation
            once
            animationNum={3}
            timelineRef={timelineRef}
            className="flex flex-wrap items-center gap-4"
          >
            <Link
              href="/docs/installation"
              className="group relative flex items-center gap-2 rounded-full bg-white px-7 py-3.5 text-base font-semibold text-black shadow-[0_0_30px_rgba(168,85,247,0.45)] transition hover:shadow-[0_0_40px_rgba(168,85,247,0.7)]"
            >
              Get Started
              <ArrowRight className="size-4 transition-transform group-hover:translate-x-0.5" />
            </Link>
            <Link
              href="/docs/first-gateway"
              className="rounded-full border border-white/20 bg-white/5 px-7 py-3.5 text-base font-medium backdrop-blur-md transition hover:bg-white/10"
            >
              Build your first gateway
            </Link>
          </TimelineAnimation>

          <TimelineAnimation
            once
            as="p"
            animationNum={4}
            timelineRef={timelineRef}
            className="max-w-md text-lg font-light leading-relaxed text-neutral-200"
          >
            Octopus is a fast, programmable API gateway written in Rust — FARP
            service discovery, dynamic routing, TLS/SNI termination, rate
            limiting, and first-class Kubernetes CRDs.
          </TimelineAnimation>
        </div>
      </div>

      {/* Capability strip */}
      <div className="relative z-10 mx-auto flex w-full max-w-7xl justify-start px-6 py-6 md:justify-end md:py-12">
        <TimelineAnimation
          once
          animationNum={5}
          timelineRef={timelineRef}
          className="grid w-full max-w-3xl grid-cols-2 gap-x-10 gap-y-5 rounded-2xl border border-white/10 bg-black/30 p-6 backdrop-blur-lg md:grid-cols-4"
        >
          {capabilities.map((cap) => (
            <div key={cap.title} className="flex flex-col gap-2">
              <cap.icon className="size-5 text-violet-300" />
              <p className="text-sm font-medium text-white">{cap.title}</p>
              <p className="text-xs text-neutral-400">{cap.label}</p>
            </div>
          ))}
        </TimelineAnimation>
      </div>
    </section>
  )
}
