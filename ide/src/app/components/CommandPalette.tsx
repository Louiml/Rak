'use client';

import React, { useState, useRef, useEffect } from 'react';
import { Icon, IconName } from './Icon';

export interface Command {
  id: string;
  label: string;
  shortcut?: string;
  icon?: IconName;
  action: () => void;
}

interface CommandPaletteProps {
  open: boolean;
  commands: Command[];
  onClose: () => void;
}

export default function CommandPalette({ open, commands, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setQuery('');
      setSelected(0);
      setTimeout(() => inputRef.current?.focus(), 0);
    }
  }, [open]);

  if (!open) return null;

  const filtered = commands.filter(c =>
    c.label.toLowerCase().includes(query.toLowerCase())
  );

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelected(s => Math.min(s + 1, filtered.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelected(s => Math.max(s - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (filtered[selected]) {
        filtered[selected].action();
        onClose();
      }
    } else if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    }
  };

  return (
    <>
      <div className="fixed inset-0 z-50 bg-black/40" onClick={onClose} />
      <div className="fixed top-1/4 left-1/2 -translate-x-1/2 z-50 w-96 bg-zinc-800 border border-zinc-600 rounded-lg shadow-2xl overflow-hidden">
        <div className="p-2 border-b border-zinc-700">
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => { setQuery(e.target.value); setSelected(0); }}
            onKeyDown={handleKeyDown}
            placeholder="Type a command..."
            className="w-full bg-zinc-900 text-zinc-200 text-sm px-3 py-2 rounded border border-zinc-700 outline-none focus:border-emerald-500"
            spellCheck={false}
          />
        </div>
        <div className="max-h-64 overflow-y-auto py-1">
          {filtered.length === 0 && (
            <div className="px-3 py-2 text-xs text-zinc-500">No commands found</div>
          )}
          {filtered.map((cmd, i) => (
            <button
              key={cmd.id}
              onClick={() => { cmd.action(); onClose(); }}
              onMouseEnter={() => setSelected(i)}
              className={`w-full flex items-center justify-between px-3 py-2 text-sm text-left transition-colors ${
                i === selected ? 'bg-emerald-600 text-white' : 'text-zinc-300 hover:bg-zinc-700'
              }`}
            >
              <span className="flex items-center gap-2">
                {cmd.icon && <Icon name={cmd.icon} size={14} className={i === selected ? 'text-white' : 'text-zinc-400'} />}
                {cmd.label}
              </span>
              {cmd.shortcut && (
                <span className={`text-[10px] ${i === selected ? 'text-white/70' : 'text-zinc-500'}`}>
                  {cmd.shortcut}
                </span>
              )}
            </button>
          ))}
        </div>
      </div>
    </>
  );
}
