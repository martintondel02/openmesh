import { useMemo, useState } from 'react';
import {
  BookOpen,
  Check,
  ChevronRight,
  KeyRound,
  Laptop,
  Radio,
  ShieldCheck,
  Sparkles,
  Wifi,
} from 'lucide-react';
import { useAppStore } from '../../store/app';
import { Badge, Button, ProgressBar, Toggle } from '../../components/ui/primitives';
import { Field, Select, TextInput } from '../../components/ui/forms';
import { HelpLink, HelpTip, Term } from '../../components/ui/help';
import { BEGINNER_COPY } from '../../lib/beginner-copy';
import { checklistForMode, checklistProgress, isSetupStateComplete } from '../../lib/setup';
import { LegalLimitLine } from '../../components/regulatory/Regulatory';
import { deriveRegulatoryState } from '../../lib/regulatory';
import { REGION_IDS, REGIONS } from '../../lib/airtime';
import { cn } from '../../lib/utils';

const SIMPLE_PROFILES = [
  { id: 'MEDIUM_FAST', label: 'Balanced', hint: 'Good range and speed for most homes. Start here.' },
  { id: 'LONG_FAST', label: 'Longer range', hint: 'Reaches further, but messages take a little longer.' },
  { id: 'SHORT_FAST', label: 'Shorter range', hint: 'Faster nearby; useful when the mesh is dense.' },
];

export function BeginnerSettings({ setMode }: { setMode: (mode: 'beginner' | 'advanced') => void }) {
  const config = useAppStore((state) => state.config);
  const settings = useAppStore((state) => state.settings);
  const setupState = useAppStore((state) => state.setupState);
  const ownNodeId = useAppStore((state) => state.ownNodeId);
  const updateConfig = useAppStore((state) => state.updateConfig);
  const saveSettings = useAppStore((state) => state.saveSettings);
  const resetIdentity = useAppStore((state) => state.resetIdentity);
  const openOnboarding = useAppStore((state) => state.openOnboarding);
  const openHelp = useAppStore((state) => state.openHelp);
  const pushToast = useAppStore((state) => state.pushToast);
  const bridgeStatus = useAppStore((state) => state.bridgeStatus);
  const regulatory = deriveRegulatoryState(config, bridgeStatus);
  const setDemoMode = useAppStore((state) => state.setDemoMode);

  const [nodeName, setNodeName] = useState(config?.node.name ?? '');
  const [role, setRole] = useState(config?.node.class ?? 'client');
  const [regionId, setRegionId] = useState(config?.radio.region ?? 'EU868');
  const [profileId, setProfileId] = useState(config?.radio.profile ?? 'MEDIUM_FAST');
  const [bridgeUrl, setBridgeUrl] = useState(settings.bridgeUrl);
  const [token, setToken] = useState(settings.token);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null);

  const checks = useMemo(
    () =>
      checklistForMode('beginner', {
        identity: Boolean(setupState?.checks.identity || ownNodeId),
        radio: Boolean(setupState?.checks.radio || bridgeStatus?.radio?.connected),
        api_token: Boolean(setupState?.checks.api_token || settings.token),
        channel: Boolean(setupState?.checks.channel),
        backbone: Boolean(setupState?.checks.backbone),
        agent: Boolean(setupState?.checks.agent),
      }),
    [setupState, ownNodeId, bridgeStatus, settings.token],
  );
  const progress = checklistProgress(checks);
  const complete = isSetupStateComplete(setupState) || progress.done === progress.total;

  const saveNode = () => {
    updateConfig({
      node: {
        ...(config?.node ?? { announce_interval_s: 1800 }),
        name: nodeName.trim() || config?.node.name || 'my-node',
        class: role,
      },
    });
    pushToast({ kind: 'success', title: 'Node name saved', message: 'Other nodes will hear the new name on the next announce.', helpTopic: 'contacts-identity' });
  };

  const saveRadio = () => {
    updateConfig({
      radio: {
        ...(config?.radio ?? { tx_power_dbm: 14 }),
        region: regionId,
        profile: profileId,
      },
    });
    pushToast({
      kind: 'success',
      title: 'Radio settings saved',
      message: 'The radio restarts briefly. Nearby nodes will adapt automatically.',
      helpTopic: 'radio-and-region',
    });
  };

  const testBridge = async () => {
    setTesting(true);
    setTestResult(null);
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 5000);
    try {
      const base = bridgeUrl.replace(/\/$/, '').replace(/^ws/, 'http');
      const response = await fetch(`${base}/api/health`, {
        headers: { Accept: 'application/json', 'X-OpenMesh-Client': 'web', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
        signal: controller.signal,
      });
      setTestResult(
        response.ok
          ? { ok: true, message: 'Your node answered. Save to keep these connection details.' }
          : { ok: false, message: `The node answered with status ${response.status}. Check the token and try again.` },
      );
    } catch {
      setTestResult({
        ok: false,
        message: 'Nothing answered at that address. Check the bridge is running, then try again.',
      });
    } finally {
      clearTimeout(timer);
      setTesting(false);
    }
  };

  const saveBridge = () => {
    saveSettings({ bridgeUrl: bridgeUrl.trim() || settings.bridgeUrl, token: token.trim() });
    pushToast({ kind: 'success', title: 'Connection details saved', message: 'The panel will reconnect using them.', helpTopic: 'install' });
  };

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto max-w-5xl px-4 py-4 sm:px-6 lg:py-6">
        <div className="flex flex-wrap items-center gap-2">
          <Badge tone="accent" icon={Sparkles}>Beginner mode</Badge>
          <p className="text-2xs text-slate-500">{BEGINNER_COPY.settingsIntro}</p>
        </div>

        <div className="mt-4 grid gap-4 lg:grid-cols-[minmax(0,1fr)_320px]">
          <div className="space-y-4">
            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
              <CardTitle icon={Laptop} title="This device" helpKey="settings.node.name" />
              <div className="mt-3 grid gap-3 sm:grid-cols-2">
                <Field label={BEGINNER_COPY.nodeNameLabel} hint={BEGINNER_COPY.nodeNameHelp} labelAccessory={<HelpTip helpKey="settings.node.name" label="Node name" />}>
                  {({ id }) => (
                    <TextInput id={id} value={nodeName} maxLength={32} onChange={(event) => setNodeName(event.target.value)} placeholder="my-phone" />
                  )}
                </Field>
                <Field label={BEGINNER_COPY.roleLabel} hint={BEGINNER_COPY.roleHelp} labelAccessory={<HelpTip helpKey="settings.node.class" label="Device role" />}>
                  {({ id }) => (
                    <Select id={id} value={role} onChange={(event) => setRole(event.target.value)}>
                      <option value="client">Phone or laptop — I carry it</option>
                      <option value="repeater">Repeater — stays in one place</option>
                      <option value="gateway">Gateway — bridges to the internet</option>
                      <option value="tracker">Tracker — battery powered</option>
                    </Select>
                  )}
                </Field>
              </div>
              <div className="mt-3 flex items-center justify-between gap-2">
                <p className="text-3xs text-slate-600">
                  {ownNodeId ? `Public id ${ownNodeId.slice(0, 8)}… — share it only when you want to be found.` : 'No identity yet.'}
                </p>
                <Button variant="primary" size="sm" icon={Check} onClick={saveNode}>Save name</Button>
              </div>
            </section>

            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
              <CardTitle icon={Radio} title="Radio coverage" helpKey="settings.radio.profile" />
              <div className="mt-3 grid gap-3 sm:grid-cols-2">
                <Field label="Radio coverage" hint={BEGINNER_COPY.radioHelp} labelAccessory={<HelpTip helpKey="settings.radio.profile" label="Radio coverage" />}>
                  {({ id }) => (
                    <Select id={id} value={profileId} onChange={(event) => setProfileId(event.target.value)}>
                      {SIMPLE_PROFILES.map((profile) => (
                        <option key={profile.id} value={profile.id}>{profile.label}</option>
                      ))}
                    </Select>
                  )}
                </Field>
                <Field label="Country band" hint="Choose the band your country allows." labelAccessory={<HelpTip helpKey="settings.radio.region" label="Country band" />}>
                  {({ id }) => (
                    <Select id={id} value={regionId} onChange={(event) => setRegionId(event.target.value)}>
                      {REGION_IDS.map((id) => {
                        const region = REGIONS[id]!;
                        return <option key={id} value={id}>{region.label}</option>;
                      })}
                    </Select>
                  )}
                </Field>
              </div>
              <p className="mt-2 text-3xs leading-relaxed text-slate-600">
                {SIMPLE_PROFILES.find((profile) => profile.id === profileId)?.hint}{' '}
                Exact bandwidth and airtime numbers live in Advanced mode.
              </p>
              <LegalLimitLine legal={regulatory.legal} mode="beginner" className="mt-2" />
              <div className="mt-3 flex items-center justify-between gap-2">
                <HelpLink topic="radio-and-region">How coverage works</HelpLink>
                <Button variant="primary" size="sm" icon={Check} onClick={saveRadio}>Save radio</Button>
              </div>
            </section>

            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
              <CardTitle icon={ShieldCheck} title="Privacy" helpKey="settings.position.sharing" />
              <div className="mt-3 space-y-3">
                <Toggle
                  id="beginner-position"
                  checked={Boolean(config?.features.position_sharing)}
                  onChange={(checked) => updateConfig({ features: { ...(config?.features ?? { auto_announce: true, store_forward: true, position_sharing: false }), position_sharing: checked } })}
                  label={BEGINNER_COPY.positionLabel}
                  description={BEGINNER_COPY.positionHelp}
                  labelAccessory={<HelpTip helpKey="settings.position.sharing" label={BEGINNER_COPY.positionLabel} />}
                />
                <Toggle
                  id="beginner-store-forward"
                  checked={Boolean(config?.features.store_forward)}
                  onChange={(checked) => updateConfig({ features: { ...(config?.features ?? { auto_announce: true, store_forward: true, position_sharing: false }), store_forward: checked } })}
                  label="Hold messages for people who are offline"
                  description="Encrypted messages wait on this device until the person is back in range. This is called store-and-forward."
                  labelAccessory={<HelpTip helpKey="settings.store.forward" label="Store-and-forward" />}
                />
              </div>
              <p className="mt-2 text-3xs leading-relaxed text-slate-600">
                Messages and channels are end-to-end encrypted. Relays see only encrypted envelopes, never your words.
              </p>
            </section>

            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
              <CardTitle icon={Wifi} title={BEGINNER_COPY.bridgeLabel} helpKey="settings.bridge.url" />
              <div className="mt-3 space-y-3">
                <Field label="Address of your node" hint={BEGINNER_COPY.bridgeHelp} labelAccessory={<HelpTip helpKey="settings.bridge.url" label="Address of your node" />}>
                  {({ id }) => (
                    <TextInput id={id} value={bridgeUrl} onChange={(event) => setBridgeUrl(event.target.value)} placeholder="http://127.0.0.1:8787" />
                  )}
                </Field>
                <Field label="Access token" hint="Optional on a local machine. Keep it private; it is never shown in help or logs." labelAccessory={<HelpTip helpKey="settings.bridge.token" label="Access token" />}>
                  {({ id }) => (
                    <TextInput id={id} type="password" value={token} onChange={(event) => setToken(event.target.value)} placeholder="bridge token" autoComplete="off" />
                  )}
                </Field>
              </div>
              {testResult ? (
                <p role="status" className={cn('mt-2 rounded-lg border px-3 py-2 text-2xs leading-relaxed', testResult.ok ? 'border-emerald-400/25 bg-emerald-400/[0.06] text-emerald-200' : 'border-rose-400/25 bg-rose-400/[0.06] text-rose-200')}>
                  {testResult.message}
                </p>
              ) : null}
              <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
                <HelpLink topic="install">How to start the bridge</HelpLink>
                <div className="flex items-center gap-2">
                  <Button variant="ghost" size="sm" loading={testing} onClick={() => void testBridge()}>Test</Button>
                  <Button variant="primary" size="sm" icon={Check} onClick={saveBridge}>Save connection</Button>
                </div>
              </div>
            </section>

            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
              <CardTitle icon={Sparkles} title="Try it safely" helpKey="settings.bridge.demo" />
              <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
                <p className="text-2xs leading-relaxed text-slate-400">
                  Demo mode shows a seeded mesh with nothing transmitted. Useful for learning or screenshots.
                </p>
                <Button variant="ghost" size="sm" onClick={() => void setDemoMode(true)}>Switch to demo</Button>
              </div>
            </section>
          </div>

          <div className="space-y-4">
            <section className="rounded-2xl border border-emerald-400/20 bg-emerald-400/[0.05] p-4">
              <div className="flex items-start gap-2.5">
                <span className="grid h-9 w-9 shrink-0 place-items-center rounded-xl border border-emerald-400/25 bg-emerald-400/10 text-emerald-300">
                  <ShieldCheck className="h-4 w-4" aria-hidden="true" />
                </span>
                <div>
                  <h3 className="flex items-center gap-1.5 text-sm font-semibold text-white">
                    {BEGINNER_COPY.secureCardTitle}
                    <HelpTip helpKey="onboarding.secure" label="Secure my node" />
                  </h3>
                  <p className="mt-1 text-2xs leading-relaxed text-slate-400">{BEGINNER_COPY.secureCardBody}</p>
                </div>
              </div>
              <div className="mt-3">
                <div className="flex items-center justify-between text-3xs text-slate-500">
                  <span>{complete ? 'All checks passed' : `${progress.done} of ${progress.total} checks passed`}</span>
                  <span>{progress.pct}%</span>
                </div>
                <ProgressBar value={progress.pct} className="mt-1.5" tone={complete ? 'accent' : 'sky'} />
              </div>
              <ul className="mt-3 space-y-1.5">
                {checks.map((item) => (
                  <li key={item.key} className="flex items-center gap-1.5 text-2xs">
                    {item.ok ? (
                      <Check className="h-3.5 w-3.5 shrink-0 text-emerald-300" aria-hidden="true" />
                    ) : (
                      <ChevronRight className="h-3.5 w-3.5 shrink-0 text-slate-600" aria-hidden="true" />
                    )}
                    <span className={item.ok ? 'text-slate-400 line-through decoration-slate-600' : 'text-slate-300'}>{item.label}</span>
                  </li>
                ))}
              </ul>
              <Button variant="primary" size="sm" fullWidth className="mt-3" onClick={openOnboarding}>
                {complete ? 'Review secure setup' : BEGINNER_COPY.secureCardAction}
              </Button>
            </section>

            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
              <h3 className="flex items-center gap-1.5 text-2xs font-semibold uppercase tracking-[0.14em] text-slate-500">
                <BookOpen className="h-3.5 w-3.5 text-slate-400" aria-hidden="true" />
                Learn as you go
              </h3>
              <div className="mt-2 space-y-1.5">
                {[
                  ['getting-started', 'Your first mesh in 10 minutes'],
                  ['messaging', 'Messages and offline delivery'],
                  ['security', 'What is encrypted, what a relay sees'],
                  ['glossary', 'Every term in one place'],
                ].map(([topic, label]) => (
                  <button
                    key={topic}
                    type="button"
                    onClick={() => openHelp(topic)}
                    className="focus-ring flex w-full items-center justify-between gap-2 rounded-lg px-1.5 py-1.5 text-left text-2xs text-slate-400 hover:bg-white/[0.04] hover:text-slate-200"
                  >
                    {label}
                    <ChevronRight className="h-3 w-3 shrink-0 text-slate-600" aria-hidden="true" />
                  </button>
                ))}
              </div>
            </section>

            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
              <h3 className="text-2xs font-semibold uppercase tracking-[0.14em] text-slate-500">Mode</h3>
              <p className="mt-1.5 text-2xs leading-relaxed text-slate-400">
                Beginner keeps the safe basics. Advanced shows every radio, identity, backbone and agent control.
              </p>
              <Button
                variant="primary"
                size="sm"
                fullWidth
                className="mt-3"
                onClick={() => {
                  setMode('advanced');
                  pushToast({ kind: 'info', title: 'Advanced mode', message: 'Every setting is now visible. Switching mode never changed a setting.', helpTopic: 'getting-started' });
                }}
              >
                {BEGINNER_COPY.showAdvanced}
              </Button>
              <p className="mt-2 text-3xs leading-relaxed text-slate-600">{BEGINNER_COPY.advancedHint}</p>
            </section>

            <section className="rounded-2xl border border-rose-400/15 bg-rose-400/[0.03] p-4">
              <h3 className="flex items-center gap-1.5 text-2xs font-semibold uppercase tracking-[0.14em] text-rose-200/80">
                <KeyRound className="h-3.5 w-3.5" aria-hidden="true" />
                Start over
              </h3>
              <p className="mt-1.5 text-3xs leading-relaxed text-slate-500">
                Resetting creates a new identity. Contacts must verify you again. Back up first; advanced settings explain the details.
              </p>
              <Button variant="danger" size="sm" fullWidth className="mt-2" onClick={() => void resetIdentity()}>
                Reset this node…
              </Button>
            </section>
          </div>
        </div>

        <p className="mt-4 flex items-center gap-1 text-3xs text-slate-600">
          <Term term="Airtime" /> and other terms are explained where they appear.
          <button type="button" onClick={() => openHelp('glossary')} className="focus-ring rounded underline-offset-2 hover:text-slate-400 hover:underline">
            Open the glossary
          </button>
        </p>
      </div>
    </div>
  );
}

function CardTitle({ icon: Icon, title, helpKey }: { icon: typeof Radio; title: string; helpKey: string }) {
  return (
    <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.14em] text-slate-500">
      <Icon className="h-3.5 w-3.5 text-emerald-300/80" aria-hidden="true" />
      {title}
      <HelpTip helpKey={helpKey} label={title} />
    </h3>
  );
}
