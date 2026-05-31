'use client'

import React from 'react'
import { ArrowUpRight } from 'lucide-react'

// Representative requests/sec over a 24h window — illustrative benchmark data.
const data = [
  42, 48, 45, 53, 61, 58, 67, 74, 71, 83, 92, 88, 97, 109, 104, 118, 126, 121,
  133, 142, 138, 151, 147, 158,
]

const WIDTH = 600
const HEIGHT = 220
const PAD_X = 8
const PAD_Y = 18

function buildPaths(values: number[]) {
  const max = Math.max(...values)
  const min = Math.min(...values)
  const range = max - min || 1
  const stepX = (WIDTH - PAD_X * 2) / (values.length - 1)

  const points = values.map((v, i) => {
    const x = PAD_X + i * stepX
    const y = PAD_Y + (HEIGHT - PAD_Y * 2) * (1 - (v - min) / range)
    return [x, y] as const
  })

  // Smooth line via Catmull-Rom → cubic Bézier conversion.
  let line = `M ${points[0][0]},${points[0][1]}`
  for (let i = 0; i < points.length - 1; i++) {
    const p0 = points[i === 0 ? 0 : i - 1]
    const p1 = points[i]
    const p2 = points[i + 1]
    const p3 = points[i + 2 < points.length ? i + 2 : points.length - 1]
    const c1x = p1[0] + (p2[0] - p0[0]) / 6
    const c1y = p1[1] + (p2[1] - p0[1]) / 6
    const c2x = p2[0] - (p3[0] - p1[0]) / 6
    const c2y = p2[1] - (p3[1] - p1[1]) / 6
    line += ` C ${c1x},${c1y} ${c2x},${c2y} ${p2[0]},${p2[1]}`
  }

  const area = `${line} L ${points[points.length - 1][0]},${HEIGHT} L ${points[0][0]},${HEIGHT} Z`
  return { line, area }
}

export const ClippedAreaChart = () => {
  const { line, area } = buildPaths(data)

  return (
    <div className="flex h-full flex-col">
      <div className="mb-6 flex items-start justify-between">
        <div>
          <p className="mb-2 text-[10px] font-bold uppercase tracking-[0.2em] text-zinc-500">
            Requests / sec
          </p>
          <p className="text-4xl font-semibold tracking-tighter text-white">
            128K
          </p>
        </div>
        <span className="inline-flex items-center gap-1 rounded-full bg-violet-500/15 px-2.5 py-1 text-xs font-bold text-violet-300">
          <ArrowUpRight className="size-3.5" />
          +18.4%
        </span>
      </div>

      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        preserveAspectRatio="none"
        className="h-44 w-full"
        role="img"
        aria-label="Requests per second over the last 24 hours"
      >
        <defs>
          <linearGradient id="octo-area" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#a855f7" stopOpacity="0.55" />
            <stop offset="100%" stopColor="#a855f7" stopOpacity="0" />
          </linearGradient>
        </defs>
        {/* horizontal grid lines */}
        {[0.25, 0.5, 0.75].map((g) => (
          <line
            key={g}
            x1={0}
            x2={WIDTH}
            y1={HEIGHT * g}
            y2={HEIGHT * g}
            stroke="#27272a"
            strokeWidth={1}
          />
        ))}
        <path d={area} fill="url(#octo-area)" />
        <path
          d={line}
          fill="none"
          stroke="#c084fc"
          strokeWidth={2.5}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>

      <div className="mt-4 flex justify-between text-[10px] font-medium uppercase tracking-widest text-zinc-600">
        <span>00:00</span>
        <span>06:00</span>
        <span>12:00</span>
        <span>18:00</span>
        <span>now</span>
      </div>
    </div>
  )
}
