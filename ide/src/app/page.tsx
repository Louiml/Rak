'use client';

import React, { useState, useRef, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import FileExplorer from './components/FileExplorer';
import CodeEditor from './components/CodeEditor';
import TitleBar from './components/TitleBar';
import ExamplesMenu from './components/ExamplesMenu';
import Terminal from './components/Terminal';
import CommandPalette, { Command } from './components/CommandPalette';
import QuickOpen from './components/QuickOpen';
import { TabBarWithMenu } from './components/TabContextMenu';
import { Tab } from './components/TabBar';
import { Example } from './components/examples';
import { Icon } from './components/Icon';
import SettingsModal, { Settings, loadSettings, saveSettings } from './components/Settings';
import Tutorial, { TourStep } from './components/Tutorial';

export type { Tab };

interface ConsoleOutput {
  type: 'output' | 'error';
  content: string;
  timestamp: Date;
}

type RunMode = 'interp' | 'vm' | 'bench';

interface Toast { id: number; message: string; type: 'success' | 'error' | 'info'; }

const DEFAULT_CODE = `// Rak v0.4 — pipeline, regex, binary patterns, traits!
// Run: Ctrl+R   Terminal: Ctrl+Shift+T   Palette: Ctrl+Shift+P

// Pipeline operator |>
fn inc(n) { return n + 1 }
fn dbl(n) { return n * 2 }
dump 5 |> inc |> dbl        // 12

// Regex literals /pattern/flags
let re = /\\d+/g
dump re.find_all("a1 b22 c333")   // [1, 22, 333]

// Traits: Display drives dump
struct Point { x: int, y: int }
impl Display for Point {
    fn fmt(self) { return fmt("({}, {})", self.x, self.y) }
}
dump Point { x: 3, y: 4 }   // (3, 4)
`;

export default function IDE() {
  const [settings, setSettings] = useState<Settings>(() => loadSettings());
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [tutorialOpen, setTutorialOpen] = useState(false);
  // Default to `true` so the server-rendered and client first render match
  // (avoids hydration mismatch); reconcile to the real localStorage value
  // after mount.
  const [tutorialSeen, setTutorialSeen] = useState(true);
  useEffect(() => {
    setTutorialSeen(localStorage.getItem('rak.ide.tutorialDone') === '1');
  }, []);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [currentPath, setCurrentPath] = useState<string>('');
  const [workspaceOpen, setWorkspaceOpen] = useState(false);
  const [consoleOutput, setConsoleOutput] = useState<ConsoleOutput[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  const [runMode, setRunMode] = useState<RunMode>(settings.defaultRunMode);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [consoleOpen, setConsoleOpen] = useState(true);
  const [terminalOpen, setTerminalOpen] = useState(false);
  const [terminalHeight, setTerminalHeight] = useState(200);
  const [sidebarWidth, setSidebarWidth] = useState(256);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [quickOpen, setQuickOpen] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [cursorPos, setCursorPos] = useState({ line: 1, col: 1 });
  const consoleRef = useRef<HTMLDivElement>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const toastIdRef = useRef(0);
  const resizeRef = useRef<{ type: 'terminal' | 'sidebar'; startY: number; startH: number; startW: number; startX: number } | null>(null);
  const recentOutputs = useRef(new Map<string, number>());

  const activeTab = tabs.find((t) => t.id === activeTabId) || null;

  const showToast = useCallback((message: string, type: 'success' | 'error' | 'info' = 'info') => {
    const id = toastIdRef.current++;
    setToasts(prev => [...prev, { id, message, type }]);
    setTimeout(() => setToasts(prev => prev.filter(t => t.id !== id)), 3000);
  }, []);

useEffect(() => {
    const setup = async () => {
      const u1 = await listen<{ stream: string; text: string }>('rak-output', (e) => {
        const key = `${e.payload.stream}:${e.payload.text}`;
        const now = Date.now();
        const last = recentOutputs.current.get(key) ?? 0;
        if (now - last < 100) return;
        recentOutputs.current.set(key, now);
        setConsoleOutput(prev => [...prev, { type: e.payload.stream === 'stderr' ? 'error' : 'output', content: e.payload.text, timestamp: new Date() }]);
      });
      const u2 = await listen('rak-done', () => {
        setIsRunning(false);
        setConsoleOutput(prev => [...prev, { type: 'output', content: '> Finished', timestamp: new Date() }]);
      });
      unlistenRef.current = () => { u1(); u2(); };
    };
    setup();
    return () => { unlistenRef.current?.(); };
  }, []);

  useEffect(() => {
    if (consoleRef.current) consoleRef.current.scrollTop = consoleRef.current.scrollHeight;
  }, [consoleOutput]);

  // Persist settings to localStorage whenever they change.
  useEffect(() => { saveSettings(settings); }, [settings]);

  // Auto-start the first-run tutorial when the user enters the workspace.
  useEffect(() => {
    if (workspaceOpen && !tutorialSeen) {
      setTutorialOpen(true);
      localStorage.setItem('rak.ide.tutorialDone', '1');
    }
  }, [workspaceOpen, tutorialSeen]);

  const handleOpenFolder = async () => {
    try {
      const selected = await open({ directory: true, multiple: false, title: 'Open Workspace Folder' });
      if (typeof selected === 'string' && selected) {
        setCurrentPath(selected);
        setWorkspaceOpen(true);
        setTabs([]);
        setActiveTabId(null);
        showToast('Workspace opened', 'info');
      }
    } catch (e) { console.error('Failed to open folder:', e); }
  };

  const handleNewFile = async () => {
    if (!currentPath) { handleNewScratchFile(); return; }
    const name = prompt('File name:', 'untitled.rak');
    if (!name) return;
    const path = `${currentPath}/${name}`;
    try {
      await invoke('create_file', { path });
      setTabs(prev => [...prev, { id: path, name, path, content: '// New Rak script\n', isDirty: false }]);
      setActiveTabId(path);
      showToast(`Created ${name}`, 'success');
    } catch (e) { showToast(`Error: ${e}`, 'error'); }
  };

  const handleNewScratchFile = () => {
    const id = `scratch_${Date.now()}`;
    setTabs(prev => [...prev, { id, name: 'untitled.rak', path: '', content: DEFAULT_CODE, isDirty: false }]);
    setActiveTabId(id);
  };

  const updateTabContent = (id: string, content: string) => {
    setTabs(prev => prev.map(t => t.id === id ? { ...t, content, isDirty: t.content !== content } : t));
  };

  const handleFileSelect = async (path: string, name: string) => {
    const existing = tabs.find(t => t.path === path);
    if (existing) { setActiveTabId(existing.id); return; }
    try {
      const content = await invoke('read_file', { path }) as string;
      setTabs(prev => [...prev, { id: path, name, path, content, isDirty: false }]);
      setActiveTabId(path);
    } catch (e) { showToast(`Error: ${e}`, 'error'); }
  };

  const handleTabClose = (id: string) => {
    const tab = tabs.find(t => t.id === id);
    if (tab?.isDirty && !confirm(`Close ${tab.name} without saving?`)) return;
    setTabs(prev => {
      const filtered = prev.filter(t => t.id !== id);
      if (filtered.length === 0) setActiveTabId(null);
      else if (id === activeTabId) setActiveTabId(filtered[0].id);
      return filtered;
    });
  };

  const handleCloseOthers = (id: string) => {
    setTabs(prev => {
      const kept = prev.filter(t => t.id === id || (!t.isDirty));
      if (!kept.find(t => t.id === id)) setActiveTabId(null);
      return kept;
    });
  };

  const handleCloseRight = (id: string) => {
    const idx = tabs.findIndex(t => t.id === id);
    setTabs(prev => prev.filter((_, i) => i <= idx));
  };

  const handleCloseAll = () => {
    const dirty = tabs.filter(t => t.isDirty);
    if (dirty.length > 0 && !confirm(`Close ${dirty.length} unsaved tab(s)?`)) return;
    setTabs([]);
    setActiveTabId(null);
  };

  const handleSave = async () => {
    if (!activeTab) return;
    if (!activeTab.path) {
      const name = prompt('Save as:', activeTab.name);
      if (!name) return;
      const path = `${currentPath}/${name}`;
      try {
        await invoke('save_file', { path, content: activeTab.content });
        setTabs(prev => prev.map(t => t.id === activeTab.id ? { ...t, path, name, isDirty: false } : t));
        showToast('Saved', 'success');
      } catch (e) { showToast(`Save error: ${e}`, 'error'); }
      return;
    }
    try {
      await invoke('save_file', { path: activeTab.path, content: activeTab.content });
      setTabs(prev => prev.map(t => t.id === activeTab.id ? { ...t, isDirty: false } : t));
      showToast('Saved', 'success');
    } catch (e) { showToast(`Save error: ${e}`, 'error'); }
  };

  const handleRun = async () => {
    if (!activeTab) return;
    setIsRunning(true);
    setConsoleOutput(prev => [...prev, { type: 'output', content: `> Running ${activeTab.name} (${runMode})...`, timestamp: new Date() }]);
    try {
      await invoke('run_rak', { mode: runMode, source: activeTab.content });
    } catch (e) {
      setConsoleOutput(prev => [...prev, { type: 'error', content: String(e), timestamp: new Date() }]);
      setIsRunning(false);
    }
  };

  const handleStop = async () => {
    try { await invoke('stop_rak'); } catch {}
    setIsRunning(false);
    setConsoleOutput(prev => [...prev, { type: 'error', content: '> Stopped', timestamp: new Date() }]);
  };

  const handleClearConsole = () => setConsoleOutput([]);

  const handleOpenExample = (ex: Example) => {
    const id = `ex_${ex.filename}_${Date.now()}`;
    setTabs(prev => [...prev, { id, name: ex.filename, path: '', content: ex.source, isDirty: false }]);
    setActiveTabId(id);
  };

  const handleCopyPath = (path: string) => {
    if (!path) return;
    navigator.clipboard.writeText(path).then(() => showToast('Path copied', 'success'));
  };

  const handleRevealInExplorer = (path: string) => {
    if (!path) return;
    const dir = path.includes('\\') ? path.split('\\').slice(0, -1).join('\\') : path.split('/').slice(0, -1).join('/');
    invoke('open_in_explorer', { path: dir || path }).catch(() => {});
  };

  const handleNextTab = () => {
    if (tabs.length < 2) return;
    const idx = tabs.findIndex(t => t.id === activeTabId);
    setActiveTabId(tabs[(idx + 1) % tabs.length].id);
  };

  // --- Global keyboard shortcuts ---
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key === 's' && !e.shiftKey) { e.preventDefault(); handleSave(); }
      else if (mod && e.key === 'o' && !e.shiftKey) { e.preventDefault(); handleOpenFolder(); }
      else if (mod && e.key === 'n' && !e.shiftKey) { e.preventDefault(); handleNewFile(); }
      else if (mod && e.key === 'r' && !e.shiftKey) { e.preventDefault(); handleRun(); }
      else if (e.key === 'F5') { e.preventDefault(); handleRun(); }
      else if (mod && e.shiftKey && e.key === 'T') { e.preventDefault(); setTerminalOpen(o => !o); }
      else if (mod && e.key === 'b' && !e.shiftKey) { e.preventDefault(); setSidebarOpen(o => !o); }
      else if (mod && e.key === 'j' && !e.shiftKey) { e.preventDefault(); setConsoleOpen(o => !o); }
      else if (mod && e.key === 'p' && !e.shiftKey) { e.preventDefault(); setQuickOpen(true); }
      else if (mod && e.shiftKey && e.key === 'P') { e.preventDefault(); setPaletteOpen(true); }
      else if (mod && e.key === 'w' && !e.shiftKey) { e.preventDefault(); if (activeTabId) handleTabClose(activeTabId); }
      else if (mod && e.key === 'Tab') { e.preventDefault(); handleNextTab(); }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [activeTab, activeTabId, tabs]);

  // --- Resize handlers ---
  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!resizeRef.current) return;
      if (resizeRef.current.type === 'terminal') {
        const dy = resizeRef.current.startY - e.clientY;
        setTerminalHeight(Math.max(80, Math.min(600, resizeRef.current.startH + dy)));
      } else {
        const dx = e.clientX - resizeRef.current.startX;
        setSidebarWidth(Math.max(160, Math.min(500, resizeRef.current.startW + dx)));
      }
    };
    const onMouseUp = () => { resizeRef.current = null; };
    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
    return () => { window.removeEventListener('mousemove', onMouseMove); window.removeEventListener('mouseup', onMouseUp); };
  }, []);

  const getLineClass = (content: string) => {
    if (content.startsWith('[SCAN]')) return 'text-cyan-400';
    if (content.startsWith('[FETCH]')) return 'text-blue-400';
    if (content.startsWith('[DUMP]')) return 'text-yellow-400';
    if (content.startsWith('[TRACE]')) return 'text-purple-400';
    if (content.startsWith('>')) return 'text-zinc-500';
    return 'text-zinc-300';
  };

  // --- Command palette ---
  const commands: Command[] = [
    { id: 'save', label: 'Save File', shortcut: 'Ctrl+S', icon: 'save', action: handleSave },
    { id: 'run', label: 'Run File', shortcut: 'Ctrl+R', icon: 'play', action: handleRun },
    { id: 'newfile', label: 'New File', shortcut: 'Ctrl+N', icon: 'file-plus', action: handleNewFile },
    { id: 'openfolder', label: 'Open Folder', shortcut: 'Ctrl+O', icon: 'folder-open', action: handleOpenFolder },
    { id: 'terminal', label: 'Toggle Terminal', shortcut: 'Ctrl+Shift+T', icon: 'terminal', action: () => setTerminalOpen(o => !o) },
    { id: 'sidebar', label: 'Toggle Sidebar', shortcut: 'Ctrl+B', icon: 'panel-left', action: () => setSidebarOpen(o => !o) },
    { id: 'console', label: 'Toggle Console', shortcut: 'Ctrl+J', icon: 'monitor', action: () => setConsoleOpen(o => !o) },
    { id: 'quickopen', label: 'Quick Open File', shortcut: 'Ctrl+P', icon: 'search', action: () => setQuickOpen(true) },
    { id: 'interp', label: 'Mode: Interpreter', icon: 'zap', action: () => { setRunMode('interp'); showToast('Interpreter mode', 'info'); } },
    { id: 'vm', label: 'Mode: VM', icon: 'zap', action: () => { setRunMode('vm'); showToast('VM mode', 'info'); } },
    { id: 'bench', label: 'Mode: Bench', icon: 'bar-chart', action: () => { setRunMode('bench'); showToast('Bench mode', 'info'); } },
    { id: 'clearconsole', label: 'Clear Console', icon: 'eraser', action: handleClearConsole },
    { id: 'closeall', label: 'Close All Tabs', icon: 'x', action: handleCloseAll },
    { id: 'settings', label: 'Settings', icon: 'gear', action: () => setSettingsOpen(true) },
    { id: 'tutorial', label: 'Show Tutorial', icon: 'book', action: () => setTutorialOpen(true) },
  ];

  const tutorialSteps: TourStep[] = [
    { title: 'Welcome to Rak IDE', body: 'A quick tour of the editor. New in v0.4: pipeline `|>`, regex literals, binary pattern matching, and trait protocols. Press Next to continue, or Skip.' },
    { target: 'editor', title: 'Editor', onEnter: () => { if (tabs.length === 0) handleNewScratchFile(); }, body: 'Write Rak code here. Try a pipeline: `5 |> inc |> dbl`. Regex literals like `/\\d+/g` and char literals like \'P\' are syntax-highlighted.' },
    { target: 'run', title: 'Run', body: 'Run the current file with Ctrl+R (or F5). Stop a running script with the Stop button.' },
    { target: 'mode', title: 'Run mode', body: 'Switch between Interpreter (tree-walker), VM (bytecode, ~6x faster), and Bench to compare both.' },
    { target: 'console', onEnter: () => setConsoleOpen(true), title: 'Console', body: 'Program output is printed here. Toggle it with Ctrl+J. Lines are colour-coded by [DUMP]/[SCAN]/[FETCH]/[TRACE].' },
    { target: 'terminal', onEnter: () => setTerminalOpen(true), title: 'Terminal', body: 'A built-in shell for running rakc, rakpkg, or system commands. Toggle with Ctrl+Shift+T.' },
    { target: 'examples', title: 'Examples', body: 'Open example scripts — including the new pipeline, regex, binary patterns, and traits demos.' },
    { title: 'Commands', body: 'Ctrl+Shift+P opens the command palette. Ctrl+P is quick file open. Ctrl+B toggles the sidebar. Find Settings or Show Tutorial here any time.' },
    { title: "You're ready", body: 'That\u2019s the tour. Re-open it any time from the command palette (Show Tutorial) or Settings. Happy hacking.' },
  ];

  if (!workspaceOpen) {
    return (
      <div className="flex flex-col h-screen bg-zinc-950 text-zinc-100 font-mono overflow-hidden">
        <TitleBar />
        <div className="flex flex-1 items-center justify-center">
          <div className="text-center space-y-6">
            <div className="space-y-2">
              <h1 className="text-5xl font-bold text-emerald-400">Rak</h1>
              <p className="text-zinc-500 text-sm">Programming Language for Hackers &amp; OSINT Investigators</p>
            </div>
            <div className="space-y-3">
              <button onClick={handleOpenFolder} className="px-8 py-3 bg-emerald-600 hover:bg-emerald-500 text-white font-semibold rounded-lg transition-colors text-sm">Open Folder</button>
              <div>
                <button onClick={() => { setCurrentPath(''); setWorkspaceOpen(true); handleNewScratchFile(); }} className="px-8 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg transition-colors text-sm">New Scratch File</button>
              </div>
            </div>
            <div className="text-xs text-zinc-600 max-w-md mx-auto">
              English keywords, first-class hexadecimal, real networking, bytecode VM, SQL server, self-hosting. Built for recon, security research, and general-purpose systems programming.
            </div>
            {!tutorialSeen && (
              <div className="text-[11px] text-emerald-500/80">A quick tour will start when you open a file.</div>
            )}
            {tutorialSeen && (
              <button onClick={() => setTutorialOpen(true)} className="text-[11px] text-zinc-500 hover:text-zinc-300 underline">Replay tutorial</button>
            )}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-screen bg-zinc-950 text-zinc-100 font-mono overflow-hidden">
      <TitleBar />

      {/* Menu Bar */}
      <div className="flex items-center justify-between px-4 py-1.5 bg-zinc-900 border-b border-zinc-800 text-xs">
        <div className="flex items-center gap-1">
          <button onClick={() => setSidebarOpen(!sidebarOpen)} className="px-2 py-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded" title="Toggle Explorer (Ctrl+B)">Explorer</button>
          <button onClick={handleNewFile} className="px-2 py-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded" title="New File (Ctrl+N)">New File</button>
          <button onClick={handleOpenFolder} className="px-2 py-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded" title="Open Folder (Ctrl+O)">Open Folder</button>
          <button onClick={handleSave} className="px-3 py-1 text-zinc-300 hover:bg-zinc-800 rounded" title="Save (Ctrl+S)">Save</button>
          <span data-tour="examples"><ExamplesMenu onOpen={handleOpenExample} /></span>
        </div>
        <div className="flex items-center gap-1">
          <button onClick={() => setTerminalOpen(!terminalOpen)} data-tour="terminal" className="px-2 py-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded" title="Toggle Terminal (Ctrl+Shift+T)">Terminal</button>
          <button onClick={() => setConsoleOpen(!consoleOpen)} className="px-2 py-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded" title="Toggle Console (Ctrl+J)">Console</button>
          <select value={runMode} onChange={(e) => setRunMode(e.target.value as RunMode)} disabled={isRunning} data-tour="mode" title="Run mode" className="ml-2 bg-zinc-800 text-zinc-300 text-xs px-2 py-1 rounded border border-zinc-700 outline-none focus:border-emerald-500 disabled:opacity-50">
            <option value="interp">Interpreter</option>
            <option value="vm">VM</option>
            <option value="bench">Bench</option>
          </select>
          {isRunning && <button onClick={handleStop} className="px-4 py-1 bg-red-600 hover:bg-red-500 text-white font-semibold rounded ml-1" title="Stop">Stop</button>}
          <button onClick={handleRun} data-tour="run" disabled={isRunning || !activeTab} className="px-4 py-1 bg-emerald-600 hover:bg-emerald-500 disabled:bg-zinc-700 text-white font-semibold rounded flex items-center gap-2 ml-1">
            {isRunning ? <><div className="w-3 h-3 border-2 border-white/30 border-t-white rounded-full animate-spin" />Running...</> : <>Run</>}
          </button>
          <button onClick={() => setSettingsOpen(true)} className="px-2 py-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded ml-1" title="Settings">
            <Icon name="gear" size={14} />
          </button>
        </div>
      </div>

      {/* Main Content */}
      <div className="flex flex-1 overflow-hidden">
        {/* Sidebar */}
        {sidebarOpen && (
          <>
            <div style={{ width: sidebarWidth }} className="flex-shrink-0">
              <FileExplorer onFileSelect={handleFileSelect} currentPath={currentPath} onPathChange={setCurrentPath} activeFile={activeTab?.path || null} onToast={showToast} />
            </div>
            <div
              className="w-1 cursor-col-resize bg-zinc-800 hover:bg-emerald-500 transition-colors flex-shrink-0"
              onMouseDown={(e) => { resizeRef.current = { type: 'sidebar' as const, startY: 0, startX: e.clientX, startH: 0, startW: sidebarWidth }; }}
            />
          </>
        )}

        {/* Editor + Terminal */}
        <div className="flex flex-1 flex-col overflow-hidden">
          {/* Breadcrumb */}
          {activeTab && (
            <div className="px-3 py-1 bg-zinc-900 border-b border-zinc-800 text-[10px] text-zinc-500 truncate">
              {currentPath ? `${currentPath} / ` : ''}{activeTab.name}{activeTab.isDirty ? ' ●' : ''}
            </div>
          )}
          <TabBarWithMenu
            tabs={tabs}
            activeTabId={activeTabId || ''}
            onTabSelect={setActiveTabId}
            onTabClose={handleTabClose}
            onCloseOthers={handleCloseOthers}
            onCloseRight={handleCloseRight}
            onCloseAll={handleCloseAll}
            onCopyPath={handleCopyPath}
            onRevealInExplorer={handleRevealInExplorer}
          />
          <div data-tour="editor" className="flex-1 flex flex-col min-h-0">
            {activeTab ? (
              <CodeEditor
                value={activeTab.content}
                onChange={(content) => updateTabContent(activeTab.id, content)}
                onRun={handleRun}
                onCursorChange={setCursorPos}
                fontSize={settings.fontSize}
                tabSize={settings.tabSize}
                autoClose={settings.autoClose}
              />
            ) : (
              <div className="flex-1 flex flex-col items-center justify-center text-zinc-600 text-sm space-y-4 cursor-pointer" onClick={handleNewFile}>
                <div className="text-4xl text-emerald-500/30">R</div>
                <div>No file open</div>
                <div className="text-xs text-zinc-700">Click to create a new file, or press Ctrl+P to search</div>
                <div className="flex gap-2 mt-2">
                  <button onClick={(e) => { e.stopPropagation(); handleNewScratchFile(); }} className="px-3 py-1 bg-zinc-800 hover:bg-zinc-700 rounded text-xs">New Scratch</button>
                  <button onClick={(e) => { e.stopPropagation(); setQuickOpen(true); }} className="px-3 py-1 bg-zinc-800 hover:bg-zinc-700 rounded text-xs">Quick Open</button>
                </div>
              </div>
            )}
          </div>

          {/* Terminal */}
          {terminalOpen && (
            <>
              <div
                className="h-1 cursor-row-resize bg-zinc-800 hover:bg-emerald-500 transition-colors flex-shrink-0"
                onMouseDown={(e) => { resizeRef.current = { type: 'terminal' as const, startY: e.clientY, startX: 0, startH: terminalHeight, startW: 0 }; }}
              />
              <Terminal cwd={currentPath || '.'} height={terminalHeight} />
            </>
          )}
        </div>

        {/* Console */}
        {consoleOpen && (
          <div data-tour="console" className="flex flex-col w-96 border-l border-zinc-800 bg-zinc-950 flex-shrink-0">
            <div className="flex items-center justify-between px-3 py-1.5 bg-zinc-900 border-b border-zinc-800">
              <span className="text-xs text-zinc-400 font-semibold">Console</span>
              <div className="flex items-center gap-2">
                <button onClick={handleClearConsole} className="text-xs text-zinc-500 hover:text-zinc-300" title="Clear">Clear</button>
              </div>
            </div>
            <div ref={consoleRef} className="flex-1 overflow-y-auto p-3 space-y-0.5" onContextMenu={(e) => {
              e.preventDefault();
              navigator.clipboard.readText().then(() => {});
            }}>
              {consoleOutput.length === 0 && <div className="text-xs text-zinc-600 italic">Ready to run Rak code...</div>}
              {consoleOutput.map((line, i) => (
                <div key={i} className={`text-xs font-mono ${line.type === 'error' ? 'text-red-400' : getLineClass(line.content)}`}>{line.content}</div>
              ))}
            </div>
          </div>
        )}
      </div>

      {/* Status Bar */}
      <div className="flex items-center justify-between px-4 py-1 bg-zinc-900 border-t border-zinc-800 text-[10px] text-zinc-500">
        <div className="flex items-center gap-4">
          <span className="text-emerald-500">Rak v0.4</span>
          <span>{runMode === 'vm' ? 'VM Mode' : runMode === 'bench' ? 'Bench Mode' : 'Interpreter Mode'}</span>
          {activeTab?.isDirty && <span className="text-yellow-500">Unsaved changes</span>}
          {currentPath && <span className="truncate max-w-[200px]">{currentPath}</span>}
        </div>
        <div className="flex items-center gap-4">
          <span>Ln {cursorPos.line}, Col {cursorPos.col}</span>
          <span>UTF-8</span>
          <span>Rak</span>
          <span>{activeTab?.name || 'No file'}</span>
        </div>
      </div>

      {/* Command Palette */}
      <CommandPalette open={paletteOpen} commands={commands} onClose={() => setPaletteOpen(false)} />

      {/* Quick Open */}
      <QuickOpen open={quickOpen} workspacePath={currentPath} onOpen={handleFileSelect} onClose={() => setQuickOpen(false)} />

      {/* Settings */}
      {settingsOpen && (
        <SettingsModal
          settings={settings}
          onChange={setSettings}
          onClose={() => setSettingsOpen(false)}
          onReplayTutorial={() => setTutorialOpen(true)}
        />
      )}

      {/* First-run tutorial */}
      {tutorialOpen && (
        <Tutorial steps={tutorialSteps} onClose={() => setTutorialOpen(false)} />
      )}

      {/* Toasts */}
      <div className="fixed bottom-8 right-4 z-50 space-y-2">
        {toasts.map(t => (
          <div key={t.id} className={`px-4 py-2 rounded-lg shadow-xl text-sm font-mono ${
            t.type === 'success' ? 'bg-emerald-600 text-white' : t.type === 'error' ? 'bg-red-600 text-white' : 'bg-zinc-700 text-zinc-200'
          }`}>
            {t.message}
          </div>
        ))}
      </div>
    </div>
  );
}
