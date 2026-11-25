import { forwardRef, useId } from 'react';
import type { ComponentType, InputHTMLAttributes, ReactNode, SelectHTMLAttributes, TextareaHTMLAttributes } from 'react';
import { AlertCircle, Check } from 'lucide-react';
import { cn } from '../../lib/utils';

/* ------------------------------------------------------------------ */
/* Field shell                                                        */
/* ------------------------------------------------------------------ */

export interface FieldProps {
  label: string;
  hint?: string;
  error?: string | null;
  success?: string | null;
  required?: boolean;
  children: (ids: { id: string; describedBy?: string }) => ReactNode;
  className?: string;
  labelAccessory?: ReactNode;
}

export function Field({ label, hint, error, success, required, children, className, labelAccessory }: FieldProps) {
  const id = useId();
  const describedBy = error
    ? `${id}-error`
    : success
      ? `${id}-success`
      : hint
        ? `${id}-hint`
        : undefined;
  return (
    <div className={cn('space-y-1.5', className)}>
      <div className="flex items-baseline justify-between gap-2">
        <label htmlFor={id} className="text-xs font-medium text-slate-300">
          {label}
          {required ? <span className="ml-0.5 text-emerald-400">*</span> : null}
        </label>
        {labelAccessory}
      </div>
      {children({ id, describedBy })}
      {error ? (
        <p id={`${id}-error`} role="alert" className="flex items-center gap-1 text-2xs text-rose-300">
          <AlertCircle className="h-3 w-3 shrink-0" aria-hidden="true" />
          {error}
        </p>
      ) : success ? (
        <p id={`${id}-success`} className="flex items-center gap-1 text-2xs text-emerald-300">
          <Check className="h-3 w-3 shrink-0" aria-hidden="true" />
          {success}
        </p>
      ) : hint ? (
        <p id={`${id}-hint`} className="text-2xs leading-relaxed text-slate-500">
          {hint}
        </p>
      ) : null}
    </div>
  );
}

export const TextInput = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  function TextInput({ className, ...rest }, ref) {
    return <input ref={ref} className={cn('field-shell h-9 py-0', className)} {...rest} />;
  },
);

export function TextArea({ className, ...rest }: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea className={cn('field-shell min-h-[76px] resize-y leading-relaxed', className)} {...rest} />;
}

export function Select({ className, children, ...rest }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <div className="relative">
      <select className={cn('field-shell h-9 appearance-none py-0 pr-8', className)} {...rest}>
        {children}
      </select>
      <svg
        aria-hidden="true"
        viewBox="0 0 20 20"
        className="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-slate-500"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.7"
      >
        <path d="M6 8l4 4 4-4" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    </div>
  );
}

export interface SliderProps {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  label: string;
  formatValue?: (value: number) => string;
  markers?: { value: number; label: string }[];
  disabled?: boolean;
  id?: string;
  labelAccessory?: ReactNode;
}

export function Slider({ value, min, max, step = 1, onChange, label, formatValue, markers, disabled, id, labelAccessory }: SliderProps) {
  const progress = max > min ? ((value - min) / (max - min)) * 100 : 0;
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between text-xs">
        <span className="inline-flex items-center gap-1.5">
          <label htmlFor={id} className="font-medium text-slate-300">
            {label}
          </label>
          {labelAccessory}
        </span>
        <span className="font-mono text-2xs text-emerald-300">{formatValue ? formatValue(value) : value}</span>
      </div>
      <input
        id={id}
        type="range"
        className="slider"
        style={{ ['--slider-progress' as string]: `${progress}%` }}
        value={value}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        aria-label={label}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      {markers ? (
        <div className="flex justify-between text-3xs text-slate-600">
          {markers.map((marker) => (
            <span key={marker.value}>{marker.label}</span>
          ))}
        </div>
      ) : null}
    </div>
  );
}

export function Checkbox({
  checked,
  onChange,
  label,
  description,
  disabled,
  id,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
  id?: string;
}) {
  return (
    <label htmlFor={id} className={cn('flex cursor-pointer items-start gap-2.5', disabled && 'cursor-not-allowed opacity-50')}>
      <input
        id={id}
        type="checkbox"
        className="peer sr-only"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span
        aria-hidden="true"
        className={cn(
          'mt-0.5 grid h-4 w-4 shrink-0 place-items-center rounded border transition-colors',
          checked ? 'border-emerald-400 bg-emerald-400 text-emerald-950' : 'border-white/15 bg-white/[0.04]',
        )}
      >
        {checked ? <Check className="h-3 w-3" strokeWidth={3} /> : null}
      </span>
      <span className="min-w-0">
        <span className="block text-xs font-medium text-slate-300 peer-focus-visible:underline">{label}</span>
        {description ? <span className="mt-0.5 block text-2xs text-slate-500">{description}</span> : null}
      </span>
    </label>
  );
}

export function InlineCode({ children }: { children: ReactNode }) {
  return (
    <code className="rounded-md border border-white/[0.07] bg-ink-950/70 px-1.5 py-0.5 font-mono text-[11px] text-slate-300">
      {children}
    </code>
  );
}

export function Callout({
  tone = 'info',
  icon: Icon,
  title,
  children,
}: {
  tone?: 'info' | 'warn' | 'danger' | 'success';
  icon?: ComponentType<{ className?: string }>;
  title?: string;
  children: ReactNode;
}) {
  return (
    <div
      className={cn(
        'flex gap-2.5 rounded-xl border p-3 text-xs leading-relaxed',
        tone === 'info' && 'border-sky-400/20 bg-sky-400/[0.06] text-sky-100/80',
        tone === 'warn' && 'border-amber-400/25 bg-amber-400/[0.07] text-amber-100/85',
        tone === 'danger' && 'border-rose-400/25 bg-rose-400/[0.07] text-rose-100/85',
        tone === 'success' && 'border-emerald-400/25 bg-emerald-400/[0.07] text-emerald-100/85',
      )}
    >
      {Icon ? <Icon className="mt-0.5 h-4 w-4 shrink-0 opacity-80" /> : null}
      <div className="min-w-0">
        {title ? <p className="mb-0.5 font-medium">{title}</p> : null}
        {children}
      </div>
    </div>
  );
}
