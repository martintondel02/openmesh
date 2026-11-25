import { useState } from 'react';
import { cn } from '../lib/utils';
import { useAppStore } from '../store/app';
import { ConversationList } from './chats/ConversationList';
import { ChatThread } from './chats/ChatThread';
import { EmptyThreadState } from './chats/MessageBubble';
import { NewChatModal } from './chats/NewChatModal';
import { CreateChannelModal, JoinChannelModal } from './chats/ChannelModals';

export function ChatsScreen() {
  const activeThread = useAppStore((state) => state.activeThread);
  const openThread = useAppStore((state) => state.openThread);
  const [newChatOpen, setNewChatOpen] = useState(false);
  const [createChannelOpen, setCreateChannelOpen] = useState(false);
  const [joinChannelOpen, setJoinChannelOpen] = useState(false);

  return (
    <div className="flex min-h-0 flex-1">
      <section
        aria-label="Conversations"
        className={cn(
          'flex w-full shrink-0 flex-col border-r border-white/[0.06] bg-ink-925/35 md:w-[318px] lg:w-[344px]',
          activeThread && 'max-md:hidden',
        )}
      >
        <ConversationList onNewChat={() => setNewChatOpen(true)} />
      </section>

      <section
        aria-label="Conversation"
        className={cn('flex min-w-0 flex-1 flex-col', !activeThread && 'max-md:hidden')}
      >
        {activeThread ? <ChatThread /> : <EmptyThreadState />}
      </section>

      <NewChatModal
        open={newChatOpen}
        onClose={() => setNewChatOpen(false)}
        onCreateChannel={() => setCreateChannelOpen(true)}
        onJoinChannel={() => setJoinChannelOpen(true)}
      />

      <CreateChannelModal
        open={createChannelOpen}
        onClose={() => setCreateChannelOpen(false)}
        onCreated={(channelId) => {
          setCreateChannelOpen(false);
          openThread({ kind: 'group', id: channelId });
        }}
      />

      <JoinChannelModal
        open={joinChannelOpen}
        onClose={() => setJoinChannelOpen(false)}
        onJoined={(channelId) => {
          setJoinChannelOpen(false);
          openThread({ kind: 'group', id: channelId });
        }}
      />
    </div>
  );
}
