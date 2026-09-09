'use client';

import React from 'react';
import { Icon, IconName } from './Icon';

export interface ContextMenuItem {
  label?: string;
  action?: () => void;
  separator?: boolean;
  disabled?: boolean;
  icon?: IconName;
}

interface EditorContextMenuProps {
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
}

export default function EditorContextMenu({ x, y, items, onClose }: EditorContextMenuProps) {
  return (
    <>
      <div className="fixed inset-0 z-50" onClick={onClose} onContextMenu={(e) => { e.preventDefault(); onClose(); }} />
      <div
        className="fixed z-50 bg-zinc-800 border border-zinc-700 rounded-lg shadow-xl py-1 min-w-[180px]"
        style={{ left: x, top: y }}
      >
        {items.map((item, i) => {
          if (item.separator) {
            return <div key={i} className="border-t border-zinc-700 my-1" />;
          }
          return (
            <button
              key={i}
              disabled={item.disabled}
              onClick={() => {
                item.action?.();
                onClose();
              }}
              className="w-full text-left px-3 py-1.5 text-xs text-zinc-300 hover:bg-emerald-600 hover:text-white disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-zinc-300 flex items-center gap-2"
            >
              {item.icon && <span className="w-4 flex items-center justify-center"><Icon name={item.icon} size={13} className="text-zinc-400" /></span>}
              {item.label}
            </button>
          );
        })}
      </div>
    </>
  );
}