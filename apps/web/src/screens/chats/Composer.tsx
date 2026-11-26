import { useEffect, useMemo, useRef, useState } from 'react';
import type { ClipboardEvent, DragEvent } from 'react';
import { ArrowUp, ImagePlus, Mic, Paperclip, Reply, TriangleAlert, X } from 'lucide-react';
import { useAppStore } from '../../store/app';
import type { ActiveThread } from '../../store/app';
import { estimateText, getProfile } from '../../lib/airtime';
import { formatBytes, formatSeconds, graphemeLength, messagePreview, cn } from '../../lib/utils';
import { IconButton, Kbd } from '../../components/ui/primitives';
import { SignalBars } from '../../components/ui/signals';
import { ImageComposerModal } from './ImageComposer';
import { createDemoImageFile } from '../../lib/attachments';
import { voiceAttachmentLabel } from '../../lib/media-tiers';
import { VoiceRecorder, type RecordedVoice } from './VoiceRecorder';

/**
 * Chat composer: text, image attachments (with client-side compression) and
 * Opus voice notes, plus a live airtime estimate for whatever is being sent.
 */
export function Composer({ thread }: { thread: ActiveThread }) {
  const config = useAppStore((state) => state.config);
  const appMode = useAppStore((state) => state.mode);
  const messages = useAppStore((state) => state.messages);
  const groupMessages = useAppStore((state) => state.groupMessages);
  const replyTo = useAppStore((state) => state.replyTo);
  const setReplyTo = useAppStore((state) => state.setReplyTo);
  const sendText = useAppStore((state) => state.sendText);
  const sendAttachment = useAppStore((state) => state.sendAttachment);
  const pushToast = useAppStore((state) => state.pushToast);
  const node = useAppStore((state) => state.nodes[thread.kind === 'direct' ? thread.id : ''] ?? null);
  const channelName = useAppStore((state) => (thread.kind === 'group' ? state.channels[thread.id]?.name : undefined));

  const profileId = config?.radio.profile ?? 'MEDIUM_FAST';
  const profile = getProfile(profileId);

  const draftKey = `${thread.kind}:${thread.id}`;
  const setComposerDraft = useAppStore((state) => state.setComposerDraft);
  const clearComposerDraft = useAppStore((state) => state.clearComposerDraft);
  const [text, setTextState] = useState(() => useAppStore.getState().composerDrafts[draftKey] ?? '');
  const setText = (value: string) => {
    setTextState(value);
    setComposerDraft(draftKey, value);
  };
  const [sending, setSending] = useState(false);
  const [imageFile, setImageFile] = useState<File | null>(null);
  const [imageOpen, setImageOpen] = useState(false);
  const [recorderOpen, setRecorderOpen] = useState(false);
  const [voiceSending, setVoiceSending] = useState(false);
  const [dragActive, setDragActive] = useState(false);
  const dragDepth = useRef(0);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setTextState(useAppStore.getState().composerDrafts[draftKey] ?? '');
    setReplyTo(null);
    // Focus the composer when switching threads (desktop only).
    if (window.matchMedia?.('(min-width: 768px)').matches) {
      requestAnimationFrame(() => textareaRef.current?.focus());
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [thread.id, thread.kind, draftKey]);

  // Demo/screenshot helpers: ?media=image opens a synthetic tier preview, ?media=voice the recorder.
  const demoMediaHandled = useRef(false);
  useEffect(() => {
    if (demoMediaHandled.current) return;
    if (appMode !== 'demo') return;
    let requested: string | null = null;
    try {
      requested = new URLSearchParams(window.location.search).get('media');
    } catch {
      requested = null;
    }
    if (!requested) return;
    if (requested === 'voice') {
      demoMediaHandled.current = true;
      setRecorderOpen(true);
    } else if (requested === 'image' || requested === 'tiers' || requested === 'media-tiers') {
      demoMediaHandled.current = true;
      void createDemoImageFile()
        .then((file) => {
          setImageFile(file);
          setImageOpen(true);
        })
        .catch(() => undefined);
    }
    // Demo hydration changes mode after the first render.
  }, [appMode]);

  const replyTarget = useMemo(() => {
    if (!replyTo) return null;
    const all = thread.kind === 'direct' ? messages : groupMessages;
    const found = all.find((message) => message.msg_id === replyTo);
    if (!found) return null;
    const senderName =
      'from_name' in found
        ? found.from_name
        : found.outgoing
          ? 'You'
          : node?.name ?? thread.id.slice(0, 6);
    return { name: senderName, text: messagePreview(found.text ?? (found.kind === 'attachment' ? 'Attachment' : 'Message'), 'Message', 90) };
  }, [replyTo, messages, groupMessages, thread, node]);

  const estimate = useMemo(
    () => estimateText(text, profileId, { replyTo: Boolean(replyTo) }),
    [text, profileId, replyTo],
  );

  const focusComposer = () => textareaRef.current?.focus();

  const submit = async () => {
    const trimmed = text.trim();
    if (!trimmed || sending) return;
    setSending(true);
    try {
      await sendText(trimmed);
      setTextState('');
      clearComposerDraft(draftKey);
      focusComposer();
    } finally {
      setSending(false);
    }
  };

  const onPickImage = (file: File | null | undefined) => {
    if (!file) return;
    setImageFile(file);
    setImageOpen(true);
  };

  const onDrop = (event: DragEvent) => {
    event.preventDefault();
    dragDepth.current = 0;
    setDragActive(false);
    const file = event.dataTransfer.files?.[0];
    if (file) onPickImage(file);
  };

  const onPaste = (event: ClipboardEvent) => {
    const file = Array.from(event.clipboardData.items)
      .find((item) => item.kind === 'file' && item.type.startsWith('image/'))
      ?.getAsFile();
    if (file) {
      event.preventDefault();
      onPickImage(file);
    }
  };

  const onVoiceSend = async (voice: RecordedVoice) => {
    setVoiceSending(true);
    try {
      await sendAttachment(
        {
          blob: voice.blob,
          mime: voice.mime,
          filename: `voice-${voice.tier}-${new Date().toISOString().slice(11, 19).replace(/:/g, '')}.${voice.codec === 'codec2' ? 'c2' : 'ogg'}`,
          durationMs: voice.durationMs,
          waveform: voice.waveform,
          localUrl: voice.objectUrl,
        },
        thread,
      );
      pushToast({
        kind: 'success',
        title: 'Voice note queued',
        message: `${voiceAttachmentLabel(voice.mime, voice.durationMs, voice.blob.size)} · ~${formatBytes(voice.blob.size)} on the wire`,
      });
      setRecorderOpen(false);
    } catch (cause) {
      pushToast({
        kind: 'error',
        title: 'Voice note failed',
        message: cause instanceof Error ? cause.message : String(cause),
      });
    } finally {
      setVoiceSending(false);
    }
  };

  const chars = graphemeLength(text);

  return (
    <div
      className="relative border-t border-white/[0.06] bg-ink-925/75 px-3 pb-3 pt-2.5 backdrop-blur-md md:px-5"
      onDragEnter={(event) => {
        event.preventDefault();
        dragDepth.current += 1;
        setDragActive(true);
      }}
      onDragOver={(event) => event.preventDefault()}
      onDragLeave={(event) => {
        event.preventDefault();
        dragDepth.current = Math.max(0, dragDepth.current - 1);
        if (dragDepth.current === 0) setDragActive(false);
      }}
      onDrop={onDrop}
    >
      {dragActive ? (
        <div className="pointer-events-none absolute inset-2 z-20 grid place-items-center rounded-xl border-2 border-dashed border-emerald-400/60 bg-emerald-400/[0.08] backdrop-blur-sm">
          <div className="flex items-center gap-2 text-sm font-medium text-emerald-200">
            <ImagePlus className="h-4 w-4" aria-hidden="true" />
            Drop an image to compress and send
          </div>
        </div>
      ) : null}

      {replyTarget ? (
        <div className="mb-2 flex items-start gap-2 rounded-lg border border-white/[0.07] bg-white/[0.03] px-2.5 py-1.5">
          <Reply className="mt-0.5 h-3.5 w-3.5 shrink-0 text-emerald-300/80" aria-hidden="true" />
          <div className="min-w-0 flex-1">
            <p className="text-3xs font-medium text-emerald-300/90">Replying to {replyTarget.name}</p>
            <p className="truncate text-2xs text-slate-400">{replyTarget.text}</p>
          </div>
          <IconButton icon={X} label="Cancel reply" size="sm" onClick={() => setReplyTo(null)} />
        </div>
      ) : null}

      {recorderOpen ? (
        <VoiceRecorder
          sending={voiceSending}
          onCancel={() => setRecorderOpen(false)}
          onSend={(voice) => void onVoiceSend(voice)}
        />
      ) : (
        <div className="flex items-end gap-1.5">
          <div className="flex items-center gap-1 pb-1">
            <IconButton
              icon={Paperclip}
              label="Attach an image (choose Tiny/Medium/High/Original)"
              onClick={() => fileInputRef.current?.click()}
            />
            <IconButton
              icon={Mic}
              label="Record a voice note"
              onClick={() => setRecorderOpen(true)}
            />
          </div>

          <div className="relative min-w-0 flex-1">
            <textarea
              ref={textareaRef}
              value={text}
              rows={1}
              placeholder={thread.kind === 'group' ? `Message #${channelName ?? 'channel'}` : `Message ${node?.name ?? 'node'}`}
              aria-label="Message text"
              onChange={(event) => setText(event.target.value)}
              onPaste={onPaste}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
                  event.preventDefault();
                  void submit();
                }
                if (event.key === 'Escape') {
                  if (replyTo) {
                    event.preventDefault();
                    setReplyTo(null);
                  }
                }
              }}
              className="field-shell max-h-40 min-h-[38px] resize-none py-2.5 pr-3 leading-relaxed"
            />
          </div>

          <button
            type="button"
            onClick={() => void submit()}
            disabled={!text.trim() || sending}
            aria-label="Send message"
            title="Send message (Enter)"
            className={cn(
              'mb-0.5 grid h-9 w-9 shrink-0 place-items-center rounded-xl transition-all active:scale-95',
              text.trim()
                ? 'bg-gradient-to-b from-emerald-400 to-emerald-500 text-emerald-950 shadow-[0_8px_20px_-10px_rgba(16,185,129,0.9)] hover:from-emerald-300 hover:to-emerald-400'
                : 'bg-white/[0.05] text-slate-600',
            )}
          >
            <ArrowUp className="h-4 w-4" strokeWidth={2.4} aria-hidden="true" />
          </button>
        </div>
      )}

      <div className="mt-1.5 flex min-h-[16px] items-center gap-2 pl-1 text-3xs text-slate-600">
        {text.trim() ? (
          <>
            <span className={cn('font-mono', estimate.fitsSingleFrame ? 'text-slate-500' : 'text-amber-400/90')}>
              ~{formatSeconds(estimate.airtimeMs)} airtime · {estimate.frames} {estimate.fitsSingleFrame ? 'frame' : 'frames'}
            </span>
            {!estimate.fitsSingleFrame ? (
              <span className="flex items-center gap-1 text-amber-400/80">
                <TriangleAlert className="h-2.5 w-2.5" aria-hidden="true" />
                long text becomes a resource transfer
              </span>
            ) : null}
            {chars > 280 ? <span className="tabular-nums text-slate-500">{chars} chars</span> : null}
            <span className="ml-auto hidden items-center gap-1 sm:flex">
              <Kbd>Enter</Kbd> send · <Kbd>Shift</Kbd>+<Kbd>Enter</Kbd> newline
            </span>
          </>
        ) : (
          <>
            <span className="hidden items-center gap-1.5 sm:flex">
              <ImagePlus className="h-3 w-3" aria-hidden="true" />
              Images are downscaled and compressed locally before they touch the mesh
            </span>
            <span className="text-slate-600 sm:hidden">Images compressed locally · airtime shown before send</span>
            <span className="ml-auto hidden items-center gap-2 sm:flex">
              <SignalBars rssi={node?.rssi ?? null} />
              <span className="text-slate-600">{profile.label}</span>
            </span>
          </>
        )}
      </div>

      <input
        ref={fileInputRef}
        type="file"
        accept="image/*"
        className="hidden"
        aria-label="Choose an image"
        onChange={(event) => {
          onPickImage(event.target.files?.[0]);
          event.target.value = '';
        }}
      />

      <ImageComposerModal
        open={imageOpen}
        file={imageFile}
        thread={thread}
        onClose={() => {
          setImageOpen(false);
          setImageFile(null);
        }}
      />
    </div>
  );
}
