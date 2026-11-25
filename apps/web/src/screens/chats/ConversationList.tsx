import { useEffect, useMemo, useRef, useState } from 'react';
import { Hash, MessageSquarePlus, Plus, Search, ShieldCheck, Users, X } from 'lucide-react';
import { useAppStore } from '../../store/app';
import { selectConversations } from '../../store/selectors';
import { formatRelativeTime } from '../../lib/utils';
import { Avatar, EmptyState, IconButton, SectionTitle, SegmentedControl } from '../../components/ui/primitives';
import { DeliveryTicks } from '../../components/ui/signals';
import { GetStartedCard } from './GetStartedCard';

type Filter = 'all' | 'group' | 'direct';

function useMinuteClock(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, []);
  return now;
}

export function ConversationList({ onNewChat }: { onNewChat: () => void }) {
  const messages = useAppStore((state) => state.messages);
  const groupMessages = useAppStore((state) => state.groupMessages);
  const nodes = useAppStore((state) => state.nodes);
  const contacts = useAppStore((state) => state.contacts);
  const channels = useAppStore((state) => state.channels);
  const readCursors = useAppStore((state) => state.readCursors);
  const activeThread = useAppStore((state) => state.activeThread);
  const openThread = useAppStore((state) => state.openThread);
  const now = useMinuteClock();

  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<Filter>('all');
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
      if (event.key === 'Escape' && document.activeElement === searchRef.current) {
        setQuery('');
        searchRef.current?.blur();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  const conversations = useMemo(
    () =>
      selectConversations(
        { messages, groupMessages, nodes, channels, contacts, readCursors, activeThread, ownNodeId: null },
        now,
      ),
    [messages, groupMessages, nodes, channels, contacts, readCursors, activeThread, now],
  );

  const normalizedQuery = query.trim().toLowerCase();
  const visible = conversations.filter((conversation) => {
    if (filter === 'group' && conversation.kind !== 'group') return false;
    if (filter === 'direct' && conversation.kind !== 'direct') return false;
    if (!normalizedQuery) return true;
    return (
      conversation.name.toLowerCase().includes(normalizedQuery) ||
      conversation.lastText.toLowerCase().includes(normalizedQuery) ||
      conversation.address.toLowerCase().includes(normalizedQuery)
    );
  });
  const visibleChannels = visible.filter((conversation) => conversation.kind === 'group');
  const visibleDirect = visible.filter((conversation) => conversation.kind === 'direct');
  const totalUnread = conversations.reduce((sum, conversation) => sum + conversation.unread, 0);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="space-y-2.5 border-b border-white/[0.06] px-3 pb-3 pt-3.5">
        <div className="flex items-center gap-2">
          <p className="text-2xs font-medium text-slate-500">
            {conversations.length} {conversations.length === 1 ? 'conversation' : 'conversations'}
          </p>
          {totalUnread > 0 ? (
            <span className="rounded-full bg-emerald-400/15 px-1.5 py-0.5 text-3xs font-semibold tabular-nums text-emerald-300">
              {totalUnread} unread
            </span>
          ) : null}
          <div className="ml-auto flex items-center gap-1">
            <IconButton icon={MessageSquarePlus} label="New conversation" onClick={onNewChat} />
          </div>
        </div>

        <div className="relative">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-slate-500" aria-hidden="true" />
          <input
            ref={searchRef}
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search chats, channels, nodes…"
            aria-label="Search conversations"
            className="field-shell h-8 py-0 pl-8 pr-14 text-xs [&::-webkit-search-cancel-button]:hidden"
          />
          {query ? (
            <button
              type="button"
              aria-label="Clear search"
              onClick={() => setQuery('')}
              className="absolute right-2 top-1/2 grid h-5 w-5 -translate-y-1/2 place-items-center rounded text-slate-500 hover:bg-white/[0.06] hover:text-slate-300"
            >
              <X className="h-3 w-3" aria-hidden="true" />
            </button>
          ) : (
            <span className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2">
              <kbd className="kbd">⌘K</kbd>
            </span>
          )}
        </div>

        <SegmentedControl<Filter>
          ariaLabel="Filter conversations"
          value={filter}
          onChange={setFilter}
          className="w-full justify-between"
          options={[
            { value: 'all', label: 'All', count: conversations.length },
            { value: 'group', label: 'Channels', count: conversations.filter((c) => c.kind === 'group').length },
            { value: 'direct', label: 'Direct', count: conversations.filter((c) => c.kind === 'direct').length },
          ]}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2">
        <GetStartedCard />
        {visibleChannels.length > 0 ? (
          <div>
            <div className="flex items-center gap-1.5 px-2 pb-1 pt-1.5">
              <SectionTitle>Channels</SectionTitle>
              <ShieldCheck className="h-3 w-3 text-emerald-400/50" aria-hidden="true" />
            </div>
            {visibleChannels.map((conversation) => (
              <ConversationRow
                key={conversation.id}
                conversation={conversation}
                active={activeThread?.kind === 'group' && activeThread.id === conversation.id}
                now={now}
                onSelect={() => openThread({ kind: 'group', id: conversation.id })}
              />
            ))}
          </div>
        ) : null}

        {visibleDirect.length > 0 ? (
          <div className={visibleChannels.length > 0 ? 'mt-1' : ''}>
            <div className="flex items-center gap-1.5 px-2 pb-1 pt-2">
              <SectionTitle>Direct messages</SectionTitle>
            </div>
            {visibleDirect.map((conversation) => (
              <ConversationRow
                key={conversation.id}
                conversation={conversation}
                active={activeThread?.kind === 'direct' && activeThread.id === conversation.id}
                now={now}
                onSelect={() => openThread({ kind: 'direct', id: conversation.id })}
              />
            ))}
          </div>
        ) : null}

        {visible.length === 0 ? (
          <EmptyState
            icon={query ? Search : Users}
            title={query ? 'No conversations match' : 'No conversations yet'}
            description={
              query
                ? 'Try a node name, a channel, or a node id.'
                : 'Start a direct message with a node your radio has heard, or create a channel for your group.'
            }
            className="py-10"
            helpTopic="messaging"
            action={
              <button
                type="button"
                onClick={onNewChat}
                className="focus-ring inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.05] px-3 py-1.5 text-xs font-medium text-slate-200 hover:bg-white/[0.08]"
              >
                <Plus className="h-3.5 w-3.5" aria-hidden="true" />
                New conversation
              </button>
            }
          />
        ) : null}
      </div>

      <div className="hidden border-t border-white/[0.06] p-2 md:block">
        <button
          type="button"
          onClick={onNewChat}
          className="focus-ring flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-xs font-medium text-slate-400 transition-colors hover:bg-white/[0.04] hover:text-slate-200"
        >
          <Plus className="h-3.5 w-3.5" aria-hidden="true" />
          New conversation
          <span className="ml-auto font-mono text-3xs text-slate-600">{conversations.length}</span>
        </button>
      </div>
    </div>
  );
}

function ConversationRow({
  conversation,
  active,
  now,
  onSelect,
}: {
  conversation: ReturnType<typeof selectConversations>[number];
  active: boolean;
  now: number;
  onSelect: () => void;
}) {
  const isGroup = conversation.kind === 'group';
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={active ? 'true' : undefined}
      className={
        'focus-ring relative flex w-full items-start gap-2.5 rounded-lg px-2 py-2 text-left transition-colors ' +
        (active ? 'bg-white/[0.07]' : 'hover:bg-white/[0.035]')
      }
    >
      {isGroup ? (
        <span className="relative grid h-10 w-10 shrink-0 place-items-center rounded-xl border border-emerald-400/20 bg-gradient-to-br from-emerald-400/20 to-sky-500/10 text-emerald-300">
          <Hash className="h-4 w-4" aria-hidden="true" />
        </span>
      ) : (
        <Avatar name={conversation.name} seed={conversation.id} size="md" online={conversation.online} />
      )}
      <span className="min-w-0 flex-1">
        <span className="flex items-baseline gap-2">
          <span className={'truncate text-[13px] font-medium ' + (conversation.unread > 0 ? 'text-white' : 'text-slate-200')}>
            {conversation.name}
          </span>
          <span className="ml-auto shrink-0 text-3xs tabular-nums text-slate-500">
            {conversation.lastAt ? formatRelativeTime(conversation.lastAt, now) : ''}
          </span>
        </span>
        <span className="mt-0.5 flex items-center gap-1 text-2xs text-slate-500">
          {!isGroup && conversation.outgoing && conversation.delivery ? (
            <DeliveryTicks state={conversation.delivery} />
          ) : null}
          <span className={'truncate ' + (conversation.unread > 0 ? 'text-slate-400' : 'text-slate-500')}>
            {conversation.lastText}
          </span>
          {conversation.unread > 0 ? (
            <span className="ml-auto grid min-w-[18px] shrink-0 place-items-center rounded-full bg-emerald-400 px-1 text-[10px] font-semibold leading-[18px] text-emerald-950">
              {conversation.unread > 99 ? '99+' : conversation.unread}
            </span>
          ) : isGroup ? (
            <span className="ml-auto shrink-0" title="Signed channel">
              <ShieldCheck className="h-3 w-3 text-slate-600" aria-hidden="true" />
            </span>
          ) : null}
        </span>
      </span>
    </button>
  );
}
