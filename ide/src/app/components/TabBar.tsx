'use client';

import React from 'react';

export interface Tab {
  id: string;
  name: string;
  path: string;
  content: string;
  isDirty: boolean;
}

interface TabBarProps {
  tabs: Tab[];
  activeTabId: string | null;
  onTabSelect: (id: string) => void;
  onTabClose: (id: string) => void;
}

export default function TabBar({ tabs, activeTabId, onTabSelect, onTabClose }: TabBarProps) {
  if (tabs.length === 0) return null;

  return (
    <div className="flex items-center bg-zinc-900 border-b border-zinc-800 overflow-x-auto">
      {tabs.map((tab) => (
        <div
          key={tab.id}
          onClick={() => onTabSelect(tab.id)}
          className={`group flex items-center gap-2 px-3 py-2 text-xs cursor-pointer border-r border-zinc-800 transition-colors whitespace-nowrap ${
            activeTabId === tab.id
              ? 'bg-zinc-950 text-zinc-100 border-b-2 border-b-emerald-500'
              : 'text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300'
          }`}
        >
          <span className="text-[10px]">{tab.name.endsWith('.rak') ? '🔴' : '📄'}</span>
          <span>{tab.name}</span>
          {tab.isDirty && <span className="text-emerald-500 text-[10px]">●</span>}
          <button
            onClick={(e) => {
              e.stopPropagation();
              onTabClose(tab.id);
            }}
            className="opacity-0 group-hover:opacity-100 text-zinc-600 hover:text-zinc-300 ml-1 px-1"
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}