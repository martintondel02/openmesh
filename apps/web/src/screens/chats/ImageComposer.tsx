import { useEffect, useMemo, useRef, useState } from 'react';
import { Check, Clock, Image as ImageIcon, Loader2, SendHorizontal, TriangleAlert } from 'lucide-react';
import { useAppStore } from '../../store/app';
import type { ActiveThread } from '../../store/app';
import {
  compressImageToTier,
  encodedImageUploadName,
  readImageDimensions,
  validateImageFile,
  type TierCompressedImage,
} from '../../lib/attachments';
import { estimateAttachment, getProfile, getRegion, hourlyAirtimeBudgetMs, maxAttachmentBytes } from '../../lib/airtime';
import {
  IMAGE_TIERS,
  imageFormatLabel,
  imageTierPolicy,
  suggestImageTier,
  tierAirtimeSummary,
  type ImageTierId,
} from '../../lib/media-tiers';
import { formatBytes, formatSeconds, cn } from '../../lib/utils';
import { SLOW_MEDIA_WALL_CLOCK_MS, formatMediaExpectation, mediaWallClockMs } from '../../lib/media-transfer';
import { Badge, Button } from '../../components/ui/primitives';
import { Modal } from '../../components/ui/overlay';
import { Field, TextInput } from '../../components/ui/forms';

export interface ImageComposerModalProps {
  open: boolean;
  file: File | null;
  thread: ActiveThread | null;
  onClose: () => void;
}

/**
 * Image send flow: pick a shared tier (content/presets/media.json), compress to its
 * dimensions/quality, then show the measured bytes, airtime and duty-cycle cost before send.
 */
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
  const policy = imageTierPolicy(profileId);

  const [tier, setTier] = useState<ImageTierId>('medium');
  const [suggestion, setSuggestion] = useState<ReturnType<typeof suggestImageTier> | null>(null);
  const [compressed, setCompressed] = useState<TierCompressedImage | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [caption, setCaption] = useState('');
  const [originalConfirmed, setOriginalConfirmed] = useState(false);
  const [sending, setSending] = useState(false);
  const previousUrl = useRef<string | null>(null);
  const requestRef = useRef(0);

  useEffect(() => {
    if (!open) return;
    setCaption('');
    setError(null);
    setCompressed(null);
    setOriginalConfirmed(false);
    return () => {
      if (previousUrl.current) URL.revokeObjectURL(previousUrl.current);
      previousUrl.current = null;
    };
  }, [open, file]);

  useEffect(() => {
    if (!open || !file) return;
    const validation = validateImageFile(file);
    if (validation) {
      setError(validation);
      return;
    }
    let cancelled = false;
    void readImageDimensions(file)
      .then((dims) => {
        if (cancelled) return;
        const next = suggestImageTier({
          profileId,
          sourceBytes: file.size,
          sourceWidth: dims.width,
          sourceHeight: dims.height,
        });
        setSuggestion(next);
        setTier(next.suggested);
      })
      .catch(() => {
        if (cancelled) return;
        setSuggestion(
          suggestImageTier({ profileId, sourceBytes: file.size, sourceWidth: 1600, sourceHeight: 1067 }),
        );
        setTier('medium');
      });
    return () => {
      cancelled = true;
    };
  }, [open, file, profileId]);

  useEffect(() => {
    if (!open || !file || !tier) return;
    if (tier === 'original' && !policy.originalAllowed) {
      setTier(suggestion?.suggested ?? 'high');
      return;
    }
    let cancelled = false;
    const requestId = requestRef.current + 1;
    requestRef.current = requestId;
    setBusy(true);
    setError(null);
    const timer = setTimeout(() => {
      compressImageToTier(file, tier)
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
    }, 120);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [open, file, tier, policy.originalAllowed, suggestion?.suggested]);

  // Label the format actually produced by Auto (strictly smaller of AVIF/WebP; WebP wins ties).
  const encoderLabel = compressed ? imageFormatLabel(compressed.mime) : busy ? 'Encoding…' : 'Auto';
  const encodingComparison = useMemo(() => {
    const alternatives = compressed?.alternatives ?? [];
    if (alternatives.length < 2) return null;
    const sizes = alternatives.map((option) => `${imageFormatLabel(option.mime)} ${formatBytes(option.bytes)}`).join(' · ');
    return `${sizes} — sent ${imageFormatLabel(compressed?.mime)}`;
  }, [compressed]);
  const effectiveBytes = compressed?.blob.size ?? 0;
  const estimate = useMemo(
    () => estimateAttachment(Math.max(1, effectiveBytes), profileId, regionId),
    [effectiveBytes, profileId, regionId],
  );
  const airtime = useMemo(
    () => tierAirtimeSummary(Math.max(1, effectiveBytes), profileId, regionId),
    [effectiveBytes, profileId, regionId],
  );
  const budgetPct = budgetMs ? (estimate.airtimeMs / budgetMs) * 100 : null;
  const effectiveDuty = config?.radio.duty_cycle_pct ?? region.dutyCyclePct;
  const backgroundHint = useMemo(() => {
    const wallClockMs = mediaWallClockMs(estimate, effectiveDuty);
    if (wallClockMs < SLOW_MEDIA_WALL_CLOCK_MS) return null;
    const expectation = formatMediaExpectation(wallClockMs);
    if (!expectation) return null;
    const dutyText = effectiveDuty ? `the ${effectiveDuty} % legal duty cycle` : 'the radio\u2019s pacing';
    return `At ${dutyText} this takes ${expectation} of wall-clock time. It keeps sending in the background after you press Send — you can close this dialog.`;
  }, [estimate, effectiveDuty]);
  const overCap = effectiveBytes > cap;
  const originalBlocked = tier === 'original' && (!policy.originalAllowed || !originalConfirmed);
  const canSend = Boolean(compressed) && !busy && !error && !overCap && !originalBlocked;
  const suggestedTier = suggestion?.suggested ?? null;
  const capKiB = Math.round(cap / 1024);

  const handleSend = async () => {
    if (!compressed || !file || !canSend) return;
    setSending(true);
    try {
      await sendAttachment(
        {
          blob: compressed.blob,
          mime: compressed.mime,
          filename: encodedImageUploadName(file, compressed.mime),
          localUrl: compressed.objectUrl,
          width: compressed.width,
          height: compressed.height,
          caption: caption.trim() || undefined,
        },
        thread ?? undefined,
      );
      pushToast({
        kind: 'success',
        title: `${compressed.tier === 'original' ? 'Original' : compressed.tier[0]!.toUpperCase() + compressed.tier.slice(1)} image queued`,
        message: `${formatBytes(compressed.blob.size)} · ~${formatSeconds(estimate.airtimeMs)} airtime · ${estimate.chunks} chunks`,
      });
      previousUrl.current = null;
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
      description={`Choose a tier; Auto encodes AVIF and WebP at matching visual quality and sends the smaller file (WebP wins a tie). The measured bytes and airtime are shown before sending (${formatBytes(cap)} cap on ${profile.label}).`}
      icon={ImageIcon}
      size="lg"
      dismissible={!sending}
      footer={
        <>
          <div className="mr-auto flex flex-col gap-0.5 text-2xs text-slate-500">
            <span className="flex items-center gap-2">
              <Badge tone={overCap ? 'rose' : budgetPct !== null && budgetPct > 25 ? 'amber' : 'accent'}>
                ~{formatSeconds(estimate.airtimeMs)} airtime
              </Badge>
              <span className="tabular-nums">{estimate.chunks} chunks</span>
            </span>
            <span className="text-3xs">{airtime.label}</span>
          </div>
          <Button variant="ghost" onClick={onClose} disabled={sending}>Cancel</Button>
          <Button
            variant="primary"
            icon={SendHorizontal}
            onClick={() => void handleSend()}
            disabled={!canSend}
            loading={sending}
          >
            Send {compressed ? `${formatBytes(compressed.blob.size)}` : 'image'}
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <div>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <p className="text-2xs font-semibold uppercase tracking-[0.12em] text-slate-500">Image tier</p>
            {suggestion ? (
              <p className="text-3xs text-slate-500">
                Suggested for {profile.label}:{' '}
                <span className="font-medium text-emerald-300">{IMAGE_TIERS.find((t) => t.id === suggestedTier)?.label}</span>
                {' '}· cap {capKiB} KiB
              </p>
            ) : null}
          </div>
          <div className="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-4" role="group" aria-label="Image tier">
            {IMAGE_TIERS.map((option) => {
              const blocked = option.id === 'original' && !policy.originalAllowed;
              const selected = tier === option.id;
              const suggested = suggestedTier === option.id;
              return (
                <button
                  key={option.id}
                  type="button"
                  onClick={() => setTier(option.id)}
                  disabled={blocked || busy}
                  aria-pressed={selected}
                  className={cn(
                    'focus-ring rounded-xl border px-2.5 py-2 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-45',
                    selected
                      ? 'border-emerald-400/40 bg-emerald-400/[0.08]'
                      : 'border-white/[0.08] bg-white/[0.02] hover:border-white/20',
                  )}
                >
                  <span className="flex items-center gap-1 text-xs font-semibold text-slate-200">
                    {option.label}
                    {selected ? <Check className="h-3 w-3 text-emerald-300" aria-hidden="true" /> : null}
                    {suggested ? <Badge tone="accent" className="ml-auto">suggested</Badge> : null}
                  </span>
                  <span className="mt-0.5 block text-3xs text-slate-500">{option.targetLabel}</span>
                  <span className="mt-0.5 block text-3xs text-slate-600">
                    <span title={option.id === 'original' ? undefined : encodingComparison ?? undefined}>
                      {option.id === 'original' ? 'needs confirm' : encoderLabel}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
          {!policy.originalAllowed ? (
            <p className="mt-2 flex items-start gap-1.5 text-3xs leading-relaxed text-amber-300/80">
              <TriangleAlert className="mt-0.5 h-3 w-3 shrink-0" aria-hidden="true" />
              {policy.reason}
            </p>
          ) : null}
          {suggestion && tier !== suggestedTier && suggestedTier ? (
            <button
              type="button"
              onClick={() => setTier(suggestedTier)}
              className="focus-ring mt-2 inline-flex items-center gap-1 rounded-md text-3xs font-medium text-emerald-300 hover:text-emerald-200"
            >
              Use suggested {IMAGE_TIERS.find((option) => option.id === suggestedTier)?.label} tier
            </button>
          ) : null}
        </div>

        <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
          <div className="relative overflow-hidden rounded-xl border border-white/[0.07] bg-ink-950/70">
            {compressed ? (
              <img
                src={compressed.objectUrl}
                alt="Tier preview"
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
                  Optimising image…
                </span>
              </div>
            ) : null}
            {compressed ? (
              <span className="absolute right-1.5 top-1.5 rounded-md bg-ink-950/75 px-1.5 py-0.5 text-3xs text-slate-300 backdrop-blur">
                <span title={encodingComparison ?? undefined}>
                  {compressed.width}×{compressed.height} · {formatBytes(compressed.blob.size)}
                </span>
              </span>
            ) : null}
          </div>

          <div className="space-y-3">
            <Field label="Caption" hint="Optional — sent alongside the image manifest.">
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
            <div className="grid grid-cols-2 gap-2">
              <Metric label="Tier" value={compressed?.tier ?? tier} hint={compressed?.mime.split('/')[1] ?? 'planning'} />
              <Metric label="Actual size" value={formatBytes(effectiveBytes)} hint={`from ${formatBytes(file?.size ?? 0)} original`} />
              <Metric label="Airtime" value={`~${formatSeconds(estimate.airtimeMs)}`} hint={airtime.label} />
              <Metric
                label="Hourly duty"
                value={budgetPct === null ? 'no cap' : `${budgetPct.toFixed(1)} %`}
                hint={budgetPct !== null && budgetPct > 25 ? 'over a quarter of an hour' : 'within budget'}
              />
            </div>
            <div>
              <div className="h-1.5 overflow-hidden rounded-full bg-white/[0.07]">
                <div
                  className={cn('h-full rounded-full', budgetPct !== null && budgetPct > 25 ? 'bg-amber-400' : 'bg-emerald-400')}
                  style={{ width: `${Math.min(100, budgetPct ?? 0)}%` }}
                />
              </div>
              <p className="mt-1.5 text-3xs leading-relaxed text-slate-500">
                {overCap ? (
                  <span className="flex items-center gap-1 text-rose-300">
                    <TriangleAlert className="h-3 w-3" aria-hidden="true" />
                    Over the {formatBytes(cap)} cap — choose a smaller tier.
                  </span>
                ) : (
                  <>{region.notes} The image occupies {estimate.percentOfCap.toFixed(0)} % of the profile attachment cap.</>
                )}
              </p>
              {backgroundHint ? (
                <p className="mt-1.5 flex items-start gap-1 text-3xs leading-relaxed text-slate-500">
                  <Clock className="mt-0.5 h-3 w-3 shrink-0" aria-hidden="true" />
                  <span>{backgroundHint}</span>
                </p>
              ) : null}
            </div>
            {tier === 'original' && policy.originalAllowed ? (
              <label className="flex items-start gap-2 rounded-lg border border-amber-400/25 bg-amber-400/[0.06] px-2.5 py-2 text-2xs text-amber-200">
                <input
                  type="checkbox"
                  checked={originalConfirmed}
                  onChange={(event) => setOriginalConfirmed(event.target.checked)}
                  className="mt-0.5 h-3.5 w-3.5 accent-amber-400"
                />
                <span>
                  I understand Original sends the unmodified file with no resizing. It must still fit the profile cap
                  and may take much longer on the mesh.
                </span>
              </label>
            ) : null}
          </div>
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

function Metric({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-2.5 py-2">
      <p className="text-3xs uppercase tracking-[0.1em] text-slate-500">{label}</p>
      <p className="mt-0.5 truncate text-[13px] font-semibold tabular-nums capitalize text-slate-100">{value}</p>
      {hint ? <p className="mt-0.5 truncate text-3xs text-slate-600">{hint}</p> : null}
    </div>
  );
}
