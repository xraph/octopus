import Link from 'next/link'
import React from 'react'
import { ArrowRight } from 'lucide-react'
import { OctopusMark } from '@/components/brand/octopus-logo'

const columns = [
  {
    heading: 'Product',
    links: [
      { label: 'Installation', href: '/docs/installation' },
      { label: 'First Gateway', href: '/docs/first-gateway' },
      { label: 'Configuration', href: '/docs/configuration' },
      { label: 'Kubernetes', href: '/docs/kubernetes' },
    ],
  },
  {
    heading: 'Resources',
    links: [
      { label: 'Documentation', href: '/docs' },
      { label: 'Core Concepts', href: '/docs/concepts' },
      { label: 'Guides', href: '/docs/guides' },
      { label: 'API Reference', href: '/docs/api' },
    ],
  },
  {
    heading: 'Community',
    links: [
      { label: 'GitHub', href: 'https://github.com/xraph/octopus' },
      {
        label: 'Issues',
        href: 'https://github.com/xraph/octopus/issues',
      },
      {
        label: 'Discussions',
        href: 'https://github.com/xraph/octopus/discussions',
      },
    ],
  },
]

export const Footer = () => {
  return (
    <footer className="border-t border-zinc-900 bg-zinc-950 px-6 py-20 text-white">
      <div className="mx-auto grid max-w-7xl grid-cols-1 items-end gap-12 md:grid-cols-2">
        <div>
          <h2 className="mb-4 text-6xl font-light tracking-tighter md:text-7xl">
            Route smarter.
          </h2>
          <div className="mt-12 flex items-center gap-3">
            <OctopusMark size={44} idSuffix="footer" title="Octopus" />
            <span className="text-2xl font-semibold tracking-tight">
              Octopus
            </span>
          </div>
        </div>

        <div className="space-y-8">
          <h2 className="text-6xl font-light tracking-tighter text-zinc-700 md:text-7xl">
            Ship faster.
          </h2>
          <p className="max-w-md font-light leading-relaxed text-zinc-400">
            Octopus is open source and built for developers who run production
            traffic. Star it, extend it with Rhai, and deploy it on Kubernetes —
            all from a single config file.
          </p>
          <Link
            href="/docs/installation"
            className="group inline-flex items-center gap-2 rounded-full bg-gradient-to-r from-violet-500 to-fuchsia-500 px-7 py-3 text-xs font-bold uppercase tracking-[0.2em] text-white transition-all hover:shadow-[0_0_30px_rgba(168,85,247,0.5)]"
          >
            Get Started
            <ArrowRight className="size-4 transition-transform group-hover:translate-x-0.5" />
          </Link>
        </div>
      </div>

      <div className="mx-auto mt-24 grid max-w-7xl grid-cols-2 gap-10 border-t border-zinc-900 pt-12 md:grid-cols-3">
        {columns.map((col) => (
          <div key={col.heading}>
            <p className="mb-4 text-[10px] font-bold uppercase tracking-[0.3em] text-zinc-600">
              {col.heading}
            </p>
            <ul className="space-y-3">
              {col.links.map((link) => (
                <li key={link.label}>
                  <Link
                    href={link.href}
                    className="text-sm text-zinc-400 transition-colors hover:text-violet-300"
                  >
                    {link.label}
                  </Link>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>

      <div className="mx-auto mt-20 flex max-w-7xl flex-col justify-between gap-2 border-t border-zinc-900 pt-8 text-[10px] font-medium uppercase tracking-[0.3em] text-zinc-600 md:flex-row">
        <p>© 2026 Octopus · xraph</p>
        <p>Built with Rust</p>
        <a
          href="https://github.com/xraph/octopus"
          target="_blank"
          rel="noreferrer"
          className="transition-colors hover:text-zinc-300"
        >
          github.com/xraph/octopus
        </a>
      </div>
    </footer>
  )
}
