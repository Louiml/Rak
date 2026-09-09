'use client';

import React, { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { FileIcon, Icon } from './Icon';

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
  onToast: (msg: string, type?: 'success' | 'error' | 'info') => void;
}

const SEP = (p: string) => (p.includes('\\') ? '\\' : '/');
const basename = (p: string) => p.split(/[\\\/]/).pop() || p;

export default function FileExplorer({ onFileSelect, currentPath, onPathChange, activeFile, onToast }: FileExplorerProps) {
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [expandedDirs, setExpandedDirs] = useState<Set<string>>(new Set());
  const [isLoading, setIsLoading] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string; isDir: boolean } | null>(null);
  const [bgContextMenu, setBgContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [bgDragOver, setBgDragOver] = useState(false);
  const [newItemMode, setNewItemMode] = useState<'file' | 'dir' | null>(null);
  const [newItemName, setNewItemName] = useState('');
  const [newItemParent, setNewItemParent] = useState('');
  const [renaming, setRenaming] = useState<{ path: string; name: string } | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);

  const refresh = useCallback(() => setRefreshKey((k) => k + 1), []);

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
  }, [currentPath, loadDir, refreshKey]);

  const toggleExpand = (path: string) => {
    setExpandedDirs((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
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
        refresh();
        onToast('Deleted', 'success');
      } catch (error) {
        onToast(`Delete failed: ${error}`, 'error');
      }
    }
    setContextMenu(null);
  };

  const handleRename = async () => {
    if (!contextMenu) return;
    setRenaming({ path: contextMenu.path, name: basename(contextMenu.path) });
    setContextMenu(null);
  };

  const submitRename = async () => {
    if (!renaming || !renaming.name.trim()) { setRenaming(null); return; }
    const parts = renaming.path.split(/[\\\/]/);
    parts[parts.length - 1] = renaming.name;
    const newPath = parts.join(SEP(renaming.path));
    try {
      await invoke('rename_file', { oldPath: renaming.path, newPath });
      refresh();
      onToast('Renamed', 'success');
    } catch (e) {
      onToast(`Rename failed: ${e}`, 'error');
    }
    setRenaming(null);
  };

  const handleDuplicate = async () => {
    if (!contextMenu) return;
    try {
      await invoke('duplicate_file', { src: contextMenu.path });
      refresh();
      onToast('Duplicated', 'success');
    } catch (e) {
      onToast(`Duplicate failed: ${e}`, 'error');
    }
    setContextMenu(null);
  };

  const handleCopyPath = async () => {
    if (!contextMenu) return;
    try { await navigator.clipboard.writeText(contextMenu.path); onToast('Path copied', 'success'); } catch {}
    setContextMenu(null);
  };

  const handleReveal = async () => {
    if (!contextMenu) return;
    const target = contextMenu.isDir ? contextMenu.path : contextMenu.path.split(/[\\\/]/).slice(0, -1).join('\\');
    try { await invoke('open_in_explorer', { path: target }); } catch (e) { onToast(`Reveal failed: ${e}`, 'error'); }
    setContextMenu(null);
  };

  const handleNewItem = (type: 'file' | 'dir', parentPath: string) => {
    setNewItemMode(type);
    setNewItemParent(parentPath);
    setNewItemName('');
    setContextMenu(null);
    setBgContextMenu(null);
  };

  const submitNewItem = async () => {
    if (!newItemName.trim()) return;
    const s = SEP(newItemParent);
    const path = `${newItemParent}${newItemParent.endsWith(s) ? '' : s}${newItemName}`;
    try {
      if (newItemMode === 'file') await invoke('create_file', { path });
      else await invoke('create_dir', { path });
      refresh();
    } catch (error) {
      onToast(`Create failed: ${error}`, 'error');
    }
    setNewItemMode(null);
  };

  // Move a dragged file/folder into destDir (via rename_file).
  const handleDrop = async (e: React.DragEvent, destDir: string) => {
    e.preventDefault();
    e.stopPropagation();
    const src = e.dataTransfer.getData('text/plain');
    if (!src) return;
    const name = basename(src);
    const s = SEP(destDir);
    const dest = `${destDir}${destDir.endsWith(s) ? '' : s}${name}`;
    if (dest === src) return; // already in that directory
    if (destDir === src || destDir.startsWith(src + '\\') || destDir.startsWith(src + '/')) {
      onToast("Can't move a folder into itself", 'error');
      return;
    }
    try {
      await invoke('rename_file', { oldPath: src, newPath: dest });
      refresh();
      onToast(`Moved ${name}`, 'success');
    } catch (err) {
      onToast(`Move failed: ${err}`, 'error');
    }
  };

  const newItemInline = newItemMode ? (
    <div className="px-2 py-1 border-b border-zinc-800">
      <div className="flex items-center gap-1">
        {newItemMode === 'file' ? <Icon name="file-plus" size={12} className="text-zinc-500" /> : <Icon name="folder-plus" size={12} className="text-zinc-500" />}
        <input
          autoFocus
          value={newItemName}
          onChange={(e) => setNewItemName(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') submitNewItem(); if (e.key === 'Escape') setNewItemMode(null); }}
          onBlur={() => { if (newItemName.trim()) submitNewItem(); else setNewItemMode(null); }}
          placeholder={newItemMode === 'file' ? 'filename.rak' : 'foldername'}
          className="flex-1 bg-zinc-800 text-zinc-200 text-xs px-1 py-0.5 rounded outline-none border border-zinc-700 focus:border-emerald-500"
        />
      </div>
    </div>
  ) : null;

  return (
    <div className="flex flex-col h-full bg-zinc-900 border-r border-zinc-800 w-64">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 bg-zinc-800 border-b border-zinc-700">
        <span className="text-xs font-semibold text-zinc-400">Explorer</span>
        <div className="flex gap-1">
          <button onClick={() => handleNewItem('file', currentPath)} className="text-zinc-400 hover:text-zinc-200 px-1" title="New File"><Icon name="file-plus" size={14} /></button>
          <button onClick={() => handleNewItem('dir', currentPath)} className="text-zinc-400 hover:text-zinc-200 px-1" title="New Folder"><Icon name="folder-plus" size={14} /></button>
          <button onClick={goUp} className="text-zinc-400 hover:text-zinc-200 px-1" title="Go Up"><Icon name="arrow-up" size={14} /></button>
          <button onClick={refresh} className="text-zinc-400 hover:text-zinc-200 px-1" title="Refresh"><Icon name="rotate-cw" size={14} /></button>
        </div>
      </div>

      {/* Path */}
      <div className="px-2 py-1 text-[10px] text-zinc-500 truncate border-b border-zinc-800">{currentPath}</div>

      {newItemInline}

      {/* Entries (tree) */}
      <div
        className={`flex-1 overflow-y-auto ${bgDragOver ? 'bg-emerald-500/5' : ''}`}
        onContextMenu={(e) => { e.preventDefault(); setBgContextMenu({ x: e.clientX, y: e.clientY }); }}
        onDragOver={(e) => { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; setBgDragOver(true); }}
        onDragLeave={(e) => { if (e.currentTarget === e.target) setBgDragOver(false); }}
        onDrop={(e) => { setBgDragOver(false); handleDrop(e, currentPath); }}
      >
        {isLoading ? (
          <div className="p-4 text-xs text-zinc-500 text-center">Loading...</div>
        ) : entries.length === 0 ? (
          <div className="p-4 text-xs text-zinc-600 text-center">No files</div>
        ) : (
          entries.map((entry) => (
            <EntryRow
              key={entry.path}
              entry={entry}
              depth={0}
              activeFile={activeFile}
              onFileSelect={onFileSelect}
              expandedDirs={expandedDirs}
              toggleExpand={toggleExpand}
              onContextMenu={handleContextMenu}
              onDrop={handleDrop}
              renaming={renaming}
              setRenaming={setRenaming}
              submitRename={submitRename}
              refreshKey={refreshKey}
            />
          ))
        )}
      </div>

      {/* Entry Context Menu */}
      {contextMenu && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => { setContextMenu(null); setBgContextMenu(null); }} />
          <div
            className="fixed z-50 bg-zinc-800 border border-zinc-700 rounded shadow-lg py-1 min-w-[160px] max-h-80 overflow-y-auto"
            style={{ left: Math.min(contextMenu.x, window.innerWidth - 180), top: Math.min(contextMenu.y, window.innerHeight - 320) }}
          >
            <button onClick={() => handleNewItem('file', contextMenu.isDir ? contextMenu.path : currentPath)} className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-300 hover:bg-emerald-600 hover:text-white"><Icon name="file-plus" size={13} className="text-zinc-400" /> New File</button>
            <button onClick={() => handleNewItem('dir', contextMenu.isDir ? contextMenu.path : currentPath)} className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-300 hover:bg-emerald-600 hover:text-white"><Icon name="folder-plus" size={13} className="text-zinc-400" /> New Folder</button>
            <div className="border-t border-zinc-700 my-1" />
            <button onClick={handleDuplicate} className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-300 hover:bg-zinc-700"><Icon name="copy" size={13} className="text-zinc-400" /> Duplicate</button>
            <button onClick={handleRename} className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-300 hover:bg-zinc-700"><Icon name="pencil" size={13} className="text-zinc-400" /> Rename</button>
            <button onClick={handleCopyPath} className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-300 hover:bg-zinc-700"><Icon name="copy" size={13} className="text-zinc-400" /> Copy Path</button>
            <button onClick={handleReveal} className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-300 hover:bg-zinc-700"><Icon name="folder-search" size={13} className="text-zinc-400" /> Reveal in Explorer</button>
            <button onClick={refresh} className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-300 hover:bg-zinc-700"><Icon name="rotate-cw" size={13} className="text-zinc-400" /> Refresh</button>
            <div className="border-t border-zinc-700 my-1" />
            <button onClick={handleDelete} className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-red-400 hover:bg-red-600 hover:text-white"><Icon name="trash" size={13} /> Delete</button>
          </div>
        </>
      )}

      {/* Background Context Menu */}
      {bgContextMenu && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setBgContextMenu(null)} onContextMenu={(e) => { e.preventDefault(); setBgContextMenu(null); }} />
          <div className="fixed z-50 bg-zinc-800 border border-zinc-700 rounded shadow-lg py-1 min-w-[160px]" style={{ left: Math.min(bgContextMenu.x, window.innerWidth - 180), top: Math.min(bgContextMenu.y, window.innerHeight - 200) }}>
            <button onClick={() => handleNewItem('file', currentPath)} className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-300 hover:bg-emerald-600 hover:text-white"><Icon name="file-plus" size={13} className="text-zinc-400" /> New File</button>
            <button onClick={() => handleNewItem('dir', currentPath)} className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-300 hover:bg-emerald-600 hover:text-white"><Icon name="folder-plus" size={13} className="text-zinc-400" /> New Folder</button>
            <div className="border-t border-zinc-700 my-1" />
            <button onClick={() => { refresh(); setBgContextMenu(null); }} className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-300 hover:bg-zinc-700"><Icon name="rotate-cw" size={13} className="text-zinc-400" /> Refresh</button>
          </div>
        </>
      )}
    </div>
  );
}

// --- Recursive tree row ---

interface EntryRowProps {
  entry: FileEntry;
  depth: number;
  activeFile: string | null;
  onFileSelect: (path: string, name: string) => void;
  expandedDirs: Set<string>;
  toggleExpand: (path: string) => void;
  onContextMenu: (e: React.MouseEvent, entry: FileEntry) => void;
  onDrop: (e: React.DragEvent, destDir: string) => void;
  renaming: { path: string; name: string } | null;
  setRenaming: (r: { path: string; name: string } | null) => void;
  submitRename: () => void;
  refreshKey: number;
}

function EntryRow(props: EntryRowProps) {
  const { entry, depth, activeFile, onFileSelect, expandedDirs, toggleExpand, onContextMenu, onDrop, renaming, setRenaming, submitRename, refreshKey } = props;
  const [children, setChildren] = useState<FileEntry[] | null>(null);
  const [over, setOver] = useState(false);
  const ghostRef = useRef<HTMLDivElement | null>(null);
  const expanded = expandedDirs.has(entry.path);

  useEffect(() => {
    if (!entry.is_dir || !expanded) return;
    let cancelled = false;
    invoke<FileEntry[]>('list_dir', { path: entry.path })
      .then((r) => { if (!cancelled) setChildren(r); })
      .catch(() => { if (!cancelled) setChildren([]); });
    return () => { cancelled = true; };
  }, [expanded, entry.path, refreshKey, entry.is_dir]);

  const isRenaming = renaming?.path === entry.path;

  const onDragStart = (e: React.DragEvent) => {
    e.dataTransfer.setData('text/plain', entry.path);
    e.dataTransfer.effectAllowed = 'move';
    const ghost = document.createElement('div');
    const iconSvg = entry.is_dir
      ? '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#38bdf8" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 20a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2z"/></svg>'
      : '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#a1a1aa" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/></svg>';
    ghost.style.cssText = 'position:fixed;top:-1000px;left:-1000px;display:flex;align-items:center;gap:6px;padding:5px 9px;background:#27272a;border:1px solid #3f3f46;border-radius:6px;font-size:12px;color:#e4e4e7;font-family:ui-monospace,monospace;pointer-events:none;z-index:9999;';
    ghost.innerHTML = `${iconSvg}<span></span>`;
    ghost.querySelector('span')!.textContent = entry.name;
    document.body.appendChild(ghost);
    ghostRef.current = ghost;
    e.dataTransfer.setDragImage(ghost, 14, 12);
  };

  const onDragEnd = () => {
    if (ghostRef.current) { document.body.removeChild(ghostRef.current); ghostRef.current = null; }
    setOver(false);
  };

  return (
    <>
      <div
        draggable
        onDragStart={onDragStart}
        onDragEnd={onDragEnd}
        onDragOver={entry.is_dir ? (e) => { e.preventDefault(); e.stopPropagation(); e.dataTransfer.dropEffect = 'move'; setOver(true); } : undefined}
        onDragLeave={() => setOver(false)}
        onDrop={entry.is_dir ? (e) => { setOver(false); onDrop(e, entry.path); } : undefined}
        onClick={() => { if (entry.is_dir) toggleExpand(entry.path); else onFileSelect(entry.path, entry.name); }}
        onContextMenu={(e) => onContextMenu(e, entry)}
        className={`flex items-center gap-1 pr-2 py-1 text-xs cursor-pointer transition-colors rounded-sm mx-1 select-none ${
          activeFile === entry.path
            ? 'bg-emerald-500/20 text-emerald-400'
            : over
              ? 'bg-emerald-500/20 text-emerald-300 ring-1 ring-emerald-500'
              : 'text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200'
        }`}
        style={{ paddingLeft: depth * 12 + 4 }}
      >
        {entry.is_dir ? (
          <span className="text-zinc-500 w-3 flex-shrink-0">
            <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={12} />
          </span>
        ) : (
          <span className="w-3 flex-shrink-0" />
        )}
        <span className="flex-shrink-0"><FileIcon name={entry.name} isDir={entry.is_dir} size={14} /></span>
        {isRenaming ? (
          <input
            autoFocus
            value={renaming!.name}
            onChange={(e) => setRenaming({ path: entry.path, name: e.target.value })}
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => { if (e.key === 'Enter') submitRename(); if (e.key === 'Escape') setRenaming(null); }}
            onBlur={submitRename}
            className="flex-1 bg-zinc-800 text-zinc-200 text-xs px-1 py-0.5 rounded outline-none border border-emerald-600 min-w-0"
          />
        ) : (
          <span className="truncate">{entry.name}</span>
        )}
      </div>
      {expanded && children && children.map((c) => (
        <EntryRow key={c.path} entry={c} depth={depth + 1} activeFile={activeFile} onFileSelect={onFileSelect} expandedDirs={expandedDirs} toggleExpand={toggleExpand} onContextMenu={onContextMenu} onDrop={onDrop} renaming={renaming} setRenaming={setRenaming} submitRename={submitRename} refreshKey={refreshKey} />
      ))}
    </>
  );
}
