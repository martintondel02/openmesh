/**
 * Compact peer + link stability card.
 *
 * Used as a clickable chip in the direct-message thread header (opens the details modal) and as
 * a strip in the node inspector. All wording comes from the shared display/radio-link helpers so
 * the quality vocabulary matches Settings and the Shell connection panel.
 */

import { useMemo } from 'react';
import type { ReactNode } from 'react';
import { ShieldCheck, Signal } from 'lucide-react';
import { useAppStore } from '../../store/app';
import type { NodeData } from '../../types/api';
import { cn, formatNodeId, formatRole, signalLabel, snrLabel } from '../../lib/utils';
import { nameWithShortId, peerLinkFacts, type PeerQualityTone } from '../../lib/display';
import { describeRadioLink, formatLastFrameAge, radioLinkText, radioLinkTone, type RadioTone } from '../../lib/radio-link';
import { CopyButton, SignalBars } from '../../components/ui/signals';

const QUALITY_DOT: Record<PeerQualityTone, string> = {
  emerald: 'bg-emerald-400',
  lime: 'bg-lime-400',
  amber: 'bg-amber-400',
  slate: 'bg-slate-500',
};

const QUALITY_TEXT: Record<PeerQualityTone, string> = {
  emerald: 'text-emerald-300',
  lime: 'text-lime-300',
  amber: 'text-amber-300',
  slate: 'text-slate-400',
};

const RADIO_DOT: Record<RadioTone, string> = {
  emerald: 'bg-emerald-400',
  amber: 'bg-amber-400',
  rose: 'bg-rose-400',
  slate: 'bg-slate-500',
};

export function PeerLinkChip({
  node,
  contactName,
  isContact,
  onClick,
  className,
}: {
  node: NodeData;
  contactName?: string | null;
  isContact?: boolean;
  onClick?: () => void;
  className?: string;
}) {
  const nowMs = Date.now();
  const facts = peerLinkFacts({
    nodeId: node.node_id,
    announcedName: node.name,
    contactName,
    isContact: isContact ?? node.is_contact,
    role: node.class,
    hops: node.hops,
    rssi: node.rssi,
    snr: node.snr,
    lastSeenMs: node.last_seen_ms,
    nowMs,
  });
  return (
    <button
      type="button"
      onClick={onClick}
      title={`${facts.name} · ${facts.quality.label} — ${facts.quality.hint}`}
      aria-label={`Peer and link details for ${facts.name}. Link quality ${facts.quality.label}. ${facts.quality.hint}`}
      className={cn(
        'focus-ring hidden items-center gap-1.5 rounded-full border border-white/[0.08] bg-white/[0.03] py-1 pl-2 pr-2.5 text-3xs text-slate-400 transition-colors hover:bg-white/[0.07] sm:inline-flex',
        className,
      )}
    >
      <span className={cn('h-1.5 w-1.5 rounded-full', QUALITY_DOT[facts.quality.tone])} aria-hidden="true" />
      <span className={cn('font-medium', QUALITY_TEXT[facts.quality.tone])}>{facts.quality.label}</span>
      <span className="text-slate-600" aria-hidden="true">·</span>
      <SignalBars rssi={node.rssi} className="h-3" />
      <span className="font-mono tabular-nums">{node.rssi === null ? '—' : `${node.rssi} dBm`}</span>
      <span className="text-slate-600" aria-hidden="true">·</span>
      <span>
        {facts.hops ?? '—'} {facts.hops === 1 ? 'hop' : 'hops'}
      </span>
    </button>
  );
}

/** Full peer + own-radio facts, used in the thread-header modal and the node inspector. */
export function PeerLinkPanel({
  node,
  contactName,
  isContact,
}: {
  node: NodeData;
  contactName?: string | null;
  isContact?: boolean;
}) {
  const hello = useAppStore((state) => state.hello);
  const bridgeStatus = useAppStore((state) => state.bridgeStatus);
  const radio = useMemo(() => describeRadioLink(hello, bridgeStatus), [hello, bridgeStatus]);
  const radioText = radioLinkText(radio);
  const nowMs = Date.now();
  const facts = peerLinkFacts({
    nodeId: node.node_id,
    announcedName: node.name,
    contactName,
    isContact: isContact ?? node.is_contact,
    role: node.class,
    hops: node.hops,
    rssi: node.rssi,
    snr: node.snr,
    lastSeenMs: node.last_seen_ms,
    nowMs,
  });
  const lastFrame = formatLastFrameAge(radio.lastRxMs, nowMs);
  const pinned = facts.isContact;

  return (
    <div className="space-y-4">
      <div className="flex items-start gap-3">
        <span className={cn('mt-1 h-2.5 w-2.5 shrink-0 rounded-full', QUALITY_DOT[facts.quality.tone])} aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <p className="flex flex-wrap items-baseline gap-x-2 text-sm font-semibold text-white">
            {facts.name}
            <span className="font-mono text-2xs font-normal text-slate-500">{facts.shortId}</span>
            {pinned ? (
              <span className="inline-flex items-center gap-1 rounded-full border border-emerald-400/25 bg-emerald-400/10 px-1.5 py-0.5 text-3xs font-medium text-emerald-300">
                <ShieldCheck className="h-2.5 w-2.5" aria-hidden="true" />
                contact
              </span>
            ) : (
              <span className="rounded-full border border-white/10 bg-white/[0.03] px-1.5 py-0.5 text-3xs text-slate-400">
                not a contact
              </span>
            )}
          </p>
          <p className={cn('mt-0.5 text-xs font-medium', QUALITY_TEXT[facts.quality.tone])}>
            {facts.quality.label} <span className="font-normal text-slate-500">— {facts.quality.hint}</span>
          </p>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-2">
        <PeerMetric label="Role" value={formatRole(facts.role)} />
        <PeerMetric label="Hops" value={`${facts.hops ?? '—'} ${facts.hops === 1 ? 'hop' : 'hops'}`} />
        <PeerMetric label="Signal">
          <span className="flex items-center gap-2">
            <SignalBars rssi={facts.rssi} />
            <span className="font-mono text-xs tabular-nums text-slate-200">{facts.rssi ?? '—'} dBm</span>
          </span>
        </PeerMetric>
        <PeerMetric label="SNR" value={snrLabel(facts.snr)} />
        <PeerMetric label="Last heard" value={`${facts.lastHeard} · ${node.last_seen_ms ? new Date(node.last_seen_ms).toLocaleString() : 'never'}`} />
        <PeerMetric label="Node id" value={formatNodeId(facts.nodeId)} mono />
      </div>

      <div className="rounded-xl border border-white/[0.06] bg-white/[0.02] p-3">
        <p className="flex items-center gap-1.5 text-3xs font-semibold uppercase tracking-[0.12em] text-slate-500">
          <RadioGlyph />
          Your radio link
        </p>
        <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
          <span className={cn('h-1.5 w-1.5 rounded-full', RADIO_DOT[radioLinkTone(radio)])} aria-hidden="true" />
          <span className="font-medium text-slate-200">{radioText.statusLabel}</span>
          {radioText.deviceLabel ? <span className="font-mono text-2xs text-slate-500">{radioText.deviceLabel}</span> : null}
          {radioText.settingsLabel ? <span className="text-2xs text-slate-500">· {radioText.settingsLabel}</span> : null}
        </div>
        <p className="mt-1 text-2xs text-slate-500">
          {lastFrame ? `Last frame ${lastFrame}.` : 'No frames heard yet.'} This link carries your peer’s messages too — a weak
          peer signal plus a busy radio can delay transfers.
        </p>
      </div>

      <div className="flex items-center gap-2">
        <CopyButton value={facts.nodeId} label="Copy node id" size="md" variant="surface" />
        <p className="text-2xs text-slate-500">Signal: {signalLabel(facts.rssi)}</p>
      </div>
    </div>
  );
}

/** Compact quality strip for the node inspector drawer (avoids duplicating the metric grid). */
export function PeerLinkQualityStrip({
  node,
  contactName,
  isContact,
}: {
  node: NodeData;
  contactName?: string | null;
  isContact?: boolean;
}) {
  const hello = useAppStore((state) => state.hello);
  const bridgeStatus = useAppStore((state) => state.bridgeStatus);
  const radio = useMemo(() => describeRadioLink(hello, bridgeStatus), [hello, bridgeStatus]);
  const radioText = radioLinkText(radio);
  const nowMs = Date.now();
  const facts = peerLinkFacts({
    nodeId: node.node_id,
    announcedName: node.name,
    contactName,
    isContact: isContact ?? node.is_contact,
    role: node.class,
    hops: node.hops,
    rssi: node.rssi,
    snr: node.snr,
    lastSeenMs: node.last_seen_ms,
    nowMs,
  });
  return (
    <div className="space-y-2 rounded-xl border border-white/[0.06] bg-white/[0.02] p-3">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
        <span className={cn('h-1.5 w-1.5 rounded-full', QUALITY_DOT[facts.quality.tone])} aria-hidden="true" />
        <span className={cn('font-medium', QUALITY_TEXT[facts.quality.tone])}>{facts.quality.label}</span>
        <span className="text-slate-600" aria-hidden="true">·</span>
        <span className="text-slate-400">{facts.lastHeard}</span>
        <span className="text-slate-600" aria-hidden="true">·</span>
        <span className={cn('inline-flex items-center gap-1', facts.isContact ? 'text-emerald-300' : 'text-slate-400')}>
          {facts.isContact ? <ShieldCheck className="h-3 w-3" aria-hidden="true" /> : null}
          {facts.isContact ? 'pinned contact' : 'not pinned'}
        </span>
      </div>
      <p className="text-2xs leading-relaxed text-slate-500">{facts.quality.hint}</p>
      <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-2xs text-slate-500">
        <span className={cn('h-1.5 w-1.5 rounded-full', RADIO_DOT[radioLinkTone(radio)])} aria-hidden="true" />
        <span className="text-slate-400">Your radio: {radioText.statusLabel}</span>
        {radioText.deviceLabel ? <span className="font-mono">{radioText.deviceLabel}</span> : null}
      </p>
      <p className="text-2xs text-slate-500">{nameWithShortId(facts.name, facts.nodeId)}</p>
    </div>
  );
}

function PeerMetric({
  label,
  value,
  mono = false,
  children,
}: {
  label: string;
  value?: string;
  mono?: boolean;
  children?: ReactNode;
}) {
  return (
    <div className="rounded-lg border border-white/[0.05] bg-white/[0.015] px-2.5 py-2">
      <p className="text-3xs uppercase tracking-[0.1em] text-slate-500">{label}</p>
      <div className={cn('mt-0.5 text-xs text-slate-200', mono && 'break-all font-mono text-2xs')}>{children ?? value}</div>
    </div>
  );
}

function RadioGlyph() {
  return <Signal className="h-3 w-3" aria-hidden="true" />;
}
