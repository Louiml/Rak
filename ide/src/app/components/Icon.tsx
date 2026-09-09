'use client';

import React from 'react';

export type IconName =
  | 'save' | 'play' | 'file-plus' | 'file-text' | 'folder' | 'folder-open' | 'folder-plus'
  | 'folder-search' | 'terminal' | 'panel-left' | 'monitor' | 'search' | 'zap' | 'bar-chart'
  | 'eraser' | 'trash' | 'x' | 'chevron-down' | 'chevron-up' | 'chevron-right' | 'scissors'
  | 'clipboard' | 'copy' | 'pencil' | 'rotate-cw' | 'replace' | 'arrow-up' | 'arrow-down'
  | 'key' | 'box' | 'file-code' | 'tab' | 'ruler' | 'message-square' | 'gear' | 'dot'
  | 'list' | 'check' | 'book';

interface IconProps {
  name: IconName;
  size?: number;
  className?: string;
}

const FILLED = new Set<IconName>(['play', 'zap', 'dot']);

const PATHS: Record<IconName, React.ReactNode> = {
  save: <><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" /><path d="M17 21v-8H7v8" /><path d="M7 3v5h8" /></>,
  play: <path d="M8 5v14l11-7z" />,
  'file-plus': <><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /><path d="M12 12v6" /><path d="M9 15h6" /></>,
  'file-text': <><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /><path d="M8 13h8" /><path d="M8 17h8" /><path d="M8 9h2" /></>,
  folder: <path d="M4 20a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2z" />,
  'folder-open': <><path d="M4 20a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v1H4z" /><path d="M3 11h18l-2 8a2 2 0 0 1-2 2H5z" /></>,
  'folder-plus': <><path d="M4 20a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2z" /><path d="M12 11v6" /><path d="M9 14h6" /></>,
  'folder-search': <><path d="M4 20a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2z" /><circle cx="14" cy="15" r="2" /><path d="m15.5 16.5 1.5 1.5" /></>,
  terminal: <><path d="M4 17l6-6-6-6" /><path d="M12 19h8" /></>,
  'panel-left': <><rect x="3" y="3" width="18" height="18" rx="2" /><path d="M9 3v18" /></>,
  monitor: <><rect x="2" y="3" width="20" height="14" rx="2" /><path d="M8 21h8" /><path d="M12 17v4" /></>,
  search: <><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /></>,
  zap: <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" />,
  'bar-chart': <><path d="M3 20h18" /><path d="M7 20v-5" /><path d="M12 20v-9" /><path d="M17 20V6" /></>,
  eraser: <><path d="M7 21 3.4 17.4a2 2 0 0 1 0-2.8L13 5l6 6-7.6 7.6a2 2 0 0 1-2.8 0z" /><path d="M14 21H7" /><path d="m5 11 8 8" /></>,
  trash: <><path d="M3 6h18" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" /><path d="M10 11v6" /><path d="M14 11v6" /></>,
  x: <><path d="M18 6 6 18" /><path d="m6 6 12 12" /></>,
  'chevron-down': <path d="m6 9 6 6 6-6" />,
  'chevron-up': <path d="m18 15-6-6-6 6" />,
  'chevron-right': <path d="m9 18 6-6-6-6" />,
  scissors: <><circle cx="6" cy="6" r="3" /><circle cx="6" cy="18" r="3" /><path d="M20 4 8.7 15.3" /><path d="M14.5 14.5 20 20" /><path d="M8.5 8.5 12 12" /></>,
  clipboard: <><rect x="8" y="2" width="8" height="4" rx="1" /><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" /></>,
  copy: <><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></>,
  pencil: <><path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" /></>,
  'rotate-cw': <><path d="M21 12a9 9 0 1 1-3-6.7" /><path d="M21 3v6h-6" /></>,
  replace: <><path d="M17 2.1 21 6l-4 3.9" /><path d="M3 11v-1a4 4 0 0 1 4-4h14" /><path d="M7 21.9 3 18l4-3.9" /><path d="M21 13v1a4 4 0 0 1-4 4H3" /></>,
  'arrow-up': <><path d="M12 19V5" /><path d="m5 12 7-7 7 7" /></>,
  'arrow-down': <><path d="M12 5v14" /><path d="m19 12-7 7-7-7" /></>,
  key: <><circle cx="7.5" cy="15.5" r="4.5" /><path d="m10.5 12.5 8-8" /><path d="m16 6 3 3" /><path d="m13 9 2 2" /></>,
  box: <><path d="M21 8 12 3 3 8v8l9 5 9-5z" /><path d="M3 8l9 5 9-5" /><path d="M12 13v8" /></>,
  'file-code': <><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /><path d="m9 13-2 2 2 2" /><path d="m13 13 2 2-2 2" /></>,
  tab: <><path d="M3 7h8" /><path d="m7 3 4 4-4 4" /><path d="M15 4v16" /></>,
  ruler: <><path d="M3 17 17 3l4 4L7 21z" /><path d="m7 13 2 2" /><path d="m10 10 2 2" /><path d="m13 7 2 2" /></>,
  'message-square': <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />,
  gear: <><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" /></>,
  dot: <circle cx="12" cy="12" r="4" />,
  list: <><path d="M8 6h13" /><path d="M8 12h13" /><path d="M8 18h13" /><path d="M3 6h.01" /><path d="M3 12h.01" /><path d="M3 18h.01" /></>,
  check: <path d="M20 6 9 17l-5-5" />,
  book: <><path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" /><path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" /></>,
};

export function Icon({ name, size = 16, className = '' }: IconProps) {
  const fill = FILLED.has(name);
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={fill ? 'currentColor' : 'none'}
      stroke={fill ? 'none' : 'currentColor'}
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {PATHS[name]}
    </svg>
  );
}

// --- File-type colored badge tiles ---

const FILE_BADGES: Record<string, { label: string; bg: string; fg: string }> = {
  rak: { label: 'R', bg: '#10b981', fg: '#04140d' },
  rs: { label: 'rs', bg: '#f97316', fg: '#1a0c00' },
  js: { label: 'JS', bg: '#eab308', fg: '#1a1500' },
  mjs: { label: 'JS', bg: '#eab308', fg: '#1a1500' },
  ts: { label: 'TS', bg: '#3b82f6', fg: '#0a1530' },
  tsx: { label: 'TSX', bg: '#3b82f6', fg: '#0a1530' },
  jsx: { label: 'JSX', bg: '#eab308', fg: '#1a1500' },
  html: { label: '<>', bg: '#f97316', fg: '#1a0c00' },
  htm: { label: '<>', bg: '#f97316', fg: '#1a0c00' },
  css: { label: '#', bg: '#38bdf8', fg: '#031a26' },
  scss: { label: '#', bg: '#38bdf8', fg: '#031a26' },
  json: { label: '{}', bg: '#22c55e', fg: '#04190f' },
  toml: { label: 'T', bg: '#a78bfa', fg: '#140a26' },
  md: { label: 'M', bg: '#71717a', fg: '#0a0a0a' },
  svg: { label: '<>', bg: '#f59e0b', fg: '#1a1100' },
  lock: { label: 'L', bg: '#52525b', fg: '#0a0a0a' },
};

interface FileIconProps {
  name: string;
  isDir?: boolean;
  size?: number;
}

export function FileIcon({ name, isDir = false, size = 16 }: FileIconProps) {
  if (isDir) return <Icon name="folder" size={size} className="text-sky-400" />;
  const dot = name.lastIndexOf('.');
  const ext = dot >= 0 ? name.slice(dot + 1).toLowerCase() : '';
  const badge = FILE_BADGES[ext];
  if (!badge) return <Icon name="file-text" size={size} className="text-zinc-400" />;
  return (
    <span
      style={{ width: size, height: size, background: badge.bg, color: badge.fg }}
      className="inline-flex items-center justify-center rounded-[3px] text-[7px] font-bold leading-none shrink-0 select-none"
    >
      {badge.label}
    </span>
  );
}
