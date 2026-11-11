// The console's pages. One list drives the navigation, the links between pages and the
// decision of who sees what; a page that is not built yet is simply not in it.

import type { IconName } from './icons';

/** `everyone`; `enterprise`: any signed-in user; `admin`: administrators (anyone in development). */
export type Audience = 'everyone' | 'enterprise' | 'admin' | 'enterprise-admin';

export interface Page {
  href: string;
  label: string;
  icon: IconName;
  audience: Audience;
}

export interface Section {
  title: string;
  pages: Page[];
}

export const NAVIGATION: Section[] = [
  {
    title: 'Gateway',
    pages: [{ href: '/', label: 'Overview', icon: 'overview', audience: 'everyone' }],
  },
];

const HREFS = new Set(NAVIGATION.flatMap((section) => section.pages.map((page) => page.href)));

/** `href` when the console has that page, otherwise `undefined` (so nothing links to a 404). */
export function page(href: string): string | undefined {
  return HREFS.has(href) ? href : undefined;
}

/** Whether a page of `audience` is for this viewer. */
export function visibleTo(audience: Audience, viewer: { enterprise: boolean; admin: boolean }) {
  switch (audience) {
    case 'everyone':
      return true;
    case 'enterprise':
      return viewer.enterprise;
    case 'admin':
      return viewer.admin;
    case 'enterprise-admin':
      return viewer.enterprise && viewer.admin;
  }
}
