import { useEffect, useMemo, useState } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  Bike,
  CheckCircle2,
  Circle,
  MessagesSquare,
  RadioTower,
  Router,
  ShieldCheck,
  Smartphone,
  Sparkles,
  UserPlus,
  Users,
} from 'lucide-react';
import { Button, Badge } from '../../components/ui/primitives';
import { Field, Select, TextInput } from '../../components/ui/forms';
import { Modal } from '../../components/ui/overlay';
import { Toggle } from '../../components/ui/primitives';
import { HelpTip } from '../../components/ui/help';
import { LegalLimitLine } from '../../components/regulatory/Regulatory';
import { formatLegalLimit, legalLimitsForRegion } from '../../lib/regulatory';
import { CopyButton } from '../../components/ui/signals';
import { QRCode } from '../../components/common/QRCode';
import { useAppStore } from '../../store/app';
import { COUNTRIES, regionForCountry, recommendedFor, setupRole, SETUP_ROLES, inviteShareText } from '../../lib/setup';
import { copyText } from '../../components/ui/signals';
import type { NextStepAction, SetupRole } from '../../types/setup';
import type { LucideIcon } from 'lucide-react';

const ROLE_ICONS: Record<SetupRole, LucideIcon> = {
  client: Smartphone,
  repeater: RadioTower,
  gateway: Router,
  tracker: Bike,
};

const NEXT_STEP_ICONS: Record<string, LucideIcon> = {
  message: MessagesSquare,
  contact: UserPlus,
  backbone: RadioTower,
  agent: Sparkles,
  invite: Users,
};

const STEP_TITLES: Record<string, { title: string; description: string }> = {
  role: {
    title: 'What is this box?',
    description: 'Three quick questions, then we apply secure defaults. About two minutes.',
  },
  place: {
    title: 'Where are you?',
    description: 'Your country sets a legal radio band. We only store the country code.',
  },
  secure: {
    title: 'Secure my node',
    description: 'One button sets up a private channel and safe defaults. You can change anything later.',
  },
  success: {
    title: 'Your mesh is ready',
    description: 'A private channel and safe defaults are in place. Here is what to do next.',
  },
};

export function OnboardingWizard() {
  const onboarding = useAppStore((state) => state.onboarding);
  const uiMode = useAppStore((state) => state.uiMode);
  const open = onboarding.open;
  const step = onboarding.step;
  const draft = onboarding.draft;
  const result = onboarding.result;
  const closeOnboarding = useAppStore((state) => state.closeOnboarding);
  const onboardingNext = useAppStore((state) => state.onboardingNext);
  const onboardingBack = useAppStore((state) => state.onboardingBack);
  const updateDraft = useAppStore((state) => state.updateOnboardingDraft);
  const submitQuickSetup = useAppStore((state) => state.submitQuickSetup);
  const setScreen = useAppStore((state) => state.setScreen);
  const setUiMode = useAppStore((state) => state.setUiMode);
  const recommendation = useAppStore((state) => state.setupRecommendation);
  const loadRecommendation = useAppStore((state) => state.loadSetupRecommendation);
  const [copiedInvite, setCopiedInvite] = useState(false);

  useEffect(() => {
    if (!open) return;
    void loadRecommendation(draft.country, draft.role);
  }, [open, draft.country, draft.role, loadRecommendation]);

  const localRecommended = useMemo(() => recommendedFor(draft.country, draft.role), [draft.country, draft.role]);
  const recommended = recommendation ?? localRecommended;
  const legal = recommended.legal ?? legalLimitsForRegion(recommended.region);
  const regionInfo = regionForCountry(draft.country);
  const isLast = step === 'secure';
  const meta = STEP_TITLES[step] ?? STEP_TITLES.role!;

  const goTo = (action: NextStepAction | string | null | undefined) => {
    if (action === 'message') setScreen('chats');
    else if (action === 'contact') setScreen('contacts');
    else if (action === 'backbone') setScreen('backbone');
    else if (action === 'agent') setScreen('agents');
    closeOnboarding();
  };

  const copyInvite = async () => {
    if (!result?.channel) return;
    const ok = await copyText(inviteShareText(result.channel));
    if (ok) {
      setCopiedInvite(true);
      setTimeout(() => setCopiedInvite(false), 1600);
    }
  };

  const footer = (() => {
    if (step === 'success') {
      return (
        <>
          <span className="mr-auto text-3xs text-slate-500">You can change anything later in Settings.</span>
          <Button variant="ghost" onClick={() => { closeOnboarding(); setScreen('settings'); }}>
            Review settings
          </Button>
          <Button variant="primary" onClick={closeOnboarding}>
            Done
          </Button>
        </>
      );
    }
    return (
      <>
        <Button variant="ghost" onClick={closeOnboarding}>
          Do this later
        </Button>
        <span className="mr-auto" />
        {step !== 'role' ? (
          <Button variant="ghost" icon={ArrowLeft} onClick={onboardingBack}>
            Back
          </Button>
        ) : null}
        {isLast ? (
          <Button variant="primary" icon={ShieldCheck} loading={onboarding.submitting} onClick={() => void submitQuickSetup()}>
            Secure my node
          </Button>
        ) : (
          <Button variant="primary" icon={ArrowRight} onClick={onboardingNext}>
            Continue
          </Button>
        )}
      </>
    );
  })();

  return (
    <Modal
      open={open}
      onClose={closeOnboarding}
      title={meta.title}
      description={meta.description}
      icon={step === 'success' ? CheckCircle2 : Sparkles}
      size="lg"
      footer={footer}
    >
      <div className="mb-4 flex items-center gap-2" aria-label={`Step ${['role', 'place', 'secure', 'success'].indexOf(step) + 1} of 4`}>
        {(['role', 'place', 'secure', 'success'] as const).map((key) => (
          <span
            key={key}
            className={
              'h-1 flex-1 rounded-full ' +
              (step === key || (step === 'success' && key !== 'success') ? 'bg-emerald-400/70' : 'bg-white/[0.07]')
            }
          />
        ))}
      </div>

      {step === 'role' ? (
        <div>
          <div className="flex items-center gap-1.5 text-2xs font-medium text-slate-400">
            What will this device do? <HelpTip helpKey="onboarding.role" label="Device role" />
          </div>
          <div className="mt-2 grid gap-2 sm:grid-cols-2">
            {SETUP_ROLES.map((role) => {
              const Icon = ROLE_ICONS[role.id];
              const selected = draft.role === role.id;
              return (
                <button
                  key={role.id}
                  type="button"
                  onClick={() => updateDraft({ role: role.id })}
                  aria-pressed={selected}
                  className={
                    'focus-ring flex items-start gap-3 rounded-xl border p-3 text-left transition-colors ' +
                    (selected
                      ? 'border-emerald-400/40 bg-emerald-400/[0.08]'
                      : 'border-white/[0.07] bg-white/[0.02] hover:border-white/15 hover:bg-white/[0.04]')
                  }
                >
                  <span
                    className={
                      'grid h-9 w-9 shrink-0 place-items-center rounded-lg border ' +
                      (selected ? 'border-emerald-400/30 bg-emerald-400/10 text-emerald-300' : 'border-white/10 bg-white/[0.04] text-slate-400')
                    }
                  >
                    <Icon className="h-4 w-4" aria-hidden="true" />
                  </span>
                  <span className="min-w-0">
                    <span className="flex items-center gap-1.5 text-[13px] font-semibold text-slate-100">
                      {role.label}
                      {selected ? <CheckCircle2 className="h-3.5 w-3.5 text-emerald-300" aria-hidden="true" /> : null}
                    </span>
                    <span className="mt-0.5 block text-2xs leading-relaxed text-slate-500">{role.description}</span>
                    <span className="mt-1 block text-3xs text-slate-600">Examples: {role.examples}</span>
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      ) : null}

      {step === 'place' ? (
        <div className="space-y-4">
          <Field
            label="Country"
            hint="Used to pick a legal radio region. Your exact location is never sent."
            labelAccessory={<HelpTip helpKey="onboarding.country" label="Country" />}
          >
            {({ id }) => (
              <Select
                id={id}
                value={draft.country}
                onChange={(event) => {
                  updateDraft({ country: event.target.value });
                  void loadRecommendation(event.target.value, draft.role);
                }}
              >
                {COUNTRIES.map((country) => (
                  <option key={country.code} value={country.code}>
                    {country.name} — {country.region}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          {regionInfo.warning ? (
            <p className="rounded-lg border border-amber-400/20 bg-amber-400/[0.06] px-3 py-2 text-2xs leading-relaxed text-amber-200/80">
              {regionInfo.warning}
            </p>
          ) : null}
          <Field
            label="Node name"
            hint="Other people see this name. A nickname is fine."
            labelAccessory={<HelpTip helpKey="onboarding.name" label="Node name" />}
          >
            {({ id }) => (
              <div className="flex gap-2">
                <TextInput
                  id={id}
                  value={draft.nodeName}
                  placeholder="e.g. my-phone"
                  maxLength={32}
                  onChange={(event) => updateDraft({ nodeName: event.target.value })}
                />
                <Button
                  variant="ghost"
                  onClick={() => updateDraft({ nodeName: recommendedFor(draft.country, draft.role).role === 'client' ? 'my-phone' : `my-${draft.role}` })}
                >
                  Suggest one
                </Button>
              </div>
            )}
          </Field>
          <div className="rounded-xl border border-white/[0.07] bg-white/[0.02] p-3">
            <Toggle
              id="onboarding-privacy"
              checked={draft.privacy === 'private'}
              onChange={(checked) => updateDraft({ privacy: checked ? 'private' : 'shared' })}
              label="Keep my location private"
              description="On: position stays on your device until you deliberately share it. This is the safer default."
              labelAccessory={<HelpTip helpKey="onboarding.privacy" label="Keep my location private" />}
            />
          </div>
          <div className="rounded-xl border border-white/[0.07] bg-white/[0.02] p-3 text-2xs leading-relaxed text-slate-500">
            <span className="font-medium text-slate-300">{setupRole(draft.role).label}</span> will use the{' '}
            <span className="font-medium text-slate-300">{friendlyProfile(recommended.profile, uiMode)}</span> radio preset in{' '}
            <span className="font-medium text-slate-300">{recommended.region}</span>. Announcing every{' '}
            {Math.round((recommended.announce_interval_s ?? 1800) / 60)} minutes.
            {recommended.warning ? <span className="mt-1 block text-amber-300/80">{recommended.warning}</span> : null}
          </div>
          <LegalLimitLine legal={legal} mode={uiMode} className="mt-1" />
        </div>
      ) : null}

      {step === 'secure' ? (
        <div className="space-y-4">
          <div className="rounded-xl border border-emerald-400/20 bg-emerald-400/[0.06] p-4">
            <div className="flex items-start gap-3">
              <span className="grid h-9 w-9 shrink-0 place-items-center rounded-lg border border-emerald-400/25 bg-emerald-400/10 text-emerald-300">
                <ShieldCheck className="h-4 w-4" aria-hidden="true" />
              </span>
              <div>
                <p className="text-[13px] font-semibold text-slate-100">We will apply these safe defaults</p>
                <ul className="mt-1.5 space-y-1 text-2xs leading-relaxed text-slate-400">
                  <li>• Token-protected access and the API stays loopback-only.</li>
                  <li>• A private channel with its own key, ready to invite someone.</li>
                  <li>• {draft.privacy === 'private' ? 'Position sharing off until you choose to share.' : 'Position sharing on, only with your channel.'}</li>
                  <li>• Legal radio region {recommended.region} and the {recommended.profile} profile.</li>
                <li>• Legal maxima: {formatLegalLimit(legal, 'advanced')}</li>
                  <li>• Store-and-forward {recommended.store_forward ? 'on' : 'off'} for this role.</li>
                </ul>
                <div className="mt-2">
                  <HelpTip helpKey="onboarding.secure" label="Secure my node" />
                </div>
              </div>
            </div>
          </div>
          <div className="grid gap-2 sm:grid-cols-3">
            <Summary label="Role" value={setupRole(draft.role).label} />
            <Summary label="Node name" value={draft.nodeName || `my-${draft.role}`} />
            <Summary label="Country / region" value={`${draft.country} · ${recommended.region}`} />
          </div>
          {onboarding.error ? (
            <p role="alert" className="rounded-lg border border-rose-400/25 bg-rose-400/[0.06] px-3 py-2 text-2xs leading-relaxed text-rose-200">
              {onboarding.error}
            </p>
          ) : null}
        </div>
      ) : null}

      {step === 'success' && result ? (
        <div className="space-y-4">
          <div className="flex items-start gap-3 rounded-xl border border-emerald-400/25 bg-emerald-400/[0.07] p-4">
            <CheckCircle2 className="mt-0.5 h-5 w-5 shrink-0 text-emerald-300" aria-hidden="true" />
            <div>
              <p className="text-sm font-semibold text-slate-100">Secure setup complete</p>
              <p className="mt-0.5 text-2xs leading-relaxed text-slate-400">
                {uiMode === 'advanced'
                  ? 'Every value was applied through the bridge quick-setup preset. Review it in Settings → Advanced.'
                  : 'You can send your first message now — no reading required.'}{' '}
                You can change anything later in Settings.
              </p>
            </div>
          </div>

          <div className="grid gap-2 sm:grid-cols-2">
            <div className="rounded-xl border border-white/[0.07] bg-white/[0.02] p-3">
              <p className="text-2xs font-semibold uppercase tracking-[0.14em] text-slate-500">Checklist</p>
              <ul className="mt-2 space-y-1.5">
                {result.checklist.map((item) => (
                  <li key={String(item.key)} className="flex items-start gap-2 text-2xs text-slate-400">
                    {item.ok ? (
                      <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0 text-emerald-300" aria-hidden="true" />
                    ) : (
                      <Circle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-300" aria-hidden="true" />
                    )}
                    <span>
                      {item.label}
                      {item.detail ? <span className="block text-3xs text-slate-600">{item.detail}</span> : null}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
            <div className="rounded-xl border border-white/[0.07] bg-white/[0.02] p-3">
              <p className="text-2xs font-semibold uppercase tracking-[0.14em] text-slate-500">Invite someone</p>
              {result.channel ? (
                <>
                  <p className="mt-1.5 text-2xs font-medium text-slate-200">{result.channel.name}</p>
                  <p className="mt-0.5 break-all font-mono text-3xs leading-relaxed text-slate-500">
                    {result.channel.invite_url}
                  </p>
                  <div className="mt-2 flex flex-wrap items-center gap-2">
                    <CopyButton value={inviteShareText(result.channel)} label="Copy invite" />
                    <Button variant="ghost" size="sm" onClick={() => void copyInvite()}>
                      {copiedInvite ? 'Copied' : 'Copy message'}
                    </Button>
                    <Badge tone="amber">shown once</Badge>
                  </div>
                  <p className="mt-1.5 text-3xs leading-relaxed text-slate-600">
                    This is the only time the panel shows the channel key. Copy the invite now; you can create a new one
                    from Chats later.
                  </p>
                  <div className="mt-2 flex justify-center">
                    <QRCode value={result.channel.invite_url} size={116} label="Channel invite QR code" />
                  </div>
                </>
              ) : (
                <p className="mt-1.5 text-2xs text-slate-500">A channel already exists on this node. Invite from Chats.</p>
              )}
            </div>
          </div>

          <div>
            <p className="text-2xs font-semibold uppercase tracking-[0.14em] text-slate-500">Next steps</p>
            <div className="mt-2 grid gap-2 sm:grid-cols-2">
              {result.next_steps.map((next) => {
                const Icon = NEXT_STEP_ICONS[next.action ?? ''] ?? Sparkles;
                return (
                  <button
                    key={next.title}
                    type="button"
                    onClick={() => goTo(next.action)}
                    className="focus-ring flex items-start gap-2.5 rounded-xl border border-white/[0.07] bg-white/[0.02] p-3 text-left hover:border-white/15 hover:bg-white/[0.04]"
                  >
                    <span className="grid h-7 w-7 shrink-0 place-items-center rounded-lg border border-white/10 bg-white/[0.04] text-emerald-300">
                      <Icon className="h-3.5 w-3.5" aria-hidden="true" />
                    </span>
                    <span className="min-w-0">
                      <span className="block text-2xs font-semibold text-slate-200">{next.title}</span>
                      <span className="mt-0.5 block text-3xs leading-relaxed text-slate-500">{next.detail}</span>
                    </span>
                  </button>
                );
              })}
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-2 rounded-lg border border-white/[0.06] bg-white/[0.02] px-3 py-2 text-3xs text-slate-500">
            <Badge tone="accent">Beginner mode</Badge>
            Advanced shows every field and number. Switching never changes a setting.
            <Button variant="ghost" size="sm" onClick={() => setUiMode('advanced')}>
              Switch to Advanced
            </Button>
          </div>
        </div>
      ) : null}
    </Modal>
  );
}

function friendlyProfile(profile: string, mode: 'beginner' | 'advanced'): string {
  if (mode === 'advanced') return profile;
  if (profile === 'LONG_FAST' || profile === 'LONG_SLOW') return 'longer-range';
  if (profile === 'SHORT_FAST' || profile === 'SHORT_SLOW') return 'shorter-range';
  return 'balanced';
}

function Summary({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-2.5 py-2">
      <p className="text-3xs uppercase tracking-[0.14em] text-slate-600">{label}</p>
      <p className="mt-0.5 truncate text-2xs font-medium text-slate-300">{value}</p>
    </div>
  );
}
