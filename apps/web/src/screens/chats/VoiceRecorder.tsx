import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Mic, Pause, Play, SendHorizontal, Trash2, TriangleAlert, X } from 'lucide-react';
import { useAppStore } from '../../store/app';
import { formatDuration, cn } from '../../lib/utils';
import {
  downsampleWaveform,
  pickVoiceMimeType,
  speechWaveform,
  waveformFromAnalyser,
} from '../../lib/audio';
import { formatSeconds } from '../../lib/utils';
import { estimateAttachment, voiceCapMs } from '../../lib/airtime';
import { Button, IconButton } from '../../components/ui/primitives';
import { VoiceWaveform } from './MessageBubble';

export interface RecordedVoice {
  blob: Blob;
  mime: string;
  durationMs: number;
  waveform: number[];
  objectUrl: string;
}

type RecorderState = 'requesting' | 'recording' | 'review' | 'error';

/**
 * Inline voice recorder. Uses MediaRecorder with Opus when the browser supports
 * it (`audio/ogg; codecs=opus` is the wire format from PROTOCOL §9.4, WebM is
 * the browser fallback), a WebAudio analyser for the live waveform, and stops
 * automatically at the profile's voice cap.
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
  const maxDurationMs = voiceCapMs(profile);
  const [state, setState] = useState<RecorderState>('requesting');
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
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const startRef = useRef(0);
  const peaksRef = useRef<number[]>([]);
  const stoppedRef = useRef(false);
  const sentRef = useRef(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  const cleanupCapture = useCallback(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    for (const track of streamRef.current?.getTracks() ?? []) track.stop();
    streamRef.current = null;
    analyserRef.current = null;
    if (audioContextRef.current && audioContextRef.current.state !== 'closed') {
      void audioContextRef.current.close().catch(() => undefined);
    }
    audioContextRef.current = null;
  }, []);

  useEffect(() => {
    let cancelled = false;

    async function start() {
      if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === 'undefined') {
        setState('error');
        setError('This browser cannot record audio. Type the message instead.');
        return;
      }
      try {
        const stream = await navigator.mediaDevices.getUserMedia({
          audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
        });
        if (cancelled) {
          for (const track of stream.getTracks()) track.stop();
          return;
        }
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
          const duration = Math.min(maxDurationMs, performance.now() - startRef.current);
          const blob = new Blob(chunksRef.current, { type: recorder.mimeType || mime });
          const finalBars = peaksRef.current.length
            ? downsampleWaveform(peaksRef.current, 32)
            : speechWaveform(duration, 32, 9);
          const objectUrl = URL.createObjectURL(blob);
          setRecorded({ blob, mime: blob.type || mime, durationMs: Math.round(duration), waveform: finalBars, objectUrl });
          setState('review');
          cleanupCapture();
        };

        const analyserData = new Uint8Array(analyser.fftSize);
        recorder.start(250);
        startRef.current = performance.now();
        setState('recording');

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
          if (elapsed >= maxDurationMs) {
            if (recorderRef.current?.state === 'recording') recorderRef.current.stop();
          }
        }, 110);
      } catch (cause) {
        if (cancelled) return;
        setState('error');
        setError(
          cause instanceof DOMException && (cause.name === 'NotAllowedError' || cause.name === 'SecurityError')
            ? 'Microphone permission was denied. Allow mic access, or type the message instead.'
            : 'Could not start the microphone on this device.',
        );
        cleanupCapture();
      }
    }

    void start();
    return () => {
      cancelled = true;
      stoppedRef.current = true;
      if (recorderRef.current && recorderRef.current.state === 'recording') {
        try {
          recorderRef.current.stop();
        } catch {
          // already stopped
        }
      }
      cleanupCapture();
    };
    // Intentionally run once per recorder session.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!recorded) return;
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
  }, [recorded]);

  useEffect(() => {
    return () => {
      // Once a clip has been handed to the composer the message bubble owns the URL.
      if (recorded?.objectUrl && !sentRef.current) URL.revokeObjectURL(recorded.objectUrl);
    };
  }, [recorded]);

  const stop = () => {
    if (recorderRef.current?.state === 'recording') recorderRef.current.stop();
  };

  const discard = () => {
    sentRef.current = true;
    if (recorded) URL.revokeObjectURL(recorded.objectUrl);
    setRecorded(null);
    audioRef.current?.pause();
    onCancel();
  };

  const previewToggle = () => {
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
  const remaining = Math.max(0, maxDurationMs - elapsedMs);
  const waveform = state === 'review' && recorded ? recorded.waveform : liveBars;

  return (
    <div className="rounded-2xl border border-white/[0.08] bg-ink-875/95 p-3 shadow-panel animate-slide-up">
      <div className="flex items-center gap-3">
        <div
          className={cn(
            'grid h-9 w-9 shrink-0 place-items-center rounded-full',
            state === 'recording' ? 'bg-rose-500/15 text-rose-400' : 'bg-white/[0.05] text-slate-400',
          )}
        >
          {state === 'recording' ? (
            <span className="relative flex h-2.5 w-2.5">
              <span className="absolute inset-0 animate-ping-slow rounded-full bg-rose-400/70" />
              <span className="h-2.5 w-2.5 rounded-full bg-rose-400" />
            </span>
          ) : (
            <Mic className="h-4 w-4" aria-hidden="true" />
          )}
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex items-baseline justify-between gap-3">
            <p className="text-xs font-medium text-slate-200" aria-live="polite">
              {state === 'requesting' && 'Waiting for microphone…'}
              {state === 'recording' && (
                <>
                  Recording voice note
                  <span className="ml-1.5 font-mono text-2xs tabular-nums text-rose-300">{formatDuration(elapsedMs)}</span>
                  <span className="ml-1 text-2xs font-normal text-slate-500">/ {formatDuration(maxDurationMs)}</span>
                </>
              )}
              {state === 'review' && (
                <>
                  Voice note ready
                  <span className="ml-1.5 font-mono text-2xs tabular-nums text-emerald-300">
                    {formatDuration(recorded?.durationMs ?? 0)}
                  </span>
                </>
              )}
              {state === 'error' && 'Microphone unavailable'}
            </p>
            {state === 'recording' ? (
              <span className="shrink-0 font-mono text-3xs text-slate-500">{formatDuration(remaining)} left</span>
            ) : null}
          </div>

          {state === 'recording' ? (
            <div className="mt-1.5 flex h-8 items-center">
              <VoiceWaveform waveform={waveform} progress={0.98} />
            </div>
          ) : null}

          {state === 'review' && recorded ? (
            <div className="mt-1.5 flex h-8 items-center gap-2">
              <button
                type="button"
                onClick={previewToggle}
                aria-label={previewPlaying ? 'Pause preview' : 'Play preview'}
                className="grid h-7 w-7 shrink-0 place-items-center rounded-full bg-white/10 text-white hover:bg-white/15"
              >
                {previewPlaying ? (
                  <Pause className="h-3 w-3" fill="currentColor" aria-hidden="true" />
                ) : (
                  <Play className="ml-0.5 h-3 w-3" fill="currentColor" aria-hidden="true" />
                )}
              </button>
              <VoiceWaveform
                waveform={waveform}
                progress={recorded.durationMs > 0 ? previewMs / recorded.durationMs : 0}
              />
            </div>
          ) : null}

          {state === 'error' ? (
            <p className="mt-1 flex items-start gap-1.5 text-2xs leading-relaxed text-rose-300">
              <TriangleAlert className="mt-0.5 h-3 w-3 shrink-0" aria-hidden="true" />
              {error}
            </p>
          ) : null}
        </div>

        <div className="flex shrink-0 items-center gap-1.5">
          <IconButton icon={Trash2} label="Discard recording" size="sm" onClick={discard} />
          {state === 'recording' ? (
            <Button size="sm" variant="danger" onClick={stop} icon={SendHorizontal}>
              Stop
            </Button>
          ) : null}
          {state === 'review' && recorded ? (
            <Button
              size="sm"
              variant="primary"
              icon={SendHorizontal}
              loading={sending}
              onClick={() => {
                audioRef.current?.pause();
                sentRef.current = true;
                onSend(recorded);
              }}
            >
              Send
            </Button>
          ) : null}
          {state === 'requesting' || state === 'error' ? (
            <IconButton icon={X} label="Cancel recording" size="sm" onClick={discard} />
          ) : null}
        </div>
      </div>

      {state === 'review' && estimate && recorded ? (
        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-white/[0.06] pt-2 text-3xs text-slate-500">
          <span className="font-mono text-emerald-300/90">~{formatSeconds(estimate.airtimeMs)} airtime</span>
          <span>{estimate.chunks} chunks</span>
          <span>{Math.max(1, Math.round(recorded.blob.size / 1024))} KiB · Opus</span>
          {estimate.withinCap ? null : <span className="text-amber-300">Over the {Math.round(estimate.capBytes / 1024)} KiB profile cap</span>}
        </div>
      ) : null}
    </div>
  );
}
