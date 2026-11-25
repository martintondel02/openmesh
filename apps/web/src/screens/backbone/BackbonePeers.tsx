import { useState } from 'react';
import {
  ArrowDownLeft,
  ArrowUpRight,
  Download,
  RadioTower,
  ShieldCheck,
  Unplug,
  UserX,
} from 'lucide-react';
import type { BackbonePeer } from '../../types/backbone';
import { formatLastSeen, shortenNodeId } from '../../lib/utils';
import { formatAuthMode, formatPeerDirection, formatRtt, rttTone } from '../../lib/backbone';
import { Avatar, Badge, Button, EmptyState, IconButton } from '../../components/ui/primitives';
import { CopyButton } from '../../components/ui/signals';
import { ConfirmDialog } from '../../components/ui/overlay';
import { HelpTip } from '../../components/ui/help';

/** Connected-peer table (desktop) / cards (mobile) with kick confirmation. */
export function BackbonePeers({
  peers,
  maxPeers,
  onKick,
  onOpenSetup,
  demo = false,
  canKick = true,
}: {
  peers: BackbonePeer[];
  maxPeers: number;
  onKick: (nodeId: string) => void;
  onOpenSetup: () => void;
  demo?: boolean;
  canKick?: boolean;
}) {
  const [kickTarget, setKickTarget] = useState<BackbonePeer | null>(null);

  return (
    <section className="rounded-2xl border border-white/[0.06] bg-white/[0.02]">
      <header className="flex flex-wrap items-center gap-2 border-b border-white/[0.06] px-4 py-3">
        <div className="grid h-8 w-8 place-items-center rounded-lg border border-white/[0.07] bg-white/[0.03] text-slate-400">
          <RadioTower className="h-4 w-4" aria-hidden="true" />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="flex items-center gap-1.5 text-sm font-semibold text-slate-100">
            Connected peers
            <HelpTip helpKey="backbone.stat.peers" label="Connected peers" />
            <span className="ml-1 font-mono text-2xs font-normal text-slate-500">
              {peers.length}/{maxPeers}
            </span>
          </h3>
          <p className="mt-0.5 text-2xs text-slate-500">
            {demo ? 'Seeded peers — the panel updates live as the demo mesh ticks.' : 'Authenticated OBB/1 TCP links, updated over WebSocket.'}
          </p>
        </div>
        <Badge tone={peers.length > 0 ? 'accent' : 'slate'}>
          {peers.length === 0 ? 'no peers' : `${peers.length} connected`}
        </Badge>
      </header>

      {peers.length === 0 ? (
        <EmptyState
          icon={Unplug}
          title="No peers connected yet"
          description="Follow the setup steps to install OpenMesh on a remote gateway and join it to this backbone. Peers appear here within a few seconds."
          className="py-10"
          action={
            <Button variant="secondary" size="sm" icon={Download} onClick={onOpenSetup}>
              Set up your backbone
            </Button>
          }
        />
      ) : (
        <>
          {/* Desktop table */}
          <div className="hidden overflow-x-auto lg:block">
            <table className="w-full border-collapse text-left">
              <thead>
                <tr className="border-b border-white/[0.05] text-3xs uppercase tracking-[0.1em] text-slate-500">
                  <th scope="col" className="px-3 py-2 font-medium">Peer</th>
                  <th scope="col" className="px-3 py-2 font-medium">Direction</th>
                  <th scope="col" className="px-3 py-2 font-medium">Remote</th>
                  <th scope="col" className="px-3 py-2 font-medium">RTT</th>
                  <th scope="col" className="px-3 py-2 font-medium">Traffic</th>
                  <th scope="col" className="whitespace-nowrap px-3 py-2 font-medium">Last seen</th>
                  <th scope="col" className="px-3 py-2 text-right font-medium">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {peers.map((peer) => (
                  <tr key={peer.node_id} className="group border-b border-white/[0.04] last:border-0 hover:bg-white/[0.025]">
                    <td className="px-3 py-2.5">
                      <div className="flex items-center gap-2.5">
                        <Avatar name={peer.name || peer.node_id.slice(0, 4)} seed={peer.node_id} size="sm" online />
                        <div className="min-w-0">
                          <div className="flex items-center gap-1.5">
                            <span className="truncate text-xs font-medium text-slate-200">{peer.name || 'unnamed peer'}</span>
                            {peer.auth === 'token' ? (
                              <ShieldCheck className="h-3 w-3 text-emerald-400/70" aria-label="Token authenticated" />
                            ) : (
                              <span className="text-3xs text-rose-300/80">open</span>
                            )}
                          </div>
                          <span className="flex items-center gap-1 whitespace-nowrap font-mono text-3xs text-slate-500">
                            {shortenNodeId(peer.node_id, 6, 4)}
                            <CopyButton value={peer.node_id} label="" className="h-4 w-4 px-0" />
                            <span className="text-slate-600">v{peer.app_version || '?'}</span>
                          </span>
                        </div>
                      </div>
                    </td>
                    <td className="px-3 py-2.5">
                      <DirectionBadge direction={peer.direction} />
                    </td>
                    <td className="px-3 py-2.5">
                      <span className="whitespace-nowrap font-mono text-3xs text-slate-400">{peer.remote_addr}</span>
                    </td>
                    <td className="px-3 py-2.5">
                      <RttBadge rttMs={peer.rtt_ms} />
                    </td>
                    <td className="px-3 py-2.5">
                      <TrafficCell peer={peer} />
                    </td>
                    <td className="whitespace-nowrap px-3 py-2.5 text-3xs text-slate-500">{formatLastSeen(peer.last_seen_ms)}</td>
                    <td className="px-3 py-2.5 text-right">
                      {canKick ? (
                        <IconButton
                          icon={UserX}
                          label={`Kick ${peer.name || peer.node_id}`}
                          size="sm"
                          className="opacity-0 transition-opacity hover:text-rose-300 group-hover:opacity-100 group-focus-within:opacity-100"
                          onClick={() => setKickTarget(peer)}
                        />
                      ) : null}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* Mobile cards */}
          <div className="divide-y divide-white/[0.05] lg:hidden">
            {peers.map((peer) => (
              <div key={peer.node_id} className="p-3">
                <div className="flex items-start gap-2.5">
                  <Avatar name={peer.name || peer.node_id.slice(0, 4)} seed={peer.node_id} size="md" online />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-1.5">
                      <p className="truncate text-[13px] font-medium text-slate-200">{peer.name || 'unnamed peer'}</p>
                      {peer.auth === 'token' ? (
                        <ShieldCheck className="h-3 w-3 text-emerald-400/70" aria-label="Token authenticated" />
                      ) : (
                        <span className="text-3xs text-rose-300/80">open</span>
                      )}
                      <span className="ml-auto">
                        <DirectionBadge direction={peer.direction} compact />
                      </span>
                    </div>
                    <p className="mt-0.5 flex items-center gap-1 font-mono text-3xs text-slate-500">
                      {shortenNodeId(peer.node_id, 6, 4)}
                      <CopyButton value={peer.node_id} label="" className="h-4 w-4 px-0" />
                    </p>
                    <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-3xs text-slate-500">
                      <RttBadge rttMs={peer.rtt_ms} />
                      <span className="font-mono">{peer.remote_addr}</span>
                      <span>{formatLastSeen(peer.last_seen_ms)}</span>
                    </div>
                    <div className="mt-1.5 flex items-center justify-between gap-2">
                      <TrafficCell peer={peer} />
                      {canKick ? (
                        <IconButton
                          icon={UserX}
                          label={`Kick ${peer.name || peer.node_id}`}
                          size="sm"
                          className="text-slate-500 hover:text-rose-300"
                          onClick={() => setKickTarget(peer)}
                        />
                      ) : null}
                    </div>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </>
      )}

      <ConfirmDialog
        open={Boolean(kickTarget)}
        onClose={() => setKickTarget(null)}
        onConfirm={() => {
          if (!kickTarget) return;
          onKick(kickTarget.node_id);
          setKickTarget(null);
        }}
        title={`Disconnect ${kickTarget?.name || 'this peer'}?`}
        description={`The ${formatPeerDirection(kickTarget?.direction)} OBB/1 link will be closed immediately. The peer may reconnect automatically if its token is still valid.`}
        confirmLabel="Kick peer"
        icon={UserX}
      >
        <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-3 py-2 font-mono text-3xs text-slate-400">
          <p>{kickTarget?.node_id}</p>
          <p className="mt-1 text-slate-500">
            {kickTarget?.remote_addr} · {formatAuthMode(kickTarget?.auth)} · {formatRtt(kickTarget?.rtt_ms)}
          </p>
        </div>
      </ConfirmDialog>
    </section>
  );
}

function DirectionBadge({ direction, compact = false }: { direction: string; compact?: boolean }) {
  const inbound = direction === 'inbound';
  return (
    <Badge tone={inbound ? 'accent' : 'sky'} icon={inbound ? ArrowDownLeft : ArrowUpRight}>
      {compact ? (inbound ? 'in' : 'out') : formatPeerDirection(direction)}
    </Badge>
  );
}

function RttBadge({ rttMs }: { rttMs: number | null }) {
  const tone = rttTone(rttMs);
  return (
    <Badge tone={tone === 'good' ? 'accent' : tone === 'fair' ? 'amber' : tone === 'poor' ? 'rose' : 'slate'} className="font-mono">
      {formatRtt(rttMs)}
    </Badge>
  );
}

function TrafficCell({ peer }: { peer: BackbonePeer }) {
  const compact = (value: number): string => {
    if (value < 1000) return String(Math.round(value));
    if (value < 1_000_000) return `${(value / 1000).toFixed(0)}k`;
    return `${(value / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
  };
  return (
    <div className="flex flex-col gap-0.5 whitespace-nowrap font-mono text-3xs leading-tight">
      <span className="flex items-center gap-1 text-emerald-300/80" title={`${peer.frames_in} frames / ${peer.bytes_in} bytes`}>
        <ArrowDownLeft className="h-2.5 w-2.5 shrink-0" aria-hidden="true" />
        {compact(peer.frames_in)}f · {compact(peer.bytes_in)}B
      </span>
      <span className="flex items-center gap-1 text-sky-300/80" title={`${peer.frames_out} frames / ${peer.bytes_out} bytes`}>
        <ArrowUpRight className="h-2.5 w-2.5 shrink-0" aria-hidden="true" />
        {compact(peer.frames_out)}f · {compact(peer.bytes_out)}B
      </span>
    </div>
  );
}
