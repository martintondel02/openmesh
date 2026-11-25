import { useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, ArrowRight, Gauge, KeyRound, ShieldAlert, ShieldCheck, SlidersHorizontal } from 'lucide-react';
import { useAppStore } from '../../store/app';
import type { LegalLimits, RegulatoryOverride } from '../../types/regulatory';
import {
  OVERRIDE_ACK_WORD,
  OVERRIDE_BANNER_TEXT,
  OVERRIDE_HELP_TOPIC,
  OVERRIDE_REASON_MAX_CHARS,
  OVERRIDE_WARNING,
  REGULATORY_HELP_FALLBACK_TOPIC,
  regionCentreKhz,
  defaultOverrideDraft,
  deriveRegulatoryState,
  effectiveRegulatoryValues,
  exceedsLegal,
  formatFrequencyKhz,
  formatLegalLimit,
  legalMaximaLine,
  requiresTypedConfirmation,
  validateOverrideDraft,
  type OverrideDraft,
  type OverrideIntent,
} from '../../lib/regulatory';
import { Badge, Button } from '../ui/primitives';
import { Callout, Checkbox, Field, TextInput, TextArea } from '../ui/forms';
import { Modal } from '../ui/overlay';
import { HelpLink, HelpTip } from '../ui/help';
import { cn } from '../../lib/utils';

/** Quiet beginner / detailed advanced legal-maxima line. Pure presentational. */
export function LegalLimitLine({
  legal,
  mode,
  className,
}: {
  legal: LegalLimits;
  mode: 'beginner' | 'advanced';
  className?: string;
}) {
  return (
    <p className={cn('flex items-start gap-1.5 text-2xs leading-relaxed text-slate-500', className)}>
      <ShieldCheck className="mt-0.5 h-3 w-3 shrink-0 text-emerald-400/60" aria-hidden="true" />
      <span>
        {formatLegalLimit(legal, mode)}
        {legal.source ? <span className="block text-3xs text-slate-600">{legal.source}</span> : null}
      </span>
    </p>
  );
}

/** Advanced Settings legal panel with the "verify local law" help link. */
export function LegalMaximaCard() {
  const config = useAppStore((state) => state.config);
  const status = useAppStore((state) => state.bridgeStatus);
  const openHelp = useAppStore((state) => state.openHelp);
  const regulatory = deriveRegulatoryState(config, status);
  const legal = regulatory.legal;
  return (
    <section id="legal-maxima" className="rounded-2xl border border-white/[0.06] bg-white/[0.02] px-4 py-4">
      <div className="flex flex-wrap items-center gap-2">
        <Gauge className="h-4 w-4 text-sky-300/80" aria-hidden="true" />
        <h3 className="flex items-center gap-1.5 text-xs font-semibold text-slate-200">
          Legal maxima
          <HelpTip helpKey="regulatory.legal" label="Legal maxima" />
        </h3>
        <Badge tone="slate">{legal.region}</Badge>
        <span className="ml-auto text-3xs text-slate-600">defaults = legal maxima</span>
      </div>
      <p className="mt-2 font-mono text-xs text-slate-200">{legalMaximaLine(legal)}</p>
      <dl className="mt-2 grid gap-1.5 text-2xs sm:grid-cols-3">
        <div className="rounded-lg border border-white/[0.05] bg-white/[0.015] px-2 py-1.5">
          <dt className="text-3xs uppercase tracking-[0.12em] text-slate-600">Max power</dt>
          <dd className="mt-0.5 tabular-nums text-slate-300">{legal.max_power_dbm} dBm EIRP</dd>
        </div>
        <div className="rounded-lg border border-white/[0.05] bg-white/[0.015] px-2 py-1.5">
          <dt className="text-3xs uppercase tracking-[0.12em] text-slate-600">Duty cycle</dt>
          <dd className="mt-0.5 tabular-nums text-slate-300">
            {legal.duty_cycle_pct === null ? 'no cap (fair use)' : `${legal.duty_cycle_pct} %`}
          </dd>
        </div>
        <div className="rounded-lg border border-white/[0.05] bg-white/[0.015] px-2 py-1.5">
          <dt className="text-3xs uppercase tracking-[0.12em] text-slate-600">Max bandwidth</dt>
          <dd className="mt-0.5 tabular-nums text-slate-300">{legal.max_bw_khz} kHz</dd>
        </div>
      </dl>
      {legal.source ? <p className="mt-2 text-3xs leading-relaxed text-slate-600">Source: {legal.source}</p> : null}
      <p className="mt-1 text-3xs leading-relaxed text-slate-600">
        Power is an EIRP ceiling: a high-gain antenna can push you over it. Hardware caps still apply (22 dBm on most
        SX1262 boards, 15 dBm on the T1000-E).{' '}
        <button
          type="button"
          onClick={() => openHelp(OVERRIDE_HELP_TOPIC)}
          className="focus-ring inline-flex items-center gap-1 rounded text-3xs font-medium text-emerald-300 hover:text-emerald-200"
        >
          Verify local law
          <ArrowRight className="h-2.5 w-2.5" aria-hidden="true" />
        </button>
        <HelpLink topic={REGULATORY_HELP_FALLBACK_TOPIC} className="ml-2 text-3xs">
          Radio rules
        </HelpLink>
      </p>
    </section>
  );
}

/**
 * Advanced-only "Regulatory override (emergency use)" card. Off by default; enabling
 * opens the typed-confirmation modal. Safe reductions while active can be saved
 * without retyping the acknowledgement. Disable is one tap.
 */
export function RegulatoryOverrideCard() {
  const uiMode = useAppStore((state) => state.uiMode);
  const mode = useAppStore((state) => state.mode);
  const config = useAppStore((state) => state.config);
  const status = useAppStore((state) => state.bridgeStatus);
  const openHelp = useAppStore((state) => state.openHelp);
  const disableOverride = useAppStore((state) => state.disableRegulatoryOverride);
  const setDemoOverrideActive = useAppStore((state) => state.setDemoOverrideActive);
  const regulatory = deriveRegulatoryState(config, status);
  const [modalOpen, setModalOpen] = useState(false);
  const [intent, setIntent] = useState<OverrideIntent>('enable');
  const [disabling, setDisabling] = useState(false);
  const urlHandled = useRef(false);

  useEffect(() => {
    if (urlHandled.current) return;
    let params: URLSearchParams | null = null;
    try {
      params = new URLSearchParams(window.location.search);
    } catch {
      params = null;
    }
    const requested = params?.get('regulatory') ?? null;
    if (requested !== 'active' && requested !== 'modal') return;
    if (requested === 'modal') {
      if (uiMode !== 'advanced') return;
      const timer = setTimeout(() => {
        urlHandled.current = true;
        setIntent('enable');
        setModalOpen(true);
      }, 600);
      return () => clearTimeout(timer);
    }
    // `regulatory=active` is a demo-only screenshot helper; wait for demo hydration.
    if (mode !== 'demo') return;
    if (!regulatory.active) {
      setDemoOverrideActive(true);
      return;
    }
    urlHandled.current = true;
    return undefined;
  }, [mode, uiMode, regulatory.active, setDemoOverrideActive]);

  if (uiMode !== 'advanced') return null;

  const effective = effectiveRegulatoryValues(regulatory.legal, regulatory.override);
  const open = (nextIntent: OverrideIntent) => {
    setIntent(nextIntent);
    setModalOpen(true);
  };

  const disable = async () => {
    setDisabling(true);
    try {
      await disableOverride();
    } finally {
      setDisabling(false);
    }
  };

  return (
    <section
      id="regulatory"
      className={cn(
        'rounded-2xl border px-4 py-4',
        regulatory.active ? 'border-rose-400/35 bg-rose-400/[0.06]' : 'border-white/[0.07] bg-white/[0.02]',
      )}
      aria-label="Regulatory override"
    >
      <div className="flex flex-wrap items-center gap-2">
        <ShieldAlert className={cn('h-4 w-4', regulatory.active ? 'text-rose-300' : 'text-slate-400')} aria-hidden="true" />
        <h3 className="flex items-center gap-1.5 text-xs font-semibold text-slate-200">
          Regulatory override (emergency use)
          <HelpTip helpKey="regulatory.override" label="Regulatory override" />
        </h3>
        <Badge tone={regulatory.active ? 'rose' : 'slate'} className="ml-auto">
          {regulatory.active ? 'ACTIVE' : 'off'}
        </Badge>
        <Badge tone="amber">advanced only</Badge>
      </div>

      <LegalLimitLine legal={regulatory.legal} mode="advanced" className="mt-2" />

      {regulatory.active ? (
        <div className="mt-2.5 space-y-2">
          <p role="alert" className="rounded-lg border border-rose-400/25 bg-rose-400/[0.06] px-2.5 py-2 text-2xs leading-relaxed text-rose-200">
            {regulatory.warning ?? OVERRIDE_WARNING}
          </p>
          <dl className="grid gap-1.5 text-2xs sm:grid-cols-3">
            <div className="rounded-lg border border-white/[0.05] bg-white/[0.015] px-2 py-1.5">
              <dt className="text-3xs uppercase tracking-[0.12em] text-slate-600">Effective duty</dt>
              <dd className="mt-0.5 tabular-nums text-slate-300">
                {effective.dutyCyclePct === null ? 'no cap' : `${effective.dutyCyclePct} %`}
              </dd>
            </div>
            <div className="rounded-lg border border-white/[0.05] bg-white/[0.015] px-2 py-1.5">
              <dt className="text-3xs uppercase tracking-[0.12em] text-slate-600">Effective power</dt>
              <dd className="mt-0.5 tabular-nums text-slate-300">{effective.powerDbm} dBm</dd>
            </div>
            <div className="rounded-lg border border-white/[0.05] bg-white/[0.015] px-2 py-1.5">
              <dt className="text-3xs uppercase tracking-[0.12em] text-slate-600">Frequency</dt>
              <dd className="mt-0.5 tabular-nums text-slate-300">{formatFrequencyKhz(effective.frequencyKhz)}</dd>
            </div>
          </dl>
          {regulatory.override.reason ? (
            <p className="text-3xs text-slate-500">
              Reason: <span className="text-slate-400">{regulatory.override.reason}</span>
            </p>
          ) : null}
        </div>
      ) : (
        <p className="mt-2 text-2xs leading-relaxed text-slate-500">
          Off by default. An emergency override may increase duty cycle or power beyond the legal limit. It requires
          acknowledging the risk and typing <span className="font-mono text-slate-300">OVERRIDE</span>; it never changes
          by itself and can be disabled in one tap.
        </p>
      )}

      <div className="mt-2.5 flex flex-wrap items-center gap-2">
        {regulatory.active ? (
          <>
            <Button variant="danger" size="sm" loading={disabling} onClick={() => void disable()}>
              Disable override
            </Button>
            <Button variant="ghost" size="sm" icon={SlidersHorizontal} onClick={() => open('edit')}>
              Edit limits…
            </Button>
          </>
        ) : (
          <Button variant="danger" size="sm" icon={KeyRound} onClick={() => open('enable')}>
            Enable override…
          </Button>
        )}
        <button
          type="button"
          onClick={() => openHelp(OVERRIDE_HELP_TOPIC)}
          className="focus-ring inline-flex items-center gap-1 rounded-md text-3xs font-medium text-slate-400 hover:text-emerald-300"
        >
          Emergency-use guide
          <ArrowRight className="h-2.5 w-2.5" aria-hidden="true" />
        </button>
        {mode === 'demo' ? (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setDemoOverrideActive(!regulatory.active)}
            title="Demo only: toggles the local banner and card state without sending anything"
          >
            {regulatory.active ? 'Preview override off' : 'Preview active banner'}
          </Button>
        ) : null}
      </div>
      {mode === 'demo' ? (
        <p className="mt-1.5 text-3xs text-slate-600">Demo preview only — nothing is transmitted and no API call is made.</p>
      ) : null}

      <RegulatoryOverrideModal
        open={modalOpen}
        intent={intent}
        legal={regulatory.legal}
        currentOverride={regulatory.override}
        currentFrequencyKhz={
          config?.radio.frequency_mhz ? Math.round(config.radio.frequency_mhz * 1000) : null
        }
        onClose={() => setModalOpen(false)}
      />
    </section>
  );
}

function RegulatoryOverrideModal({
  open,
  intent,
  legal,
  currentOverride,
  currentFrequencyKhz,
  onClose,
}: {
  open: boolean;
  intent: OverrideIntent;
  legal: LegalLimits;
  currentOverride: RegulatoryOverride;
  currentFrequencyKhz: number | null;
  onClose: () => void;
}) {
  const saveOverride = useAppStore((state) => state.saveRegulatoryOverride);
  const pushToast = useAppStore((state) => state.pushToast);
  const [draft, setDraft] = useState<OverrideDraft>(() => defaultOverrideDraft(legal, null));
  const [understood, setUnderstood] = useState(false);
  const [typed, setTyped] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [serverError, setServerError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setDraft(defaultOverrideDraft(legal, intent === 'edit' ? currentOverride : null));
    setUnderstood(false);
    setTyped('');
    setServerError(null);
  }, [open, intent, legal, currentOverride]);

  const validation = useMemo(() => validateOverrideDraft(draft, legal, { enabled: true, acknowledged: true }), [draft, legal]);
  const candidate = validation.override;
  const exceeds = candidate ? exceedsLegal(candidate, legal, { currentFrequencyKhz }) : false;
  const requiresTyped = candidate ? requiresTypedConfirmation({ intent, override: candidate, legal, context: { currentFrequencyKhz } }) : intent === 'enable';
  const typedOk = typed.trim() === OVERRIDE_ACK_WORD;
  const canSave = !submitting && validation.errors.length === 0 && (!requiresTyped || (understood && typedOk));
  const update = (patch: Partial<OverrideDraft>) => setDraft((current) => ({ ...current, ...patch }));

  const submit = async () => {
    if (!candidate) {
      setServerError(validation.errors.join(' '));
      return;
    }
    if (requiresTyped && !(understood && typedOk)) {
      setServerError('Tick “I understand” and type OVERRIDE to confirm the emergency override.');
      return;
    }
    setSubmitting(true);
    setServerError(null);
    const override: RegulatoryOverride = {
      ...candidate,
      enabled: true,
      acknowledged: requiresTyped ? true : currentOverride.acknowledged || true,
    };
    const result = await saveOverride(override);
    setSubmitting(false);
    if (result.ok) {
      pushToast({
        kind: 'info',
        title: 'Regulatory settings saved',
        message: 'The radio will use these limits after the next config apply.',
        helpTopic: OVERRIDE_HELP_TOPIC,
      });
      onClose();
    } else {
      setServerError(result.warning ? `${result.message ?? 'The bridge rejected the change.'} ${result.warning}` : result.message ?? 'The bridge rejected the change.');
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Regulatory override (emergency use)"
      description="This is not legal advice. Use only in a life-threatening emergency where local law permits."
      icon={ShieldAlert}
      size="lg"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="danger" loading={submitting} disabled={!canSave} onClick={() => void submit()}>
            {requiresTyped ? 'Enable override' : 'Save safe reduction'}
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <Callout tone="danger" icon={AlertTriangle} title="Full risk acknowledgement">
          {OVERRIDE_WARNING}
        </Callout>

        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Duty cycle %" hint={`Legal: ${legal.duty_cycle_pct === null ? 'no cap' : `${legal.duty_cycle_pct} %`}. 0 % means no transmissions (safe).`}>
            {({ id }) => (
              <TextInput
                id={id}
                inputMode="numeric"
                value={draft.dutyCyclePct}
                onChange={(event) => update({ dutyCyclePct: event.target.value })}
                placeholder={legal.duty_cycle_pct === null ? 'no cap' : String(legal.duty_cycle_pct)}
              />
            )}
          </Field>
          <Field
            label="Max TX power (dBm)"
            hint={`Legal max: ${legal.max_power_dbm} dBm EIRP. Hardware caps apply (SX1262 22 dBm, T1000-E 15 dBm).`}
          >
            {({ id }) => (
              <TextInput
                id={id}
                inputMode="numeric"
                value={draft.maxPowerDbm}
                onChange={(event) => update({ maxPowerDbm: event.target.value })}
                placeholder={String(legal.max_power_dbm)}
              />
            )}
          </Field>
        </div>

        <Field
          label="Frequency (kHz, optional)"
          hint="Leave blank for the region centre. Changing frequency can detune the antenna and increase interference; only use a frequency your hardware is tuned for."
        >
          {({ id }) => (
            <TextInput
              id={id}
              inputMode="numeric"
              value={draft.frequencyKhz}
              onChange={(event) => update({ frequencyKhz: event.target.value })}
              placeholder={`legal centre ≈ ${regionCentreKhz(legal.region)}`}
            />
          )}
        </Field>

        <Field label={`Reason (optional, max ${OVERRIDE_REASON_MAX_CHARS} chars)`} hint="Stored in the activity log so you can explain the emergency later. No secrets.">
          {({ id }) => (
            <TextArea
              id={id}
              rows={2}
              value={draft.reason}
              maxLength={OVERRIDE_REASON_MAX_CHARS}
              onChange={(event) => update({ reason: event.target.value })}
              placeholder="e.g. wildfire evacuation, no phone coverage"
            />
          )}
        </Field>

        {validation.errors.length ? (
          <ul className="space-y-0.5 rounded-lg border border-rose-400/25 bg-rose-400/[0.06] px-3 py-2 text-2xs text-rose-200">
            {validation.errors.map((error) => (
              <li key={error}>{error}</li>
            ))}
          </ul>
        ) : null}

        {!requiresTyped ? (
          <Callout tone="info" icon={ShieldCheck} title="Within the legal limit">
            These values are at or below the legal maxima and stay on the region frequency, so this is a safe
            reduction. It can be saved without typing the emergency acknowledgement.
          </Callout>
        ) : (
          <div className="space-y-2.5 rounded-xl border border-rose-400/25 bg-rose-400/[0.04] px-3 py-2.5">
            <Checkbox
              id="override-understand"
              checked={understood}
              onChange={setUnderstood}
              label="I understand"
              description="I understand this override may violate local radio regulations, void my radio's certification and cause interference, and that I am responsible."
            />
            <Field
              label={`Type ${OVERRIDE_ACK_WORD} to confirm`}
              error={typed && !typedOk ? `Type ${OVERRIDE_ACK_WORD} exactly (case-sensitive).` : null}
            >
              {({ id }) => (
                <TextInput
                  id={id}
                  value={typed}
                  autoComplete="off"
                  onChange={(event) => setTyped(event.target.value)}
                  placeholder={OVERRIDE_ACK_WORD}
                  className="font-mono uppercase tracking-widest"
                />
              )}
            </Field>
          </div>
        )}

        {serverError ? (
          <p role="alert" className="rounded-lg border border-rose-400/30 bg-rose-400/[0.08] px-3 py-2 text-2xs leading-relaxed text-rose-200">
            {serverError}
          </p>
        ) : null}

        <p className="text-3xs leading-relaxed text-slate-600">
          The first enable always needs the typed acknowledgement. Later edits need it again only when the draft
          exceeds the legal limit ({exceeds ? 'this draft does' : 'this draft stays within it'}); safe reductions save
          without retyping. Disabling is always one tap and restores the legal region duty.
        </p>
      </div>
    </Modal>
  );
}

/** Persistent, non-dismissible banner shown on every screen while an override is active. */
export function RegulatoryBanner() {
  const config = useAppStore((state) => state.config);
  const status = useAppStore((state) => state.bridgeStatus);
  const setUiMode = useAppStore((state) => state.setUiMode);
  const setScreen = useAppStore((state) => state.setScreen);
  const disableOverride = useAppStore((state) => state.disableRegulatoryOverride);
  const openHelp = useAppStore((state) => state.openHelp);
  const [disabling, setDisabling] = useState(false);
  const regulatory = deriveRegulatoryState(config, status);
  if (!regulatory.active) return null;

  const openControls = () => {
    setUiMode('advanced');
    setScreen('settings');
    setTimeout(() => {
      document.getElementById('regulatory')?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }, 120);
  };

  return (
    <div
      role="alert"
      aria-live="assertive"
      className="relative z-30 flex shrink-0 items-start gap-2 border-b border-rose-400/35 bg-rose-500/[0.12] px-3 py-2 text-2xs text-rose-100 md:px-5"
    >
      <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-rose-300" aria-hidden="true" />
      <p className="min-w-0 flex-1 font-medium leading-snug">
        {OVERRIDE_BANNER_TEXT}
        {regulatory.warning && regulatory.warning !== OVERRIDE_WARNING ? (
          <span className="mt-0.5 block text-3xs font-normal text-rose-200/70">{regulatory.warning}</span>
        ) : null}
      </p>
      <div className="flex shrink-0 items-center gap-1">
        <button
          type="button"
          onClick={openControls}
          className="focus-ring hidden h-5 items-center gap-1 rounded px-1.5 text-3xs font-medium hover:bg-rose-400/15 sm:inline-flex"
        >
          Open Advanced → Regulatory
        </button>
        <button
          type="button"
          onClick={() => openHelp(OVERRIDE_HELP_TOPIC)}
          className="focus-ring inline-flex h-5 items-center rounded px-1.5 text-3xs hover:bg-rose-400/15"
        >
          Help
        </button>
        <Button
          variant="danger"
          size="sm"
          loading={disabling}
          onClick={() => {
            setDisabling(true);
            void disableOverride().finally(() => setDisabling(false));
          }}
        >
          Disable
        </Button>
      </div>
    </div>
  );
}

/** Demo-only helper used by the card/screenshot URL; exported for tests of the visible state. */
export function isOverrideVisible(override: RegulatoryOverride, active: boolean): boolean {
  return active || (override.enabled && override.acknowledged);
}
