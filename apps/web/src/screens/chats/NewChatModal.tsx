import { useMemo, useState } from 'react';
import { Hash, Link2, MessageSquarePlus, Plus, Radio, Search, Users } from 'lucide-react';
import { useAppStore } from '../../store/app';
import { displayName } from '../../lib/display';
import { selectNodesSorted } from '../../store/selectors';
import { formatLastSeen, formatRole } from '../../lib/utils';
import { Avatar, Button, EmptyState } from '../../components/ui/primitives';
import { Modal } from '../../components/ui/overlay';
import { RoleBadge, SignalBars } from '../../components/ui/signals';

export function NewChatModal({
  open,
  onClose,
  onCreateChannel,
  onJoinChannel,
}: {
  open: boolean;
  onClose: () => void;
  onCreateChannel: () => void;
  onJoinChannel: () => void;
}) {
  const nodes = useAppStore((state) => state.nodes);
  const contacts = useAppStore((state) => state.contacts);
  const openThread = useAppStore((state) => state.openThread);
  const setScreen = useAppStore((state) => state.setScreen);
  const [query, setQuery] = useState('');

  const sorted = useMemo(() => selectNodesSorted(nodes), [nodes]);
  const filtered = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return sorted;
    return sorted.filter(
      (node) =>
        displayName({ announcedName: node.name, contactName: contacts[node.node_id]?.name, nodeId: node.node_id }).toLowerCase().includes(normalized) ||
        node.node_id.toLowerCase().includes(normalized) ||
        formatRole(node.class).toLowerCase().includes(normalized),
    );
  }, [sorted, query]);

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="New conversation"
      description="Open a direct message with a node your radio has heard, or start a signed channel."
      icon={MessageSquarePlus}
      size="md"
      footer={
        <Button
          variant="ghost"
          icon={Users}
          onClick={() => {
            onClose();
            setScreen('contacts');
          }}
        >
          Manage contacts
        </Button>
      }
    >
      <div className="space-y-4">
        <div className="grid grid-cols-2 gap-2">
          <button
            type="button"
            onClick={() => {
              onClose();
              onCreateChannel();
            }}
            className="focus-ring group flex flex-col items-start gap-1.5 rounded-xl border border-white/[0.07] bg-white/[0.02] p-3 text-left transition-colors hover:border-emerald-400/30 hover:bg-emerald-400/[0.05]"
          >
            <span className="grid h-8 w-8 place-items-center rounded-lg bg-emerald-400/10 text-emerald-300">
              <Hash className="h-4 w-4" aria-hidden="true" />
            </span>
            <span className="text-xs font-semibold text-slate-200">Create channel</span>
            <span className="text-2xs leading-relaxed text-slate-500">Group chat with a shared 256-bit key</span>
          </button>
          <button
            type="button"
            onClick={() => {
              onClose();
              onJoinChannel();
            }}
            className="focus-ring group flex flex-col items-start gap-1.5 rounded-xl border border-white/[0.07] bg-white/[0.02] p-3 text-left transition-colors hover:border-sky-400/30 hover:bg-sky-400/[0.05]"
          >
            <span className="grid h-8 w-8 place-items-center rounded-lg bg-sky-400/10 text-sky-300">
              <Link2 className="h-4 w-4" aria-hidden="true" />
            </span>
            <span className="text-xs font-semibold text-slate-200">Join with invite</span>
            <span className="text-2xs leading-relaxed text-slate-500">Paste an openmesh://channel link</span>
          </button>
        </div>

        <div>
          <div className="mb-2 flex items-center justify-between">
            <p className="text-2xs font-semibold uppercase tracking-[0.14em] text-slate-500">Message a node</p>
            <span className="text-3xs text-slate-600">{filtered.length} known</span>
          </div>
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-slate-500" aria-hidden="true" />
            <input
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search by name, role or node id"
              aria-label="Search nodes"
              className="field-shell h-8 py-0 pl-8 text-xs"
            />
          </div>
          <div className="mt-2 max-h-[280px] space-y-0.5 overflow-y-auto">
            {filtered.length === 0 ? (
              <EmptyState
                icon={Radio}
                title={query ? 'No node matches' : 'No nodes heard yet'}
                description={
                  query
                    ? 'Try a shorter search or the first characters of the node id.'
                    : 'Keep the bridge running — announces arrive within a few minutes.'
                }
                className="py-8"
              />
            ) : (
              filtered.map((node) => {
                const isContact = Boolean(contacts[node.node_id]);
                return (
                  <button
                    key={node.node_id}
                    type="button"
                    onClick={() => {
                      openThread({ kind: 'direct', id: node.node_id });
                      onClose();
                    }}
                    className="focus-ring flex w-full items-center gap-2.5 rounded-lg px-2 py-2 text-left transition-colors hover:bg-white/[0.04]"
                  >
                    <Avatar name={displayName({ announcedName: node.name, contactName: contacts[node.node_id]?.name, nodeId: node.node_id })} seed={node.node_id} size="sm" online={Date.now() - node.last_seen_ms < 10 * 60_000} />
                    <span className="min-w-0 flex-1">
                      <span className="flex items-center gap-1.5">
                        <span className="truncate text-xs font-medium text-slate-200">{displayName({ announcedName: node.name, contactName: contacts[node.node_id]?.name, nodeId: node.node_id })}</span>
                        {isContact ? <span className="text-3xs text-emerald-400/80">contact</span> : null}
                      </span>
                      <span className="mt-0.5 flex items-center gap-2 text-3xs text-slate-500">
                        <span className="font-mono">{node.node_id.slice(0, 8)}</span>
                        <span>{node.hops} hops</span>
                        <span>{formatLastSeen(node.last_seen_ms)}</span>
                      </span>
                    </span>
                    <RoleBadge role={node.class} />
                    <SignalBars rssi={node.rssi} />
                  </button>
                );
              })
            )}
          </div>
        </div>

        <div className="flex items-center gap-2 rounded-xl border border-white/[0.06] bg-white/[0.02] px-3 py-2 text-2xs text-slate-500">
          <Plus className="h-3.5 w-3.5 text-slate-600" aria-hidden="true" />
          Tip: add a peer to Contacts first to verify their 72-byte identity before messaging.
        </div>
      </div>
    </Modal>
  );
}
