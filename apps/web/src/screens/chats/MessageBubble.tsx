import { useEffect, useMemo, useRef, useState } from 'react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import {
  AlertTriangle,
  CornerUpLeft,
  Download,
  Info,
  Loader2,
  MapPin,
  Pause,
  Play,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
} from 'lucide-react';
import type { ChatGroupMessage, ChatMessage, DeliveryState, Position } from '../../types/api';
import { useAppStore } from '../../store/app';
import { formatClockTime, cn, formatDuration, truncateGraphemes } from '../../lib/utils';
import { deliveryPresentation, displayName } from '../../lib/display';
import { DEFAULT_PROFILE_ID, DEFAULT_REGION_ID, estimateAttachment } from '../../lib/airtime';
import { describeOutgoingMedia, mediaWallClockMs } from '../../lib/media-transfer';
import type { OutgoingMediaState } from '../../lib/media-transfer';
import { speechWaveform } from '../../lib/audio';
import { Avatar, IconButton } from '../../components/ui/primitives';
import { DeliveryStatus, SignalBars } from '../../components/ui/signals';
import { Modal } from '../../components/ui/overlay';
import { HelpLink } from '../../components/ui/help';
import { codec2PayloadDurationMs, isCodec2Mime, voiceAttachmentLabel, voiceTierForMime } from '../../lib/media-tiers';
import { playCodec2Payload } from '../../voice/pcm';

export type AnyChatMessage = ChatMessage | ChatGroupMessage;

export function isGroupMessage(message: AnyChatMessage): message is ChatGroupMessage {
  return 'channel_id' in message;
}

/* ------------------------------------------------------------------ */
/* Attachment source resolution                                       */
/* ------------------------------------------------------------------ */

export function useAttachmentSrc(message: Pick<AnyChatMessage, 'localUrl' | 'attachment'> & { seeded?: boolean }): {
  src: string | null;
  loading: boolean;
  error: string | null;
  /** True while the bridge is still transferring the bytes (incoming attachment, progress < 1). */
  transferring: boolean;
} {
  const resolveAttachmentUrl = useAppStore((state) => state.resolveAttachmentUrl);
  const [src, setSrc] = useState<string | null>(message.localUrl ?? null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const attachmentId = message.attachment?.id;
  const progress = message.attachment?.progress ?? 1;
  const transferring = Boolean(attachmentId) && progress < 1;

  useEffect(() => {
    if (message.localUrl) {
      setSrc(message.localUrl);
      setLoading(false);
      setError(null);
      return;
    }
    if (!attachmentId) {
      setSrc(null);
      setLoading(false);
      setError(null);
      return;
    }
    if (progress < 1) {
      // The bridge has not stored the bytes yet (`message_update`/`resource_progress` will bump
      // progress to 1.0). Fetching the not-yet-available id would latch an error, so wait.
      setSrc(null);
      setLoading(true);
      setError(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    resolveAttachmentUrl(attachmentId, message.attachment?.mime)
      .then((url) => {
        if (!cancelled) setSrc(url);
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(cause instanceof Error ? cause.message : 'Attachment unavailable');
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [attachmentId, message.localUrl, message.attachment?.mime, progress, resolveAttachmentUrl]);

  return { src, loading, error, transferring };
}

/* ------------------------------------------------------------------ */
/* Image                                                              */
/* ------------------------------------------------------------------ */

function ImageAttachment({ message, own = false }: { message: AnyChatMessage; own?: boolean }) {
  const { src, loading, error } = useAttachmentSrc(message);
  const [open, setOpen] = useState(false);
  const attachment = message.attachment;
  const progress = attachment?.progress ?? 1;
  const transferring = progress < 1;

  if (!attachment) return null;

  return (
    <>
      <button
        type="button"
        onClick={() => src && setOpen(true)}
        className="group/img relative mt-1 block w-full max-w-[19rem] overflow-hidden rounded-xl border border-white/[0.08] bg-ink-950/60 text-left focus-visible:ring-2 focus-visible:ring-accent/60"
        aria-label={`Open image ${attachment.name ?? 'attachment'}`}
      >
        {src ? (
          <img
            src={src}
            alt={attachment.name ?? 'Image attachment'}
            className="max-h-[18rem] w-full object-cover transition-transform duration-300 group-hover/img:scale-[1.015]"
            loading="lazy"
          />
        ) : (
          <div className="grid h-40 w-full place-items-center bg-ink-900/70 text-2xs text-slate-500">
            {error ? 'Image unavailable' : loading || transferring ? (own ? 'Sending image…' : 'Receiving image…') : 'No preview'}
          </div>
        )}
        {/* Outgoing media progress is shown by the transfer line under the bubble, with radio wording. */}
        {transferring && !own ? (
          <div className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-ink-950/90 to-transparent p-2.5 pt-8">
            <div className="flex items-center justify-between text-3xs text-slate-300">
              <span>Receiving…</span>
              <span className="tabular-nums">{Math.round(progress * 100)}%</span>
            </div>
            <div className="mt-1 h-1 overflow-hidden rounded-full bg-white/10">
              <div className="h-full rounded-full bg-emerald-400 transition-[width]" style={{ width: `${progress * 100}%` }} />
            </div>
          </div>
        ) : null}
        <span className="absolute right-1.5 top-1.5 rounded-md bg-ink-950/70 px-1.5 py-0.5 text-3xs text-slate-300 opacity-0 backdrop-blur transition-opacity group-hover/img:opacity-100">
          {attachment.size ? `${Math.max(1, Math.round(attachment.size / 1024))} KiB` : 'image'}
        </span>
      </button>

      <Modal
        open={open}
        onClose={() => setOpen(false)}
        title={attachment.name ?? 'Image'}
        description={`${attachment.mime} · ${Math.max(1, Math.round(attachment.size / 1024))} KiB`}
        size="lg"
        footer={
          src ? (
            <a
              href={src}
              download={attachment.name ?? 'openmesh-image'}
              className="btn-base h-9 gap-2 rounded-xl border border-white/10 bg-white/[0.05] px-3.5 text-[13px] text-slate-200 hover:bg-white/[0.08]"
            >
              <Download className="h-4 w-4" aria-hidden="true" />
              Save
            </a>
          ) : undefined
        }
      >
        {src ? (
          <img src={src} alt={attachment.name ?? 'Image attachment'} className="mx-auto max-h-[70vh] w-auto rounded-xl" />
        ) : (
          <div className="grid h-64 place-items-center text-sm text-slate-500">Image unavailable</div>
        )}
      </Modal>
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Voice note                                                         */
/* ------------------------------------------------------------------ */

export function VoiceWaveform({
  waveform,
  progress = 0,
  tone = 'accent',
  className,
}: {
  waveform: number[];
  progress?: number;
  tone?: 'accent' | 'light';
  className?: string;
}) {
  return (
    <span className={cn('flex h-8 flex-1 items-center gap-[2px]', className)} aria-hidden="true">
      {waveform.map((amplitude, index) => {
        const active = index / waveform.length <= progress;
        return (
          <span
            key={index}
            className={cn(
              'w-[2px] shrink-0 rounded-full transition-colors',
              tone === 'accent'
                ? active
                  ? 'bg-emerald-400'
                  : 'bg-emerald-400/25'
                : active
                  ? 'bg-white'
                  : 'bg-white/30',
            )}
            style={{ height: `${Math.max(18, Math.round(amplitude * 100))}%` }}
          />
        );
      })}
    </span>
  );
}

function VoiceAttachment({ message, own }: { message: AnyChatMessage; own: boolean }) {
  const { src, loading, error, transferring } = useAttachmentSrc(message);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const playbackRef = useRef<{ stop: () => void } | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const [playing, setPlaying] = useState(false);
  const [currentMs, setCurrentMs] = useState(0);
  const [decoderError, setDecoderError] = useState<string | null>(null);
  const [decodedDurationMs, setDecodedDurationMs] = useState<number | null>(null);
  const attachment = message.attachment;
  const mime = attachment?.mime ?? null;
  const codec2 = isCodec2Mime(mime);
  const remoteCodec2 = codec2 && !message.localUrl;
  // Messages can omit `duration_ms` (channel/phone payloads); Codec2 is fixed-rate so the codec
  // payload size gives an exact duration. The decoded duration wins once playback has loaded.
  const durationMs = attachment?.duration_ms ?? decodedDurationMs ?? codec2PayloadDurationMs(attachment?.size, mime) ?? 0;
  const waveform = useMemo(
    () => (message.waveform?.length ? message.waveform : speechWaveform(durationMs || 4000, 32, 5)),
    [message.waveform, durationMs],
  );
  const progress = durationMs > 0 ? Math.min(1, currentMs / durationMs) : 0;
  const seekWidthRef = useRef<HTMLDivElement>(null);
  const label = voiceAttachmentLabel(mime, durationMs, attachment?.size ?? null);

  useEffect(() => {
    if (!src || remoteCodec2) return;
    const audio = new Audio(src);
    audio.preload = 'metadata';
    audioRef.current = audio;
    // Opus/AAC payloads often arrive without duration metadata; read the real clip length from
    // the decoded media element so the bubble does not fall back to a generic 4 s placeholder.
    const onMetadata = () => {
      if (Number.isFinite(audio.duration) && audio.duration > 0) setDecodedDurationMs(audio.duration * 1000);
    };
    if (audio.readyState >= 1) onMetadata();
    audio.addEventListener('loadedmetadata', onMetadata);
    const onTime = () => setCurrentMs(audio.currentTime * 1000);
    const onEnd = () => {
      setPlaying(false);
      setCurrentMs(0);
    };
    const onPause = () => setPlaying(false);
    const onPlay = () => setPlaying(true);
    audio.addEventListener('timeupdate', onTime);
    audio.addEventListener('ended', onEnd);
    audio.addEventListener('pause', onPause);
    audio.addEventListener('play', onPlay);
    return () => {
      audio.pause();
      audio.removeEventListener('loadedmetadata', onMetadata);
      audio.removeEventListener('timeupdate', onTime);
      audio.removeEventListener('ended', onEnd);
      audio.removeEventListener('pause', onPause);
      audio.removeEventListener('play', onPlay);
      audioRef.current = null;
    };
  }, [src, remoteCodec2]);

  useEffect(() => {
    return () => {
      playbackRef.current?.stop();
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, []);

  const stopRemotePlayback = () => {
    playbackRef.current?.stop();
    playbackRef.current = null;
    if (timerRef.current) clearInterval(timerRef.current);
    timerRef.current = null;
    setPlaying(false);
    setCurrentMs(0);
  };

  const toggle = async () => {
    if (remoteCodec2) {
      if (playing) {
        stopRemotePlayback();
        return;
      }
      if (!src) return;
      setDecoderError(null);
      try {
        const payload = new Uint8Array(await (await fetch(src)).arrayBuffer());
        const tier = voiceTierForMime(mime);
        if (!tier || tier === 'high') throw new Error('Not a Codec2 payload');
        const playback = await playCodec2Payload(payload, tier);
        playbackRef.current = playback;
        setDecodedDurationMs(playback.durationMs);
        setPlaying(true);
        const started = performance.now();
        timerRef.current = setInterval(() => {
          const elapsed = performance.now() - started;
          setCurrentMs(Math.min(playback.durationMs, elapsed));
          if (elapsed >= playback.durationMs) stopRemotePlayback();
        }, 120);
      } catch (cause) {
        setDecoderError(cause instanceof Error ? cause.message : 'Codec2 decoder unavailable');
        setPlaying(false);
      }
      return;
    }
    const audio = audioRef.current;
    if (!audio) return;
    if (audio.paused) {
      void audio
        .play()
        .then(() => setPlaying(true))
        .catch(() => setPlaying(false));
    } else {
      audio.pause();
    }
  };

  const seek = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (remoteCodec2) return;
    const audio = audioRef.current;
    const element = seekWidthRef.current;
    if (!audio || !element || !Number.isFinite(audio.duration)) return;
    const rect = element.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
    audio.currentTime = ratio * audio.duration;
    setCurrentMs(audio.currentTime * 1000);
  };

  const totalMs = durationMs || 4000;
  const shownMs = playing || currentMs > 0 ? currentMs : totalMs;

  return (
    <div
      className={cn(
        'mt-1 w-[min(19rem,70vw)] rounded-xl border px-2.5 py-2',
        own ? 'border-emerald-300/15 bg-emerald-950/25' : 'border-white/[0.07] bg-white/[0.03]',
      )}
    >
      <div className="flex items-center gap-2.5">
        <button
          type="button"
          onClick={() => void toggle()}
          disabled={!src}
          aria-label={playing ? 'Pause voice message' : 'Play voice message'}
          className={cn(
            'grid h-8 w-8 shrink-0 place-items-center rounded-full transition-colors',
            own ? 'bg-emerald-400 text-emerald-950 hover:bg-emerald-300' : 'bg-white/10 text-white hover:bg-white/15',
            !src && 'opacity-50',
          )}
        >
          {loading ? (
            <span className="h-3 w-3 animate-spin rounded-full border-2 border-current border-t-transparent" />
          ) : playing ? (
            <Pause className="h-3.5 w-3.5" fill="currentColor" aria-hidden="true" />
          ) : (
            <Play className="ml-0.5 h-3.5 w-3.5" fill="currentColor" aria-hidden="true" />
          )}
        </button>
        <div
          ref={seekWidthRef}
          onClick={seek}
          className={cn('flex min-w-0 flex-1 items-center', remoteCodec2 ? 'cursor-default' : 'cursor-pointer')}
          role="presentation"
        >
          <VoiceWaveform waveform={waveform} progress={progress} />
        </div>
        <span className="shrink-0 font-mono text-3xs tabular-nums text-slate-400">{formatDuration(shownMs)}</span>
      </div>
      <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-3xs text-slate-500">
        <span>{label}</span>
        {transferring ? <span className="text-slate-400">Receiving…</span> : null}
        {decoderError ? (
          <span className="flex items-center gap-1 text-amber-300/90" title={decoderError}>
            <AlertTriangle className="h-2.5 w-2.5" aria-hidden="true" />
            Codec2 decoder unavailable
          </span>
        ) : null}
        {error ? <span className="flex items-center gap-1 text-amber-300/90">Audio unavailable</span> : null}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Position card                                                      */
/* ------------------------------------------------------------------ */

function PositionCard({ position, own }: { position: Position | null; own: boolean }) {
  const setScreen = useAppStore((state) => state.setScreen);
  if (!position) return null;
  return (
    <div className={cn('mt-1 overflow-hidden rounded-xl border', own ? 'border-emerald-300/15 bg-emerald-950/25' : 'border-white/[0.07] bg-white/[0.03]')}>
      <div className="flex items-center gap-2.5 px-2.5 py-2">
        <span className="grid h-8 w-8 place-items-center rounded-lg bg-sky-400/15 text-sky-300">
          <MapPin className="h-4 w-4" aria-hidden="true" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-xs font-medium text-slate-200">Position shared</p>
          <p className="truncate font-mono text-3xs text-slate-500">
            {position.lat.toFixed(5)}, {position.lon.toFixed(5)}
            {position.accuracy_m ? ` · ±${Math.round(position.accuracy_m)} m` : ''}
          </p>
        </div>
        <button
          type="button"
          onClick={() => setScreen('map')}
          className="focus-ring shrink-0 rounded-md border border-white/10 bg-white/[0.05] px-2 py-1 text-3xs font-medium text-slate-300 hover:bg-white/[0.09]"
        >
          Map
        </button>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Sender colours                                                     */
/* ------------------------------------------------------------------ */

const SENDER_COLORS = ['text-emerald-300', 'text-sky-300', 'text-violet-300', 'text-amber-300', 'text-rose-300', 'text-cyan-300'];

function senderColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i += 1) hash = (Math.imul(hash, 31) + name.charCodeAt(i)) >>> 0;
  return SENDER_COLORS[hash % SENDER_COLORS.length]!;
}

/* ------------------------------------------------------------------ */
/* Outgoing media transfer line                                       */
/* ------------------------------------------------------------------ */

/**
 * A slow clock for outgoing-media bubbles: it flips a silent transfer into the
 * stalled state even when no store event arrives to re-render the row.
 */
function useMediaClock(active: boolean, intervalMs = 5_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(timer);
  }, [active, intervalMs]);
  return now;
}

function MediaTransferLine({
  media,
  onRetry,
}: {
  media: OutgoingMediaState;
  onRetry: () => void;
}) {
  if (media.phase !== 'preparing' && media.phase !== 'transferring' && media.phase !== 'stalled') return null;
  const percent = media.progress === null ? null : Math.round(media.progress * 100);

  if (media.phase === 'stalled') {
    return (
      <div className="mt-1 w-full max-w-[21rem] pr-1 text-3xs text-amber-300">
        <div className="flex flex-wrap items-center gap-1.5">
          <AlertTriangle className="h-3 w-3 shrink-0" aria-hidden="true" />
          <span>{media.label}</span>
          {media.canRetry ? (
            <button
              type="button"
              onClick={onRetry}
              className="focus-ring inline-flex items-center gap-1 rounded px-1 py-0.5 font-medium text-amber-200 hover:bg-amber-400/10"
            >
              <RefreshCw className="h-2.5 w-2.5" aria-hidden="true" />
              Retry
            </button>
          ) : null}
        </div>
        {media.hint ? <p className="mt-1 leading-relaxed text-amber-200/75">{media.hint}</p> : null}
      </div>
    );
  }

  return (
    <div className="mt-1 w-full max-w-[21rem] pr-1 text-3xs text-slate-500">
      <div className="flex items-center justify-between gap-2">
        <span className="flex items-center gap-1">
          {media.phase === 'transferring' ? <Loader2 className="h-2.5 w-2.5 animate-spin" aria-hidden="true" /> : null}
          {media.label}
        </span>
        {percent !== null ? <span className="font-mono tabular-nums">{percent}%</span> : null}
      </div>
      {percent !== null ? (
        <div
          role="progressbar"
          aria-label="Media transfer progress"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
          className="mt-1 h-1 overflow-hidden rounded-full bg-white/[0.08]"
        >
          <div
            className={cn('h-full rounded-full transition-[width]', media.slow ? 'bg-amber-400' : 'bg-emerald-400')}
            style={{ width: `${percent}%` }}
          />
        </div>
      ) : null}
      {media.hint ? <p className="mt-1 leading-relaxed">{media.hint}</p> : null}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Message bubble                                                     */
/* ------------------------------------------------------------------ */

export interface MessageBubbleProps {
  message: AnyChatMessage;
  peerName?: string;
  showAvatar: boolean;
  showSender: boolean;
  resolveReply: (msgId: number) => { name: string; text: string } | null;
  onReply: (message: AnyChatMessage) => void;
  onRetry: (msgId: number) => void;
  onJumpTo: (msgId: number) => void;
  nowMs?: number;
}

export function MessageBubble({ message, peerName, showAvatar, showSender, resolveReply, onReply, onRetry, onJumpTo, nowMs = Date.now() }: MessageBubbleProps) {
  const own = message.outgoing;
  const group = isGroupMessage(message);
  const nodes = useAppStore((state) => state.nodes);
  const contacts = useAppStore((state) => state.contacts);
  const delivery: DeliveryState = !group && 'delivery' in message ? (message.delivery as DeliveryState) : own ? 'sent' : 'sent';
  const senderName = own
    ? 'You'
    : group
      ? displayName({
          announcedName: message.from_name,
          contactName: contacts[message.from]?.name,
          nodeId: message.from,
        })
      : peerName?.trim() || displayName({
          announcedName: nodes[message.peer]?.name,
          contactName: contacts[message.peer]?.name,
          nodeId: message.peer,
        });
  const replyToId = 'reply_to' in message ? message.reply_to : null;
  const reply = replyToId !== null ? resolveReply(replyToId) : null;
  const isRecent = nowMs - message.sent_at_ms < 90_000;
  const failed = delivery === 'failed' || Boolean(message.error);
  const pending = delivery === 'pending';
  const [detailsOpen, setDetailsOpen] = useState(false);
  const deliveryInfo = deliveryPresentation({
    state: delivery,
    group,
    peerName: peerName ?? senderName,
    error: message.error,
  });
  const config = useAppStore((state) => state.config);
  const ownAttachment = own && message.kind === 'attachment' && Boolean(message.attachment);
  const mediaClock = useMediaClock(
    ownAttachment &&
      delivery !== 'delivered' &&
      delivery !== 'read' &&
      !message.error &&
      (message.attachment?.progress ?? 0) < 1,
  );
  const outgoingMedia = useMemo<OutgoingMediaState | null>(() => {
    if (!own || !message.attachment) return null;
    const size = message.attachment.size ?? 0;
    const estimate =
      size > 0
        ? estimateAttachment(
            size,
            config?.radio.profile ?? DEFAULT_PROFILE_ID,
            config?.radio.region ?? DEFAULT_REGION_ID,
          )
        : null;
    return describeOutgoingMedia({
      attachment: message.attachment,
      delivery,
      error: message.error,
      msgId: message.msg_id,
      group,
      sentAtMs: message.sent_at_ms,
      activityAtMs: message.mediaActivityAtMs ?? null,
      nowMs: mediaClock,
      wallClockMs: estimate ? mediaWallClockMs(estimate, config?.radio.duty_cycle_pct) : null,
    });
  }, [
    own,
    message.attachment,
    message.msg_id,
    message.sent_at_ms,
    message.mediaActivityAtMs,
    message.error,
    delivery,
    group,
    mediaClock,
    config?.radio.profile,
    config?.radio.region,
    config?.radio.duty_cycle_pct,
  ]);
  const detailsRows: Array<{ label: string; value: string }> = [
    { label: 'Direction', value: own ? `You → ${group ? `#${message.channel_name}` : peerName ?? 'peer'}` : `${senderName} → You` },
    { label: 'Sent', value: `${formatClockTime(message.sent_at_ms)} · ${new Date(message.sent_at_ms).toLocaleString()}` },
    { label: 'Kind', value: message.kind },
    { label: 'Receipts', value: deliveryInfo.hasReceipt ? 'Per-recipient receipt (direct message)' : 'Broadcast — no per-recipient receipt' },
    { label: 'Status', value: `${deliveryInfo.label} — ${deliveryInfo.explanation}` },
    { label: 'Message id', value: message.msg_id ? `#${message.msg_id}` : message.tempId ?? 'optimistic' },
  ];
  if (group) {
    detailsRows.push({ label: 'Channel', value: `#${message.channel_name} · ${message.channel_id.slice(0, 8)}` });
    detailsRows.push({ label: 'Signature', value: message.signature_valid ? 'Verified sender signature' : 'Not verified' });
  }
  if (message.attachment) {
    detailsRows.push({
      label: 'Attachment',
      value: `${message.attachment.mime} · ${Math.max(1, Math.round(message.attachment.size / 1024))} KiB · ${Math.round((message.attachment.progress ?? 1) * 100)}%`,
    });
  }
  if (outgoingMedia) {
    detailsRows.push({
      label: 'Media transfer',
      value: `${outgoingMedia.label}${outgoingMedia.progress !== null ? ` · ${Math.round(outgoingMedia.progress * 100)} %` : ''}`,
    });
  }
  if (replyToId !== null) detailsRows.push({ label: 'Reply to', value: `#${replyToId}` });

  return (
    <li
      id={`msg-${message.msg_id || message.tempId}`}
      className={cn('group/msg flex w-full items-end gap-2', own ? 'flex-row-reverse' : 'flex-row', 'animate-fade-in')}
    >
      <div className="w-7 shrink-0 self-end">
        {showAvatar && !own ? (
          <Avatar name={senderName} seed={group ? message.from : message.peer} size="xs" shape="circle" />
        ) : null}
      </div>

      <div className={cn('flex min-w-0 max-w-[min(84%,34rem)] flex-col', own ? 'items-end' : 'items-start')}>
        {showSender && group && !own ? (
          <p className={cn('mb-1 flex items-center gap-1 pl-1 text-3xs font-semibold', senderColor(senderName))}>
            {senderName}
            {message.signature_valid ? (
              <ShieldCheck className="h-2.5 w-2.5 text-emerald-400/80" aria-label="Signature verified" />
            ) : (
              <ShieldAlert className="h-2.5 w-2.5 text-amber-400" aria-label="Signature not verified" />
            )}
          </p>
        ) : null}

        <div className="relative flex max-w-full items-center gap-1.5">
          {!own ? (
            <span className="opacity-0 transition-opacity group-hover/msg:opacity-100">
              <IconButton icon={CornerUpLeft} label="Reply to message" size="sm" onClick={() => onReply(message)} />
            </span>
          ) : null}

          <div
            className={cn(
              'min-w-0 max-w-full rounded-2xl border px-3 py-2 text-[13.5px] leading-relaxed shadow-xs',
              own
                ? 'rounded-br-md border-emerald-300/15 bg-gradient-to-br from-emerald-400/[0.16] to-emerald-500/[0.09] text-slate-100'
                : 'rounded-bl-md border-white/[0.06] bg-ink-800/85 text-slate-200',
              failed && 'border-rose-400/40 bg-rose-500/[0.07]',
              pending && 'opacity-75',
            )}
          >
            {reply ? (
              <button
                type="button"
                onClick={() => onJumpTo(replyToId!)}
                className={cn(
                  'mb-1.5 flex w-full items-start gap-2 rounded-lg border-l-2 px-2 py-1 text-left text-2xs transition-colors',
                  own ? 'border-emerald-300/50 bg-emerald-950/30 hover:bg-emerald-950/50' : 'border-sky-400/50 bg-white/[0.03] hover:bg-white/[0.06]',
                )}
              >
                <span className="min-w-0 flex-1">
                  <span className={cn('block font-medium', own ? 'text-emerald-300/90' : 'text-sky-300/90')}>{reply.name}</span>
                  <span className="mt-0.5 line-clamp-2 block text-slate-400">{truncateGraphemes(reply.text, 120)}</span>
                </span>
              </button>
            ) : null}

            {message.kind === 'attachment' && message.attachment?.mime.startsWith('image/') ? (
              <>
                <ImageAttachment message={message} own={own} />
                {message.text ? <p className="mt-1.5 whitespace-pre-wrap break-words">{message.text}</p> : null}
              </>
            ) : null}

            {message.kind === 'attachment' && message.attachment?.mime.startsWith('audio/') ? (
              <>
                <VoiceAttachment message={message} own={own} />
                {message.text ? <p className="mt-1.5 whitespace-pre-wrap break-words">{message.text}</p> : null}
              </>
            ) : null}

            {message.kind === 'position' ? <PositionCard position={message.position} own={own} /> : null}

            {message.kind === 'text' || (!message.attachment && message.kind !== 'position') ? (
              message.text ? <p className="whitespace-pre-wrap break-words [overflow-wrap:anywhere]">{message.text}</p> : null
            ) : null}

            <div className={cn('mt-1 flex items-center gap-1.5', own ? 'justify-end' : 'justify-start')}>
              {group && message.signature_valid ? (
                <ShieldCheck className="h-3 w-3 text-emerald-400/60" aria-label="Signed by sender" />
              ) : null}
              <time
                dateTime={new Date(message.sent_at_ms).toISOString()}
                className="text-3xs tabular-nums text-slate-500"
                title={new Date(message.sent_at_ms).toLocaleString()}
              >
                {formatClockTime(message.sent_at_ms)}
              </time>
              {own && !group ? (
                <DeliveryStatus state={delivery} peerName={peerName} error={message.error} />
              ) : null}
              {own && group ? <DeliveryStatus state="sent" group /> : null}
              <button
                type="button"
                onClick={() => setDetailsOpen(true)}
                aria-label="Message details"
                title="Message details — direction, receipts and timestamps"
                className="focus-ring grid h-4 w-4 place-items-center rounded text-slate-500 hover:text-slate-300"
              >
                <Info className="h-3 w-3" aria-hidden="true" />
              </button>
            </div>
          </div>

          {own ? (
            <span className="opacity-0 transition-opacity group-hover/msg:opacity-100">
              <IconButton icon={CornerUpLeft} label="Reply to message" size="sm" onClick={() => onReply(message)} />
            </span>
          ) : null}
        </div>

        {own && outgoingMedia ? (
          <MediaTransferLine media={outgoingMedia} onRetry={() => onRetry(message.msg_id)} />
        ) : null}

        {failed ? (
          <div className="mt-1 flex items-center gap-1.5 pr-1 text-3xs text-rose-300">
            <AlertTriangle className="h-3 w-3" aria-hidden="true" />
            <span>{message.error ?? 'Not delivered'}</span>
            {message.msg_id ? (
              <button
                type="button"
                onClick={() => onRetry(message.msg_id)}
                className="focus-ring inline-flex items-center gap-1 rounded px-1 py-0.5 font-medium text-rose-200 hover:bg-rose-400/10"
              >
                <RefreshCw className="h-2.5 w-2.5" aria-hidden="true" />
                Retry
              </button>
            ) : null}
          </div>
        ) : null}

        {pending && isRecent && !outgoingMedia ? (
          <span className="mt-1 pr-1 text-3xs text-slate-500">Sending over LoRa…</span>
        ) : null}

        <Modal
          open={detailsOpen}
          onClose={() => setDetailsOpen(false)}
          title="Message details"
          description={group ? `${senderName} in #${message.channel_name}` : `${senderName} · direct message`}
          size="sm"
        >
          <dl className="space-y-2.5">
            {detailsRows.map((row) => (
              <div key={row.label} className="grid grid-cols-[7rem_1fr] items-baseline gap-2 text-xs">
                <dt className="text-slate-500">{row.label}</dt>
                <dd className="min-w-0 break-words text-slate-200">{row.value}</dd>
              </div>
            ))}
          </dl>
          {failed ? (
            <div className="mt-3 rounded-lg border border-rose-400/25 bg-rose-500/[0.06] px-2.5 py-2 text-2xs text-rose-200">
              {deliveryInfo.explanation}
              {message.msg_id ? (
                <button
                  type="button"
                  onClick={() => {
                    setDetailsOpen(false);
                    onRetry(message.msg_id);
                  }}
                  className="focus-ring ml-2 inline-flex items-center gap-1 rounded px-1 py-0.5 font-medium text-rose-100 hover:bg-rose-400/10"
                >
                  <RefreshCw className="h-2.5 w-2.5" aria-hidden="true" />
                  Retry
                </button>
              ) : null}
            </div>
          ) : null}
        </Modal>
      </div>
    </li>
  );
}

/* ------------------------------------------------------------------ */
/* Day separator                                                      */
/* ------------------------------------------------------------------ */

export function DaySeparator({ label }: { label: string }) {
  return (
    <li className="sticky top-0 z-10 -mx-3 flex justify-center bg-ink-950 px-3 pb-1.5 pt-1 md:-mx-6">
      <span className="rounded-full border border-white/[0.08] bg-ink-900/95 px-2.5 py-0.5 text-3xs font-medium uppercase tracking-[0.1em] text-slate-500 shadow-xs backdrop-blur">
        {label}
      </span>
    </li>
  );
}

/* ------------------------------------------------------------------ */
/* Empty thread state                                                 */
/* ------------------------------------------------------------------ */

export function EmptyThreadState() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 px-8 text-center">
      <div className="relative">
        <div className="grid h-16 w-16 place-items-center rounded-2xl border border-white/[0.07] bg-white/[0.02]">
          <SignalBars rssi={-70} />
        </div>
        <span className="absolute -right-1 -top-1 h-3 w-3 rounded-full border-2 border-ink-950 bg-emerald-400" />
      </div>
      <div>
        <p className="text-sm font-medium text-slate-300">Pick a conversation</p>
        <p className="mx-auto mt-1 max-w-xs text-xs leading-relaxed text-slate-500">
          Messages hop node to node over LoRa. Nothing leaves the mesh, and every send shows its airtime cost before you press it.
        </p>
      </div>
      <HelpLink topic="getting-started">How does OpenMesh work?</HelpLink>
      <div className="flex items-center gap-3 rounded-xl border border-white/[0.05] bg-white/[0.02] px-3 py-2 text-3xs text-slate-500">
        <span className="flex items-center gap-1">
          <kbd className="kbd">Enter</kbd> send
        </span>
        <span className="flex items-center gap-1">
          <kbd className="kbd">Shift</kbd> + <kbd className="kbd">Enter</kbd> newline
        </span>
      </div>
    </div>
  );
}
