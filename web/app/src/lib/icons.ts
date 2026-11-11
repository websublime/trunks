// The console's icons: 24×24 stroke paths from the design reference. Inline SVG, so the
// console needs no icon font, no CDN and no extra request.

export const ICONS = {
  overview: 'M3 10.5 12 3l9 7.5V21h-6v-6H9v6H3z',
  servers: 'M4 4h16v6H4zM4 14h16v6H4zM8 7h.01M8 17h.01',
  tools:
    'M14.7 6.3a4 4 0 0 0-5.4 5.4L3 18l3 3 6.3-6.3a4 4 0 0 0 5.4-5.4l-2.6 2.6-2.4-.6-.6-2.4z',
  prompts: 'M4 5h16v11H9l-5 4z',
  resources: 'M6 3h8l4 4v14H6zM14 3v4h4',
  groups: 'M12 3 3 8l9 5 9-5zM3 13l9 5 9-5',
  clients: 'M7 15a4 4 0 1 1 3.5-6H21v4h-2v3h-3v-3h-5.5A4 4 0 0 1 7 15z',
  user: 'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21a8 8 0 0 1 16 0',
  teams:
    'M9 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM2 20a7 7 0 0 1 14 0M16 4.5a3.5 3.5 0 0 1 0 6.5M18 13.5a7 7 0 0 1 4 6.5',
  shield: 'M12 3 4 6v6c0 4.5 3.4 8 8 9 4.6-1 8-4.5 8-9V6z',
  diagnostics: 'M3 12h4l3-8 4 16 3-8h4',
  menu: 'M4 6h16M4 12h16M4 18h16',
  chevronDown: 'M6 9l6 6 6-6',
  clock: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7v5l3 2',
  refresh: 'M20 11a8 8 0 0 0-14.6-4.5L4 8M4 4v4h4M4 13a8 8 0 0 0 14.6 4.5L20 16M20 20v-4h-4',
  lock: 'M6 11h12v10H6zM8 11V8a4 4 0 0 1 8 0v3',
  alert: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 8v5M12 16h.01',
  globe: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18',
  terminal: 'M4 5h16v14H4zM7 9l3 3-3 3M12 15h5',
  eye: 'M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
  external: 'M14 4h6v6M20 4l-9 9M18 14v6H4V6h6',
  spinner: 'M12 3a9 9 0 1 0 9 9',
  plus: 'M12 5v14M5 12h14',
  chevronRight: 'M9 6l6 6-6 6',
  close: 'M6 6l12 12M18 6 6 18',
  copy: 'M9 9h11v11H9zM5 15H4V4h11v1',
  more: 'M5 12h.01M12 12h.01M19 12h.01',
  warning: 'M12 4 2.5 20h19zM12 10v4M12 17h.01',
  rotate: 'M20 12a8 8 0 1 1-2.3-5.7L20 8M20 3v5h-5',
  edit: 'M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4',
  trash: 'M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13',
  check: 'M5 12.5 10 17 19 7',
  search: 'M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4',
  checkCircle: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM8 12.5l3 3 5-6',
  signOut: 'M15 4h4v16h-4M10 8l-4 4 4 4M6 12h10',
  info: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01',
  minusCircle: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM8 12h8',
  arrowRight: 'M5 12h14M13 6l6 6-6 6',
} as const;

export type IconName = keyof typeof ICONS;
