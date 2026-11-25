import { useEffect, useMemo, useRef, useState } from 'react';
import { Image as ImageIcon, Loader2, SendHorizontal, TriangleAlert } from 'lucide-react';
import { useAppStore } from '../../store/app';
import type { ActiveThread } from '../../store/app';
import { compressImage, defaultImageTarget, validateImageFile, type CompressedImage } from '../../lib/attachments';
import { estimateAttachment, getProfile, getRegion, maxAttachmentBytes, hourlyAirtimeBudgetMs } from '../../lib/airtime';
import { formatBytes, formatSeconds, cn } from '../../lib/utils';
import { Button, Badge } from '../../components/ui/primitives';
import { Modal } from '../../components/ui/overlay';
import { Field, Slider, TextInput } from '../../components/ui/forms';

export interface ImageComposerModalProps {
  open: boolean;
  file: File | null;
  thread: ActiveThread | null;
  onClose: () => void;
}

export function ImageComposerModal({ open, file, thread, onClose }: ImageComposerModalProps) {
  const config = useAppStore((state) => state.config);
  const sendAttachment = useAppStore((state) => state.sendAttachment);
  const pushToast = useAppStore((state) => state.pushToast);
  const profileId = config?.radio.profile ?? 'MEDIUM_FAST';
  const regionId = config?.radio.region ?? 'EU868';
  const profile = getProfile(profileId);
  const region = getRegion(regionId);
  const cap = maxAttachmentBytes(profileId);
  const budgetMs = hourlyAirtimeBudgetMs(regionId);

  const [targetBytes, setTargetBytes] = useState(() => defaultImageTarget(cap));
  const [compressed, setCompressed] = useState<CompressedImage | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [caption, setCaption] = useState('');
  const [sending, setSending] = useState(false);
  const previousUrl = useRef<string | null>(null);
  const requestRef = useRef(0);

  useEffect(() => {
    if (!open) return;
    setTargetBytes(defaultImageTarget(cap));
    setCaption('');
    setError(null);
    setCompressed(null);
    return () => {
      if (previousUrl.current) URL.revokeObjectURL(previousUrl.current);
      previousUrl.current = null;
    };
  }, [open, cap, file]);

  useEffect(() => {
    if (!open || !file) return;
    const validation = validateImageFile(file);
    if (validation) {
      setError(validation);
      return;
    }
    let cancelled = false;
    const requestId = requestRef.current + 1;
    requestRef.current = requestId;
    setBusy(true);
    setError(null);
    const timer = setTimeout(() => {
      compressImage(file, { targetBytes, onProgress: () => undefined })
        .then((result) => {
          if (cancelled || requestId !== requestRef.current) {
            URL.revokeObjectURL(result.objectUrl);
            return;
          }
          if (previousUrl.current) URL.revokeObjectURL(previousUrl.current);
          previousUrl.current = result.objectUrl;
          setCompressed(result);
          setBusy(false);
        })
        .catch((cause: unknown) => {
          if (cancelled) return;
          setError(cause instanceof Error ? cause.message : 'Could not compress this image');
          setBusy(false);
        });
    }, 140);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [open, file, targetBytes]);

  const effectiveBytes = compressed?.blob.size ?? targetBytes;
  const estimate = useMemo(() => estimateAttachment(effectiveBytes, profileId, regionId), [effectiveBytes, profileId, regionId]);
  const budgetPct = budgetMs ? (estimate.airtimeMs / budgetMs) * 100 : null;
  const overCap = effectiveBytes > cap;
  const capKiB = Math.round(cap / 1024);
  const sliderMax = Math.max(1, Math.round(cap / 1024));

  const handleSend = async () => {
    if (!compressed || !file) return;
    setSending(true);
    try {
      await sendAttachment(
        {
          blob: compressed.blob,
          mime: compressed.mime,
          filename: file.name || 'image',
          localUrl: compressed.objectUrl,
          width: compressed.width,
          height: compressed.height,
          caption: caption.trim() || undefined,
        },
        thread ?? undefined,
      );
      pushToast({
        kind: 'success',
        title: 'Image queued',
        message: `${estimate.chunks} chunks · ~${formatSeconds(estimate.airtimeMs)} of airtime`,
      });
      previousUrl.current = null; // ownership transferred to the chat bubble
      onClose();
    } catch (cause) {
      pushToast({ kind: 'error', title: 'Could not send image', message: cause instanceof Error ? cause.message : String(cause) });
    } finally {
      setSending(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Send an image over LoRa"
      description={`Compressed locally to fit your ${profile.label} profile (${formatBytes(cap)} cap).`}
      icon={ImageIcon}
      size="lg"
      dismissible={!sending}
      footer={
        <>
          <div className="mr-auto hidden items-center gap-2 text-2xs text-slate-500 sm:flex">
            <Badge tone={overCap ? 'rose' : budgetPct !== null && budgetPct > 25 ? 'amber' : 'accent'}>
              ~{formatSeconds(estimate.airtimeMs)} airtime
            </Badge>
            <span className="tabular-nums">{estimate.chunks} chunks</span>
          </div>
          <Button variant="ghost" onClick={onClose} disabled={sending}>
            Cancel
          </Button>
          <Button
            variant="primary"
            icon={SendHorizontal}
            onClick={() => void handleSend()}
            disabled={!compressed || busy || Boolean(error) || overCap}
            loading={sending}
          >
            Send image
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
          <div className="relative overflow-hidden rounded-xl border border-white/[0.07] bg-ink-950/70">
            {compressed ? (
              <img
                src={compressed.objectUrl}
                alt="Compressed preview"
                className={cn('max-h-56 w-full object-contain transition-opacity', busy && 'opacity-40')}
              />
            ) : (
              <div className="grid h-48 place-items-center text-xs text-slate-500">
                {error ? 'Preview unavailable' : 'Compressing…'}
              </div>
            )}
            {busy ? (
              <div className="absolute inset-0 grid place-items-center bg-ink-950/40">
                <span className="flex items-center gap-2 rounded-full border border-white/10 bg-ink-900/90 px-3 py-1.5 text-2xs text-slate-300">
                  <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                  Re-encoding…
                </span>
              </div>
            ) : null}
          </div>

          <div className="space-y-3.5">
            <Field label="Caption" hint="Optional — text is sent alongside the image manifest.">
              {({ id }) => (
                <TextInput
                  id={id}
                  value={caption}
                  maxLength={180}
                  placeholder="Trail was muddy but the ridge was clear…"
                  onChange={(event) => setCaption(event.target.value)}
                />
              )}
            </Field>

            <div>
              <Slider
                id="image-target-size"
                label="Target size"
                min={1}
                max={sliderMax}
                step={1}
                value={Math.min(sliderMax, Math.max(1, Math.round(targetBytes / 1024)))}
                onChange={(value) => {
                  setTargetBytes(Math.max(1024, value * 1024));
                  setBusy(true);
                }}
                formatValue={(value) => `~${value} KiB`}
                markers={[
                  { value: 1, label: '1 KiB' },
                  { value: sliderMax, label: `${capKiB} KiB cap` },
                ]}
              />
              <div className="mt-2 flex flex-wrap gap-1.5">
                <PresetButton
                  label="Fastest"
                  onClick={() => {
                    setTargetBytes(2048);
                    setBusy(true);
                  }}
                  active={Math.round(targetBytes / 1024) === 2}
                />
                <PresetButton
                  label="Balanced"
                  onClick={() => {
                    setTargetBytes(defaultImageTarget(cap));
                    setBusy(true);
                  }}
                  active={Math.round(targetBytes / 1024) === Math.round(defaultImageTarget(cap) / 1024)}
                />
                <PresetButton
                  label={`Max (${capKiB} KiB)`}
                  onClick={() => {
                    setTargetBytes(cap);
                    setBusy(true);
                  }}
                  active={Math.round(targetBytes / 1024) === capKiB}
                />
              </div>
            </div>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
          <Metric label="Compressed" value={formatBytes(effectiveBytes)} hint={`from ${formatBytes(file?.size ?? 0)}`} />
          <Metric
            label="Dimensions"
            value={compressed ? `${compressed.width}×${compressed.height}` : '—'}
            hint={compressed ? `q${Math.round(compressed.quality * 100)} · ${compressed.mime.split('/')[1]}` : 'planning'}
          />
          <Metric label="Chunks" value={String(estimate.chunks)} hint={`${estimate.totalFrames} frames total`} />
          <Metric
            label="Transfer"
            value={`~${formatSeconds(estimate.transferMs)}`}
            hint={`wall clock ~${formatSeconds(estimate.wallClockMs)}`}
          />
        </div>

        <div className="rounded-xl border border-white/[0.06] bg-white/[0.02] p-3">
          <div className="flex items-center justify-between text-3xs">
            <span className="text-slate-400">Airtime vs. hourly duty-cycle budget</span>
            <span className={cn('tabular-nums', budgetPct !== null && budgetPct > 25 ? 'text-amber-300' : 'text-slate-400')}>
              {budgetPct === null ? 'no duty cycle' : `${budgetPct.toFixed(1)}% of 1 h budget`}
            </span>
          </div>
          <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-white/[0.07]">
            <div
              className={cn('h-full rounded-full', budgetPct !== null && budgetPct > 25 ? 'bg-amber-400' : 'bg-emerald-400')}
              style={{ width: `${Math.min(100, budgetPct ?? 0)}%` }}
            />
          </div>
          <p className="mt-2 text-3xs leading-relaxed text-slate-500">
            {overCap ? (
              <span className="flex items-center gap-1 text-rose-300">
                <TriangleAlert className="h-3 w-3" aria-hidden="true" />
                Over the {formatBytes(cap)} {profile.label} attachment cap — lower the target size.
              </span>
            ) : (
              <>
                {region.notes} A {formatBytes(effectiveBytes)} image takes ~{formatSeconds(estimate.airtimeMs)} of
                transmit time; {estimate.windowCount} selective-repeat windows are used.
              </>
            )}
          </p>
        </div>

        {error ? (
          <p className="flex items-center gap-1.5 text-2xs text-rose-300">
            <TriangleAlert className="h-3.5 w-3.5" aria-hidden="true" />
            {error}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}

function PresetButton({ label, onClick, active }: { label: string; onClick: () => void; active: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'focus-ring rounded-md border px-2 py-1 text-3xs font-medium transition-colors',
        active
          ? 'border-emerald-400/40 bg-emerald-400/10 text-emerald-300'
          : 'border-white/[0.08] bg-white/[0.03] text-slate-400 hover:border-white/[0.16] hover:text-slate-200',
      )}
    >
      {label}
    </button>
  );
}

function Metric({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-2.5 py-2">
      <p className="text-3xs uppercase tracking-[0.1em] text-slate-500">{label}</p>
      <p className="mt-0.5 truncate text-[13px] font-semibold tabular-nums text-slate-100">{value}</p>
      {hint ? <p className="mt-0.5 truncate text-3xs text-slate-600">{hint}</p> : null}
    </div>
  );
}
