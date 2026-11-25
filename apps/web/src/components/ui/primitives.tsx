import type { ButtonHTMLAttributes, HTMLAttributes, ReactNode } from 'react';
import { Loader2 } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { cn } from '../../lib/utils';
import { HelpLink, HelpTip } from './help';

/* ------------------------------------------------------------------ */
/* Button                                                             */
/* ------------------------------------------------------------------ */

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'subtle';
export type ButtonSize = 'sm' | 'md' | 'lg';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: LucideIcon;
  iconRight?: LucideIcon;
  loading?: boolean;
  fullWidth?: boolean;
}

const BUTTON_VARIANTS: Record<ButtonVariant, string> = {
  primary:
    'bg-gradient-to-b from-emerald-400 to-emerald-500 text-emerald-950 shadow-[0_1px_0_rgba(255,255,255,0.25)_inset,0_8px_20px_-10px_rgba(16,185,129,0.8)] hover:from-emerald-300 hover:to-emerald-400',
  secondary:
    'border border-white/10 bg-white/[0.05] text-slate-200 hover:border-white/20 hover:bg-white/[0.08]',
  ghost: 'text-slate-300 hover:bg-white/[0.06] hover:text-white',
  danger:
    'border border-rose-500/25 bg-rose-500/10 text-rose-300 hover:border-rose-400/40 hover:bg-rose-500/15',
  subtle: 'bg-white/[0.03] text-slate-300 hover:bg-white/[0.06]',
};

const BUTTON_SIZES: Record<ButtonSize, string> = {
  sm: 'h-7 px-2.5 text-xs gap-1.5 rounded-lg',
  md: 'h-9 px-3.5 text-[13px] gap-2 rounded-xl',
  lg: 'h-10 px-5 text-sm gap-2 rounded-xl',
};

export function Button({
  variant = 'secondary',
  size = 'md',
  icon: Icon,
  iconRight: IconRight,
  loading = false,
  fullWidth = false,
  className,
  children,
  disabled,
  type = 'button',
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      disabled={disabled || loading}
      className={cn(
        'btn-base focus-ring',
        BUTTON_VARIANTS[variant],
        BUTTON_SIZES[size],
        fullWidth && 'w-full',
        className,
      )}
      {...rest}
    >
      {loading ? (
        <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
      ) : Icon ? (
        <Icon className={cn(size === 'sm' ? 'h-3.5 w-3.5' : 'h-4 w-4')} aria-hidden="true" />
      ) : null}
      {children}
      {IconRight ? <IconRight className="h-4 w-4" aria-hidden="true" /> : null}
    </button>
  );
}

/* ------------------------------------------------------------------ */
/* IconButton                                                         */
/* ------------------------------------------------------------------ */

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: LucideIcon;
  label: string;
  size?: 'sm' | 'md';
  variant?: 'ghost' | 'surface' | 'primary';
  active?: boolean;
}

export function IconButton({
  icon: Icon,
  label,
  size = 'md',
  variant = 'ghost',
  active = false,
  className,
  ...rest
}: IconButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={cn(
        'focus-ring inline-grid place-items-center rounded-lg transition-colors',
        size === 'sm' ? 'h-7 w-7' : 'h-8 w-8',
        variant === 'surface' &&
          'border border-white/10 bg-white/[0.04] text-slate-300 hover:border-white/20 hover:text-white',
        variant === 'ghost' && 'text-slate-400 hover:bg-white/[0.07] hover:text-slate-100',
        variant === 'primary' && 'bg-emerald-400 text-emerald-950 hover:bg-emerald-300',
        active && 'bg-emerald-400/15 text-emerald-300',
        className,
      )}
      {...rest}
    >
      <Icon className={size === 'sm' ? 'h-3.5 w-3.5' : 'h-[17px] w-[17px]'} aria-hidden="true" />
    </button>
  );
}

/* ------------------------------------------------------------------ */
/* Badge / chip                                                       */
/* ------------------------------------------------------------------ */

export type BadgeTone = 'neutral' | 'accent' | 'amber' | 'sky' | 'violet' | 'rose' | 'slate';

const BADGE_TONES: Record<BadgeTone, string> = {
  neutral: 'border-white/10 bg-white/[0.05] text-slate-300',
  accent: 'border-emerald-400/25 bg-emerald-400/10 text-emerald-300',
  amber: 'border-amber-400/25 bg-amber-400/10 text-amber-300',
  sky: 'border-sky-400/25 bg-sky-400/10 text-sky-300',
  violet: 'border-violet-400/25 bg-violet-400/10 text-violet-300',
  rose: 'border-rose-400/25 bg-rose-400/10 text-rose-300',
  slate: 'border-slate-400/20 bg-slate-400/[0.07] text-slate-400',
};

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: BadgeTone;
  icon?: LucideIcon;
  size?: 'sm' | 'md';
}

export function Badge({ tone = 'neutral', icon: Icon, size = 'sm', className, children, ...rest }: BadgeProps) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded-md border font-medium leading-none',
        size === 'sm' ? 'px-1.5 py-1 text-3xs' : 'px-2 py-1 text-2xs',
        BADGE_TONES[tone],
        className,
      )}
      {...rest}
    >
      {Icon ? <Icon className="h-3 w-3" aria-hidden="true" /> : null}
      {children}
    </span>
  );
}

/* ------------------------------------------------------------------ */
/* Avatar                                                             */
/* ------------------------------------------------------------------ */

const AVATAR_GRADIENTS = [
  'from-emerald-400/25 to-teal-500/10 text-emerald-200 border-emerald-400/25',
  'from-sky-400/25 to-blue-500/10 text-sky-200 border-sky-400/25',
  'from-violet-400/25 to-indigo-500/10 text-violet-200 border-violet-400/25',
  'from-amber-400/25 to-orange-500/10 text-amber-200 border-amber-400/25',
  'from-rose-400/25 to-pink-500/10 text-rose-200 border-rose-400/25',
  'from-cyan-400/25 to-teal-500/10 text-cyan-200 border-cyan-400/25',
];

function hashString(input: string): number {
  let hash = 0;
  for (let i = 0; i < input.length; i += 1) hash = (Math.imul(hash, 31) + input.charCodeAt(i)) >>> 0;
  return hash;
}

export interface AvatarProps {
  name: string;
  seed?: string;
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl';
  shape?: 'rounded' | 'circle';
  online?: boolean;
  className?: string;
}

const AVATAR_SIZES = {
  xs: 'h-6 w-6 text-[9px]',
  sm: 'h-8 w-8 text-[10px]',
  md: 'h-10 w-10 text-xs',
  lg: 'h-12 w-12 text-sm',
  xl: 'h-16 w-16 text-lg',
};

export function Avatar({ name, seed, size = 'md', shape = 'rounded', online, className }: AvatarProps) {
  const gradient = AVATAR_GRADIENTS[hashString(seed ?? name) % AVATAR_GRADIENTS.length]!;
  return (
    <span className={cn('relative inline-flex shrink-0', className)}>
      <span
        aria-hidden="true"
        className={cn(
          'grid place-items-center border bg-gradient-to-br font-semibold tracking-wide',
          shape === 'circle' ? 'rounded-full' : 'rounded-xl',
          AVATAR_SIZES[size],
          gradient,
        )}
      >
        {initialsOf(name)}
      </span>
      {online !== undefined ? (
        <span
          aria-label={online ? 'online' : 'offline'}
          className={cn(
            'absolute -bottom-0.5 -right-0.5 rounded-full border-2 border-ink-900',
            online ? 'bg-emerald-400' : 'bg-slate-600',
            size === 'xs' || size === 'sm' ? 'h-2.5 w-2.5' : 'h-3 w-3',
          )}
        />
      ) : null}
    </span>
  );
}

function initialsOf(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean).slice(0, 2);
  if (parts.length === 0) return '?';
  return parts.map((part) => Array.from(part)[0]?.toUpperCase() ?? '').join('');
}

/* ------------------------------------------------------------------ */
/* Toggle                                                             */
/* ------------------------------------------------------------------ */

export interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
  id?: string;
  labelAccessory?: ReactNode;
}

export function Toggle({ checked, onChange, label, description, disabled, id, labelAccessory }: ToggleProps) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="min-w-0">
        <span className="flex items-center gap-1.5">
          <label htmlFor={id} className="block cursor-pointer text-[13px] font-medium text-slate-200">
            {label}
          </label>
          {labelAccessory}
        </span>
        {description ? <p className="mt-0.5 text-xs leading-relaxed text-slate-500">{description}</p> : null}
      </div>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={cn(
          'focus-ring relative mt-0.5 h-5 w-9 shrink-0 rounded-full border transition-colors duration-200 disabled:opacity-40',
          checked ? 'border-emerald-400/40 bg-emerald-400/80' : 'border-white/10 bg-white/[0.08]',
        )}
      >
        <span
          className={cn(
            'absolute top-0.5 h-3.5 w-3.5 rounded-full bg-white shadow transition-transform duration-200',
            checked ? 'translate-x-[1.15rem]' : 'translate-x-0.5',
          )}
        />
      </button>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Progress / spinner / misc                                          */
/* ------------------------------------------------------------------ */

export function ProgressBar({
  value,
  className,
  tone = 'accent',
}: {
  value: number;
  className?: string;
  tone?: 'accent' | 'amber' | 'sky';
}) {
  const pct = Math.min(100, Math.max(0, value));
  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(pct)}
      className={cn('h-1.5 w-full overflow-hidden rounded-full bg-white/[0.07]', className)}
    >
      <div
        className={cn(
          'h-full rounded-full transition-[width] duration-300',
          tone === 'accent' && 'bg-emerald-400',
          tone === 'amber' && 'bg-amber-400',
          tone === 'sky' && 'bg-sky-400',
        )}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

export function Spinner({ className }: { className?: string }) {
  return <Loader2 className={cn('h-4 w-4 animate-spin text-slate-400', className)} aria-hidden="true" />;
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}

export function SectionTitle({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <h3 className={cn('text-2xs font-semibold uppercase tracking-[0.14em] text-slate-500', className)}>
      {children}
    </h3>
  );
}

export function EmptyState({
  icon: Icon,
  title,
  description,
  action,
  helpTopic,
  className,
}: {
  icon: LucideIcon;
  title: string;
  description?: string;
  action?: ReactNode;
  helpTopic?: string;
  className?: string;
}) {
  return (
    <div className={cn('flex flex-col items-center justify-center gap-3 px-6 py-14 text-center', className)}>
      <div className="grid h-12 w-12 place-items-center rounded-2xl border border-white/[0.07] bg-white/[0.03]">
        <Icon className="h-5 w-5 text-slate-500" aria-hidden="true" />
      </div>
      <div>
        <p className="text-sm font-medium text-slate-300">{title}</p>
        {description ? <p className="mt-1 max-w-xs text-xs leading-relaxed text-slate-500">{description}</p> : null}
      </div>
      {action}
      {helpTopic ? <HelpLink topic={helpTopic} /> : null}
    </div>
  );
}

export function SegmentedControl<T extends string>({
  value,
  options,
  onChange,
  className,
  ariaLabel,
}: {
  value: T;
  options: { value: T; label: string; icon?: LucideIcon; count?: number }[];
  onChange: (value: T) => void;
  className?: string;
  ariaLabel: string;
}) {
  return (
    <div
      role="tablist"
      aria-label={ariaLabel}
      className={cn('inline-flex items-center gap-0.5 rounded-lg border border-white/[0.07] bg-ink-900/70 p-0.5', className)}
    >
      {options.map((option) => {
        const Icon = option.icon;
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            role="tab"
            aria-selected={selected}
            type="button"
            onClick={() => onChange(option.value)}
            className={cn(
              'focus-ring inline-flex items-center gap-1.5 rounded-[7px] px-2.5 py-1 text-xs font-medium transition-colors',
              selected ? 'bg-white/[0.09] text-white shadow-sm' : 'text-slate-400 hover:text-slate-200',
            )}
          >
            {Icon ? <Icon className="h-3.5 w-3.5" aria-hidden="true" /> : null}
            {option.label}
            {option.count !== undefined ? (
              <span className={cn('text-3xs tabular-nums', selected ? 'text-slate-300' : 'text-slate-500')}>
                {option.count}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}

export function Stat({ label, value, hint, helpKey }: { label: string; value: ReactNode; hint?: string; helpKey?: string }) {
  return (
    <div className="min-w-0">
      <p className="flex items-center gap-1 text-3xs font-medium uppercase tracking-[0.14em] text-slate-500">
        {label}
        {helpKey ? <HelpTip helpKey={helpKey} label={label} /> : null}
      </p>
      <p className="mt-1 truncate text-lg font-semibold tabular-nums text-slate-100">{value}</p>
      {hint ? <p className="mt-0.5 truncate text-3xs text-slate-500">{hint}</p> : null}
    </div>
  );
}

export function Tooltip({ label, children }: { label: string; children: ReactNode }) {
  return (
    <span className="group/tooltip relative inline-flex">
      {children}
      <span
        role="tooltip"
        className="pointer-events-none absolute -top-8 left-1/2 z-50 -translate-x-1/2 whitespace-nowrap rounded-md border border-white/10 bg-ink-800 px-2 py-1 text-3xs text-slate-200 opacity-0 shadow-pop transition-opacity duration-150 group-hover/tooltip:opacity-100 group-focus-within/tooltip:opacity-100"
      >
        {label}
      </span>
    </span>
  );
}

export function Copyable({ value, className, mono = true }: { value: string; className?: string; mono?: boolean }) {
  return (
    <code
      className={cn(
        'block truncate rounded-lg border border-white/[0.07] bg-ink-950/60 px-2.5 py-1.5 text-xs text-slate-300',
        mono && 'font-mono',
        className,
      )}
    >
      {value}
    </code>
  );
}

