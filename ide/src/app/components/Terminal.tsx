'use client';

import React, { useState, useRef, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';

interface TerminalProps {
  cwd: string;
  height: number;
}

interface TermLine {
  type: 'cmd' | 'stdout' | 'stderr' | 'info';
  text: string;
}

export default function Terminal({ cwd, height }: TerminalProps) {
  const [lines, setLines] = useState<TermLine[]>([
    { type: 'info', text: `Rak Terminal — working directory: ${cwd || '.'}` },
    { type: 'info', text: 'Type commands and press Enter. Ctrl+L to clear.' },
  ]);
  const [input, setInput] = useState('');
  const [history, setHistory] = useState<string[]>([]);
  const [histIdx, setHistIdx] = useState(-1);
  const [running, setRunning] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  useEffect(() => {
    const setup = async () => {
      const u1 = await listen<{ stream: string; text: string }>('shell-output', (e) => {
        setLines((prev) => [...prev, { type: e.payload.stream === 'stderr' ? 'stderr' : 'stdout', text: e.payload.text }]);
      });
      const u2 = await listen('shell-done', () => { setRunning(false); });
      unlistenRef.current = () => { u1(); u2(); };
    };
    setup();
    return () => { unlistenRef.current?.(); };
  }, []);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [lines]);

  const runCommand = useCallback(async (cmd: string) => {
    if (!cmd.trim()) return;
    setLines((prev) => [...prev, { type: 'cmd', text: `${cwd || '.'}> ${cmd}` }]);
    setHistory((prev) => [...prev, cmd]);
    setHistIdx(-1);
    setRunning(true);
    try {
      await invoke('run_shell', { cmd, cwd: cwd || '.' });
    } catch (e) {
      setLines((prev) => [...prev, { type: 'stderr', text: String(e) }]);
      setRunning(false);
    }
  }, [cwd]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      if (running) return;
      runCommand(input);
      setInput('');
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (history.length === 0) return;
      const idx = histIdx === -1 ? history.length - 1 : Math.max(0, histIdx - 1);
      setHistIdx(idx);
      setInput(history[idx]);
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (histIdx === -1) return;
      const idx = histIdx + 1;
      if (idx >= history.length) { setHistIdx(-1); setInput(''); }
      else { setHistIdx(idx); setInput(history[idx]); }
    } else if (e.key === 'l' && e.ctrlKey) {
      e.preventDefault();
      setLines([]);
    } else if (e.key === 'c' && e.ctrlKey) {
      if (running) {
        e.preventDefault();
        invoke('shell_stop');
        setRunning(false);
        setLines((prev) => [...prev, { type: 'info', text: '^C' }]);
      }
    }
  };

  const getLineColor = (type: TermLine['type']) => {
    switch (type) {
      case 'cmd': return 'text-emerald-400 font-semibold';
      case 'stdout': return 'text-zinc-300';
      case 'stderr': return 'text-red-400';
      case 'info': return 'text-zinc-500';
    }
  };

  return (
    <div className="flex flex-col bg-zinc-950 border-t border-zinc-800 overflow-hidden" style={{ height }}>
      <div className="flex items-center justify-between px-3 py-1 bg-zinc-900 border-b border-zinc-800">
        <span className="text-xs text-zinc-400 font-semibold">Terminal</span>
        <div className="flex items-center gap-2">
          <span className="text-[10px] text-zinc-600 truncate max-w-[200px]">{cwd || '.'}</span>
          {running && <span className="text-[10px] text-yellow-400 animate-pulse">running...</span>}
          <button
            onClick={() => setLines([])}
            className="text-xs text-zinc-500 hover:text-zinc-300"
            title="Clear (Ctrl+L)"
          >
            Clear
          </button>
          <button
            onClick={() => { if (running) invoke('shell_stop'); setRunning(false); }}
            className="text-xs text-red-500 hover:text-red-300"
            title="Kill (Ctrl+C)"
          >
            Kill
          </button>
        </div>
      </div>
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-3 py-2 space-y-0.5 font-mono text-xs">
        {lines.map((line, i) => (
          <div key={i} className={`whitespace-pre-wrap break-all ${getLineColor(line.type)}`}>
            {line.text}
          </div>
        ))}
      </div>
      <div className="flex items-center px-3 py-1.5 border-t border-zinc-800 bg-zinc-900">
        <span className="text-emerald-400 text-xs mr-2 select-none">{cwd || '.'} $</span>
        <input
          ref={inputRef}
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={running}
          spellCheck={false}
          autoComplete="off"
          className="flex-1 bg-transparent text-zinc-200 text-xs font-mono outline-none disabled:opacity-50"
          placeholder={running ? 'waiting...' : 'type a command...'}
        />
      </div>
    </div>
  );
}
