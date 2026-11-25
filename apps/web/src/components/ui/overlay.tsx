import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { AlertTriangle, BookOpen, CheckCircle2, Clipboard, Info, X, XCircle } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { cn } from '../../lib/utils';
import { Button, IconButton } from './primitives';
import { copyText } from './signals';
import { useAppStore } from '../../store/app';

/* ------------------------------------------------------------------ */
/* Modal                                                              */
/* ------------------------------------------------------------------ */

export type ModalSize = 'sm' | 'md' | 'lg' | 'xl';

const MODAL_WIDTHS: Record<ModalSize, string> = {
  sm: 'max-w-sm',
  md: 'max-w-lg',
  lg: 'max-w-2xl',
  xl: 'max-w-4xl',
};

export interface ModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  icon?: LucideIcon;
  children: ReactNode;
  footer?: ReactNode;
  size?: ModalSize;
  /** Prevent closing on backdrop click (e.g. while an upload is in flight). */
  dismissible?: boolean;
  className?: string;
}

export function Modal({
  open,
  onClose,
  title,
  description,
  icon: Icon,
  children,
  footer,
  size = 'md',
  dismissible = true,
  className,
}: ModalProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const previouslyFocused = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) return;
    previouslyFocused.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const timer = setTimeout(() => {
      const first = panelRef.current?.querySelector<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), textarea, select, [tabindex]:not([tabindex="-1"])',
      );
      (first ?? panelRef.current)?.focus();
    }, 30);
    return () => {
      clearTimeout(timer);
      previouslyFocused.current?.focus?.();
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && dismissible) {
        event.stopPropagation();
        onClose();
      }
      if (event.key !== 'Tab' || !panelRef.current) return;
      const focusable = Array.from(
        panelRef.current.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((el) => el.offsetParent !== null);
      if (focusable.length === 0) return;
      const first = focusable[0]!;
      const last = focusable[focusable.length - 1]!;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown, true);
    return () => document.removeEventListener('keydown', onKeyDown, true);
  }, [open, dismissible, onClose]);

  if (!open) return null;

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-end justify-center p-0 sm:items-center sm:p-6">
      <div
        aria-hidden="true"
        onClick={() => dismissible && onClose()}
        className="absolute inset-0 bg-ink-950/75 backdrop-blur-[3px] animate-fade-in"
      />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        className={cn(
          'relative z-10 flex max-h-[92vh] w-full flex-col overflow-hidden rounded-t-2xl border border-white/[0.09] bg-ink-875 shadow-pop animate-slide-up sm:rounded-2xl',
          MODAL_WIDTHS[size],
          className,
        )}
      >
        <header className="flex items-start gap-3 border-b border-white/[0.06] px-5 py-4">
          {Icon ? (
            <div className="mt-0.5 grid h-8 w-8 shrink-0 place-items-center rounded-lg border border-emerald-400/20 bg-emerald-400/10 text-emerald-300">
              <Icon className="h-4 w-4" aria-hidden="true" />
            </div>
          ) : null}
          <div className="min-w-0 flex-1">
            <h2 className="text-[15px] font-semibold tracking-tight text-white">{title}</h2>
            {description ? <p className="mt-0.5 text-xs leading-relaxed text-slate-400">{description}</p> : null}
          </div>
          <IconButton icon={X} label="Close dialog" size="sm" onClick={onClose} />
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
        {footer ? (
          <footer className="flex items-center justify-end gap-2 border-t border-white/[0.06] bg-ink-900/60 px-5 py-3.5">
            {footer}
          </footer>
        ) : null}
      </div>
    </div>,
    document.body,
  );
}

/* ------------------------------------------------------------------ */
/* Drawer (right sheet on desktop, bottom sheet on mobile)            */
/* ------------------------------------------------------------------ */

export interface DrawerProps {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: ReactNode;
  footer?: ReactNode;
  widthClassName?: string;
}

export function Drawer({ open, onClose, title, description, children, footer, widthClassName = 'sm:max-w-[420px]' }: DrawerProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const timer = setTimeout(() => panelRef.current?.focus(), 20);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKeyDown, true);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('keydown', onKeyDown, true);
      previouslyFocused?.focus?.();
    };
  }, [open, onClose]);

  if (!open) return null;
  return createPortal(
    <div className="fixed inset-0 z-50">
      <div
        aria-hidden="true"
        onClick={onClose}
        className="absolute inset-0 bg-ink-950/70 backdrop-blur-[2px] animate-fade-in"
      />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        className={cn(
          'absolute inset-x-0 bottom-0 flex max-h-[88vh] flex-col overflow-hidden rounded-t-2xl border border-white/[0.09] bg-ink-875 shadow-pop animate-slide-up',
          'sm:inset-y-0 sm:left-auto sm:right-0 sm:max-h-none sm:rounded-none sm:rounded-l-2xl sm:border-y-0 sm:border-r-0 sm:animate-slide-in-right',
          widthClassName,
          'sm:w-full',
        )}
      >
        <header className="flex items-start gap-3 border-b border-white/[0.06] px-5 py-4">
          <div className="min-w-0 flex-1">
            <h2 className="text-[15px] font-semibold tracking-tight text-white">{title}</h2>
            {description ? <p className="mt-0.5 text-xs leading-relaxed text-slate-400">{description}</p> : null}
          </div>
          <IconButton icon={X} label="Close panel" size="sm" onClick={onClose} />
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
        {footer ? <footer className="border-t border-white/[0.06] bg-ink-900/60 px-5 py-3.5">{footer}</footer> : null}
      </div>
    </div>,
    document.body,
  );
}

/* ------------------------------------------------------------------ */
/* Confirm dialog                                                     */
/* ------------------------------------------------------------------ */

export interface ConfirmDialogProps {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  description: string;
  confirmLabel?: string;
  cancelLabel?: string;
  tone?: 'danger' | 'primary';
  icon?: LucideIcon;
  busy?: boolean;
  children?: ReactNode;
}

export function ConfirmDialog({
  open,
  onClose,
  onConfirm,
  title,
  description,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  tone = 'danger',
  icon: Icon,
  busy,
  children,
}: ConfirmDialogProps) {
  return (
    <Modal
      open={open}
      onClose={onClose}
      title={title}
      description={description}
      icon={Icon}
      size="sm"
      dismissible={!busy}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            {cancelLabel}
          </Button>
          <Button
            variant={tone === 'danger' ? 'danger' : 'primary'}
            onClick={onConfirm}
            loading={busy}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children}
    </Modal>
  );
}

/* ------------------------------------------------------------------ */
/* Toasts                                                             */
/* ------------------------------------------------------------------ */

export interface ToastItem {
  id: string;
  kind: 'info' | 'success' | 'error' | 'warn';
  title: string;
  message?: string;
  helpTopic?: string;
  diagnostics?: Record<string, unknown>;
}

const TOAST_ICONS: Record<ToastItem['kind'], LucideIcon> = {
  info: Info,
  success: CheckCircle2,
  error: XCircle,
  warn: AlertTriangle,
};

const TOAST_TONES: Record<ToastItem['kind'], string> = {
  info: 'border-sky-400/25 text-sky-200',
  success: 'border-emerald-400/25 text-emerald-200',
  error: 'border-rose-400/30 text-rose-200',
  warn: 'border-amber-400/30 text-amber-200',
};

export function ToastStack({ toasts, onDismiss }: { toasts: ToastItem[]; onDismiss: (id: string) => void }) {
  const openHelp = useAppStore((state) => state.openHelp);
  if (toasts.length === 0) return null;
  return createPortal(
    <div
      aria-live="polite"
      aria-atomic="false"
      className="pointer-events-none fixed inset-x-0 bottom-20 z-[60] flex flex-col items-center gap-2 px-3 sm:bottom-5 sm:left-auto sm:right-5 sm:items-end sm:px-0"
    >
      {toasts.map((toast) => (
        <ToastCard key={toast.id} toast={toast} onDismiss={onDismiss} onHelp={openHelp} />
      ))}
    </div>,
    document.body,
  );
}

function ToastCard({
  toast,
  onDismiss,
  onHelp,
}: {
  toast: ToastItem;
  onDismiss: (id: string) => void;
  onHelp: (topic?: string | null, anchor?: string | null) => void;
}) {
  const [copied, setCopied] = useState(false);
  const Icon = TOAST_ICONS[toast.kind];
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);
  const copyDiagnostics = async () => {
    const payload = {
      at: new Date().toISOString(),
      kind: toast.kind,
      title: toast.title,
      message: toast.message ?? null,
      url: typeof window !== 'undefined' ? window.location.href : null,
      ...toast.diagnostics,
    };
    const ok = await copyText(JSON.stringify(payload, null, 2));
    if (!ok) return;
    setCopied(true);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), 1600);
  };
  return (
    <div
      role="status"
      className={cn(
        'pointer-events-auto flex w-full max-w-sm items-start gap-2.5 rounded-xl border bg-ink-850/95 px-3.5 py-3 shadow-pop backdrop-blur animate-slide-up',
        TOAST_TONES[toast.kind],
      )}
    >
      <Icon className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <p className="text-xs font-semibold">{toast.title}</p>
        {toast.message ? <p className="mt-0.5 text-2xs leading-relaxed text-slate-400">{toast.message}</p> : null}
        {toast.helpTopic || toast.diagnostics ? (
          <div className="mt-1.5 flex flex-wrap items-center gap-2">
            {toast.helpTopic ? (
              <button
                type="button"
                onClick={() => onHelp(toast.helpTopic ?? null)}
                className="focus-ring inline-flex items-center gap-1 rounded-md text-2xs font-medium text-emerald-300 hover:text-emerald-200"
              >
                <BookOpen className="h-3 w-3" aria-hidden="true" />
                Get help
              </button>
            ) : null}
            {toast.diagnostics ? (
              <button
                type="button"
                onClick={() => void copyDiagnostics()}
                className="focus-ring inline-flex items-center gap-1 rounded-md text-2xs font-medium text-slate-400 hover:text-slate-200"
              >
                <Clipboard className="h-3 w-3" aria-hidden="true" />
                {copied ? 'Copied diagnostics' : 'Copy diagnostics'}
              </button>
            ) : null}
          </div>
        ) : null}
      </div>
      <button
        type="button"
        aria-label="Dismiss notification"
        onClick={() => onDismiss(toast.id)}
        className="focus-ring -mr-1 -mt-1 grid h-6 w-6 shrink-0 place-items-center rounded-md text-slate-500 hover:bg-white/[0.06] hover:text-slate-300"
      >
        <X className="h-3.5 w-3.5" aria-hidden="true" />
      </button>
    </div>
  );
}
