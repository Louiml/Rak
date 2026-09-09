'use client';

import React from 'react';
import { Icon, IconName } from './Icon';

export interface Settings {
  fontSize: number;
  tabSize: number;
  autoClose: boolean;
  defaultRunMode: 'interp' | 'vm' | 'bench';
  showTutorial: boolean; // re-run flag (transient; not persisted)
}

export const DEFAULT_SETTINGS: Settings = {
  fontSize: 14,
  tabSize: 4,
  autoClose: true,
  defaultRunMode: 'interp',
  showTutorial: false,
};

const STORAGE_KEY = 'rak.ide.settings';

export function loadSettings(): Settings {
  if (typeof window === 'undefined') return DEFAULT_SETTINGS;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_SETTINGS;
    const parsed = JSON.parse(raw);
    return { ...DEFAULT_SETTINGS, ...parsed, showTutorial: false };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function saveSettings(s: Settings) {
  if (typeof window === 'undefined') return;
  const { showTutorial, ...persist } = s;
  void showTutorial;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(persist));
}

interface RowProps { label: string; hint?: string; children: React.ReactNode; }

function Row({ label, hint, children }: RowProps) {
  return (
    <div className="flex items-center justify-between py-2.5 border-b border-zinc-800 last:border-0">
      <div className="flex flex-col">
        <span className="text-sm text-zinc-200">{label}</span>
        {hint && <span className="text-[11px] text-zinc-500">{hint}</span>}
      </div>
      {children}
    </div>
  );
}

function Toggle({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      onClick={() => onChange(!on)}
      className={`relative w-10 h-6 rounded-full transition-colors ${on ? 'bg-emerald-600' : 'bg-zinc-700'}`}
      role="switch"
      aria-checked={on}
    >
      <span className={`absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white transition-transform ${on ? 'translate-x-4' : ''}`} />
    </button>
  );
}

interface SettingsModalProps {
  settings: Settings;
  onChange: (s: Settings) => void;
  onClose: () => void;
  onReplayTutorial: () => void;
}

export default function SettingsModal({ settings, onChange, onClose, onReplayTutorial }: SettingsModalProps) {
  const set = (patch: Partial<Settings>) => onChange({ ...settings, ...patch });
  return (
    <>
      <div className="fixed inset-0 z-[80] bg-black/50" onClick={onClose} />
      <div className="fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-[90] w-[420px] bg-zinc-900 border border-zinc-700 rounded-xl shadow-2xl overflow-hidden">
        <div className="flex items-center justify-between px-4 py-3 border-b border-zinc-800">
          <span className="flex items-center gap-2 text-sm font-semibold text-zinc-200">
            <Icon name="gear" size={16} className="text-emerald-500" /> Settings
          </span>
          <button onClick={onClose} className="text-zinc-500 hover:text-zinc-200"><Icon name="x" size={16} /></button>
        </div>
        <div className="px-4 py-2">
          <Row label="Editor font size" hint="Pixels (10–24)">
            <div className="flex items-center gap-2">
              <input
                type="range" min={10} max={24} value={settings.fontSize}
                onChange={(e) => set({ fontSize: Number(e.target.value) })}
                className="w-32 accent-emerald-500"
              />
              <span className="text-xs text-zinc-400 w-6 text-right">{settings.fontSize}</span>
            </div>
          </Row>
          <Row label="Tab size" hint="Spaces per indent">
            <select
              value={settings.tabSize}
              onChange={(e) => set({ tabSize: Number(e.target.value) })}
              className="bg-zinc-800 text-zinc-200 text-sm px-2 py-1 rounded border border-zinc-700 outline-none focus:border-emerald-500"
            >
              <option value={2}>2 spaces</option>
              <option value={4}>4 spaces</option>
              <option value={8}>8 spaces</option>
            </select>
          </Row>
          <Row label="Auto-close brackets" hint="Insert matching pairs as you type">
            <Toggle on={settings.autoClose} onChange={(v) => set({ autoClose: v })} />
          </Row>
          <Row label="Default run mode" hint="Used when launching the IDE">
            <select
              value={settings.defaultRunMode}
              onChange={(e) => set({ defaultRunMode: e.target.value as Settings['defaultRunMode'] })}
              className="bg-zinc-800 text-zinc-200 text-sm px-2 py-1 rounded border border-zinc-700 outline-none focus:border-emerald-500"
            >
              <option value="interp">Interpreter</option>
              <option value="vm">VM</option>
              <option value="bench">Bench</option>
            </select>
          </Row>
          <Row label="Tutorial" hint="Replay the first-run tour">
            <button
              onClick={() => { onReplayTutorial(); onClose(); }}
              className="px-3 py-1 text-xs bg-zinc-800 hover:bg-zinc-700 text-zinc-200 rounded border border-zinc-700"
            >
              Replay tour
            </button>
          </Row>
        </div>
        <div className="flex justify-end gap-2 px-4 py-3 border-t border-zinc-800">
          <button onClick={onClose} className="px-4 py-1.5 text-sm bg-emerald-600 hover:bg-emerald-500 text-white rounded">Done</button>
        </div>
      </div>
    </>
  );
}

export const SETTINGS_ICON: IconName = 'gear';
