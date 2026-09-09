'use client';

import React, { useState, useRef, useEffect } from 'react';
import { EXAMPLES, Example } from './examples';
import { FileIcon, Icon } from './Icon';

interface ExamplesMenuProps {
  onOpen: (ex: Example) => void;
}

export default function ExamplesMenu({ onOpen }: ExamplesMenuProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen(!open)}
        className="px-2 py-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded"
        title="Open example"
      >
        Examples <Icon name="chevron-down" size={12} className="inline-block" />
      </button>
      {open && (
        <div className="absolute top-full left-0 mt-1 z-30 bg-zinc-800 border border-zinc-700 rounded-lg shadow-xl py-1 min-w-[220px]">
          {EXAMPLES.map((ex) => (
            <button
              key={ex.filename}
              onClick={() => {
                onOpen(ex);
                setOpen(false);
              }}
              className="w-full text-left px-3 py-1.5 text-xs text-zinc-300 hover:bg-emerald-600 hover:text-white flex items-center gap-2"
            >
              <FileIcon name={ex.filename} size={14} />
              <span className="font-mono">{ex.filename}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
