import Link from "next/link";
import {
  ArrowRight,
  Zap,
  Network,
  Route,
  Gauge,
  Shield,
  Layers,
  Boxes,
} from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
import { ReactNode } from "react";
import { LineShadowText } from "@/components/ui/line-shadow-text";
// import { useTheme } from "next-themes";

/**
 * Hero Section Component
 * Modern hero with gradient background and call-to-action
 */
function HeroSection() {
  // const theme = useTheme()
  // const shadowColor = theme.resolvedTheme === "dark" ? "white" : "black"
  return (
    <section className="relative overflow-hidden bg-gradient-to-br from-background via-background to-muted/20 py-24 sm:py-32">
      <div className="absolute inset-0 bg-grid-white/[0.02] bg-[size:60px_60px]" />
      <div className="relative mx-auto max-w-7xl px-6 lg:px-8">
        <div className="mx-auto max-w-2xl text-center">
          <Badge variant="outline" className="mb-4">
            <Zap className="mr-1 h-3 w-3" />
            Cloud-Native API Gateway
          </Badge>
          <h1 className="text-4xl font-bold tracking-tight text-foreground sm:text-6xl">
            Octo
            <LineShadowText className="italic" shadowColor='black'>
            pus
          </LineShadowText>
          </h1>
          <p className="mt-6 text-lg leading-8 text-muted-foreground">
            A fast, programmable API gateway written in Rust. FARP service
            discovery, dynamic host &amp; path routing, TLS/SNI termination, rate
            limiting, and first-class Kubernetes CRDs.
          </p>
          <div className="mt-10 flex items-center justify-center gap-x-6">
            <Link
              href="/docs/installation"
              className="rounded-md bg-brand px-3.5 py-2.5 text-sm font-semibold text-brand-foreground shadow-sm hover:bg-brand/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand"
            >
              Get Started
              <ArrowRight className="ml-2 h-4 w-4 inline" />
            </Link>
            <Link
              href="/docs/first-gateway"
              className="text-sm font-semibold leading-6 text-foreground hover:text-brand"
            >
              Build your first gateway <span aria-hidden="true">→</span>
            </Link>
          </div>
        </div>
      </div>
    </section>
  );
}

/**
 * Feature Cards Section with Aceternity UI Bento Grid
 * Showcases key capabilities of Octopus with glassmorphism effects and hover animations
 */
function FeaturesSection() {
  const features = [
    {
      icon: Network,
      title: "FARP Service Discovery",
      description:
        "Upstreams register themselves over FARP, so the gateway always routes to healthy, up-to-date backends — no manual endpoint wiring.",
      className: "md:col-span-2",
      header: (
        <div className="flex h-full min-h-[6rem] w-full flex-1 rounded-xl bg-gradient-to-br from-blue-500/10 via-purple-500/10 to-pink-500/10 dark:from-blue-500/20 dark:via-purple-500/20 dark:to-pink-500/20" />
      ),
    },
    {
      icon: Route,
      title: "Dynamic Routing & Proxy",
      description:
        "Host- and path-based matching backed by a high-performance radix trie, reverse-proxying HTTP, gRPC, WebSocket, SSE, and GraphQL.",
      className: "md:col-span-1",
      header: (
        <div className="flex h-full min-h-[6rem] w-full flex-1 rounded-xl bg-gradient-to-br from-green-500/10 via-emerald-500/10 to-teal-500/10 dark:from-green-500/20 dark:via-emerald-500/20 dark:to-teal-500/20" />
      ),
    },
    {
      icon: Gauge,
      title: "Load Balancing & Health",
      description:
        "Weighted and round-robin load balancing with active health checks and circuit breaking to shed failing upstreams automatically.",
      className: "md:col-span-1",
      header: (
        <div className="flex h-full min-h-[6rem] w-full flex-1 rounded-xl bg-gradient-to-br from-orange-500/10 via-red-500/10 to-pink-500/10 dark:from-orange-500/20 dark:via-red-500/20 dark:to-pink-500/20" />
      ),
    },
    {
      icon: Shield,
      title: "TLS, SNI & mTLS",
      description:
        "Terminate TLS with SNI-based certificate selection, mutual TLS, and hot certificate reload — no restarts required.",
      className: "md:col-span-2",
      header: (
        <div className="flex h-full min-h-[6rem] w-full flex-1 rounded-xl bg-gradient-to-br from-cyan-500/10 via-blue-500/10 to-indigo-500/10 dark:from-cyan-500/20 dark:via-blue-500/20 dark:to-indigo-500/20" />
      ),
    },
    {
      icon: Layers,
      title: "Middleware & Rate Limiting",
      description:
        "Composable middleware for rate limiting, CORS, compression, and auth — plus custom logic in embedded Rhai scripts.",
      className: "md:col-span-1",
      header: (
        <div className="flex h-full min-h-[6rem] w-full flex-1 rounded-xl bg-gradient-to-br from-violet-500/10 via-purple-500/10 to-fuchsia-500/10 dark:from-violet-500/20 dark:via-purple-500/20 dark:to-fuchsia-500/20" />
      ),
    },
    {
      icon: Boxes,
      title: "Kubernetes Native",
      description:
        "Run as an operator with native CRDs and Gateway API support, installable straight from a Helm chart.",
      className: "md:col-span-2",
      header: (
        <div className="flex h-full min-h-[6rem] w-full flex-1 rounded-xl bg-gradient-to-br from-yellow-500/10 via-orange-500/10 to-red-500/10 dark:from-yellow-500/20 dark:via-orange-500/20 dark:to-red-500/20" />
      ),
    },
  ];

  return (
    <section className="py-24 sm:py-32 relative">
      {/* Background with subtle pattern */}
      <div className="absolute inset-0 bg-gradient-to-br from-background via-background/95 to-muted/20" />
      <div className="absolute inset-0 bg-[radial-gradient(circle_at_50%_50%,rgba(120,119,198,0.1),transparent_50%)]" />

      <div className="relative mx-auto max-w-7xl px-6 lg:px-8">
        <div className="mx-auto max-w-2xl text-center">
          <h2 className="text-3xl font-bold tracking-tight text-foreground sm:text-4xl bg-gradient-to-r from-foreground to-foreground/70 bg-clip-text">
            Everything you need to route production traffic
          </h2>
          <p className="mt-4 text-lg text-muted-foreground">
            From service discovery to TLS termination, Octopus gives you a
            complete, programmable edge for your services.
          </p>
        </div>

        {/* Aceternity UI Bento Grid */}
        <div className="mx-auto mt-16 grid max-w-7xl grid-cols-1 gap-4 sm:mt-20 md:auto-rows-[18rem] md:grid-cols-3">
          {features.map((feature, index) => (
            <div
              key={index}
              className={cn(
                "group/bento row-span-1 flex flex-col justify-between space-y-4 rounded-xl border border-transparent bg-white p-4 shadow-input transition duration-200 hover:shadow-xl dark:border-white/[0.2] dark:bg-black dark:shadow-none",
                feature.className
              )}
            >
              {/* Header with gradient background */}
              {feature.header}

              {/* Content with hover animation */}
              <div className="transition duration-200 group-hover/bento:translate-x-2">
                {/* Icon */}
                <div className="mb-2">
                  <feature.icon className="h-4 w-4 text-neutral-500 dark:text-neutral-400" />
                </div>

                {/* Title */}
                <div className="mb-2 mt-2 font-sans font-bold text-neutral-600 dark:text-neutral-200">
                  {feature.title}
                </div>

                {/* Description */}
                <div className="font-sans text-xs font-normal text-neutral-600 dark:text-neutral-300">
                  {feature.description}
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

/**
 * Quick Start Section
 * Shows installation and basic usage
 */
function QuickStartSection() {
  return (
    <section className="py-24 sm:py-32 bg-muted/30">
      <div className="mx-auto max-w-7xl px-6 lg:px-8">
        <div className="mx-auto max-w-2xl text-center">
          <h2 className="text-3xl font-bold tracking-tight text-foreground sm:text-4xl">
            Get started in minutes
          </h2>
          <p className="mt-4 text-lg text-muted-foreground">
            Run a production-ready gateway from a single config file — install
            with Docker, Cargo, or Helm.
          </p>
        </div>
        <div className="mx-auto mt-16 max-w-4xl">
          <div className="grid gap-8 lg:grid-cols-2">
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <span className="flex h-6 w-6 items-center justify-center rounded-full bg-brand text-xs font-bold text-brand-foreground">
                    1
                  </span>
                  Install Octopus
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="rounded-lg bg-muted p-4">
                  <pre className="overflow-x-auto text-sm">
                    <code>{`# Run with Docker
docker run -p 8080:8080 \\
  -v "$(pwd)/config.yaml:/etc/octopus/config.yaml" \\
  ghcr.io/xraph/octopus:latest

# Deploy on Kubernetes with Helm
helm install octopus oci://ghcr.io/xraph/charts/octopus \\
  --namespace octopus --create-namespace

# Or build from source (Rust 1.75+)
git clone https://github.com/xraph/octopus.git
cd octopus && make release`}</code>
                  </pre>
                </div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <span className="flex h-6 w-6 items-center justify-center rounded-full bg-brand text-xs font-bold text-brand-foreground">
                    2
                  </span>
                  Configure &amp; run
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="space-y-3 rounded-lg bg-muted p-4">
                  <pre className="overflow-x-auto text-sm">
                    <code>{`# config.yaml
gateway:
  listen: "0.0.0.0:8080"

upstreams:
  - name: example-service
    instances:
      - id: example-1
        host: 127.0.0.1
        port: 8081

routes:
  - path: /api
    upstream: example-service
    strip_prefix: /api`}</code>
                  </pre>
                  <pre className="overflow-x-auto text-sm text-muted-foreground">
                    <code>{`# validate, then serve
octopus validate -c config.yaml
octopus serve -c config.yaml`}</code>
                  </pre>
                </div>
              </CardContent>
            </Card>
          </div>
          <div className="mt-8 text-center">
            <Link
              href="/docs/first-gateway"
              className="inline-flex items-center rounded-md bg-brand px-4 py-2 text-sm font-semibold text-brand-foreground shadow-sm hover:bg-brand/90"
            >
              Build your first gateway
              <ArrowRight className="ml-2 h-4 w-4" />
            </Link>
          </div>
        </div>
      </div>
    </section>
  );
}

/**
 * Navigation Cards Section
 * Main documentation sections with visual cards
 */
function NavigationSection() {
  const sections = [
    {
      title: "Getting Started",
      description:
        "Install Octopus and stand up your first gateway in minutes.",
      href: "/docs/installation",
      icon: "🚀",
    },
    {
      title: "Core Concepts",
      description:
        "Routing, service discovery, load balancing, and the request lifecycle.",
      href: "/docs/concepts",
      icon: "🧠",
    },
    {
      title: "Configuration",
      description:
        "Configure upstreams, routes, TLS, middleware, and observability.",
      href: "/docs/configuration",
      icon: "⚙️",
    },
    {
      title: "Kubernetes",
      description:
        "Deploy with the operator, native CRDs, Gateway API, and Helm.",
      href: "/docs/kubernetes",
      icon: "☸️",
    },
    {
      title: "Guides",
      description:
        "Step-by-step tutorials for common gateway scenarios.",
      href: "/docs/guides",
      icon: "📖",
    },
    {
      title: "API Reference",
      description: "Admin API and FARP reference for automating your gateway.",
      href: "/docs/api",
      icon: "📚",
    },
  ];

  return (
    <section className="py-24 sm:py-32">
      <div className="mx-auto max-w-7xl px-6 lg:px-8">
        <div className="mx-auto max-w-2xl text-center">
          <h2 className="text-3xl font-bold tracking-tight text-foreground sm:text-4xl">
            Explore the documentation
          </h2>
          <p className="mt-4 text-lg text-muted-foreground">
            Everything you need to route, secure, and scale traffic to your
            services.
          </p>
        </div>
        <div className="mx-auto mt-16 grid max-w-2xl grid-cols-1 gap-6 sm:mt-20 lg:mx-0 lg:max-w-none lg:grid-cols-3">
          {sections.map((section, index) => (
            <Link key={index} href={section.href} className="group">
              <Card className="h-full transition-all duration-200 hover:shadow-lg hover:border-brand/50 group-hover:scale-[1.02]">
                <CardHeader>
                  <div className="flex items-center gap-3">
                    <span className="text-2xl">{section.icon}</span>
                    <CardTitle className="text-xl group-hover:text-brand transition-colors">
                      {section.title}
                    </CardTitle>
                  </div>
                </CardHeader>
                <CardContent>
                  <CardDescription className="text-base">
                    {section.description}
                  </CardDescription>
                </CardContent>
              </Card>
            </Link>
          ))}
        </div>
      </div>
    </section>
  );
}

/**
 * Community Section
 * Links to community resources and contribution
 */
function CommunitySection() {
  return (
    <section className="py-24 sm:py-32 bg-muted/30">
      <div className="mx-auto max-w-7xl px-6 lg:px-8">
        <div className="mx-auto max-w-2xl text-center">
          <h2 className="text-3xl font-bold tracking-tight text-foreground sm:text-4xl">
            Join the community
          </h2>
          <p className="mt-4 text-lg text-muted-foreground">
            Octopus is open source and built by developers, for developers.
          </p>
        </div>
        <div className="mx-auto mt-16 grid max-w-2xl grid-cols-1 gap-8 sm:mt-20 lg:mx-0 lg:max-w-none lg:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <span className="text-xl">🐙</span>
                GitHub Repository
              </CardTitle>
            </CardHeader>
            <CardContent>
              <CardDescription className="text-base mb-4">
                Star the project, report issues, and contribute to the codebase.
              </CardDescription>
              <Link
                href="https://github.com/xraph/octopus"
                className="inline-flex items-center text-sm font-semibold text-brand hover:text-brand/80"
              >
                View on GitHub
                <ArrowRight className="ml-1 h-4 w-4" />
              </Link>
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <span className="text-xl">💬</span>
                Community Support
              </CardTitle>
            </CardHeader>
            <CardContent>
              <CardDescription className="text-base mb-4">
                Get help, share ideas, and connect with other developers.
              </CardDescription>
              <Link
                href="/docs/guides"
                className="inline-flex items-center text-sm font-semibold text-brand hover:text-brand/80"
              >
                Browse the guides
                <ArrowRight className="ml-1 h-4 w-4" />
              </Link>
            </CardContent>
          </Card>
        </div>
      </div>
    </section>
  );
}

/**
 * Community Section
 * Links to community resources and contribution
 */
function ContainerSection({ children }: { children: ReactNode }) {
  return (
    <section className="container mx-auto max-w-6xl px-6 lg:px-8">
      {children}
    </section>
  );
}

/**
 * Main Home Page Component
 * Combines all sections into a comprehensive landing page
 */
export default function HomePage() {
  return (
    <main className="min-h-screen">
      <ContainerSection>
        <div className="h-full border-l border-r border-border">
          <HeroSection />
        </div>
      </ContainerSection>
      <Separator />

      <ContainerSection>
        <div className="h-full border-l border-r border-border">
          <FeaturesSection />
          <Separator />
          <QuickStartSection />
          <Separator />
          <NavigationSection />
          <Separator />
          <CommunitySection />
        </div>
      </ContainerSection>
    </main>
  );
}
