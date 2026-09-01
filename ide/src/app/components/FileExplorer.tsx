'use client';

import React, { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

interface FileExplorerProps {
  onFileSelect: (path: string, name: string) => void;
  currentPath: string;
  onPathChange: (path: string) => void;
  activeFile: string | null;
}

export default function FileExplorer({ onFileSelect, currentPath, onPathChange, activeFile }: FileExplorerProps) {
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [expandedDirs, setExpandedDirs] = useState<Set<string>>(new Set());
  const [isLoading, setIsLoading] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string; isDir: boolean } | null>(null);
  const [newItemMode, setNewItemMode] = useState<'file' | 'dir' | null>(null);
  const [newItemName, setNewItemName] = useState('');
  const [newItemParent, setNewItemParent] = useState('');

  const loadDir = useCallback(async (path: string) => {
    setIsLoading(true);
    try {
      const result: FileEntry[] = await invoke('list_dir', { path });
      setEntries(result);
    } catch (error) {
      console.error('Failed to load directory:', error);
    }
    setIsLoading(false);
  }, []);

  useEffect(() => {
    loadDir(currentPath);
  }, [currentPath, loadDir]);

  const handleEntryClick = (entry: FileEntry) => {
    if (entry.is_dir) {
      if (expandedDirs.has(entry.path)) {
        setExpandedDirs(prev => {
          const next = new Set(prev);
          next.delete(entry.path);
          return next;
        });
      } else {
        setExpandedDirs(prev => new Set(prev).add(entry.path));
        onPathChange(entry.path);
      }
    } else {
      onFileSelect(entry.path, entry.name);
    }
  };

  const goUp = () => {
    const parent = currentPath.split('\\').slice(0, -1).join('\\') || currentPath.split('/').slice(0, -1).join('/');
    if (parent && parent !== currentPath) {
      onPathChange(parent);
    }
  };

  const handleContextMenu = (e: React.MouseEvent, entry: FileEntry) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, path: entry.path, isDir: entry.is_dir });
  };

  const handleDelete = async () => {
    if (contextMenu && confirm(`Delete ${contextMenu.isDir ? 'directory' : 'file'}?`)) {
      try {
        await invoke('delete_file', { path: contextMenu.path });
        loadDir(currentPath);
      } catch (error) {
        console.error('Delete failed:', error);
      }
    }
    setContextMenu(null);
  };

  const handleNewItem = (type: 'file' | 'dir', parentPath: string) => {
    setNewItemMode(type);
    setNewItemParent(parentPath);
    setNewItemName('');
    setContextMenu(null);
  };

  const submitNewItem = async () => {
    if (!newItemName.trim()) return;
    const path = `${newItemParent}${newItemParent.endsWith('/') || newItemParent.endsWith('\\') ? '' : '/'}${newItemName}`;
    try {
      if (newItemMode === 'file') {
        await invoke('create_file', { path });
      } else {
        await invoke('create_dir', { path });
      }
      loadDir(currentPath);
    } catch (error) {
      console.error('Create failed:', error);
    }
    setNewItemMode(null);
  };

  const getFileIcon = (name: string, isDir: boolean) => {
    if (isDir) return '📁';
    if (name.endsWith('.rak')) return '🔴';
    if (name.endsWith('.rs')) return '🦀';
    if (name.endsWith('.js') || name.endsWith('.ts') || name.endsWith('.tsx')) return '🟨';
    if (name.endsWith('.html')) return '🟧';
    if (name.endsWith('.css')) return '🟦';
    if (name.endsWith('.json')) return '🟩';
    if (name.endsWith('.md')) return '📝';
    return '📄';
  };

  return (
    <div className="flex flex-col h-full bg-zinc-900 border-r border-zinc-800 w-64">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 bg-zinc-800 border-b border-zinc-700">
        <span className="text-xs font-semibold text-zinc-400">Explorer</span>
        <div className="flex gap-1">
          <button 
            onClick={() => handleNewItem('file', currentPath)}
            className="text-zinc-400 hover:text-zinc-200 text-xs px-1"
            title="New File"
          >
            📄+
          </button>
          <button 
            onClick={() => handleNewItem('dir', currentPath)}
            className="text-zinc-400 hover:text-zinc-200 text-xs px-1"
            title="New Folder"
          >
            📁+
          </button>
          <button 
            onClick={goUp}
            className="text-zinc-400 hover:text-zinc-200 text-xs px-1"
            title="Go Up"
          >
            ⬆️
          </button>
        </div>
      </div>

      {/* Path */}
      <div className="px-2 py-1 text-[10px] text-zinc-500 truncate border-b border-zinc-800">
        {currentPath}
      </div>

      {/* New Item Input */}
      {newItemMode && (
        <div className="px-2 py-1 border-b border-zinc-800">
          <div className="flex items-center gap-1">
            <span className="text-xs">{newItemMode === 'file' ? '📄' : '📁'}</span>
            <input
              autoFocus
              value={newItemName}
              onChange={(e) => setNewItemName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && submitNewItem()}
              onBlur={() => newItemName.trim() && submitNewItem()}
              placeholder={newItemMode === 'file' ? 'filename.rak' : 'foldername'}
              className="flex-1 bg-zinc-800 text-zinc-200 text-xs px-1 py-0.5 rounded outline-none border border-zinc-700 focus:border-emerald-500"
            />
          </div>
        </div>
      )}

      {/* Entries */}
      <div className="flex-1 overflow-y-auto">
        {isLoading ? (
          <div className="p-4 text-xs text-zinc-500 text-center">Loading...</div>
        ) : entries.length === 0 ? (
          <div className="p-4 text-xs text-zinc-600 text-center">No files</div>
        ) : (
          entries.map((entry) => (
            <div
              key={entry.path}
              onClick={() => handleEntryClick(entry)}
              onContextMenu={(e) => handleContextMenu(e, entry)}
              className={`flex items-center gap-2 px-3 py-1.5 text-xs cursor-pointer transition-colors ${
                activeFile === entry.path
                  ? 'bg-emerald-500/20 text-emerald-400'
                  : 'text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200'
              }`}
            >
              <span className="text-xs select-none">{getFileIcon(entry.name, entry.is_dir)}</span>
              <span className="truncate">{entry.name}</span>
            </div>
          ))
        )}
      </div>

      {/* Context Menu */}
      {contextMenu && (
        <>
          <div 
            className="fixed inset-0 z-40"
            onClick={() => setContextMenu(null)} 
          />
          <div
            className="fixed z-50 bg-zinc-800 border border-zinc-700 rounded shadow-lg py-1 min-w-[120px]"
            style={{ left: contextMenu.x, top: contextMenu.y }}
          >
            <button
              onClick={() => handleNewItem('file', contextMenu.isDir ? contextMenu.path : currentPath)}
              className="w-full text-left px-3 py-1.5 text-xs text-zinc-300 hover:bg-zinc-700"
            >
              New File
            </button>
            <button
              onClick={() => handleNewItem('dir', contextMenu.isDir ? contextMenu.path : currentPath)}
              className="w-full text-left px-3 py-1.5 text-xs text-zinc-300 hover:bg-zinc-700"
            >
              New Folder
            </button>
            <div className="border-t border-zinc-700 my-1" />
            <button
              onClick={handleDelete}
              className="w-full text-left px-3 py-1.5 text-xs text-red-400 hover:bg-zinc-700"
            >
              Delete
            </button>
          </div>
        </>
      )}
    </div>
  );
}