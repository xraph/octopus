import { Hero } from "@/components/landing/hero";
import { Features } from "@/components/landing/features";
import { Stats } from "@/components/landing/stats";
import { Footer } from "@/components/landing/footer";

/**
 * Octopus landing page.
 *
 * A full-bleed, dark, premium marketing page built from rebranded ui-layouts
 * blocks: an animated shader hero, a feature grid, gateway performance stats,
 * and a footer — all themed in the Octopus violet brand.
 */
export default function HomePage() {
  return (
    <main className="min-h-screen bg-black">
      <Hero />
      <Features />
      <Stats />
      <Footer />
    </main>
  );
}
