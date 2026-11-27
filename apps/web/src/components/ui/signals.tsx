import { useEffect, useRef, useState } from 'react';
import {
  AlertTriangle,
  Battery,
  BatteryFull,
  BatteryLow,
  BatteryMedium,
  Bluetooth,
  Cable,
  Check,
  CheckCheck,
  Clock,
  Copy,
  HardDrive,
  MapPin,
  Monitor,
  Radio,
  Sun,
  Usb,
  Volume2,
  Wifi,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import type { Cap, DeliveryState, RoleClass } from '../../types/api';
import { batteryTone, cn, formatRole, roleColor, signalBars, signalLabel } from '../../lib/utils';
import { deliveryPresentation } from '../../lib/display';

/* ------------------------------------------------------------------ */
/* Signal bars                                                        */
/* ------------------------------------------------------------------ */

const BAR_HEIGHTS = [4, 7, 10, 13] as const;
const BAR_TONES = ['bg-slate-600', 'bg-orange-400', 'bg-amber-400', 'bg-lime-400', 'bg-emerald-400'] as const;

export function SignalBars({
  rssi,
  className,
  withLabel = false,
}: {
  rssi: number | null | undefined;
  className?: string;
  withLabel?: boolean;
}) {
  const bars = signalBars(rssi);
  return (
    <span
      className={cn('inline-flex items-end gap-1', className)}
      role="img"
      aria-label={signalLabel(rssi)}
      title={signalLabel(rssi)}
    >
      <span className="flex items-end gap-[2px]">
        {BAR_HEIGHTS.map((height, index) => (
          <span
            key={height}
            style={{ height }}
            className={cn(
              'w-[3px] rounded-[1px]',
              index < bars ? BAR_TONES[bars] : 'bg-white/[0.09]',
            )}
          />
        ))}
      </span>
      {withLabel ? <span className="font-mono text-2xs tabular-nums text-slate-400">{rssi ?? '—'} dBm</span> : null}
    </span>
  );
}

/* ------------------------------------------------------------------ */
/* Battery                                                            */
/* ------------------------------------------------------------------ */

export function BatteryIndicator({ pct, withLabel = true, className }: { pct: number | null | undefined; withLabel?: boolean; className?: string }) {
  const tone = batteryTone(pct);
  const Icon: LucideIcon =
    pct === null || pct === undefined
      ? Battery
      : pct >= 85
        ? BatteryFull
        : pct >= 40
          ? BatteryMedium
          : BatteryLow;
  const color =
    tone === 'good' ? 'text-emerald-400' : tone === 'fair' ? 'text-amber-400' : tone === 'low' ? 'text-rose-400' : 'text-slate-500';
  return (
    <span
      className={cn('inline-flex items-center gap-1', color, className)}
      title={pct === null || pct === undefined ? 'No battery telemetry' : `${pct}% battery`}
    >
      <Icon className="h-4 w-4" aria-hidden="true" />
      {withLabel ? <span className="font-mono text-2xs tabular-nums">{pct === null || pct === undefined ? '—' : `${pct}%`}</span> : null}
    </span>
  );
}

/* ------------------------------------------------------------------ */
/* Role badge                                                         */
/* ------------------------------------------------------------------ */

export function RoleBadge({ role, className, compact = false }: { role: RoleClass | string | null | undefined; className?: string; compact?: boolean }) {
  const color = roleColor(role);
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 rounded-md border border-white/[0.07] bg-white/[0.03] px-1.5 py-1 text-3xs font-medium leading-none text-slate-300',
        className,
      )}
    >
      <span className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: color }} aria-hidden="true" />
      {compact ? null : formatRole(role)}
    </span>
  );
}

/* ------------------------------------------------------------------ */
/* Capability icons                                                   */
/* ------------------------------------------------------------------ */

const CAP_META: Partial<Record<Cap | string, { icon: LucideIcon; label: string }>> = {
  gps: { icon: MapPin, label: 'GPS' },
  wifi: { icon: Wifi, label: 'Wi-Fi' },
  ethernet: { icon: Cable, label: 'Ethernet' },
  store: { icon: HardDrive, label: 'Store & forward' },
  relay: { icon: Radio, label: 'Relay' },
  ble: { icon: Bluetooth, label: 'Bluetooth' },
  usb: { icon: Usb, label: 'USB' },
  screen: { icon: Monitor, label: 'Screen' },
  audio_out: { icon: Volume2, label: 'Audio out' },
  solar: { icon: Sun, label: 'Solar' },
};

export function CapsIcons({ caps, max = 6, className }: { caps: string[] | null | undefined; max?: number; className?: string }) {
  const list = (caps ?? []).filter((cap) => CAP_META[cap]);
  const shown = list.slice(0, max);
  const hidden = list.length - shown.length;
  return (
    <span className={cn('inline-flex items-center gap-1.5 text-slate-500', className)}>
      {shown.map((cap) => {
        const meta = CAP_META[cap]!;
        const Icon = meta.icon;
        return (
          <span key={cap} title={meta.label} className="grid h-5 w-5 place-items-center rounded border border-white/[0.06] bg-white/[0.03]">
            <Icon className="h-3 w-3" aria-hidden="true" />
            <span className="sr-only">{meta.label}</span>
          </span>
        );
      })}
      {hidden > 0 ? <span className="text-3xs tabular-nums text-slate-600">+{hidden}</span> : null}
    </span>
  );
}

/* ------------------------------------------------------------------ */
/* Delivery ticks                                                     */
/* ------------------------------------------------------------------ */

export function DeliveryTicks({ state, className }: { state: DeliveryState; className?: string }) {
  const base = cn('h-3.5 w-3.5', className);
  switch (state) {
    case 'pending':
      return <Clock className={cn(base, 'text-slate-500')} aria-label="Sending" />;
    case 'sent':
      return <Check className={cn(base, 'text-slate-400')} aria-label="Sent to bridge" />;
    case 'delivered':
      return <CheckCheck className={cn(base, 'text-slate-300')} aria-label="Delivered" />;
    case 'read':
      return <CheckCheck className={cn(base, 'text-sky-400')} aria-label="Read" />;
    case 'failed':
      return <AlertTriangle className={cn(base, 'text-rose-400')} aria-label="Failed to send" />;
    default:
      return null;
  }
}

/**
 * Delivery status with an explicit label + explanation (not icon-only).
 *
 * Direct messages explain the per-recipient receipt; channel messages explain that they are
 * broadcast with no delivered/read receipt. The full sentence is also the tooltip/aria text so
 * the meaning is available without relying on icon shape or colour.
 */
export function DeliveryStatus({
  state,
  group = false,
  peerName,
  error,
  className,
}: {
  state: DeliveryState;
  group?: boolean;
  peerName?: string | null;
  error?: string | null;
  className?: string;
}) {
  const presentation = deliveryPresentation({ state, group, peerName, error });
  const toneClass = {
    slate: 'text-slate-400',
    sky: 'text-sky-300',
    emerald: 'text-emerald-300',
    amber: 'text-amber-300',
    rose: 'text-rose-300',
  }[presentation.tone];
  return (
    <span
      className={cn('inline-flex items-center gap-1 text-3xs', toneClass, className)}
      title={presentation.explanation}
      aria-label={`${presentation.label}. ${presentation.explanation}`}
    >
      <DeliveryTicks state={presentation.state} className="h-3 w-3" />
      <span>{presentation.label}</span>
    </span>
  );
}

/* ------------------------------------------------------------------ */
/* Copy button                                                        */
/* ------------------------------------------------------------------ */

export async function copyText(value: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
      return true;
    }
  } catch {
    // Fall through to the legacy path (Tauri/WebKitGTK can deny clipboard-write).
  }
  try {
    const textarea = document.createElement('textarea');
    textarea.value = value;
    textarea.setAttribute('readonly', '');
    textarea.style.position = 'fixed';
    textarea.style.opacity = '0';
    document.body.appendChild(textarea);
    textarea.select();
    const ok = document.execCommand('copy');
    document.body.removeChild(textarea);
    return ok;
  } catch {
    return false;
  }
}

export function CopyButton({
  value,
  label = 'Copy',
  copiedLabel = 'Copied',
  className,
  size = 'sm',
  variant = 'ghost',
}: {
  value: string;
  label?: string;
  copiedLabel?: string;
  className?: string;
  size?: 'sm' | 'md';
  variant?: 'ghost' | 'surface';
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);
  return (
    <button
      type="button"
      onClick={async () => {
        const ok = await copyText(value);
        if (!ok) return;
        setCopied(true);
        if (timer.current) clearTimeout(timer.current);
        timer.current = setTimeout(() => setCopied(false), 1600);
      }}
      className={cn(
        'focus-ring inline-flex shrink-0 items-center justify-center gap-1 rounded-lg border font-medium transition-colors',
        size === 'sm' ? 'h-6 px-1.5 text-3xs' : 'h-8 px-2.5 text-2xs',
        variant === 'surface'
          ? 'border-white/10 bg-white/[0.04] text-slate-300 hover:border-white/20 hover:text-white'
          : 'border-transparent text-slate-400 hover:bg-white/[0.06] hover:text-slate-200',
        className,
      )}
      aria-label={`${label} to clipboard`}
    >
      {copied ? <Check className="h-3 w-3 text-emerald-400" aria-hidden="true" /> : <Copy className="h-3 w-3" aria-hidden="true" />}
      {copied ? copiedLabel : label}
    </button>
  );
}
