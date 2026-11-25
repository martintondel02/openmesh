import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties, ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { BookOpen, HelpCircle } from 'lucide-react';
import { cn } from '../../lib/utils';
import { glossaryEntry } from '../../lib/glossary';
import { helpTipContent } from '../../lib/help-tips';
import { useAppStore } from '../../store/app';

/* ------------------------------------------------------------------ */
/* Anchored popover primitive (portal, fixed position, keyboard safe) */
/* ------------------------------------------------------------------ */

interface AnchoredState {
  open: boolean;
  style: CSSProperties;
}

const INITIAL_ANCHOR: AnchoredState = { open: false, style: { position: 'fixed', top: 0, left: 0 } };

function useAnchoredPopover() {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [anchor, setAnchor] = useState<AnchoredState>(INITIAL_ANCHOR);

  const position = useCallback(() => {
    const trigger = triggerRef.current;
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const width = Math.min(320, window.innerWidth - 24);
    const left = Math.min(Math.max(12, rect.left + rect.width / 2 - width / 2), window.innerWidth - width - 12);
    const estimatedHeight = 178;
    const top = rect.bottom + 8 + estimatedHeight > window.innerHeight && rect.top > estimatedHeight
      ? rect.top - estimatedHeight - 8
      : rect.bottom + 8;
    setAnchor({ open: true, style: { position: 'fixed', top: Math.max(12, top), left, width, zIndex: 80 } });
  }, []);

  const close = useCallback(() => setAnchor((current) => (current.open ? { ...current, open: false } : current)), []);

  useEffect(() => {
    if (!anchor.open) return;
    position();
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (triggerRef.current?.contains(target) || panelRef.current?.contains(target)) return;
      close();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        close();
        triggerRef.current?.focus();
      }
    };
    const onScroll = () => position();
    document.addEventListener('mousedown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    window.addEventListener('resize', position);
    window.addEventListener('scroll', onScroll, true);
    return () => {
      document.removeEventListener('mousedown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('resize', position);
      window.removeEventListener('scroll', onScroll, true);
    };
  }, [anchor.open, close, position]);

  useLayoutEffect(() => {
    if (!anchor.open) return;
    const panel = panelRef.current;
    if (!panel) return;
    const rect = panel.getBoundingClientRect();
    if (rect.bottom > window.innerHeight - 8) {
      const trigger = triggerRef.current;
      if (trigger) {
        const triggerRect = trigger.getBoundingClientRect();
        setAnchor((current) => ({
          ...current,
          style: { ...current.style, top: Math.max(12, triggerRect.top - rect.height - 8) },
        }));
      }
    }
  }, [anchor.open]);

  const toggle = useCallback(() => {
    setAnchor((current) => {
      if (current.open) return { ...current, open: false };
      return current;
    });
    // If the panel is not open, position it after the close state has settled.
    if (!anchor.open) {
      requestAnimationFrame(() => position());
    }
  }, [anchor.open, position]);

  return { triggerRef, panelRef, anchor, toggle, close };
}

/* ------------------------------------------------------------------ */
/* HelpTip                                                            */
/* ------------------------------------------------------------------ */

export function HelpTip({
  helpKey,
  label,
  className,
}: {
  /** Key in `src/lib/help-tips.ts`. */
  helpKey: string;
  /** Accessible label for the trigger, e.g. the field name. */
  label?: string;
  className?: string;
}) {
  const content = helpTipContent(helpKey);
  const openHelp = useAppStore((state) => state.openHelp);
  const { triggerRef, panelRef, anchor, toggle, close } = useAnchoredPopover();
  const popoverId = useId();
  if (!content) return null;
  const term = content.term ? glossaryEntry(content.term) : null;

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        aria-label={`Help: ${label ?? helpKey}`}
        aria-haspopup="dialog"
        aria-expanded={anchor.open}
        aria-describedby={anchor.open ? popoverId : undefined}
        onClick={toggle}
        className={cn(
          'focus-ring inline-grid h-4 w-4 shrink-0 place-items-center rounded-full border border-white/10 bg-white/[0.04] text-slate-500 transition-colors hover:border-emerald-400/30 hover:text-emerald-300',
          className,
        )}
      >
        <HelpCircle className="h-3 w-3" aria-hidden="true" />
      </button>
      {anchor.open
        ? createPortal(
            <div
              ref={panelRef}
              id={popoverId}
              role="dialog"
              aria-label={`Help: ${label ?? helpKey}`}
              style={anchor.style}
              className="rounded-xl border border-white/10 bg-ink-850 p-3 text-left shadow-pop animate-fade-in"
            >
              <p className="text-2xs leading-relaxed text-slate-200">{content.what}</p>
              <p className="mt-1.5 text-2xs leading-relaxed text-slate-500">{content.why}</p>
              {term ? (
                <p className="mt-2 rounded-lg border border-white/[0.06] bg-white/[0.025] px-2 py-1.5 text-3xs leading-relaxed text-slate-500">
                  <span className="font-medium text-slate-300">{term.term}:</span> {term.definition}
                </p>
              ) : null}
              <button
                type="button"
                onClick={() => {
                  close();
                  openHelp(content.topic);
                }}
                className="focus-ring mt-2.5 inline-flex items-center gap-1 rounded-md text-2xs font-medium text-emerald-300 hover:text-emerald-200"
              >
                <BookOpen className="h-3 w-3" aria-hidden="true" />
                Read more
              </button>
            </div>,
            document.body,
          )
        : null}
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Term (glossary popover)                                            */
/* ------------------------------------------------------------------ */

export function Term({ term, className }: { term: string; className?: string }) {
  const entry = glossaryEntry(term);
  const openHelp = useAppStore((state) => state.openHelp);
  const { triggerRef, panelRef, anchor, toggle, close } = useAnchoredPopover();
  const popoverId = useId();
  if (!entry) return <span className={className}>{term}</span>;
  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="dialog"
        aria-expanded={anchor.open}
        aria-describedby={anchor.open ? popoverId : undefined}
        onClick={toggle}
        className={cn(
          'focus-ring cursor-help rounded-sm decoration-slate-500/70 decoration-dotted underline-offset-2 hover:text-slate-200 hover:decoration-emerald-400/70',
          className,
        )}
      >
        {term}
      </button>
      {anchor.open
        ? createPortal(
            <div
              ref={panelRef}
              id={popoverId}
              role="dialog"
              aria-label={`Glossary: ${term}`}
              style={anchor.style}
              className="rounded-xl border border-white/10 bg-ink-850 p-3 text-left shadow-pop animate-fade-in"
            >
              <p className="text-2xs font-semibold text-slate-200">{entry.term}</p>
              <p className="mt-1 text-2xs leading-relaxed text-slate-400">{entry.definition}</p>
              <button
                type="button"
                onClick={() => {
                  close();
                  openHelp(entry.helpTopic);
                }}
                className="focus-ring mt-2 inline-flex items-center gap-1 rounded-md text-2xs font-medium text-emerald-300 hover:text-emerald-200"
              >
                <BookOpen className="h-3 w-3" aria-hidden="true" />
                Learn more
              </button>
            </div>,
            document.body,
          )
        : null}
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Small links used by empty states, banners and onboarding           */
/* ------------------------------------------------------------------ */

export function HelpLink({
  topic,
  children = 'How does this work?',
  className,
}: {
  topic: string;
  children?: ReactNode;
  className?: string;
}) {
  const openHelp = useAppStore((state) => state.openHelp);
  return (
    <button
      type="button"
      onClick={() => openHelp(topic)}
      className={cn(
        'focus-ring inline-flex items-center gap-1 rounded-md text-2xs font-medium text-slate-400 underline-offset-2 hover:text-emerald-300 hover:underline',
        className,
      )}
    >
      <BookOpen className="h-3 w-3" aria-hidden="true" />
      {children}
    </button>
  );
}

export function HelpTipLabel({
  helpKey,
  children,
  className,
}: {
  helpKey: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <span className={cn('inline-flex items-center gap-1.5', className)}>
      {children}
      <HelpTip helpKey={helpKey} label={typeof children === 'string' ? children : undefined} />
    </span>
  );
}
