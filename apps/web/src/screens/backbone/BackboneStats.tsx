import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { Activity, ArrowDownLeft, ArrowUpRight, Database, Inbox, ShieldAlert, Users } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import type { BackboneStats } from '../../store/backbone-selectors';
import { formatBytes, cn } from '../../lib/utils';
import { formatCount, formatThroughput } from '../../lib/backbone';
import { Badge } from '../../components/ui/primitives';
import { HelpTip } from '../../components/ui/help';

interface StatCardProps {
  icon: LucideIcon;
  label: string;
  value: string;
  hint?: string;
  helpKey: string;
  tone?: 'default' | 'accent' | 'danger';
  children?: ReactNode;
}

function StatCard({ icon: Icon, label, value, hint, helpKey, tone = 'default', children }: StatCardProps) {
  return (
    <div
      className={cn(
        'flex flex-col justify-between rounded-xl border bg-white/[0.02] px-3 py-2.5',
        tone === 'accent' && 'border-emerald-400/20 bg-emerald-400/[0.04]',
        tone === 'danger' && 'border-rose-400/20 bg-rose-400/[0.04]',
        tone === 'default' && 'border-white/[0.06]',
      )}
    >
      <div className="flex items-center gap-1.5 text-3xs uppercase tracking-[0.12em] text-slate-500">
        <Icon className="h-3 w-3" aria-hidden="true" />
        <span className="truncate">{label}</span>
        <HelpTip helpKey={helpKey} label={label} />
      </div>
      <div className="mt-1.5 flex items-end justify-between gap-2">
        <p className={cn('truncate text-[17px] font-semibold leading-none tabular-nums', tone === 'danger' ? 'text-rose-200' : 'text-slate-100')}>
          {value}
        </p>
        {children}
      </div>
      {hint ? <p className="mt-1 truncate text-3xs text-slate-600">{hint}</p> : null}
    </div>
  );
}

/** Tiny inline sparkline for live throughput (no chart library). */
function Sparkline({ samples, tone }: { samples: number[]; tone: 'in' | 'out' }) {
  const width = 62;
  const height = 22;
  const max = Math.max(1, ...samples);
  const points = samples.length
    ? samples
        .map((sample, index) => {
          const x = samples.length === 1 ? width : (index / (samples.length - 1)) * width;
          const y = height - (Math.max(0, sample) / max) * (height - 3) - 1.5;
          return `${x.toFixed(1)},${y.toFixed(1)}`;
        })
        .join(' ')
    : '';
  const color = tone === 'in' ? '#34d399' : '#38bdf8';
  return (
    <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`} aria-hidden="true" className="shrink-0 overflow-visible">
      {samples.length > 1 ? (
        <polyline points={points} fill="none" stroke={color} strokeWidth={1.5} strokeLinejoin="round" strokeLinecap="round" />
      ) : (
        <line x1="0" y1={height - 1.5} x2={width} y2={height - 1.5} stroke={color} strokeOpacity={0.3} strokeWidth={1} />
      )}
    </svg>
  );
}

/**
 * Six live stat cards for the backbone. Throughput keeps a 24-sample local
 * history so the sparkline animates without extra bridge traffic.
 */
export function BackboneStats({ stats }: { stats: BackboneStats }) {
  const [inSamples, setInSamples] = useState<number[]>(() => [stats.throughputInBps]);
  const lastIn = useRef(stats.throughputInBps);

  useEffect(() => {
    if (stats.throughputInBps === lastIn.current) return;
    lastIn.current = stats.throughputInBps;
    setInSamples((samples) => [...samples.slice(-23), stats.throughputInBps]);
  }, [stats.throughputInBps]);

  return (
    <div className="grid grid-cols-2 gap-2.5 md:grid-cols-3 xl:grid-cols-6">
      <StatCard
        icon={Users}
        label="Peers"
        helpKey="backbone.stat.peers"
        value={`${stats.peers}/${stats.maxPeers}`}
        hint={stats.peers > 0 ? 'authenticated TCP links' : 'waiting for gateways'}
        tone={stats.running && stats.peers > 0 ? 'accent' : 'default'}
      />
      <StatCard
        icon={ArrowDownLeft}
        label="Frames"
        helpKey="backbone.stat.frames"
        value={`${formatCount(stats.framesIn)} / ${formatCount(stats.framesOut)}`}
        hint="frames in / out"
      />
      <StatCard
        icon={Activity}
        label="Throughput"
        helpKey="backbone.stat.throughput"
        value={formatThroughput(stats.throughputInBps)}
        hint={`in · out ${formatThroughput(stats.throughputOutBps)}`}
      >
        <span className="flex flex-col items-end gap-0.5">
          <Sparkline samples={inSamples} tone="in" />
          <span className="text-[9px] text-sky-400/70">
            <ArrowUpRight className="inline h-2 w-2" aria-hidden="true" /> {formatThroughput(stats.throughputOutBps)}
          </span>
        </span>
      </StatCard>
      <StatCard
        icon={Database}
        label="Bytes"
        helpKey="backbone.stat.bytes"
        value={formatBytes(stats.bytesIn + stats.bytesOut)}
        hint={`↓ ${formatBytes(stats.bytesIn)} · ↑ ${formatBytes(stats.bytesOut)}`}
      />
      <StatCard
        icon={Inbox}
        label="Store-forward"
        helpKey="backbone.stat.storeForward"
        value={String(stats.storeForwardEnvelopes)}
        hint="envelopes held for offline nodes"
      />
      <StatCard
        icon={ShieldAlert}
        label="Auth failures"
        helpKey="backbone.stat.authFailures"
        value={String(stats.authFailures)}
        hint={stats.authFailures > 0 ? 'rejected handshakes' : 'no rejected handshakes'}
        tone={stats.authFailures > 0 ? 'danger' : 'default'}
      >
        {stats.authFailures > 0 ? <Badge tone="rose">watch</Badge> : <Badge tone="accent">clean</Badge>}
      </StatCard>
    </div>
  );
}

export { Sparkline };
