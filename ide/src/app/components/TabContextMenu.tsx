'use client';

import React, { useState } from 'react';
import { Tab } from './TabBar';

interface TabContextMenuProps {
  tab: Tab;
  x: number;
  y: number;
  onClose: () => void;
  onCloseTab: () => void;
  onCloseOthers: () => void;
  onCloseRight: () => void;
  onCloseAll: () => void;
  onCopyPath: () => void;
  onRevealInExplorer: () => void;
}

export default function TabContextMenu(props: TabContextMenuProps) {
  const items: { label: string; icon?: string; action: () => void; danger?: boolean }[] = [
    { label: 'Close', icon: '✕', action: props.onCloseTab },
    { label: 'Close Others', icon: '✕', action: props.onCloseOthers },
    { label: 'Close to the Right', icon: '✕', action: props.onCloseRight },
    { label: 'Close All', icon: '✕', action: props.onCloseAll },
    { label: 'Copy Path', icon: '📋', action: props.onCopyPath },
    { label: 'Reveal in Explorer', icon: '📁', action: props.onRevealInExplorer },
  ];

  return (
    <>
      <div className="fixed inset-0 z-50" onClick={props.onClose} onContextMenu={(e) => { e.preventDefault(); props.onClose(); }} />
      <div
        className="fixed z-50 bg-zinc-800 border border-zinc-600 rounded-lg shadow-2xl py-1 min-w-[180px]"
        style={{ left: props.x, top: props.y }}
      >
        {items.map((item, i) => (
          <button
            key={i}
            onClick={() => { item.action(); props.onClose(); }}
            className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-300 hover:bg-emerald-600 hover:text-white"
          >
            {item.icon && <span className="text-[10px] w-4">{item.icon}</span>}
            {item.label}
          </button>
        ))}
      </div>
    </>
  );
}

interface TabBarWithMenuProps {
  tabs: Tab[];
  activeTabId: string | null;
  onTabSelect: (id: string) => void;
  onTabClose: (id: string) => void;
  onCloseOthers: (id: string) => void;
  onCloseRight: (id: string) => void;
  onCloseAll: () => void;
  onCopyPath: (path: string) => void;
  onRevealInExplorer: (path: string) => void;
}

export function TabBarWithMenu(props: TabBarWithMenuProps) {
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; tab: Tab } | null>(null);

  return (
    <>
      <div className="flex items-center bg-zinc-900 border-b border-zinc-800 overflow-x-auto">
        {props.tabs.map((tab) => (
          <div
            key={tab.id}
            onClick={() => props.onTabSelect(tab.id)}
            onContextMenu={(e) => {
              e.preventDefault();
              setCtxMenu({ x: e.clientX, y: e.clientY, tab });
            }}
            className={`group flex items-center gap-2 px-3 py-2 text-xs cursor-pointer border-r border-zinc-800 transition-colors whitespace-nowrap ${
              props.activeTabId === tab.id
                ? 'bg-zinc-950 text-zinc-100 border-b-2 border-b-emerald-500'
                : 'text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300'
            }`}
          >
            <span className="text-[10px]">{tab.name.endsWith('.rak') ? '🔴' : '📄'}</span>
            <span>{tab.name}</span>
            {tab.isDirty && <span className="text-emerald-500 text-[10px]">●</span>}
            <button
              onClick={(e) => { e.stopPropagation(); props.onTabClose(tab.id); }}
              className="opacity-0 group-hover:opacity-100 text-zinc-600 hover:text-zinc-300 ml-1 px-1"
            >
              ✕
            </button>
          </div>
        ))}
      </div>
      {ctxMenu && (
        <TabContextMenu
          tab={ctxMenu.tab}
          x={ctxMenu.x}
          y={ctxMenu.y}
          onClose={() => setCtxMenu(null)}
          onCloseTab={() => props.onTabClose(ctxMenu.tab.id)}
          onCloseOthers={() => props.onCloseOthers(ctxMenu.tab.id)}
          onCloseRight={() => props.onCloseRight(ctxMenu.tab.id)}
          onCloseAll={() => props.onCloseAll()}
          onCopyPath={() => props.onCopyPath(ctxMenu.tab.path)}
          onRevealInExplorer={() => props.onRevealInExplorer(ctxMenu.tab.path)}
        />
      )}
    </>
  );
}
