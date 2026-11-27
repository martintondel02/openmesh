import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Check, Loader2, Mic, Pause, Play, SendHorizontal, Trash2, TriangleAlert, X } from 'lucide-react';
import { useAppStore } from '../../store/app';
import { formatDuration, formatBytes, formatSeconds, cn } from '../../lib/utils';
import { downsampleWaveform, pickVoiceMimeType, speechWaveform, waveformFromAnalyser } from '../../lib/audio';
import { estimateAttachment } from '../../lib/airtime';
import { SLOW_MEDIA_WALL_CLOCK_MS, formatMediaExpectation, mediaWallClockMs } from '../../lib/media-transfer';
import {
  DEFAULT_VOICE_TIER,
  VOICE_TIERS,
  estimateVoiceTierBytes,
  isCodec2Mime,
  tierAirtimeSummary,
  voiceAttachmentLabel,
  voiceTier,
  voiceTierCapMs,
  type VoiceTierId,
} from '../../lib/media-tiers';
import { encodePcmCodec2, probeCodec2 } from '../../voice/codec2';
import {
  floatToPcm16,
  pcmDurationMs,
  pcmToWavBlob,
  pcmWaveform,
  playCodec2Payload,
  resampleTo8kMono,
  startPcmCapture,
  type PcmCaptureHandle,
} from '../../voice/pcm';
import { Badge, Button, IconButton } from '../../components/ui/primitives';
import { VoiceWaveform } from './MessageBubble';

export interface RecordedVoice {
  blob: Blob;
  mime: string;
  durationMs: number;
  waveform: number[];
  objectUrl: string;
  tier: VoiceTierId;
  codec: 'codec2' | 'opus';
}

type RecorderStage = 'choose' | 'requesting' | 'recording' | 'encoding' | 'review' | 'error';

/**
 * Voice recorder with the shared Tiny/Medium/High tiers.
 *
 * Tiny/Medium capture raw PCM (ScriptProcessor) → OfflineAudioContext resample to 8 kHz mono →
 * Codec2 WASM. High keeps the existing MediaRecorder Opus/OGG path. If the WASM artifact is
 * absent, the two Codec2 tiers are disabled with a note and High still works.
 */
export function VoiceRecorder({
  onCancel,
  onSend,
  sending = false,
}: {
  onCancel: () => void;
  onSend: (voice: RecordedVoice) => void;
  sending?: boolean;
}) {
  const config = useAppStore((state) => state.config);
  const profile = config?.radio.profile ?? 'MEDIUM_FAST';
  const region = config?.radio.region ?? 'EU868';

  const [stage, setStage] = useState<RecorderStage>('choose');
  const [tier, setTier] = useState<VoiceTierId>(DEFAULT_VOICE_TIER);
  const [codec2, setCodec2] = useState<{ state: 'probing' | 'available' | 'unavailable'; message: string | null }>({
    state: 'probing',
    message: null,
  });
  const [error, setError] = useState<string | null>(null);
  const [elapsedMs, setElapsedMs] = useState(0);
  const [liveBars, setLiveBars] = useState<number[]>(() => Array.from({ length: 40 }, () => 0.08));
  const [recorded, setRecorded] = useState<RecordedVoice | null>(null);
  const [previewPlaying, setPreviewPlaying] = useState(false);
  const [previewMs, setPreviewMs] = useState(0);

  const streamRef = useRef<MediaStream | null>(null);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const pcmRef = useRef<PcmCaptureHandle | null>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const startRef = useRef(0);
  const peaksRef = useRef<number[]>([]);
  const stoppedRef = useRef(false);
  const sentRef = useRef(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const codec2PlaybackRef = useRef<{ stop: () => void } | null>(null);
  const previewTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const tierMeta = voiceTier(tier);
  const cap = voiceTierCapMs(tier, profile);
  const maxDurationMs = cap.capMs;

  useEffect(() => {
    let cancelled = false;
    void probeCodec2().then((result) => {
      if (cancelled) return;
      setCodec2({ state: result.available ? 'available' : 'unavailable', message: result.message });
      if (!result.available && (tier === 'tiny' || tier === 'medium')) setTier('high');
    });
    return () => {
      cancelled = true;
    };
    // Probe once per recorder session.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const cleanupCapture = useCallback(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    pcmRef.current?.cancel();
    pcmRef.current = null;
    for (const track of streamRef.current?.getTracks() ?? []) track.stop();
    streamRef.current = null;
    analyserRef.current = null;
    if (audioContextRef.current && audioContextRef.current.state !== 'closed') {
      void audioContextRef.current.close().catch(() => undefined);
    }
    audioContextRef.current = null;
  }, []);

  useEffect(() => {
    return () => {
      if (recorderRef.current?.state === 'recording') {
        try {
          recorderRef.current.stop();
        } catch {
          // already stopped
        }
      }
      cleanupCapture();
    };
  }, [cleanupCapture]);

  useEffect(() => {
    if (!recorded) return;
    if (recorded.codec === 'opus') {
      const audio = new Audio(recorded.objectUrl);
      audioRef.current = audio;
      const onTime = () => setPreviewMs(audio.currentTime * 1000);
      const onEnd = () => {
        setPreviewPlaying(false);
        setPreviewMs(0);
      };
      audio.addEventListener('timeupdate', onTime);
      audio.addEventListener('ended', onEnd);
      return () => {
        audio.pause();
        audio.removeEventListener('timeupdate', onTime);
        audio.removeEventListener('ended', onEnd);
        audioRef.current = null;
      };
    }
    return undefined;
  }, [recorded]);

  useEffect(() => {
    return () => {
      if (recorded?.objectUrl && !sentRef.current) URL.revokeObjectURL(recorded.objectUrl);
      if (previewTimerRef.current) clearInterval(previewTimerRef.current);
      codec2PlaybackRef.current?.stop();
    };
  }, [recorded]);

  const resetPreview = () => {
    audioRef.current?.pause();
    codec2PlaybackRef.current?.stop();
    codec2PlaybackRef.current = null;
    if (previewTimerRef.current) clearInterval(previewTimerRef.current);
    previewTimerRef.current = null;
    setPreviewPlaying(false);
    setPreviewMs(0);
  };

  const startHigh = async () => {
    if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === 'undefined') {
      setStage('error');
      setError('This browser cannot record audio. Type the message instead.');
      return;
    }
    setStage('requesting');
    setError(null);
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
      });
      streamRef.current = stream;
      const mime = pickVoiceMimeType((type) => MediaRecorder.isTypeSupported(type));
      const recorder = new MediaRecorder(stream, { mimeType: mime, audioBitsPerSecond: 16_000 });
      recorderRef.current = recorder;
      chunksRef.current = [];
      const audioContext = new AudioContext();
      audioContextRef.current = audioContext;
      const source = audioContext.createMediaStreamSource(stream);
      const analyser = audioContext.createAnalyser();
      analyser.fftSize = 512;
      analyser.smoothingTimeConstant = 0.65;
      source.connect(analyser);
      analyserRef.current = analyser;
      recorder.ondataavailable = (event) => {
        if (event.data.size > 0) chunksRef.current.push(event.data);
      };
      recorder.onstop = () => {
        if (stoppedRef.current) return;
        stoppedRef.current = true;
        const duration = Math.round(Math.min(maxDurationMs, performance.now() - startRef.current));
        const blob = new Blob(chunksRef.current, { type: recorder.mimeType || mime });
        const finalBars = peaksRef.current.length ? downsampleWaveform(peaksRef.current, 32) : speechWaveform(duration, 32, 9);
        setRecorded({
          blob,
          mime: blob.type || mime,
          durationMs: duration,
          waveform: finalBars,
          objectUrl: URL.createObjectURL(blob),
          tier: 'high',
          codec: 'opus',
        });
        setStage('review');
        cleanupCapture();
      };
      const analyserData = new Uint8Array(analyser.fftSize);
      recorder.start(250);
      startRef.current = performance.now();
      setStage('recording');
      intervalRef.current = setInterval(() => {
        const elapsed = performance.now() - startRef.current;
        setElapsedMs(Math.min(maxDurationMs, elapsed));
        if (analyserRef.current) {
          analyserRef.current.getByteTimeDomainData(analyserData);
          const bars = waveformFromAnalyser(analyserData, 40);
          setLiveBars(bars);
          const peak = Math.max(0.05, bars.reduce((max, value) => Math.max(max, value), 0));
          peaksRef.current.push(peak);
        }
        if (elapsed >= maxDurationMs && recorderRef.current?.state === 'recording') recorderRef.current.stop();
      }, 110);
    } catch (cause) {
      setStage('error');
      setError(
        cause instanceof DOMException && (cause.name === 'NotAllowedError' || cause.name === 'SecurityError')
          ? 'Microphone permission was denied. Allow mic access, or type the message instead.'
          : 'Could not start the microphone on this device.',
      );
      cleanupCapture();
    }
  };

  const startCodec2 = async () => {
    setStage('requesting');
    setError(null);
    try {
      const handle = await startPcmCapture();
      pcmRef.current = handle;
      startRef.current = performance.now();
      setStage('recording');
      intervalRef.current = setInterval(() => {
        const elapsed = performance.now() - startRef.current;
        setElapsedMs(Math.min(maxDurationMs, elapsed));
        const level = handle.level();
        setLiveBars(Array.from({ length: 40 }, (_, index) => Math.max(0.05, level * (0.65 + 0.35 * Math.sin(index * 1.7)))));
        peaksRef.current.push(Math.max(0.05, level));
        if (elapsed >= maxDurationMs) void finishCodec2();
      }, 110);
    } catch (cause) {
      setStage('error');
      setError(
        cause instanceof DOMException && (cause.name === 'NotAllowedError' || cause.name === 'SecurityError')
          ? 'Microphone permission was denied. Allow mic access, or type the message instead.'
          : cause instanceof Error
            ? cause.message
            : 'Could not start the microphone on this device.',
      );
      cleanupCapture();
    }
  };

  const finishCodec2 = async () => {
    const handle = pcmRef.current;
    if (!handle || stoppedRef.current) return;
    stoppedRef.current = true;
    if (intervalRef.current) clearInterval(intervalRef.current);
    intervalRef.current = null;
    setStage('encoding');
    try {
      const samples = await handle.stop();
      pcmRef.current = null;
      const resampled = await resampleTo8kMono(samples, handle.sampleRate);
      const pcm = floatToPcm16(resampled);
      const payload = await encodePcmCodec2(pcm, tier);
      const durationMs = pcmDurationMs(pcm);
      const blob = new Blob([payload.slice().buffer], { type: tierMeta.mime });
      const previewBlob = pcmToWavBlob(pcm);
      setRecorded({
        blob,
        mime: tierMeta.mime,
        durationMs,
        waveform: pcmWaveform(pcm),
        objectUrl: URL.createObjectURL(previewBlob),
        tier,
        codec: 'codec2',
      });
      setStage('review');
      cleanupCapture();
    } catch (cause) {
      setStage('error');
      setError(cause instanceof Error ? cause.message : 'Could not encode this Codec2 recording.');
      cleanupCapture();
    }
  };

  const start = () => {
    stoppedRef.current = false;
    peaksRef.current = [];
    setElapsedMs(0);
    setLiveBars(Array.from({ length: 40 }, () => 0.08));
    if (tier === 'high') void startHigh();
    else void startCodec2();
  };

  const stop = () => {
    if (tier === 'high') {
      if (recorderRef.current?.state === 'recording') recorderRef.current.stop();
    } else {
      void finishCodec2();
    }
  };

  const discard = () => {
    sentRef.current = true;
    resetPreview();
    if (recorded) URL.revokeObjectURL(recorded.objectUrl);
    setRecorded(null);
    onCancel();
  };

  const previewToggle = async () => {
    if (!recorded) return;
    if (recorded.codec === 'codec2') {
      if (previewPlaying) {
        resetPreview();
        return;
      }
      try {
        const payload = new Uint8Array(await recorded.blob.arrayBuffer());
        const playback = await playCodec2Payload(payload, recorded.tier);
        codec2PlaybackRef.current = playback;
        setPreviewPlaying(true);
        const started = performance.now();
        previewTimerRef.current = setInterval(() => {
          const elapsed = performance.now() - started;
          setPreviewMs(Math.min(recorded.durationMs, elapsed));
          if (elapsed >= recorded.durationMs) resetPreview();
        }, 100);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : 'Could not play this Codec2 clip.');
      }
      return;
    }
    const audio = audioRef.current;
    if (!audio) return;
    if (audio.paused) {
      void audio.play().then(() => setPreviewPlaying(true)).catch(() => setPreviewPlaying(false));
    } else {
      audio.pause();
      setPreviewPlaying(false);
    }
  };

  const estimate = useMemo(
    () => (recorded ? estimateAttachment(recorded.blob.size, profile, region) : null),
    [recorded, profile, region],
  );
  const slowExpectation =
    estimate && mediaWallClockMs(estimate, config?.radio.duty_cycle_pct) >= SLOW_MEDIA_WALL_CLOCK_MS
      ? formatMediaExpectation(mediaWallClockMs(estimate, config?.radio.duty_cycle_pct))
      : null;
  const estimatedFullTierBytes = estimateVoiceTierBytes(maxDurationMs, tier);
  const tierCost = tierAirtimeSummary(Math.max(1, estimatedFullTierBytes), profile, region);
  const remaining = Math.max(0, maxDurationMs - elapsedMs);
  const waveform = stage === 'review' && recorded ? recorded.waveform : liveBars;
  const canStart = stage === 'choose' && (tier === 'high' || codec2.state === 'available');
  const tierDisabled = (id: VoiceTierId) => id !== 'high' && codec2.state !== 'available';

  return (
    <div className="rounded-2xl border border-white/[0.08] bg-ink-875/95 p-3 shadow-panel animate-slide-up">
      <div className="flex items-center gap-3">
        <div
          className={cn(
            'grid h-9 w-9 shrink-0 place-items-center rounded-full',
            stage === 'recording' ? 'bg-rose-500/15 text-rose-400' : stage === 'encoding' ? 'bg-sky-400/15 text-sky-300' : 'bg-white/[0.05] text-slate-400',
          )}
        >
          {stage === 'recording' ? (
            <span className="relative flex h-2.5 w-2.5">
              <span className="absolute inset-0 animate-ping-slow rounded-full bg-rose-400/70" />
              <span className="h-2.5 w-2.5 rounded-full bg-rose-400" />
            </span>
          ) : stage === 'encoding' ? (
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          ) : (
            <Mic className="h-4 w-4" aria-hidden="true" />
          )}
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex items-baseline justify-between gap-3">
            <p className="text-xs font-medium text-slate-200" aria-live="polite">
              {stage === 'choose' && 'Voice note tier'}
              {stage === 'requesting' && 'Waiting for microphone…'}
              {stage === 'recording' && (
                <>
                  Recording {tierMeta.label}
                  <span className="ml-1.5 font-mono text-2xs tabular-nums text-rose-300">{formatDuration(elapsedMs)}</span>
                  <span className="ml-1 text-2xs font-normal text-slate-500">/ {formatDuration(maxDurationMs)}</span>
                </>
              )}
              {stage === 'encoding' && 'Encoding Codec2…'}
              {stage === 'review' && (
                <>
                  Voice note ready
                  <span className="ml-1.5 font-mono text-2xs tabular-nums text-emerald-300">{formatDuration(recorded?.durationMs ?? 0)}</span>
                  <Badge tone="slate" className="ml-1.5">{tierMeta.label}</Badge>
                </>
              )}
              {stage === 'error' && 'Microphone unavailable'}
            </p>
            {stage === 'recording' ? <span className="shrink-0 font-mono text-3xs text-slate-500">{formatDuration(remaining)} left</span> : null}
          </div>

          {stage === 'choose' ? (
            <div className="mt-2 space-y-2">
              <div className="grid gap-1.5 sm:grid-cols-3" role="group" aria-label="Voice tier">
                {VOICE_TIERS.map((option) => {
                  const optionCap = voiceTierCapMs(option.id, profile);
                  const disabled = tierDisabled(option.id);
                  const selected = tier === option.id;
                  return (
                    <button
                      key={option.id}
                      type="button"
                      disabled={disabled}
                      aria-pressed={selected}
                      onClick={() => setTier(option.id)}
                      className={cn(
                        'focus-ring rounded-xl border px-2.5 py-2 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-45',
                        selected ? 'border-emerald-400/40 bg-emerald-400/[0.08]' : 'border-white/[0.08] bg-white/[0.02] hover:border-white/20',
                      )}
                    >
                      <span className="flex items-center gap-1 text-xs font-semibold text-slate-200">
                        {option.label}
                        {selected ? <Check className="h-3 w-3 text-emerald-300" aria-hidden="true" /> : null}
                      </span>
                      <span className="mt-0.5 block text-3xs text-slate-500">
                        {option.codec === 'codec2' ? `Codec2 ${option.mode === '1300' ? '1.3k' : '3.2k'}` : `Opus ${Math.round(option.bitsPerSecond / 1000)} kbps`}
                      </span>
                      <span className="mt-0.5 block text-3xs text-slate-600">
                        ≤ {formatDuration(optionCap.capMs)} · ~{formatBytes(estimateVoiceTierBytes(optionCap.capMs, option.id))}
                      </span>
                    </button>
                  );
                })}
              </div>
              {codec2.state === 'unavailable' ? (
                <p className="flex items-start gap-1.5 rounded-lg border border-amber-400/20 bg-amber-400/[0.05] px-2.5 py-1.5 text-3xs leading-relaxed text-amber-200/85">
                  <TriangleAlert className="mt-0.5 h-3 w-3 shrink-0" aria-hidden="true" />
                  {codec2.message ?? 'Codec2 voice is unavailable in this build. High-quality Opus still works.'}
                </p>
              ) : null}
              <p className="text-3xs leading-relaxed text-slate-500">
                {tierMeta.description} Effective cap {formatDuration(cap.capMs)}{' '}
                {cap.byteCapMs < cap.tierCapMs ? `(profile byte cap is lower than the ${tierMeta.maxDurationS} s tier cap)` : ''}.{' '}
                {tierCost.label}.
              </p>
            </div>
          ) : null}

          {stage === 'recording' ? (
            <div className="mt-1.5 flex h-8 items-center">
              <VoiceWaveform waveform={waveform} progress={0.98} />
            </div>
          ) : null}

          {stage === 'review' && recorded ? (
            <div className="mt-1.5 flex h-8 items-center gap-2">
              <button
                type="button"
                onClick={() => void previewToggle()}
                aria-label={previewPlaying ? 'Pause preview' : 'Play preview'}
                className="grid h-7 w-7 shrink-0 place-items-center rounded-full bg-white/10 text-white hover:bg-white/15"
              >
                {previewPlaying ? <Pause className="h-3 w-3" fill="currentColor" aria-hidden="true" /> : <Play className="ml-0.5 h-3 w-3" fill="currentColor" aria-hidden="true" />}
              </button>
              <VoiceWaveform waveform={waveform} progress={recorded.durationMs > 0 ? previewMs / recorded.durationMs : 0} />
            </div>
          ) : null}

          {stage === 'error' ? (
            <p className="mt-1 flex items-start gap-1.5 text-2xs leading-relaxed text-rose-300">
              <TriangleAlert className="mt-0.5 h-3 w-3 shrink-0" aria-hidden="true" />
              {error}
            </p>
          ) : null}
        </div>

        <div className="flex shrink-0 items-center gap-1.5">
          <IconButton icon={Trash2} label="Discard recording" size="sm" onClick={discard} />
          {stage === 'choose' ? (
            <Button size="sm" variant="primary" icon={Mic} disabled={!canStart} onClick={start}>
              Record
            </Button>
          ) : null}
          {stage === 'recording' ? (
            <Button size="sm" variant="danger" onClick={stop} icon={SendHorizontal}>Stop</Button>
          ) : null}
          {stage === 'review' && recorded ? (
            <Button
              size="sm"
              variant="primary"
              icon={SendHorizontal}
              loading={sending}
              onClick={() => {
                resetPreview();
                sentRef.current = true;
                onSend(recorded);
              }}
            >
              Send
            </Button>
          ) : null}
          {stage === 'requesting' || stage === 'error' || stage === 'encoding' ? (
            <IconButton icon={X} label="Cancel recording" size="sm" onClick={discard} />
          ) : null}
        </div>
      </div>

      {stage === 'review' && estimate && recorded ? (
        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-white/[0.06] pt-2 text-3xs text-slate-500">
          <span className="font-mono text-emerald-300/90">~{formatSeconds(estimate.airtimeMs)} airtime</span>
          <span>{estimate.chunks} chunks</span>
          <span>{voiceAttachmentLabel(recorded.mime, recorded.durationMs, recorded.blob.size)}</span>
          {isCodec2Mime(recorded.mime) ? <span className="text-sky-300/80">Codec2 raw · 8 kHz mono</span> : <span>Opus/OGG</span>}
          {estimate.withinCap ? null : <span className="text-amber-300">Over the {Math.round(estimate.capBytes / 1024)} KiB profile cap</span>}
          {slowExpectation ? (
            <span className="w-full text-slate-500">
              At the radio&rsquo;s duty cycle this takes {slowExpectation}; it keeps sending in the background after you press
              Send.
            </span>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
