'use client';

import React, { useState, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { FileIcon } from './Icon';

interface QuickFileEntry { name: string; path: string; }

interface QuickOpenProps {
  open: boolean;
  workspacePath: string;
  onOpen: (path: string, name: string) => void;
  onClose: () => void;
}

export default function QuickOpen({ open, workspacePath, onOpen, onClose }: QuickOpenProps) {
  const [query, setQuery] = useState('');
  const [files, setFiles] = useState<QuickFileEntry[]>([]);
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open && workspacePath) {
      setQuery('');
      setSelected(0);
      invoke<QuickFileEntry[]>('list_files_recursive', { path: workspacePath, ext: '' })
        .then(setFiles)
        .catch(() => setFiles([]));
      setTimeout(() => inputRef.current?.focus(), 0);
    }
  }, [open, workspacePath]);

  if (!open) return null;

  const q = query.toLowerCase();
  const filtered = files.filter(f => f.name.toLowerCase().includes(q) || f.path.toLowerCase().includes(q)).slice(0, 20);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); setSelected(s => Math.min(s + 1, filtered.length - 1)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setSelected(s => Math.max(s - 1, 0)); }
    else if (e.key === 'Enter') {
      e.preventDefault();
      if (filtered[selected]) { onOpen(filtered[selected].path, filtered[selected].name); onClose(); }
    } else if (e.key === 'Escape') { e.preventDefault(); onClose(); }
  };

  return (
    <>
      <div className="fixed inset-0 z-50 bg-black/40" onClick={onClose} />
      <div className="fixed top-1/4 left-1/2 -translate-x-1/2 z-50 w-[480px] bg-zinc-800 border border-zinc-600 rounded-lg shadow-2xl overflow-hidden">
        <div className="p-2 border-b border-zinc-700">
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => { setQuery(e.target.value); setSelected(0); }}
            onKeyDown={handleKeyDown}
            placeholder="Search files by name..."
            className="w-full bg-zinc-900 text-zinc-200 text-sm px-3 py-2 rounded border border-zinc-700 outline-none focus:border-emerald-500"
            spellCheck={false}
          />
        </div>
        <div className="max-h-64 overflow-y-auto py-1">
          {filtered.length === 0 && <div className="px-3 py-2 text-xs text-zinc-500">No files found</div>}
          {filtered.map((f, i) => (
            <button
              key={f.path}
              onClick={() => { onOpen(f.path, f.name); onClose(); }}
              onMouseEnter={() => setSelected(i)}
              className={`w-full flex items-center gap-2 px-3 py-2 text-sm text-left transition-colors ${
                i === selected ? 'bg-emerald-600 text-white' : 'text-zinc-300 hover:bg-zinc-700'
              }`}
            >
              <span className="flex items-center"><FileIcon name={f.name} size={14} /></span>
              <span className="truncate">{f.name}</span>
              <span className={`text-[10px] truncate ml-auto ${i === selected ? 'text-white/50' : 'text-zinc-600'}`}>
                {f.path.replace(workspacePath, '').replace(/^[\\\/]/, '')}
              </span>
            </button>
          ))}
        </div>
      </div>
    </>
  );
}
