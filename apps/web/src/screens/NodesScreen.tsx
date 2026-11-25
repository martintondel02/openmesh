import { useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import {
  ChevronRight,
  MapPin,
  MessageSquare,
  Radio,
  RefreshCw,
  Search,
  ShieldCheck,
  UserMinus,
  UserPlus,
} from 'lucide-react';
import { useAppStore } from '../store/app';
import { selectNodesSorted } from '../store/selectors';
import type { NodeData, RoleClass } from '../types/api';
import { buildContactLink } from '../lib/links';
import { formatLastSeen, formatNodeId, formatRole, cn, shortenNodeId } from '../lib/utils';
import { Avatar, Badge, Button, EmptyState, IconButton, Stat } from '../components/ui/primitives';
import { BatteryIndicator, CapsIcons, CopyButton, RoleBadge, SignalBars } from '../components/ui/signals';
import { ConfirmDialog, Drawer } from '../components/ui/overlay';
import { QRCode } from '../components/common/QRCode';

const ROLE_FILTERS: Array<{ id: RoleClass | 'all'; label: string }> = [
  { id: 'all', label: 'All' },
  { id: 'repeater', label: 'Repeaters' },
  { id: 'gateway', label: 'Gateways' },
  { id: 'client', label: 'Clients' },
  { id: 'tracker', label: 'Trackers' },
];

export function NodesScreen() {
  const nodes = useAppStore((state) => state.nodes);
  const contacts = useAppStore((state) => state.contacts);
  const selectedNodeId = useAppStore((state) => state.selectedNodeId);
  const inspectNode = useAppStore((state) => state.inspectNode);
  const openThread = useAppStore((state) => state.openThread);
  const setScreen = useAppStore((state) => state.setScreen);
  const refreshNodes = useAppStore((state) => state.refreshNodes);
  const announceNow = useAppStore((state) => state.announceNow);
  const [query, setQuery] = useState('');
  const [roleFilter, setRoleFilter] = useState<RoleClass | 'all'>('all');

  const sorted = useMemo(() => selectNodesSorted(nodes), [nodes]);
  const filtered = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return sorted.filter((node) => {
      if (roleFilter !== 'all' && node.class !== roleFilter) return false;
      if (!normalized) return true;
      return (
        node.name.toLowerCase().includes(normalized) ||
        node.node_id.toLowerCase().includes(normalized) ||
        formatRole(node.class).toLowerCase().includes(normalized)
      );
    });
  }, [sorted, query, roleFilter]);

  const selectedNode = selectedNodeId ? nodes[selectedNodeId] : null;

  const stats = useMemo(() => {
    const all = Object.values(nodes);
    return {
      total: all.length,
      positioned: all.filter((node) => node.position).length,
      infrastructure: all.filter((node) => node.class === 'repeater' || node.class === 'gateway').length,
      contacts: all.filter((node) => node.is_contact).length,
    };
  }, [nodes]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="border-b border-white/[0.06] px-4 py-3.5 md:px-6">
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
          {[
            { label: 'Known nodes', value: stats.total, hint: 'heard in the last 24 h' },
            { label: 'With position', value: stats.positioned, hint: 'GPS or configured' },
            { label: 'Infrastructure', value: stats.infrastructure, hint: 'repeaters + gateways' },
            { label: 'Contacts', value: stats.contacts, hint: 'verified identities' },
          ].map((stat) => (
            <div key={stat.label} className="rounded-xl border border-white/[0.06] bg-white/[0.02] px-3 py-2.5">
              <Stat label={stat.label} value={stat.value} hint={stat.hint} />
            </div>
          ))}
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-2">
          <div className="relative min-w-[200px] flex-1">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-slate-500" aria-hidden="true" />
            <input
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search nodes by name, id or role…"
              aria-label="Search nodes"
              className="field-shell h-8 py-0 pl-8 text-xs"
            />
          </div>
          <div className="no-scrollbar flex max-w-full items-center gap-1 overflow-x-auto">
            {ROLE_FILTERS.map((filter) => (
              <button
                key={filter.id}
                type="button"
                onClick={() => setRoleFilter(filter.id)}
                aria-pressed={roleFilter === filter.id}
                className={cn(
                  'focus-ring shrink-0 rounded-lg border px-2.5 py-1 text-2xs font-medium transition-colors',
                  roleFilter === filter.id
                    ? 'border-emerald-400/30 bg-emerald-400/10 text-emerald-300'
                    : 'border-white/[0.07] bg-white/[0.02] text-slate-400 hover:text-slate-200',
                )}
              >
                {filter.label}
              </button>
            ))}
          </div>
          <div className="ml-auto flex items-center gap-2">
            <Button variant="ghost" size="sm" icon={RefreshCw} onClick={refreshNodes}>
              Refresh
            </Button>
            <Button variant="secondary" size="sm" icon={Radio} onClick={announceNow}>
              Announce
            </Button>
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3 md:px-6">
        {filtered.length === 0 ? (
          <EmptyState
            icon={Radio}
            title={query || roleFilter !== 'all' ? 'No nodes match' : 'No nodes heard yet'}
            description={
              query || roleFilter !== 'all'
                ? 'Clear the filters or wait for the next announce.'
                : 'Start the bridge or switch demo mode on in Settings to populate the mesh.'
            }
            className="py-20"
          />
        ) : (
          <>
            {/* desktop table */}
            <div className="hidden overflow-hidden rounded-xl border border-white/[0.06] md:block">
              <table className="w-full border-collapse text-left">
                <thead>
                  <tr className="border-b border-white/[0.06] bg-white/[0.02] text-3xs uppercase tracking-[0.1em] text-slate-500">
                    <th scope="col" className="px-3 py-2 font-medium">Node</th>
                    <th scope="col" className="px-3 py-2 font-medium">Role</th>
                    <th scope="col" className="px-3 py-2 font-medium">Caps</th>
                    <th scope="col" className="px-3 py-2 font-medium">Hops</th>
                    <th scope="col" className="px-3 py-2 font-medium">Signal</th>
                    <th scope="col" className="px-3 py-2 font-medium">Battery</th>
                    <th scope="col" className="px-3 py-2 font-medium">Last heard</th>
                    <th scope="col" className="px-3 py-2 font-medium"><span className="sr-only">Actions</span></th>
                  </tr>
                </thead>
                <tbody>
                  {filtered.map((node) => (
                    <tr
                      key={node.node_id}
                      className="group cursor-pointer border-b border-white/[0.04] transition-colors last:border-0 hover:bg-white/[0.03]"
                      onClick={() => inspectNode(node.node_id)}
                    >
                      <td className="px-3 py-2.5">
                        <div className="flex items-center gap-2.5">
                          <Avatar name={node.name} seed={node.node_id} size="sm" online={Date.now() - node.last_seen_ms < 10 * 60_000} />
                          <div className="min-w-0">
                            <div className="flex items-center gap-1.5">
                              <span className="truncate text-xs font-medium text-slate-200">{node.name || 'unnamed node'}</span>
                              {node.is_contact ? <ShieldCheck className="h-3 w-3 text-emerald-400/80" aria-label="Contact" /> : null}
                            </div>
                            <span className="font-mono text-3xs text-slate-500">{shortenNodeId(node.node_id, 6, 4)}</span>
                          </div>
                        </div>
                      </td>
                      <td className="px-3 py-2.5"><RoleBadge role={node.class} /></td>
                      <td className="px-3 py-2.5"><CapsIcons caps={node.caps} /></td>
                      <td className="px-3 py-2.5"><span className="text-xs tabular-nums text-slate-400">{node.hops}</span></td>
                      <td className="px-3 py-2.5">
                        <span className="flex items-center gap-1.5">
                          <SignalBars rssi={node.rssi} />
                          <span className="font-mono text-3xs tabular-nums text-slate-500">{node.rssi ?? '—'} dBm</span>
                        </span>
                      </td>
                      <td className="px-3 py-2.5"><BatteryIndicator pct={node.battery_pct} /></td>
                      <td className="px-3 py-2.5 text-3xs text-slate-500">{formatLastSeen(node.last_seen_ms)}</td>
                      <td className="px-3 py-2.5 text-right">
                        <div className="flex items-center justify-end gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
                          <IconButton
                            icon={MessageSquare}
                            label={`Message ${node.name}`}
                            size="sm"
                            onClick={(event) => {
                              event.stopPropagation();
                              openThread({ kind: 'direct', id: node.node_id });
                              setScreen('chats');
                            }}
                          />
                          <ChevronRight className="h-3.5 w-3.5 text-slate-600" aria-hidden="true" />
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            {/* mobile cards */}
            <div className="space-y-2 md:hidden">
              {filtered.map((node) => (
                <button
                  key={node.node_id}
                  type="button"
                  onClick={() => inspectNode(node.node_id)}
                  className="focus-ring flex w-full items-start gap-2.5 rounded-xl border border-white/[0.06] bg-white/[0.02] p-3 text-left"
                >
                  <Avatar name={node.name} seed={node.node_id} size="md" online={Date.now() - node.last_seen_ms < 10 * 60_000} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-1.5">
                      <span className="truncate text-[13px] font-medium text-slate-200">{node.name || 'unnamed node'}</span>
                      {node.is_contact ? <ShieldCheck className="h-3 w-3 text-emerald-400/80" aria-label="Contact" /> : null}
                      <span className="ml-auto"><RoleBadge role={node.class} /></span>
                    </div>
                    <p className="mt-0.5 font-mono text-3xs text-slate-500">{shortenNodeId(node.node_id, 6, 4)}</p>
                    <div className="mt-1.5 flex items-center gap-3 text-3xs text-slate-500">
                      <SignalBars rssi={node.rssi} />
                      <BatteryIndicator pct={node.battery_pct} />
                      <span>{node.hops} hops</span>
                      <span className="ml-auto">{formatLastSeen(node.last_seen_ms)}</span>
                    </div>
                  </div>
                </button>
              ))}
            </div>
          </>
        )}
      </div>

      <NodeDrawer
        node={selectedNode}
        isContact={selectedNode ? Boolean(contacts[selectedNode.node_id]) || selectedNode.is_contact : false}
        onClose={() => inspectNode(null)}
        onMessage={(node) => {
          openThread({ kind: 'direct', id: node.node_id });
          setScreen('chats');
        }}
        onShowOnMap={() => setScreen('map')}
      />
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Node detail drawer                                                 */
/* ------------------------------------------------------------------ */

function NodeDrawer({
  node,
  isContact,
  onClose,
  onMessage,
  onShowOnMap,
}: {
  node: NodeData | null;
  isContact: boolean;
  onClose: () => void;
  onMessage: (node: NodeData) => void;
  onShowOnMap: () => void;
}) {
  const requestContactQr = useAppStore((state) => state.requestContactQr);
  const addContact = useAppStore((state) => state.addContact);
  const removeContact = useAppStore((state) => state.removeContact);
  const pushToast = useAppStore((state) => state.pushToast);
  const [qrUrl, setQrUrl] = useState<string | null>(null);
  const [qrLoading, setQrLoading] = useState(false);
  const [removeOpen, setRemoveOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!node) return;
    let cancelled = false;
    setQrUrl(null);
    const fallback =
      node.public_key && node.public_key.length > 0
        ? buildContactLink({ nodeId: node.node_id, name: node.name, identity: node.public_key })
        : null;
    setQrLoading(true);
    requestContactQr(node.node_id)
      .then((url) => {
        if (!cancelled) setQrUrl(url);
      })
      .catch(() => {
        if (!cancelled) setQrUrl(fallback);
      })
      .finally(() => {
        if (!cancelled) setQrLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [node, requestContactQr]);

  if (!node) return null;

  return (
    <>
      <Drawer
        open={Boolean(node)}
        onClose={onClose}
        title={node.name || 'unnamed node'}
        description={`${formatRole(node.class)} · ${node.hops} ${node.hops === 1 ? 'hop' : 'hops'} · ${node.is_contact || isContact ? 'contact' : 'not a contact'}`}
        footer={
          <div className="flex items-center gap-2">
            <Button variant="primary" icon={MessageSquare} className="flex-1" onClick={() => onMessage(node)}>
              Message
            </Button>
            {isContact ? (
              <Button variant="danger" icon={UserMinus} onClick={() => setRemoveOpen(true)}>
                Remove
              </Button>
            ) : (
              <Button
                variant="secondary"
                icon={UserPlus}
                loading={busy}
                onClick={async () => {
                  setBusy(true);
                  try {
                    await addContact({ nodeId: node.node_id, name: node.name, identity: node.public_key });
                  } finally {
                    setBusy(false);
                  }
                }}
              >
                Add contact
              </Button>
            )}
          </div>
        }
      >
        <div className="space-y-5 px-5 py-4">
          <div className="flex items-center gap-3">
            <Avatar name={node.name} seed={node.node_id} size="xl" online={Date.now() - node.last_seen_ms < 10 * 60_000} />
            <div className="min-w-0">
              <div className="flex items-center gap-1.5">
                <RoleBadge role={node.class} />
                {isContact ? (
                  <Badge tone="accent" icon={ShieldCheck}>verified</Badge>
                ) : null}
              </div>
              <p className="mt-1 break-all font-mono text-3xs text-slate-500">{formatNodeId(node.node_id)}</p>
              <div className="mt-1.5 flex items-center gap-2">
                <CopyButton value={node.node_id} label="Copy id" size="md" variant="surface" />
                {node.position ? (
                  <button
                    type="button"
                    onClick={onShowOnMap}
                    className="focus-ring inline-flex h-8 items-center gap-1 rounded-lg border border-white/10 bg-white/[0.04] px-2 text-2xs text-slate-300 hover:bg-white/[0.08]"
                  >
                    <MapPin className="h-3 w-3" aria-hidden="true" />
                    Map
                  </button>
                ) : null}
              </div>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-2">
            <DrawerMetric label="Signal" hint={`${node.rssi ?? '—'} dBm`}>
              <SignalBars rssi={node.rssi} withLabel />
            </DrawerMetric>
            <DrawerMetric label="SNR" hint="link quality">
              <span className="font-mono text-sm text-slate-200">{node.snr ?? '—'} dB</span>
            </DrawerMetric>
            <DrawerMetric label="Hops" hint="distance">
              <span className="text-sm font-semibold text-slate-200">{node.hops}</span>
            </DrawerMetric>
            <DrawerMetric label="Battery" hint={node.battery_pct === null ? 'no telemetry' : 'reported'}>
              <BatteryIndicator pct={node.battery_pct} />
            </DrawerMetric>
            <DrawerMetric label="Last heard" hint={new Date(node.last_seen_ms).toLocaleString()}>
              <span className="text-xs text-slate-200">{formatLastSeen(node.last_seen_ms)}</span>
            </DrawerMetric>
            <DrawerMetric label="Capabilities" hint={`${node.caps.length} advertised`}>
              <CapsIcons caps={node.caps} max={10} />
            </DrawerMetric>
          </div>

          {node.position ? (
            <div>
              <p className="mb-1.5 text-2xs font-semibold uppercase tracking-[0.12em] text-slate-500">Position</p>
              <div className="rounded-xl border border-white/[0.06] bg-white/[0.02] p-3">
                <div className="grid grid-cols-2 gap-2 text-xs">
                  <div>
                    <p className="text-3xs text-slate-500">Latitude</p>
                    <p className="font-mono text-slate-200">{node.position.lat.toFixed(5)}</p>
                  </div>
                  <div>
                    <p className="text-3xs text-slate-500">Longitude</p>
                    <p className="font-mono text-slate-200">{node.position.lon.toFixed(5)}</p>
                  </div>
                  <div>
                    <p className="text-3xs text-slate-500">Altitude</p>
                    <p className="font-mono text-slate-200">{node.position.alt_m ? `${Math.round(node.position.alt_m)} m` : '—'}</p>
                  </div>
                  <div>
                    <p className="text-3xs text-slate-500">Accuracy</p>
                    <p className="font-mono text-slate-200">
                      {node.position.accuracy_m ? `±${Math.round(node.position.accuracy_m)} m` : '—'}
                    </p>
                  </div>
                </div>
                <p className="mt-2 text-3xs text-slate-600">
                  Last fix {formatLastSeen(node.position.timestamp_ms)} · {new Date(node.position.timestamp_ms).toLocaleTimeString()}
                </p>
              </div>
            </div>
          ) : null}

          <div>
            <p className="mb-1.5 text-2xs font-semibold uppercase tracking-[0.12em] text-slate-500">Contact QR</p>
            <div className="flex flex-col items-center gap-2 rounded-xl border border-white/[0.06] bg-white/[0.02] p-4">
              {qrLoading ? (
                <div className="grid h-[180px] w-[180px] place-items-center text-2xs text-slate-500">Building invite…</div>
              ) : qrUrl ? (
                <>
                  <QRCode value={qrUrl} size={180} label={`Contact QR for ${node.name}`} />
                  <code className="w-full break-all rounded-md bg-ink-950/60 px-2 py-1.5 font-mono text-3xs text-slate-500">
                    {qrUrl}
                  </code>
                  <div className="flex items-center gap-2">
                    <CopyButton value={qrUrl} label="Copy invite link" size="md" variant="surface" />
                  </div>
                </>
              ) : (
                <p className="py-8 text-center text-2xs text-slate-500">
                  This node has not shared a public identity yet, so no contact invite can be built.
                </p>
              )}
            </div>
          </div>

          <div>
            <p className="mb-1.5 text-2xs font-semibold uppercase tracking-[0.12em] text-slate-500">Raw identity (72 bytes)</p>
            {node.public_key ? (
              <div className="rounded-xl border border-white/[0.06] bg-ink-950/60 p-2.5">
                <code className="block break-all font-mono text-3xs leading-relaxed text-slate-500">{node.public_key}</code>
                <CopyButton value={node.public_key} label="Copy identity" size="md" />
              </div>
            ) : (
              <p className="text-2xs text-slate-500">Not advertised — this node hides its identity.</p>
            )}
          </div>
        </div>
      </Drawer>

      <ConfirmDialog
        open={removeOpen}
        onClose={() => setRemoveOpen(false)}
        onConfirm={async () => {
          await removeContact(node.node_id);
          setRemoveOpen(false);
          pushToast({ kind: 'info', title: `${node.name} removed from contacts` });
        }}
        title={`Remove ${node.name}?`}
        description="You will stop verifying this identity. The node stays in the mesh view until it expires."
        confirmLabel="Remove contact"
        icon={UserMinus}
      />
    </>
  );
}

function DrawerMetric({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-2.5 py-2">
      <p className="text-3xs uppercase tracking-[0.1em] text-slate-500">{label}</p>
      <div className="mt-0.5">{children}</div>
      {hint ? <p className="mt-0.5 truncate text-3xs text-slate-600">{hint}</p> : null}
    </div>
  );
}
