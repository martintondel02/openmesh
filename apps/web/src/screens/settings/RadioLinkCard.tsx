import type { ReactNode } from 'react';
import { AlertTriangle, CheckCircle2, Loader2, Radio, RefreshCw, WifiOff } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useAppStore } from '../../store/app';
import { Badge, SectionTitle } from '../../components/ui/primitives';
import { HelpLink, HelpTip, Term } from '../../components/ui/help';
import { SignalBars } from '../../components/ui/signals';
import { cn, formatFrequency, formatPct, signalLabel, snrLabel } from '../../lib/utils';
import {
  describeRadioLink,
  formatLastFrameAge,
  isRadioLinkUnavailable,
  radioGuidance,
  radioLinkText,
  radioLinkTone,
  radioStateTone,
  reconnectAttemptLabel,
  transportState,
  transportStateLabel,
} from '../../lib/radio-link';
import type { RadioLinkState, RadioTone } from '../../lib/radio-link';

const TONE_STYLES: Record<RadioTone, { iconBox: string; text: string }> = {
  emerald: {
    iconBox: 'border-emerald-400/25 bg-emerald-400/10 text-emerald-300',
    text: 'text-emerald-300',
  },
  amber: {
    iconBox: 'border-amber-400/25 bg-amber-400/10 text-amber-300',
    text: 'text-amber-300',
  },
  rose: {
    iconBox: 'border-rose-400/25 bg-rose-400/10 text-rose-300',
    text: 'text-rose-300',
  },
  slate: {
    iconBox: 'border-white/10 bg-white/[0.04] text-slate-400',
    text: 'text-slate-400',
  },
};

const STATE_ICONS: Record<RadioLinkState, { icon: LucideIcon; spin?: boolean }> = {
  connected: { icon: CheckCircle2 },
  connecting: { icon: Loader2, spin: true },
  reconnecting: { icon: RefreshCw, spin: true },
  disconnected: { icon: WifiOff },
  virtual: { icon: Radio },
  unknown: { icon: Radio },
};

const GUIDANCE_STYLES: Record<'amber' | 'rose' | 'slate', { box: string; text: string; link: string }> = {
  amber: {
    box: 'border-amber-400/20 bg-amber-400/[0.05]',
    text: 'text-amber-100/85',
    link: 'text-amber-200/90 hover:text-amber-100',
  },
  rose: {
    box: 'border-rose-400/20 bg-rose-400/[0.05]',
    text: 'text-rose-100/85',
    link: 'text-rose-200/90 hover:text-rose-100',
  },
  slate: {
    box: 'border-white/[0.06] bg-white/[0.02]',
    text: 'text-slate-400',
    link: 'text-slate-400',
  },
};

function Stat({
  label,
  children,
  hint,
  className,
}: {
  label: ReactNode;
  children: ReactNode;
  hint?: string;
  className?: string;
}) {
  return (
    <div className={cn('min-w-[8.5rem] flex-1 rounded-lg border border-white/[0.05] bg-white/[0.02] px-2 py-1.5', className)} title={hint}>
      <dt className="flex items-center gap-1 text-3xs text-slate-500">{label}</dt>
      <dd className="mt-0.5 text-2xs text-slate-300">{children}</dd>
    </div>
  );
}

/** Signal bars plus the last-frame age, kept together so stale data is visible. */
function SignalValue({
  rssi,
  snr,
  beginner,
  lastFrame,
}: {
  rssi: number | null;
  snr: number | null;
  beginner: boolean;
  lastFrame: string | null;
}) {
  return (
    <>
      <span className="flex flex-wrap items-center gap-1.5">
        <SignalBars rssi={rssi} />
        <span className="truncate">{rssi === null ? 'No frame received yet' : signalLabel(rssi)}</span>
        {!beginner && snr !== null ? <span className="font-mono text-3xs text-slate-500">SNR {snrLabel(snr)}</span> : null}
      </span>
      {lastFrame ? <span className="mt-0.5 block text-3xs text-slate-500">last frame heard {lastFrame}</span> : null}
    </>
  );
}

/**
 * Radio link status card used in Settings (beginner "Radio coverage" and
 * advanced "Radio & airtime"). Shows the link state, the bridge-reported
 * transport, live stats and beginner guidance for every state. State is always
 * spelled out in words, not colour alone.
 */
export function RadioLinkCard({ variant = 'advanced' }: { variant?: 'beginner' | 'advanced' }) {
  const hello = useAppStore((state) => state.hello);
  const bridgeStatus = useAppStore((state) => state.bridgeStatus);
  const link = describeRadioLink(hello, bridgeStatus);
  const beginner = variant === 'beginner';
  const text = radioLinkText(link, { beginner });
  const tone = radioLinkTone(link);
  const styles = TONE_STYLES[tone];
  const { icon: StatusIcon, spin } = STATE_ICONS[link.state];
  const radio = bridgeStatus?.radio ?? null;
  const guidance = radioGuidance(link);
  const attemptLabel = reconnectAttemptLabel(link);
  const lastFrame = formatLastFrameAge(link.lastRxMs, Date.now());
  const radios = hello?.radios ?? [];
  const showHardwareStats = link.state === 'connected' && radio !== null;
  const showVirtualStats = link.state === 'virtual' && radio !== null;
  const unavailable = isRadioLinkUnavailable(link.state);
  const guidanceStyle = GUIDANCE_STYLES[link.state === 'disconnected' ? 'rose' : unavailable ? 'amber' : 'slate'];

  return (
    <section aria-label="Radio link status" className="rounded-xl border border-white/[0.06] bg-white/[0.02] p-3">
      <div className="flex items-start gap-2.5">
        <span className={cn('grid h-8 w-8 shrink-0 place-items-center rounded-lg border', styles.iconBox)}>
          <StatusIcon className={cn('h-4 w-4', spin && 'animate-spin')} aria-hidden="true" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <p role="status" className={cn('text-xs font-semibold', styles.text)}>
              {text.statusLabel}
            </p>
            {text.radioCountLabel ? (
              <Badge tone={unavailable ? (link.state === 'disconnected' ? 'rose' : 'amber') : 'neutral'}>
                {text.radioCountLabel}
              </Badge>
            ) : null}
          </div>
          {text.deviceLabel ? (
            <p className="mt-0.5 truncate font-mono text-3xs text-slate-400" title={text.deviceLabel}>
              {text.deviceLabel}
            </p>
          ) : null}
          {text.settingsLabel ? (
            <p className="mt-0.5 truncate text-3xs text-slate-500" title={text.settingsLabel}>
              {text.settingsLabel}
            </p>
          ) : null}
        </div>
        {link.state === 'connected' ? (
          <HelpLink topic="radio-and-region" className="hidden shrink-0 sm:inline-flex">
            Radio &amp; region
          </HelpLink>
        ) : null}
      </div>

      {showHardwareStats ? (
        <dl className="mt-2.5 flex flex-wrap gap-2">
          <Stat label="Signal" hint="How strongly this radio hears the mesh right now.">
            <SignalValue rssi={radio.rssi} snr={radio.snr} beginner={beginner} lastFrame={lastFrame} />
          </Stat>
          <Stat
            label={
              <>
                <Term term="Airtime" /> used
              </>
            }
            hint="Share of the time since the bridge started that this radio spent transmitting."
          >
            <span className="tabular-nums">{formatPct(radio.airtime_used_pct)}</span>
          </Stat>
          <Stat label={beginner ? 'Waiting to send' : 'TX queue'} hint="Items queued for the radio's next transmit slot; they go out automatically.">
            {radio.tx_queue === 0
              ? beginner
                ? 'Nothing waiting'
                : '0 frames'
              : beginner
                ? `${radio.tx_queue} waiting`
                : `${radio.tx_queue} ${radio.tx_queue === 1 ? 'frame' : 'frames'}`}
          </Stat>
          {!beginner ? (
            <Stat label="Frequency" hint="Effective centre frequency from the bridge's region and regulatory settings.">
              <span className="font-mono">{formatFrequency(radio.frequency_mhz)}</span>
            </Stat>
          ) : null}
          {!beginner ? (
            <Stat
              label={
                <>
                  TX power <HelpTip helpKey="regulatory.power" label="TX power" />
                </>
              }
              hint="Effective transmit power ceiling, clamped by the region."
            >
              <span className="font-mono">{radio.tx_power_dbm} dBm</span>
            </Stat>
          ) : null}
        </dl>
      ) : null}

      {showVirtualStats ? (
        <dl className="mt-2.5 flex flex-wrap gap-2">
          <Stat
            label={
              <>
                <Term term="Airtime" /> used
              </>
            }
            hint="Share of the time since the bridge started that the virtual radio spent transmitting."
          >
            <span className="tabular-nums">{formatPct(radio.airtime_used_pct)}</span>
          </Stat>
          <Stat label={beginner ? 'Waiting to send' : 'TX queue'} hint="Items queued for the radio's next transmit slot; they go out automatically.">
            {radio.tx_queue === 0
              ? beginner
                ? 'Nothing waiting'
                : '0 frames'
              : beginner
                ? `${radio.tx_queue} waiting`
                : `${radio.tx_queue} ${radio.tx_queue === 1 ? 'frame' : 'frames'}`}
          </Stat>
        </dl>
      ) : null}

      {guidance ? (
        <div className={cn('mt-2.5 rounded-lg border px-2.5 py-2', guidanceStyle.box)}>
          <p className={cn('flex items-start gap-1.5 text-2xs leading-relaxed', guidanceStyle.text)}>
            <AlertTriangle className="mt-0.5 h-3 w-3 shrink-0" aria-hidden="true" />
            <span>{guidance}</span>
          </p>
          {link.state === 'reconnecting' ? (
            <p className={cn('mt-1 pl-[18px] text-3xs font-medium', guidanceStyle.text)}>
              {attemptLabel
                ? `${attemptLabel} — the bridge retries automatically with backoff.`
                : 'The bridge retries automatically with backoff.'}
            </p>
          ) : null}
          {link.state === 'reconnecting' && lastFrame ? (
            <p className={cn('mt-0.5 pl-[18px] text-3xs', guidanceStyle.text)}>Last frame heard {lastFrame}.</p>
          ) : null}
          <div className="mt-1.5 pl-[18px]">
            <HelpLink topic={link.state === 'virtual' ? 'radio-and-region' : 'troubleshooting'} className={guidanceStyle.link}>
              {link.state === 'virtual' ? 'How virtual radios work' : 'Troubleshooting help'}
            </HelpLink>
          </div>
        </div>
      ) : null}

      {radios.length > 1 ? (
        <div className="mt-2.5 border-t border-white/[0.05] pt-2">
          <SectionTitle>Radios on this bridge</SectionTitle>
          <ul className="mt-1.5 space-y-1">
            {radios.map((item) => {
              const itemState = transportState(item);
              const itemTone = radioStateTone(itemState);
              return (
                <li key={item.id} className="flex items-center gap-1.5 text-3xs">
                  <span
                    className={cn(
                      'h-1.5 w-1.5 shrink-0 rounded-full',
                      itemTone === 'emerald' && 'bg-emerald-400',
                      itemTone === 'amber' && 'bg-amber-400',
                      itemTone === 'rose' && 'bg-rose-400',
                      itemTone === 'slate' && 'bg-slate-500',
                    )}
                    aria-hidden="true"
                  />
                  <span className="min-w-0 flex-1 truncate text-slate-400" title={item.label}>
                    {item.label}
                  </span>
                  <span
                    className={cn(
                      'shrink-0 font-medium',
                      itemTone === 'emerald' && 'text-emerald-300/80',
                      itemTone === 'amber' && 'text-amber-300/80',
                      itemTone === 'rose' && 'text-rose-300/80',
                      itemTone === 'slate' && 'text-slate-500',
                    )}
                  >
                    {transportStateLabel(item)}
                  </span>
                </li>
              );
            })}
          </ul>
        </div>
      ) : null}
    </section>
  );
}
