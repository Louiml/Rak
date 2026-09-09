'use client';

import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Icon } from './Icon';

export interface TourStep {
  /** value of the data-tour="…" attribute to highlight; omit for a centered step. */
  target?: string;
  title: string;
  body: React.ReactNode;
  /** run when the step becomes active (e.g. open a panel so the target exists). */
  onEnter?: () => void;
}

interface TutorialProps {
  steps: TourStep[];
  onClose: () => void; // called on finish or skip
}

export default function Tutorial({ steps, onClose }: TutorialProps) {
  const [step, setStep] = useState(0);
  const [, setTick] = useState(0);
  const mountedRef = useRef(false);

  const current = steps[step];
  const isLast = step === steps.length - 1;
  const next = () => (isLast ? onClose() : setStep((s) => s + 1));
  const prev = () => setStep((s) => Math.max(0, s - 1));

  // Force re-render on scroll/resize so the spotlight tracks its target.
  useEffect(() => {
    const rerender = () => setTick((t) => t + 1);
    window.addEventListener('resize', rerender);
    window.addEventListener('scroll', rerender, true);
    return () => {
      window.removeEventListener('resize', rerender);
      window.removeEventListener('scroll', rerender, true);
    };
  }, []);

  // Run the step's onEnter (after the DOM has had a chance to mount the target).
  useLayoutEffect(() => {
    if (!mountedRef.current) { mountedRef.current = true; }
    const t = setTimeout(() => { current?.onEnter?.(); setTick((x) => x + 1); }, 60);
    return () => clearTimeout(t);
  }, [step, current]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.preventDefault(); onClose(); }
      else if (e.key === 'ArrowRight') { e.preventDefault(); next(); }
      else if (e.key === 'ArrowLeft') { e.preventDefault(); prev(); }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step, next, prev]);

  if (!current) return null;

  const targetEl = current.target
    ? document.querySelector<HTMLElement>(`[data-tour="${current.target}"]`)
    : null;
  const rect = targetEl?.getBoundingClientRect();

  // Tooltip placement: below the target if there's room, else above, else centered.
  const TOOLTIP_W = 300;
  const TOOLTIP_H = 180;
  let tipStyle: React.CSSProperties;
  if (rect) {
    const left = Math.max(8, Math.min(rect.left, window.innerWidth - TOOLTIP_W - 8));
    if (rect.bottom + TOOLTIP_H + 16 < window.innerHeight) {
      tipStyle = { left, top: rect.bottom + 12, width: TOOLTIP_W };
    } else if (rect.top - TOOLTIP_H - 16 > 0) {
      tipStyle = { left, top: rect.top - TOOLTIP_H - 12, width: TOOLTIP_W };
    } else {
      tipStyle = { left: (window.innerWidth - TOOLTIP_W) / 2, top: (window.innerHeight - TOOLTIP_H) / 2, width: TOOLTIP_W };
    }
  } else {
    tipStyle = { left: (window.innerWidth - TOOLTIP_W) / 2, top: (window.innerHeight - TOOLTIP_H) / 2, width: TOOLTIP_W };
  }

  return (
    <div className="fixed inset-0 z-[70]" role="dialog" aria-label="Rak IDE tour">
      {/* Spotlight cutout via a huge box-shadow on a transparent panel over the target */}
      {rect && (
        <div
          className="fixed rounded-lg pointer-events-none"
          style={{
            left: rect.left - 4,
            top: rect.top - 4,
            width: rect.width + 8,
            height: rect.height + 8,
            boxShadow: '0 0 0 9999px rgba(0,0,0,0.72)',
            border: '1px solid rgba(16,185,129,0.7)',
          }}
        />
      )}
      {/* Full dim overlay for centered steps (no target) */}
      {!rect && <div className="fixed inset-0 bg-black/72" />}

      {/* Tooltip */}
      <div
        className="fixed z-[80] bg-zinc-900 border border-zinc-700 rounded-xl shadow-2xl p-4"
        style={tipStyle}
      >
        <div className="flex items-center justify-between mb-1">
          <span className="text-[10px] uppercase tracking-wider text-emerald-500 font-semibold">
            Step {step + 1} of {steps.length}
          </span>
          <button onClick={onClose} className="text-zinc-500 hover:text-zinc-200" title="Skip tour">
            <Icon name="x" size={14} />
          </button>
        </div>
        <h3 className="text-sm font-semibold text-zinc-100 mb-1.5">{current.title}</h3>
        <div className="text-xs text-zinc-400 leading-relaxed mb-4">{current.body}</div>
        <div className="flex items-center justify-between">
          <button
            onClick={prev}
            disabled={step === 0}
            className="text-xs px-2 py-1 text-zinc-400 hover:text-zinc-200 disabled:opacity-30 disabled:hover:text-zinc-400"
          >
            Prev
          </button>
          <div className="flex gap-1">
            {steps.map((_, i) => (
              <span
                key={i}
                className={`w-1.5 h-1.5 rounded-full ${i === step ? 'bg-emerald-500' : 'bg-zinc-700'}`}
              />
            ))}
          </div>
          <button
            onClick={next}
            className="text-xs px-3 py-1 bg-emerald-600 hover:bg-emerald-500 text-white rounded font-semibold"
          >
            {isLast ? 'Finish' : 'Next'}
          </button>
        </div>
        {step === 0 && (
          <button
            onClick={onClose}
            className="absolute -top-2.5 right-10 text-[10px] text-zinc-500 hover:text-zinc-300 bg-zinc-900 px-1.5 py-0.5 rounded"
          >
            Skip
          </button>
        )}
      </div>
    </div>
  );
}
