'use client';

import React, { useState, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import FileExplorer from './components/FileExplorer';
import TabBar, { Tab } from './components/TabBar';
import CodeEditor from './components/CodeEditor';
import TitleBar from './components/TitleBar';
import ExamplesMenu from './components/ExamplesMenu';
import { Example } from './components/examples';

interface ConsoleOutput {
  type: 'output' | 'error';
  content: string;
  timestamp: Date;
}

type RunMode = 'interp' | 'vm' | 'bench';

const DEFAULT_CODE = `// Rak OSINT Script
use net.http
use recon.dns

let target = "192.168.1.1"
let start_port = 0x0016
let end_port = 0x0050

scan target {
    range: [start_port, end_port],
    threads: 0x64,
    timeout: 0x1388
} {
    if open {
        dump fmt("Port 0x{:04X} is open", port)
    }
}

fetch fmt("http://{}/", target) {
    method: "GET",
    headers: {
        "User-Agent": "Rak-OSINT/0.1"
    }
} {
    dump headers
    trace body
}

let signature = 0xDEADBEEF
let mask = 0xFF00FF00
let result = signature & mask

dump fmt("Masked signature: 0x{:08X}", result)
`;

export default function IDE() {
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [currentPath, setCurrentPath] = useState<string>('');
  const [workspaceOpen, setWorkspaceOpen] = useState(false);
  const [consoleOutput, setConsoleOutput] = useState<ConsoleOutput[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  const [runMode, setRunMode] = useState<RunMode>('interp');
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [consoleOpen, setConsoleOpen] = useState(true);
  const consoleRef = useRef<HTMLDivElement>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  const activeTab = tabs.find((t) => t.id === activeTabId) || null;

  useEffect(() => {
    const setup = async () => {
      const u1 = await listen<{ stream: string; text: string }>('rak-output', (e) => {
        const isErr = e.payload.stream === 'stderr';
        setConsoleOutput((prev) => [
          ...prev,
          { type: isErr ? 'error' : 'output', content: e.payload.text, timestamp: new Date() },
        ]);
      });
      const u2 = await listen('rak-done', () => {
        setIsRunning(false);
        setConsoleOutput((prev) => [
          ...prev,
          { type: 'output', content: '> Finished', timestamp: new Date() },
        ]);
      });
      unlistenRef.current = () => { u1(); u2(); };
    };
    setup();
    return () => { unlistenRef.current?.(); };
  }, []);

  useEffect(() => {
    if (consoleRef.current) {
      consoleRef.current.scrollTop = consoleRef.current.scrollHeight;
    }
  }, [consoleOutput]);

  const handleOpenFolder = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Open Workspace Folder',
      });
      if (typeof selected === 'string' && selected) {
        setCurrentPath(selected);
        setWorkspaceOpen(true);
        setTabs([]);
        setActiveTabId(null);
      }
    } catch (error) {
      console.error('Failed to open folder:', error);
    }
  };

  const handleNewFile = async () => {
    if (!currentPath) return;
    const name = prompt('File name:', 'untitled.rak');
    if (!name) return;
    const path = `${currentPath}/${name}`;
    try {
      await invoke('create_file', { path });
      const newTab: Tab = {
        id: path,
        name,
        path,
        content: '// New Rak script\n',
        isDirty: false,
      };
      setTabs((prev) => [...prev, newTab]);
      setActiveTabId(path);
    } catch (error) {
      setConsoleOutput((prev) => [
        ...prev,
        { type: 'error', content: `Create error: ${error}`, timestamp: new Date() },
      ]);
    }
  };

  const handleNewScratchFile = () => {
    const id = `scratch_${Date.now()}`;
    const newTab: Tab = {
      id,
      name: 'untitled.rak',
      path: '',
      content: '// Rak scratch file\ndump "Hello, World"\n',
      isDirty: false,
    };
    setTabs((prev) => [...prev, newTab]);
    setActiveTabId(id);
  };

  const updateTabContent = (id: string, content: string) => {
    setTabs((prev) =>
      prev.map((t) =>
        t.id === id ? { ...t, content, isDirty: t.content !== content ? true : t.isDirty } : t
      )
    );
  };

  const handleFileSelect = async (path: string, name: string) => {
    const existing = tabs.find((t) => t.path === path);
    if (existing) {
      setActiveTabId(existing.id);
      return;
    }

    try {
      const content: string = await invoke('read_file', { path });
      const newTab: Tab = {
        id: path,
        name,
        path,
        content,
        isDirty: false,
      };
      setTabs((prev) => [...prev, newTab]);
      setActiveTabId(path);
    } catch (error) {
      setConsoleOutput((prev) => [
        ...prev,
        { type: 'error', content: `Error opening file: ${error}`, timestamp: new Date() },
      ]);
    }
  };

  const handleTabClose = (id: string) => {
    const tab = tabs.find((t) => t.id === id);
    if (tab?.isDirty && !confirm(`Close ${tab.name} without saving?`)) return;

    setTabs((prev) => {
      const filtered = prev.filter((t) => t.id !== id);
      if (filtered.length === 0) {
        setActiveTabId(null);
      } else if (id === activeTabId) {
        setActiveTabId(filtered[0].id);
      }
      return filtered;
    });
  };

  const handleSave = async () => {
    if (!activeTab) return;
    try {
      await invoke('save_file', { path: activeTab.path, content: activeTab.content });
      setTabs((prev) =>
        prev.map((t) => (t.id === activeTab.id ? { ...t, isDirty: false } : t))
      );
      setConsoleOutput((prev) => [
        ...prev,
        { type: 'output', content: `> Saved ${activeTab.name}`, timestamp: new Date() },
      ]);
    } catch (error) {
      setConsoleOutput((prev) => [
        ...prev,
        { type: 'error', content: `Save error: ${error}`, timestamp: new Date() },
      ]);
    }
  };

  const handleRun = async () => {
    if (!activeTab) return;
    setIsRunning(true);
    setConsoleOutput((prev) => [
      ...prev,
      { type: 'output', content: `> Running ${activeTab.name} (${runMode})...`, timestamp: new Date() },
    ]);

    try {
      await invoke('run_rak', { mode: runMode, source: activeTab.content });
    } catch (error) {
      setConsoleOutput((prev) => [
        ...prev,
        { type: 'error', content: String(error), timestamp: new Date() },
      ]);
      setIsRunning(false);
    }
  };

  const handleStop = async () => {
    try {
      await invoke('stop_rak');
      setConsoleOutput((prev) => [
        ...prev,
        { type: 'error', content: '> Stopped', timestamp: new Date() },
      ]);
    } catch (error) {
      setConsoleOutput((prev) => [
        ...prev,
        { type: 'error', content: `Stop error: ${error}`, timestamp: new Date() },
      ]);
    }
    setIsRunning(false);
  };

  const handleOpenExample = (ex: Example) => {
    const id = `ex_${ex.filename}_${Date.now()}`;
    const newTab: Tab = {
      id,
      name: ex.filename,
      path: '',
      content: ex.source,
      isDirty: false,
    };
    setTabs((prev) => [...prev, newTab]);
    setActiveTabId(id);
  };

  const handleClearConsole = () => setConsoleOutput([]);

  const getLineClass = (content: string) => {
    if (content.startsWith('[SCAN]')) return 'text-cyan-400';
    if (content.startsWith('[FETCH]')) return 'text-blue-400';
    if (content.startsWith('[DUMP]')) return 'text-yellow-400';
    if (content.startsWith('[TRACE]')) return 'text-purple-400';
    if (content.startsWith('>')) return 'text-zinc-500';
    return 'text-zinc-300';
  };

  // Welcome screen when no workspace is open
  if (!workspaceOpen) {
    return (
      <div className="flex flex-col h-screen bg-zinc-950 text-zinc-100 font-mono overflow-hidden">
        <TitleBar />
        <div className="flex flex-1 items-center justify-center">
          <div className="text-center space-y-6">
            <div className="space-y-2">
              <h1 className="text-5xl font-bold text-emerald-400">Rak</h1>
              <p className="text-zinc-500 text-sm">Programming Language for Hackers & OSINT Investigators</p>
            </div>
            <div className="space-y-3">
              <button
                onClick={handleOpenFolder}
                className="px-8 py-3 bg-emerald-600 hover:bg-emerald-500 text-white font-semibold rounded-lg transition-colors text-sm"
              >
                Open Folder
              </button>
              <div>
                <button
                  onClick={() => {
                    setCurrentPath('');
                    setWorkspaceOpen(true);
                    handleNewScratchFile();
                  }}
                  className="px-8 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg transition-colors text-sm"
                >
                  New Scratch File
                </button>
              </div>
            </div>
            <div className="text-xs text-zinc-600 max-w-md mx-auto">
              English keywords, first-class hexadecimal, real networking. Built for reconnaissance, scanning, and security research.
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-screen bg-zinc-950 text-zinc-100 font-mono overflow-hidden">
      {/* Custom Title Bar */}
      <TitleBar />

      {/* Menu Bar */}
      <div className="flex items-center justify-between px-4 py-1.5 bg-zinc-900 border-b border-zinc-800 text-xs">
        <div className="flex items-center gap-1">
          <button
            onClick={() => setSidebarOpen(!sidebarOpen)}
            className="px-2 py-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded"
            title="Toggle Explorer"
          >
            Explorer
          </button>
          <button
            onClick={handleNewFile}
            className="px-2 py-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded"
            title="New File"
          >
            New File
          </button>
          <button
            onClick={handleOpenFolder}
            className="px-2 py-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded"
            title="Open Folder"
          >
            Open Folder
          </button>
          <button
            onClick={handleSave}
            className="px-3 py-1 text-zinc-300 hover:bg-zinc-800 rounded"
          >
            Save
          </button>
          <ExamplesMenu onOpen={handleOpenExample} />
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={() => setConsoleOpen(!consoleOpen)}
            className="px-2 py-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded"
            title="Toggle Console"
          >
            Console
          </button>
          <select
            value={runMode}
            onChange={(e) => setRunMode(e.target.value as RunMode)}
            disabled={isRunning}
            title="Run mode"
            className="ml-2 bg-zinc-800 text-zinc-300 text-xs px-2 py-1 rounded border border-zinc-700 outline-none focus:border-emerald-500 disabled:opacity-50"
          >
            <option value="interp">Interpreter</option>
            <option value="vm">VM</option>
            <option value="bench">Bench</option>
          </select>
          {isRunning && (
            <button
              onClick={handleStop}
              className="px-4 py-1 bg-red-600 hover:bg-red-500 text-white font-semibold rounded flex items-center gap-2 ml-1"
              title="Stop"
            >
              Stop
            </button>
          )}
          <button
            onClick={handleRun}
            disabled={isRunning || !activeTab}
            className="px-4 py-1 bg-emerald-600 hover:bg-emerald-500 disabled:bg-zinc-700 text-white font-semibold rounded flex items-center gap-2 ml-1"
          >
            {isRunning ? (
              <>
                <div className="w-3 h-3 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                Running...
              </>
            ) : (
              <>Run</>
            )}
          </button>
        </div>
      </div>

      {/* Main Content */}
      <div className="flex flex-1 overflow-hidden">
        {/* File Explorer */}
        {sidebarOpen && (
          <FileExplorer
            onFileSelect={handleFileSelect}
            currentPath={currentPath}
            onPathChange={setCurrentPath}
            activeFile={activeTab?.path || null}
          />
        )}

        {/* Editor Area */}
        <div className="flex flex-1 flex-col overflow-hidden">
          <TabBar
            tabs={tabs}
            activeTabId={activeTabId || ''}
            onTabSelect={setActiveTabId}
            onTabClose={handleTabClose}
          />
          {activeTab ? (
            <CodeEditor
              value={activeTab.content}
              onChange={(content) => updateTabContent(activeTab.id, content)}
              onRun={handleRun}
            />
          ) : (
            <div
              className="flex-1 flex flex-col items-center justify-center text-zinc-600 text-sm space-y-4 cursor-pointer"
              onClick={handleNewFile}
            >
              <div className="text-4xl">R</div>
              <div>No file open</div>
              <div className="text-xs text-zinc-700">Click to create a new file</div>
            </div>
          )}
        </div>

        {/* Console */}
        {consoleOpen && (
          <div className="flex flex-col w-96 border-l border-zinc-800 bg-zinc-950">
            <div className="flex items-center justify-between px-3 py-1.5 bg-zinc-900 border-b border-zinc-800">
              <span className="text-xs text-zinc-400 font-semibold">Console</span>
              <div className="flex items-center gap-2">
                <button
                  onClick={handleClearConsole}
                  className="text-xs text-zinc-500 hover:text-zinc-300"
                >
                  Clear
                </button>
              </div>
            </div>
            <div ref={consoleRef} className="flex-1 overflow-y-auto p-3 space-y-0.5">
              {consoleOutput.length === 0 && (
                <div className="text-xs text-zinc-600 italic">Ready to run Rak code...</div>
              )}
              {consoleOutput.map((line, i) => (
                <div key={i} className={`text-xs font-mono ${line.type === 'error' ? 'text-red-400' : getLineClass(line.content)}`}>
                  {line.content}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>

      {/* Status Bar */}
      <div className="flex items-center justify-between px-4 py-1 bg-zinc-900 border-t border-zinc-800 text-[10px] text-zinc-500">
        <div className="flex items-center gap-4">
          <span className="text-emerald-500">Rak v0.2</span>
          <span>{runMode === 'vm' ? 'VM Mode' : runMode === 'bench' ? 'Bench Mode' : 'Interpreter Mode'}</span>
          {activeTab?.isDirty && <span className="text-yellow-500">Unsaved changes</span>}
          {currentPath && <span className="truncate max-w-[300px]">{currentPath}</span>}
        </div>
        <div className="flex items-center gap-4">
          <span>UTF-8</span>
          <span>Rak</span>
          <span>{activeTab?.name || 'No file'}</span>
        </div>
      </div>
    </div>
  );
}