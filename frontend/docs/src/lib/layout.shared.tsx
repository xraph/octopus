import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { type LinkItemType } from 'fumadocs-ui/layouts/docs';
import { AlbumIcon, Heart, LayoutTemplate } from 'lucide-react';
import { OctopusLogo, OctopusMark } from '@/components/brand/octopus-logo';

export const linkItems: LinkItemType[] = [
  {
    text: 'Documentation',
    url: '/docs',
    icon: <LayoutTemplate />,
    active: 'nested-url',
  },
  {
    icon: <AlbumIcon />,
    text: 'Blog',
    url: '/blog',
    active: 'nested-url',
  },
  // {
  //   text: 'Showcase',
  //   url: '/showcase',
  //   icon: <LayoutTemplate />,
  //   active: 'url',
  // },
  // {
  //   text: 'Sponsors',
  //   url: '/sponsors',
  //   icon: <Heart />,
  // },
];

export const logo = <OctopusMark size={22} idSuffix="shared" />;

/**
 * Shared layout configurations
 *
 * you can customise layouts individually from:
 * Home Layout: app/(home)/layout.tsx
 * Docs Layout: app/docs/layout.tsx
 */
export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: <OctopusLogo size={22} idSuffix="nav" />,
    },
    // see https://fumadocs.dev/docs/ui/navigation/links
    links: [...linkItems],
    githubUrl: 'https://github.com/xraph/octopus',
  };
}
