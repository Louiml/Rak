'use client';

import React, { useState, useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';

export default function TitleBar() {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    win.isMaximized().then(setIsMaximized).catch(() => {});
  }, []);

  const handleMinimize = async () => {
    await getCurrentWindow().minimize();
  };

  const handleMaximize = async () => {
    const win = getCurrentWindow();
    const maximized = await win.isMaximized();
    if (maximized) {
      await win.unmaximize();
      setIsMaximized(false);
    } else {
      await win.maximize();
      setIsMaximized(true);
    }
  };

  const handleClose = async () => {
    await getCurrentWindow().close();
  };

  return (
    <div
      data-tauri-drag-region
      className="flex items-center justify-between h-8 bg-zinc-900 border-b border-zinc-800 select-none flex-shrink-0"
    >
      {/* Left: Logo + Title */}
      <div data-tauri-drag-region className="flex items-center gap-2 px-3">
        <span className="text-emerald-500 font-bold text-sm">R</span>
        <span className="text-zinc-400 text-xs">Rak IDE</span>
        <span className="text-zinc-700 text-[10px]">v0.1.0</span>
      </div>

      {/* Center: Spacer (draggable) */}
      <div data-tauri-drag-region className="flex-1 h-full" />

      {/* Right: Window Controls */}
      <div className="flex items-center h-full">
        <button
          onClick={handleMinimize}
          className="h-full w-11 flex items-center justify-center text-zinc-400 hover:bg-zinc-800 transition-colors"
          title="Minimize"
        >
          <svg width="12" height="12" viewBox="0 0 12 12">
            <rect x="1" y="5.5" width="10" height="1" fill="currentColor" />
          </svg>
        </button>
        <button
          onClick={handleMaximize}
          className="h-full w-11 flex items-center justify-center text-zinc-400 hover:bg-zinc-800 transition-colors"
          title={isMaximized ? "Restore" : "Maximize"}
        >
          {isMaximized ? (
            <svg width="12" height="12" viewBox="0 0 12 12">
              <rect x="2" y="3" width="7" height="7" fill="none" stroke="currentColor" strokeWidth="1" />
              <rect x="3.5" y="1.5" width="7" height="7" fill="none" stroke="currentColor" strokeWidth="1" />
            </svg>
          ) : (
            <svg width="12" height="12" viewBox="0 0 12 12">
              <rect x="1" y="1" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1" />
            </svg>
          )}
        </button>
        <button
          onClick={handleClose}
          className="h-full w-11 flex items-center justify-center text-zinc-400 hover:bg-red-600 hover:text-white transition-colors"
          title="Close"
        >
          <svg width="12" height="12" viewBox="0 0 12 12">
            <path d="M1 1 L11 11 M11 1 L1 11" stroke="currentColor" strokeWidth="1.2" />
          </svg>
        </button>
      </div>
    </div>
  );
}