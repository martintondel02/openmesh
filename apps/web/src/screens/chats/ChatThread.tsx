import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowDown,
  Hash,
  Info,
  LogOut,
  MapPin,
  MessageSquare,
  QrCode,
  ShieldCheck,
  UserRound,
} from 'lucide-react';
import { useAppStore } from '../../store/app';
import { selectThreadHeader, selectThreadMessages } from '../../store/selectors';
import { Avatar, Badge, EmptyState, IconButton } from '../../components/ui/primitives';
import { RoleBadge } from '../../components/ui/signals';
import { ConfirmDialog, Modal } from '../../components/ui/overlay';
import { groupMessagesByDay, replyPreviewText, shortChannelId } from './chat-utils';
import { DaySeparator, EmptyThreadState, MessageBubble, isGroupMessage, type AnyChatMessage } from './MessageBubble';
import { Composer } from './Composer';
import { ChannelInviteModal } from './ChannelModals';
import { PeerLinkChip, PeerLinkPanel } from './PeerLinkCard';

export function ChatThread() {
  const activeThread = useAppStore((state) => state.activeThread);
  const closeThread = useAppStore((state) => state.closeThread);
  const setReplyTo = useAppStore((state) => state.setReplyTo);
  const retryMessage = useAppStore((state) => state.retryMessage);
  const cancelMessage = useAppStore((state) => state.cancelMessage);
  const inspectNode = useAppStore((state) => state.inspectNode);
  const sharePosition = useAppStore((state) => state.sharePosition);
  const leaveChannel = useAppStore((state) => state.leaveChannel);
  const pushToast = useAppStore((state) => state.pushToast);

  const nodes = useAppStore((state) => state.nodes);
  const contacts = useAppStore((state) => state.contacts);
  const channels = useAppStore((state) => state.channels);
  const messages = useAppStore((state) => state.messages);
  const groupMessages = useAppStore((state) => state.groupMessages);
  const header = useMemo(
    () =>
      selectThreadHeader({
        activeThread,
        nodes,
        contacts,
        channels,
        messages,
        groupMessages,
        readCursors: {},
        ownNodeId: null,
      }),
    [activeThread, nodes, contacts, channels, messages, groupMessages],
  );
  const threadMessages = useMemo(
    () => selectThreadMessages({ activeThread, messages, groupMessages }),
    [activeThread, messages, groupMessages],
  );
  const [inviteOpen, setInviteOpen] = useState(false);
  const [peerOpen, setPeerOpen] = useState(false);
  const [leaveOpen, setLeaveOpen] = useState(false);
  const [atBottom, setAtBottom] = useState(true);
  const [newCount, setNewCount] = useState(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const previousCount = useRef(0);
  const atBottomRef = useRef(true);

  const all: AnyChatMessage[] = useMemo(
    () => (activeThread?.kind === 'direct' ? threadMessages.direct : threadMessages.group) as AnyChatMessage[],
    [activeThread?.kind, threadMessages.direct, threadMessages.group],
  );

  const dayGroups = useMemo(() => groupMessagesByDay(all), [all]);

  const scrollToBottom = useCallback((smooth = false) => {
    const container = scrollRef.current;
    if (!container) return;
    container.scrollTo({ top: container.scrollHeight, behavior: smooth ? 'smooth' : 'auto' });
    atBottomRef.current = true;
    setAtBottom(true);
    setNewCount(0);
  }, []);

  // Scroll to the newest message when opening a thread (twice: images/voice
  // waveforms change the content height after the first paint).
  useEffect(() => {
    previousCount.current = all.length;
    requestAnimationFrame(() => scrollToBottom(false));
    const timer = setTimeout(() => scrollToBottom(false), 350);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeThread?.id, activeThread?.kind]);

  // Keep the view pinned to the bottom while media loads, as long as the reader
  // has not scrolled up.
  useEffect(() => {
    const container = scrollRef.current;
    if (!container) return;
    const observer = new ResizeObserver(() => {
      if (atBottomRef.current) {
        container.scrollTop = container.scrollHeight;
      }
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, [activeThread?.id, activeThread?.kind]);

  useEffect(() => {
    const delta = all.length - previousCount.current;
    previousCount.current = all.length;
    if (delta <= 0) return;
    if (atBottom) {
      requestAnimationFrame(() => scrollToBottom(false));
    } else {
      setNewCount((count) => count + delta);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [all.length, atBottom]);

  const onScroll = () => {
    const container = scrollRef.current;
    if (!container) return;
    const distance = container.scrollHeight - container.scrollTop - container.clientHeight;
    const next = distance < 96;
    atBottomRef.current = next;
    if (next !== atBottom) setAtBottom(next);
    if (next && newCount > 0) setNewCount(0);
  };

  const resolveReply = useCallback(
    (msgId: number): { name: string; text: string } | null => {
      const target = all.find((message) => message.msg_id === msgId);
      if (!target) return null;
      const own = target.outgoing;
      const name = isGroupMessage(target) ? target.from_name : own ? 'You' : header?.name ?? 'Peer';
      return { name, text: replyPreviewText(target.text, target.kind) };
    },
    [all, header?.name],
  );

  const jumpTo = (msgId: number) => {
    const element = document.getElementById(`msg-${msgId}`);
    if (!element) return;
    element.scrollIntoView({ behavior: 'smooth', block: 'center' });
    element.classList.add('rounded-xl', 'ring-2', 'ring-emerald-400/60');
    setTimeout(() => element.classList.remove('rounded-xl', 'ring-2', 'ring-emerald-400/60'), 1400);
  };

  if (!activeThread || !header) return <EmptyThreadState />;

  const isGroup = activeThread.kind === 'group';

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Thread header */}
      <header className="flex h-[60px] shrink-0 items-center gap-2.5 border-b border-white/[0.06] bg-ink-925/50 px-3 md:px-5">
        <IconButton
          icon={ArrowDown}
          label="Back to chats"
          className="rotate-90 md:hidden"
          onClick={closeThread}
        />
        <div className="relative">
          {isGroup ? (
            <span className="grid h-9 w-9 place-items-center rounded-xl border border-emerald-400/20 bg-gradient-to-br from-emerald-400/20 to-sky-500/10 text-emerald-300">
              <Hash className="h-4 w-4" aria-hidden="true" />
            </span>
          ) : (
            <Avatar name={header.name} seed={activeThread.id} size="md" online={header.online} />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <h2 className="truncate text-sm font-semibold text-white">{isGroup ? header.name : header.name}</h2>
            {isGroup ? (
              <ShieldCheck className="h-3.5 w-3.5 shrink-0 text-emerald-400/70" aria-label="Messages are signed" />
            ) : null}
            {!isGroup && header.node ? <RoleBadge role={header.node.class} /> : null}
          </div>
          <p className="mt-0.5 flex items-center gap-1.5 truncate text-3xs text-slate-500">
            {isGroup ? (
              <>
                <span className="font-mono">{shortChannelId(activeThread.id)}</span>
                <span>·</span>
                <span>{header.subtitle}</span>
              </>
            ) : (
              <>
                <span className="font-mono">{activeThread.id.slice(0, 8)}</span>
                <span>·</span>
                <span className="truncate">{header.subtitle}</span>
              </>
            )}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-0.5">
          {!isGroup && header.node ? (
            <>
              <PeerLinkChip
                node={header.node}
                contactName={contacts[activeThread.id]?.name}
                isContact={Boolean(contacts[activeThread.id])}
                onClick={() => setPeerOpen(true)}
                className="mr-1.5"
              />
              <IconButton
                icon={MapPin}
                label="Share my position here"
                onClick={() => {
                  void sharePosition({ peer: activeThread.id });
                }}
              />
              <IconButton icon={UserRound} label="Node details" onClick={() => inspectNode(activeThread.id)} />
            </>
          ) : null}
          {isGroup ? (
            <>
              <IconButton icon={QrCode} label="Invite to channel" onClick={() => setInviteOpen(true)} />
              <IconButton icon={LogOut} label="Leave channel" onClick={() => setLeaveOpen(true)} />
            </>
          ) : null}
          <IconButton
            icon={Info}
            label="Conversation details"
            onClick={() => {
              if (!isGroup) inspectNode(activeThread.id);
              else setInviteOpen(true);
            }}
          />
        </div>
      </header>

      {/* Messages */}
      <div className="relative min-h-0 flex-1">
        <div
          ref={scrollRef}
          onScroll={onScroll}
          className="h-full overflow-y-auto overscroll-contain px-3 py-3 md:px-6"
          role="log"
          aria-label={`Conversation with ${header.name}`}
          aria-live="polite"
        >
          <div className="mx-auto flex w-full max-w-3xl flex-col gap-1">
            {all.length === 0 ? (
              <EmptyState
                helpTopic="messaging"
                icon={MessageSquare}
                title={`Start chatting with ${header.name}`}
                description={
                  isGroup
                    ? 'Messages in this channel are encrypted with the shared key and signed with each sender’s Ed25519 identity.'
                    : 'Messages take the mesh path through nearby nodes. Delivery shows as a double check when an ack arrives.'
                }
                className="py-20"
              />
            ) : (
              <>
                <ThreadIntro isGroup={isGroup} name={header.name} subtitle={header.subtitle} address={activeThread.id} nodeRssi={header.node?.rssi} />
                {dayGroups.map((group) => (
                  <div key={group.key} className="flex flex-col gap-1">
                    <ul className="flex flex-col gap-1">
                      <DaySeparator label={group.label} />
                      {group.items.map((message, index) => {
                        const previous = group.items[index - 1];
                        const next = group.items[index + 1];
                        const sameSender = (other: AnyChatMessage | undefined) =>
                          Boolean(other) &&
                          other!.from === message.from &&
                          Math.abs(other!.sent_at_ms - message.sent_at_ms) < 4 * 60_000;
                        const showSender = isGroup && !sameSender(previous);
                        const showAvatar = !message.outgoing && (!next || next.outgoing || !sameSender(next));
                        return (
                          <MessageBubble
                            key={message.msg_id || message.tempId || index}
                            message={message}
                            peerName={header.name}
                            showAvatar={showAvatar}
                            showSender={showSender}
                            resolveReply={resolveReply}
                            onReply={(target) => {
                              if (!target.msg_id) return;
                              setReplyTo(target.msg_id);
                            }}
                            onRetry={(msgId) => retryMessage(msgId)}
                            onCancel={(msgId) => void cancelMessage(msgId)}
                            onJumpTo={jumpTo}
                          />
                        );
                      })}
                    </ul>
                  </div>
                ))}
              </>
            )}
            <div ref={bottomRef} className="h-2" />
          </div>
        </div>

        {(!atBottom || newCount > 0) && all.length > 0 ? (
          <button
            type="button"
            onClick={() => scrollToBottom(true)}
            className="absolute bottom-4 right-4 z-20 flex items-center gap-1.5 rounded-full border border-white/10 bg-ink-800/95 py-1.5 pl-2.5 pr-3 text-2xs font-medium text-slate-200 shadow-pop backdrop-blur transition-colors hover:bg-ink-750"
            aria-label="Jump to latest messages"
          >
            <ArrowDown className="h-3.5 w-3.5" aria-hidden="true" />
            {newCount > 0 ? `${newCount} new` : 'Latest'}
          </button>
        ) : null}
      </div>

      <Composer thread={activeThread} />

      <Modal
        open={peerOpen}
        onClose={() => setPeerOpen(false)}
        title={`${header.name} · peer & link`}
        description="Who this peer is and how stable the path to them looks right now."
        size="md"
      >
        {header.node ? (
          <PeerLinkPanel
            node={header.node}
            contactName={contacts[activeThread.id]?.name}
            isContact={Boolean(contacts[activeThread.id])}
          />
        ) : null}
      </Modal>

      <ChannelInviteModal open={inviteOpen} channel={header.channel} onClose={() => setInviteOpen(false)} />
      <ConfirmDialog
        open={leaveOpen}
        onClose={() => setLeaveOpen(false)}
        onConfirm={() => {
          leaveChannel(activeThread.id);
          setLeaveOpen(false);
          closeThread();
          pushToast({ kind: 'info', title: `Left ${header.name}` });
        }}
        title={`Leave ${header.name}?`}
        description="You will stop receiving this channel’s messages. The bridge drops the key locally; you can rejoin later with the invite."
        confirmLabel="Leave channel"
        icon={LogOut}
      />
    </div>
  );
}

function ThreadIntro({
  isGroup,
  name,
  subtitle,
  address,
  nodeRssi,
}: {
  isGroup: boolean;
  name: string;
  subtitle: string;
  address: string;
  nodeRssi?: number | null;
}) {
  return (
    <div className="mb-3 mt-2 flex flex-col items-center gap-2 text-center">
      <div className="relative">
        {isGroup ? (
          <span className="grid h-14 w-14 place-items-center rounded-2xl border border-emerald-400/20 bg-gradient-to-br from-emerald-400/15 to-sky-500/10 text-emerald-300">
            <Hash className="h-6 w-6" aria-hidden="true" />
          </span>
        ) : (
          <Avatar name={name} seed={address} size="xl" shape="circle" />
        )}
      </div>
      <div>
        <p className="text-sm font-semibold text-slate-200">{isGroup ? name : name}</p>
        <p className="mt-0.5 text-2xs text-slate-500">{subtitle}</p>
      </div>
      <Badge tone={isGroup ? 'accent' : 'sky'} icon={isGroup ? ShieldCheck : undefined}>
        {isGroup ? 'Channel · shared PSK · Ed25519 signed' : `Direct · sealed with static ECDH${nodeRssi != null ? ` · ${nodeRssi} dBm` : ''}`}
      </Badge>
      <p className="max-w-sm text-3xs leading-relaxed text-slate-600">
        {isGroup
          ? 'Anyone with the channel key can read and write; signatures prove who sent each message.'
          : 'Only this node and you can decrypt these messages. Relays forward sealed envelopes and never see content.'}
      </p>
    </div>
  );
}
