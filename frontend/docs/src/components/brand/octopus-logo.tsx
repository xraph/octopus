import { Bricolage_Grotesque } from "next/font/google";
import { cn } from "@/lib/utils";

/**
 * Octopus brand wordmark face. Distinctive geometric grotesk — deliberately not
 * the body font (Geist), so the logo reads as a logo wherever it appears.
 */
const brandFont = Bricolage_Grotesque({
  subsets: ["latin"],
  weight: ["600", "700"],
  display: "swap",
});

const OCTAGON =
  "M54.17 22.82 41.18 9.83 22.82 9.83 9.83 22.82 9.83 41.18 22.82 54.17 41.18 54.17 54.17 41.18Z";
// ingress (left) → hub → fan-out (right): the picture of an API gateway.
const FLOWS = "M16 32H30 M32 32 48.7 24.2 M32 32H49.2 M32 32 48.7 39.8";

type MarkProps = {
  size?: number;
  className?: string;
  /** "gradient" for brand violet, "mono" to inherit `currentColor`. */
  variant?: "gradient" | "mono";
  /** Disambiguates gradient ids when several marks render on one page. */
  idSuffix?: string;
  title?: string;
};

/**
 * The Octopus mark — a routing hub inside an octagon (octo = eight): one
 * gateway, many routes. Crisp from 16px (favicon) to hero scale.
 */
export function OctopusMark({
  size = 28,
  className,
  variant = "gradient",
  idSuffix = "a",
  title,
}: MarkProps) {
  if (variant === "mono") {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 64 64"
        fill="none"
        className={className}
        role={title ? "img" : "presentation"}
        aria-label={title}
        aria-hidden={title ? undefined : true}
      >
        <path
          d={OCTAGON}
          stroke="currentColor"
          strokeWidth={5}
          strokeLinejoin="round"
        />
        <path
          d={FLOWS}
          stroke="currentColor"
          strokeWidth={3.2}
          strokeLinecap="round"
          opacity={0.85}
        />
        <circle cx="48.7" cy="24.2" r="2.6" fill="currentColor" />
        <circle cx="49.2" cy="32" r="2.6" fill="currentColor" />
        <circle cx="48.7" cy="39.8" r="2.6" fill="currentColor" />
        <circle cx="16" cy="32" r="2.6" fill="currentColor" />
        <circle
          cx="32"
          cy="32"
          r="5.4"
          fill="none"
          stroke="currentColor"
          strokeWidth={3}
        />
      </svg>
    );
  }

  const gid = `octo-ramp-${idSuffix}`;
  const sid = `octo-core-${idSuffix}`;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      className={className}
      role={title ? "img" : "presentation"}
      aria-label={title}
      aria-hidden={title ? undefined : true}
    >
      <defs>
        <linearGradient
          id={gid}
          x1="14"
          y1="10"
          x2="54"
          y2="54"
          gradientUnits="userSpaceOnUse"
        >
          <stop offset="0" stopColor="#c9a4ff" />
          <stop offset="0.5" stopColor="#a855f7" />
          <stop offset="1" stopColor="#6d28d9" />
        </linearGradient>
        <linearGradient
          id={sid}
          x1="26"
          y1="26"
          x2="38"
          y2="38"
          gradientUnits="userSpaceOnUse"
        >
          <stop offset="0" stopColor="#d8bbff" />
          <stop offset="1" stopColor="#8b3dff" />
        </linearGradient>
      </defs>
      <path
        d={OCTAGON}
        stroke={`url(#${gid})`}
        strokeWidth={5}
        strokeLinejoin="round"
      />
      <path
        d={FLOWS}
        stroke={`url(#${gid})`}
        strokeWidth={3.2}
        strokeLinecap="round"
        opacity={0.9}
      />
      <circle cx="48.7" cy="24.2" r="2.6" fill="#c9a4ff" />
      <circle cx="49.2" cy="32" r="2.6" fill="#c9a4ff" />
      <circle cx="48.7" cy="39.8" r="2.6" fill="#c9a4ff" />
      <circle cx="16" cy="32" r="2.6" fill="#9b6bff" />
      <circle cx="32" cy="32" r="5.6" fill={`url(#${sid})`} />
      <circle cx="32" cy="32" r="2" fill="#0a0a0c" />
    </svg>
  );
}

/** Just the "Octopus" wordmark in the brand face — pair with <OctopusMark/>. */
export function OctopusWordmark({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        brandFont.className,
        "font-semibold tracking-[-0.03em]",
        className,
      )}
    >
      Octopus
    </span>
  );
}

type LogoProps = {
  size?: number;
  className?: string;
  textClassName?: string;
  variant?: "gradient" | "mono";
  idSuffix?: string;
};

/** Mark + "Octopus" wordmark lockup. */
export function OctopusLogo({
  size = 24,
  className,
  textClassName,
  variant = "gradient",
  idSuffix = "lockup",
}: LogoProps) {
  return (
    <span className={cn("inline-flex items-center gap-2", className)}>
      <OctopusMark
        size={size}
        variant={variant}
        idSuffix={idSuffix}
        title="Octopus"
      />
      <span
        className={cn(
          brandFont.className,
          "text-[1.05em] font-semibold tracking-[-0.03em]",
          textClassName,
        )}
      >
        Octopus
      </span>
    </span>
  );
}
