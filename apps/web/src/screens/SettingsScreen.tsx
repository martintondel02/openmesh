import { useEffect, useMemo, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import {
  AlertTriangle,
  CheckCircle2,
  Eye,
  EyeOff,
  Loader2,
  Megaphone,
  Plug,
  Radio,
  RefreshCw,
  RotateCcw,
  Save,
  ShieldCheck,
  QrCode,
  SlidersHorizontal,
  TriangleAlert,
  UserRound,
  Wifi,
  WifiOff,
} from 'lucide-react';
import { useAppStore } from '../store/app';
import type { ConfigData } from '../types/api';
import {
  announceAirtimeMs,
  assessDutyCycle,
  attachmentBytesPerHour,
  estimateAttachment,
  getProfile,
  getRegion,
  hourlyAirtimeBudgetMs,
  maxAttachmentBytes,
  PROFILE_IDS,
  RADIO_PROFILES,
  REGION_IDS,
  REGIONS,
  voiceCapMs,
} from '../lib/airtime';
import { buildHttpBaseUrl } from '../lib/ws';
import { formatBridgeResetCounts, type BridgeResetResult } from '../lib/bridge-admin';
import { radioStateTone, transportState, transportStateLabel } from '../lib/radio-link';
import { cn, formatBytes, formatSeconds, formatUptime, formatNodeId } from '../lib/utils';
import { shortNodeId } from '../lib/display';
import { Avatar, Badge, Button, SectionTitle, SegmentedControl, Toggle } from '../components/ui/primitives';
import { CopyButton } from '../components/ui/signals';
import { QRCode } from '../components/common/QRCode';
import { Callout, Field, Slider, TextInput, InlineCode } from '../components/ui/forms';
import { Modal } from '../components/ui/overlay';
import { HelpTip } from '../components/ui/help';
import { BeginnerSettings } from './settings/BeginnerSettings';
import { RadioLinkCard } from './settings/RadioLinkCard';
import { LegalMaximaCard, RegulatoryOverrideCard } from '../components/regulatory/Regulatory';

const NODE_CLASSES = [
  { value: 'client', label: 'Client', hint: 'Phone or handheld; sleeps to save power.' },
  { value: 'repeater', label: 'Repeater', hint: 'Relays traffic, stores envelopes, announces hourly.' },
  { value: 'gateway', label: 'Gateway', hint: 'Bridges mesh traffic to LAN/internet.' },
  { value: 'tracker', label: 'Tracker', hint: 'Low-power GPS beacon.' },
  { value: 'room_server', label: 'Room server', hint: 'Hosts channel history for a group.' },
];

type TestState = 'idle' | 'busy' | 'ok' | 'fail';

export function SettingsScreen() {
  const config = useAppStore((state) => state.config);
  const mode = useAppStore((state) => state.mode);
  const settings = useAppStore((state) => state.settings);
  const hello = useAppStore((state) => state.hello);
  const bridgeStatus = useAppStore((state) => state.bridgeStatus);
  const ownNodeId = useAppStore((state) => state.ownNodeId);
  const bridgeVersion = useAppStore((state) => state.bridgeVersion);
  const updateConfig = useAppStore((state) => state.updateConfig);
  const announceNow = useAppStore((state) => state.announceNow);
  const ownName = useAppStore((state) => state.ownName);
  const saveSettings = useAppStore((state) => state.saveSettings);
  const setDemoMode = useAppStore((state) => state.setDemoMode);
  const retryConnection = useAppStore((state) => state.retryConnection);
  const resetClient = useAppStore((state) => state.resetClient);
  const resetBridgeData = useAppStore((state) => state.resetBridgeData);
  const pushToast = useAppStore((state) => state.pushToast);
  const uiMode = useAppStore((state) => state.uiMode);
  const setUiMode = useAppStore((state) => state.setUiMode);

  const profile = getProfile(config?.radio.profile);
  const region = getRegion(config?.radio.region);
  const announceIntervalS = config?.node.announce_interval_s ?? 1800;
  const duty = useMemo(
    () => assessDutyCycle(profile.id, region.id, announceIntervalS),
    [profile.id, region.id, announceIntervalS],
  );
  const capBytes = maxAttachmentBytes(profile.id);
  const capEstimate = useMemo(() => estimateAttachment(capBytes, profile.id, region.id), [capBytes, profile.id, region.id]);
  const hourlyBudget = hourlyAirtimeBudgetMs(region.id);
  const bytesPerHour = attachmentBytesPerHour(profile.id, region.id);

  const [nameDraft, setNameDraft] = useState(config?.node.name ?? '');
  const [intervalDraft, setIntervalDraft] = useState(Math.round(announceIntervalS / 60));
  const [txPowerDraft, setTxPowerDraft] = useState(config?.radio.tx_power_dbm ?? 14);
  const [urlDraft, setUrlDraft] = useState(settings.bridgeUrl);
  const [tokenDraft, setTokenDraft] = useState(settings.token);
  const [showToken, setShowToken] = useState(false);
  const [testState, setTestState] = useState<TestState>('idle');
  const [testDetail, setTestDetail] = useState<string | null>(null);
  const [qrOpen, setQrOpen] = useState(false);
  const [clientResetOpen, setClientResetOpen] = useState(false);
  const [clientResetText, setClientResetText] = useState('');
  const [clientResetting, setClientResetting] = useState(false);
  const [bridgeResetOpen, setBridgeResetOpen] = useState(false);
  const [bridgeResetText, setBridgeResetText] = useState('');
  const [bridgeResetting, setBridgeResetting] = useState(false);
  const [bridgeResetResult, setBridgeResetResult] = useState<BridgeResetResult | null>(null);
  const [bridgeResetError, setBridgeResetError] = useState<string | null>(null);
  const [savingBridge, setSavingBridge] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    setNameDraft(config?.node.name ?? '');
    setIntervalDraft(Math.round((config?.node.announce_interval_s ?? 1800) / 60));
    setTxPowerDraft(config?.radio.tx_power_dbm ?? 14);
  }, [config?.node.name, config?.node.announce_interval_s, config?.radio.tx_power_dbm]);

  useEffect(() => {
    setUrlDraft(settings.bridgeUrl);
    setTokenDraft(settings.token);
  }, [settings.bridgeUrl, settings.token]);

  useEffect(
    () => () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    },
    [],
  );

  const commitNode = async (patch: Partial<ConfigData['node']>) => {
    updateConfig({ node: { ...(config?.node ?? { name: '', class: 'client', announce_interval_s: 1800 }), ...patch } });
  };

  const commitRadio = (patch: Partial<ConfigData['radio']>) => {
    updateConfig({ radio: { ...(config?.radio ?? { profile: profile.id, region: region.id, tx_power_dbm: 14 }), ...patch } });
  };

  const commitFeatures = (patch: Partial<ConfigData['features']>) => {
    updateConfig({
      features: {
        auto_announce: true,
        store_forward: true,
        position_sharing: true,
        ...(config?.features ?? {}),
        ...patch,
      },
    });
  };

  const scheduleDebounced = (action: () => void, delay = 450) => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(action, delay);
  };

  const trimmedName = nameDraft.trim();
  const nameBytes = new TextEncoder().encode(trimmedName).length;
  const nameError = trimmedName
    ? nameBytes > 32
      ? `Name is ${nameBytes}/32 bytes — shorten it.`
      : null
    : 'Enter a name (1–32 bytes).';
  const nameDirty = trimmedName !== (config?.node.name ?? '');
  const savedName = (config?.node.name ?? '').trim() || ownName || 'Unnamed node';

  const saveName = () => {
    if (nameError) return;
    setNameDraft(trimmedName);
    if (!nameDirty) return;
    updateConfig({
      node: { ...(config?.node ?? { name: '', class: 'client', announce_interval_s: 1800 }), name: trimmedName },
    });
    // Announce immediately so peers see the new name without waiting for the periodic announce.
    if (mode !== 'demo') announceNow({ silent: true });
    pushToast({
      kind: 'success',
      title: 'Display name saved',
      message:
        mode === 'demo'
          ? `Demo mesh now shows “${trimmedName}” locally.`
          : `“${trimmedName}” is in your next signed announce — nearby nodes will update shortly.`,
    });
  };

  const testConnection = async () => {
    setTestState('busy');
    setTestDetail(null);
    try {
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 2500);
      const response = await fetch(`${buildHttpBaseUrl(urlDraft)}/api/health`, {
        signal: controller.signal,
        headers: { Accept: 'application/json' },
      });
      clearTimeout(timeout);
      if (!response.ok) {
        setTestState('fail');
        setTestDetail(`HTTP ${response.status} ${response.statusText}`);
        return;
      }
      const json = (await response.json()) as { ok?: boolean; version?: string };
      setTestState('ok');
      setTestDetail(`Bridge ${json.version ?? ''} answered — no token needed for /api/health.`);
    } catch (error) {
      setTestState('fail');
      setTestDetail(error instanceof Error ? error.message : 'No response');
    }
  };

  const saveBridge = async () => {
    setSavingBridge(true);
    saveSettings({ bridgeUrl: urlDraft.trim(), token: tokenDraft.trim() });
    pushToast({ kind: 'success', title: 'Bridge settings saved', message: 'Probing the bridge now…' });
    try {
      await retryConnection();
    } finally {
      setSavingBridge(false);
    }
  };

  if (uiMode === 'beginner') {
    return <BeginnerSettings setMode={setUiMode} />;
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto max-w-4xl space-y-4 px-4 py-5 md:px-6">
        <div className="flex flex-wrap items-center gap-3">
          <p className="flex-1 text-2xs text-slate-500">
            Changes are sent to the bridge as <InlineCode>set_config</InlineCode> and apply to the next radio slot.
          </p>
          <Button variant="secondary" size="sm" icon={Megaphone} onClick={() => announceNow()}>
            Announce now
          </Button>
        </div>

        {mode === 'demo' ? (
          <Callout tone="warn" icon={AlertTriangle} title="You are editing a demo mesh">
            Changes are simulated locally and reset when demo mode restarts. Toggle demo mode off under Bridge
            connection to configure a real bridge.
          </Callout>
        ) : null}

        <SectionCard
          icon={SlidersHorizontal}
          title="Interface mode"
          description="Beginner keeps the safe basics; Advanced reveals every radio, identity, backbone and agent control."
        >
          <div className="flex flex-wrap items-center gap-3">
            <SegmentedControl
              value={uiMode}
              options={[
                { value: 'beginner', label: 'Beginner' },
                { value: 'advanced', label: 'Advanced' },
              ]}
              onChange={(next) => setUiMode(next)}
              ariaLabel="Interface mode"
              className="w-full sm:w-auto"
            />
            <HelpTip helpKey="mode.toggle" label="Interface mode" />
            <p className="text-2xs text-slate-500">Switching never changes a setting; it only changes what is shown.</p>
          </div>
        </SectionCard>

        {/* Identity */}
        <SectionCard icon={UserRound} title="Identity" description="Who this node is on the mesh. Announces are signed with this identity.">
          <div className="flex flex-wrap items-center gap-3 rounded-xl border border-emerald-400/15 bg-gradient-to-br from-emerald-400/[0.08] to-sky-500/[0.04] p-3">
            <Avatar name={savedName} seed={ownNodeId ?? 'you'} size="md" online />
            <div className="min-w-0 flex-1">
              <p className="text-3xs font-semibold uppercase tracking-[0.12em] text-emerald-300/80">You are</p>
              <p className="truncate text-sm font-semibold text-white">
                {savedName} <span className="font-mono text-2xs font-normal text-slate-400">· {shortNodeId(ownNodeId)}</span>
              </p>
              <p className="mt-0.5 text-2xs leading-relaxed text-slate-400">
                This name travels in your signed announces so contacts recognise you. Your node id stays unique even
                if two nodes pick the same name.
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              {ownNodeId ? <CopyButton value={ownNodeId} label="Copy id" size="md" variant="surface" /> : null}
              <Button variant="secondary" size="sm" icon={QrCode} onClick={() => setQrOpen(true)} disabled={!ownNodeId}>
                My QR
              </Button>
            </div>
          </div>

          <div className="grid gap-4 sm:grid-cols-2">
            <Field
              label="Node name"
              hint={`${nameBytes}/32 bytes · shown to peers in announces; announced immediately after saving.`}
              error={nameError}
              success={!nameError && nameDirty ? 'Unsaved — press Save to apply and announce.' : null}
              labelAccessory={<HelpTip helpKey="settings.node.name" label="Node name" />}
            >
              {({ id, describedBy }) => (
                <div className="flex items-center gap-1.5">
                  <TextInput
                    id={id}
                    value={nameDraft}
                    maxLength={32}
                    aria-describedby={describedBy}
                    aria-invalid={nameError ? true : undefined}
                    onChange={(event) => setNameDraft(event.target.value)}
                    onBlur={() => {
                      if (nameError) setNameDraft(config?.node.name ?? '');
                    }}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') {
                        event.preventDefault();
                        saveName();
                      }
                    }}
                    className="flex-1"
                  />
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={Save}
                    onClick={saveName}
                    disabled={Boolean(nameError) || !nameDirty}
                  >
                    Save
                  </Button>
                </div>
              )}
            </Field>

            <Field label="Node class" hint={NODE_CLASSES.find((item) => item.value === (config?.node.class ?? 'client'))?.hint} labelAccessory={<HelpTip helpKey="settings.node.class" label="Node class" />}>
              {({ id }) => (
                <select
                  id={id}
                  className="field-shell h-9 py-0"
                  value={config?.node.class ?? 'client'}
                  onChange={(event) => void commitNode({ class: event.target.value })}
                >
                  {NODE_CLASSES.map((nodeClass) => (
                    <option key={nodeClass.value} value={nodeClass.value}>
                      {nodeClass.label}
                    </option>
                  ))}
                </select>
              )}
            </Field>
          </div>

          <div className="grid gap-4 sm:grid-cols-2">
            <Field
              label="Node id (self-certifying)"
              labelAccessory={<HelpTip helpKey="settings.identity.public" label="Node id" />}
              hint="SHA-256(“openmesh/v1/node” || sign_pk || dh_pk)[0..8]"
            >
              {() => (
                <div className="flex items-center gap-1.5">
                  <code className="field-shell block flex-1 truncate py-2 font-mono text-2xs text-slate-400">
                    {ownNodeId ? formatNodeId(ownNodeId) : '—'}
                  </code>
                  {ownNodeId ? <CopyButton value={ownNodeId} label="Copy" size="md" variant="surface" /> : null}
                </div>
              )}
            </Field>
            <div className="space-y-2">
              <Slider
                id="announce-interval"
                label="Announce interval"
                labelAccessory={<HelpTip helpKey="settings.announce.interval" label="Announce interval" />}
                min={5}
                max={60}
                step={5}
                value={intervalDraft}
                formatValue={(value) => `${value} min`}
                markers={[
                  { value: 5, label: '5 min floor' },
                  { value: 60, label: '1 h' },
                ]}
                onChange={(value) => {
                  setIntervalDraft(value);
                  scheduleDebounced(() => void commitNode({ announce_interval_s: value * 60 }), 500);
                }}
              />
            </div>
          </div>

          <div className="space-y-3 border-t border-white/[0.05] pt-3">
            <Toggle
              id="auto-announce"
              label="Auto-announce"
              description="Re-announce on the interval above, plus on start and network change (5 min minimum)."
              labelAccessory={<HelpTip helpKey="settings.auto.announce" label="Auto-announce" />}
              checked={config?.features.auto_announce ?? true}
              onChange={(checked) => commitFeatures({ auto_announce: checked })}
            />
          </div>

          <Callout tone={duty.level === 'danger' ? 'danger' : duty.level === 'warn' ? 'warn' : 'info'} icon={Wifi}>
            {duty.message}{' '}
            {duty.budgetMs ? `Each announce costs ~${formatSeconds(announceAirtimeMs(profile.id))} of airtime.` : ''}
          </Callout>
        </SectionCard>

        {/* Radio */}
        <SectionCard icon={Radio} title="Radio & airtime" description="LoRa modem preset and regulatory region. Media budgets follow from these.">
          <RadioLinkCard variant="advanced" />
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Modem preset" hint={profile.description} labelAccessory={<HelpTip helpKey="settings.radio.profile" label="Modem preset" />}>
              {({ id }) => (
                <select
                  id={id}
                  className="field-shell h-9 py-0"
                  value={profile.id}
                  onChange={(event) => commitRadio({ profile: event.target.value })}
                >
                  {PROFILE_IDS.map((profileId) => (
                    <option key={profileId} value={profileId}>
                      {RADIO_PROFILES[profileId]!.label} · SF{RADIO_PROFILES[profileId]!.spreadingFactor} ·{' '}
                      {RADIO_PROFILES[profileId]!.bwKHz} kHz
                    </option>
                  ))}
                </select>
              )}
            </Field>
            <Field label="Region" hint={region.notes} labelAccessory={<HelpTip helpKey="settings.radio.region" label="Region" />}>
              {({ id }) => (
                <select
                  id={id}
                  className="field-shell h-9 py-0"
                  value={region.id}
                  onChange={(event) => commitRadio({ region: event.target.value })}
                >
                  {REGION_IDS.map((regionId) => (
                    <option key={regionId} value={regionId}>
                      {REGIONS[regionId]!.label}
                    </option>
                  ))}
                </select>
              )}
            </Field>
          </div>

          <div className="grid gap-4 sm:grid-cols-2">
            <Slider
              id="tx-power"
              label="Transmit power"
              labelAccessory={<HelpTip helpKey="settings.radio.txpower" label="Transmit power" />}
              min={5}
              max={region.maxTxPowerDbm}
              step={1}
              value={Math.min(region.maxTxPowerDbm, txPowerDraft)}
              formatValue={(value) => `${value} dBm`}
              markers={[
                { value: 5, label: '5 dBm' },
                { value: region.maxTxPowerDbm, label: `${region.maxTxPowerDbm} dBm max` },
              ]}
              onChange={(value) => {
                setTxPowerDraft(value);
                scheduleDebounced(() => commitRadio({ tx_power_dbm: value }), 500);
              }}
            />
            <div className="rounded-xl border border-white/[0.06] bg-white/[0.02] p-3">
              <SectionTitle>Computed media caps</SectionTitle>
              <dl className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1.5 text-2xs">
                <dt className="text-slate-500">Attachment cap</dt>
                <dd className="text-right font-mono text-slate-200">{formatBytes(capBytes)}</dd>
                <dt className="text-slate-500">Voice cap</dt>
                <dd className="text-right font-mono text-slate-200">{formatSeconds(voiceCapMs(profile.id), 0)}</dd>
                <dt className="text-slate-500">Max-size airtime</dt>
                <dd className="text-right font-mono text-slate-200">~{formatSeconds(capEstimate.airtimeMs)}</dd>
                <dt className="text-slate-500">Chunks at cap</dt>
                <dd className="text-right font-mono text-slate-200">{capEstimate.chunks}</dd>
                <dt className="text-slate-500">Duty budget</dt>
                <dd className="text-right font-mono text-slate-200">
                  {hourlyBudget ? `${formatSeconds(hourlyBudget, 0)}/h` : 'no limit'}
                </dd>
                <dt className="text-slate-500">Media per hour</dt>
                <dd className="text-right font-mono text-slate-200">
                  {bytesPerHour === null ? 'bandwidth-bound' : formatBytes(bytesPerHour)}
                </dd>
              </dl>
            </div>
          </div>

          {duty.level !== 'ok' ? (
            <Callout tone={duty.level === 'danger' ? 'danger' : 'warn'} icon={TriangleAlert} title="Duty-cycle warning">
              {duty.message} Consider a longer announce interval or a wider region budget.
            </Callout>
          ) : null}
        </SectionCard>

        <LegalMaximaCard />
        <RegulatoryOverrideCard />

        {/* Features */}
        <SectionCard icon={SlidersHorizontal} title="Features" description="Behaviour toggles reported by the bridge config snapshot.">
          <div className="space-y-3.5">
            <Toggle
              id="position-sharing"
              label="Position sharing"
              description="Include GPS fixes in announces and position messages when available."
              labelAccessory={<HelpTip helpKey="settings.position.sharing" label="Position sharing" />}
              checked={config?.features.position_sharing ?? true}
              onChange={(checked) => commitFeatures({ position_sharing: checked })}
            />
            <div className="h-px bg-white/[0.05]" />
            <Toggle
              id="store-forward"
              label="Store & forward"
              description="Ask repeaters to hold sealed envelopes for offline peers (up to 72 h)."
              labelAccessory={<HelpTip helpKey="settings.store.forward" label="Store & forward" />}
              checked={config?.features.store_forward ?? true}
              onChange={(checked) => commitFeatures({ store_forward: checked })}
            />
            <div className="h-px bg-white/[0.05]" />
            <Toggle
              id="auto-announce-feature"
              label="Announce on network change"
              description="Announce immediately when the radio reconnects or the region changes."
              labelAccessory={<HelpTip helpKey="settings.auto.announce" label="Announce on network change" />}
              checked={config?.features.auto_announce ?? true}
              onChange={(checked) => commitFeatures({ auto_announce: checked })}
            />
          </div>
        </SectionCard>

        {/* Bridge */}
        <SectionCard icon={Plug} title="Bridge connection" description="The local daemon that owns the radio, message store and attachments.">
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Bridge URL" hint="Loopback by default. WebSocket URL is derived automatically." labelAccessory={<HelpTip helpKey="settings.bridge.url" label="Bridge URL" />}>
              {({ id }) => (
                <TextInput
                  id={id}
                  value={urlDraft}
                  onChange={(event) => setUrlDraft(event.target.value)}
                  placeholder="http://127.0.0.1:8787"
                  className="font-mono text-xs"
                />
              )}
            </Field>
            <Field label="Bearer token" hint="From ~/.config/openmesh/bridge-token." labelAccessory={<HelpTip helpKey="settings.bridge.token" label="Bearer token" />}>
              {({ id }) => (
                <div className="relative">
                  <TextInput
                    id={id}
                    type={showToken ? 'text' : 'password'}
                    value={tokenDraft}
                    onChange={(event) => setTokenDraft(event.target.value)}
                    placeholder="32-byte token"
                    className="pr-9 font-mono text-xs"
                  />
                  <button
                    type="button"
                    aria-label={showToken ? 'Hide token' : 'Show token'}
                    onClick={() => setShowToken((value) => !value)}
                    className="absolute right-1.5 top-1/2 grid h-6 w-6 -translate-y-1/2 place-items-center rounded text-slate-500 hover:bg-white/[0.06] hover:text-slate-300"
                  >
                    {showToken ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
                  </button>
                </div>
              )}
            </Field>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <Button variant="secondary" size="sm" icon={testState === 'busy' ? Loader2 : Wifi} onClick={() => void testConnection()} loading={testState === 'busy'}>
              Test connection
            </Button>
            <Button variant="primary" size="sm" icon={Save} onClick={() => void saveBridge()} loading={savingBridge}>
              Save & reconnect
            </Button>
            <Button variant="ghost" size="sm" icon={RefreshCw} onClick={() => void retryConnection()}>
              Retry now
            </Button>
            {testState === 'ok' ? (
              <Badge tone="accent" icon={CheckCircle2}>Health OK</Badge>
            ) : testState === 'fail' ? (
              <Badge tone="rose" icon={WifiOff}>Unreachable</Badge>
            ) : null}
          </div>
          {testDetail ? <p className="text-2xs leading-relaxed text-slate-500">{testDetail}</p> : null}

          <div className="rounded-xl border border-white/[0.06] bg-white/[0.02] p-3">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-xs font-medium text-slate-200">Demo mode</p>
                <p className="mt-0.5 text-2xs leading-relaxed text-slate-500">
                  Run entirely from a seeded Oslo dataset — perfect for screenshots and UI review without a bridge.
                </p>
              </div>
              <Toggle
                id="demo-mode"
                label="Demo mode"
                checked={mode === 'demo'}
                onChange={(checked) => void setDemoMode(checked)}
              />
            </div>
          </div>

          <div className="grid gap-3 rounded-xl border border-white/[0.06] bg-white/[0.02] p-3 sm:grid-cols-2">
            <div>
              <SectionTitle>Live session</SectionTitle>
              <dl className="mt-2 space-y-1 text-2xs">
                <div className="flex justify-between gap-3">
                  <dt className="text-slate-500">Bridge version</dt>
                  <dd className="font-mono text-slate-300">{bridgeVersion ?? '—'}</dd>
                </div>
                <div className="flex justify-between gap-3">
                  <dt className="text-slate-500">Protocol</dt>
                  <dd className="font-mono text-slate-300">{hello?.protocol ?? '—'}</dd>
                </div>
                <div className="flex justify-between gap-3">
                  <dt className="text-slate-500">Uptime</dt>
                  <dd className="font-mono text-slate-300">{bridgeStatus ? formatUptime(bridgeStatus.uptime_s) : '—'}</dd>
                </div>
                <div className="flex justify-between gap-3">
                  <dt className="text-slate-500">Stored envelopes</dt>
                  <dd className="font-mono text-slate-300">{bridgeStatus?.mesh.stored_envelopes ?? '—'}</dd>
                </div>
              </dl>
            </div>
            <div>
              <SectionTitle>Radios</SectionTitle>
              <div className="mt-2 space-y-1.5">
                {(hello?.radios ?? []).length === 0 ? (
                  <p className="text-2xs text-slate-500">No radios reported.</p>
                ) : (
                  hello!.radios.map((radio) => {
                    const radioTone = radioStateTone(transportState(radio));
                    return (
                      <div key={radio.id} className="flex items-center gap-2 text-2xs">
                        <span
                          className={cn(
                            'h-1.5 w-1.5 shrink-0 rounded-full',
                            radioTone === 'emerald' && 'bg-emerald-400',
                            radioTone === 'amber' && 'bg-amber-400',
                            radioTone === 'rose' && 'bg-rose-400',
                            radioTone === 'slate' && 'bg-slate-500',
                          )}
                          aria-hidden="true"
                        />
                        <span className="min-w-0 flex-1 truncate text-slate-300">{radio.label}</span>
                        <span
                          className={cn(
                            'shrink-0 text-3xs font-medium',
                            radioTone === 'emerald' && 'text-emerald-300/80',
                            radioTone === 'amber' && 'text-amber-300/80',
                            radioTone === 'rose' && 'text-rose-300/80',
                            radioTone === 'slate' && 'text-slate-500',
                          )}
                        >
                          {transportStateLabel(radio)}
                        </span>
                        <span className="shrink-0 font-mono text-3xs text-slate-500">
                          {radio.kind} · {radio.profile.replace(/_/g, ' ')}
                        </span>
                      </div>
                    );
                  })
                )}
              </div>
            </div>
          </div>
        </SectionCard>

        {/* Danger zone: local browser reset + full bridge/data/identity reset */}
        <section className="rounded-2xl border border-rose-400/20 bg-rose-400/[0.03] p-4 md:p-5">
          <div className="flex items-center gap-2">
            <TriangleAlert className="h-4 w-4 text-rose-400" aria-hidden="true" />
            <h3 className="text-sm font-semibold text-rose-200">Danger zone</h3>
          </div>

          <div className="mt-3 space-y-3 divide-y divide-rose-400/10">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="text-xs font-medium text-slate-200">Reset this client</p>
                <p className="mt-0.5 max-w-xl text-2xs leading-relaxed text-slate-500">
                  Clears this browser&apos;s saved bridge URL and token, drafts, demo flags and local settings,
                  then reloads into first-run state. It does <span className="font-medium text-slate-300">not</span>{' '}
                  erase messages or contacts on the bridge, and does not change the node identity.
                </p>
              </div>
              <Button variant="danger" size="md" icon={RotateCcw} onClick={() => setClientResetOpen(true)}>
                Reset this client
              </Button>
            </div>

            <div className="flex flex-wrap items-center justify-between gap-3 pt-3">
              <div className="min-w-0">
                <p className="text-xs font-medium text-slate-200">Factory reset bridge data + new identity</p>
                <p className="mt-0.5 max-w-xl text-2xs leading-relaxed text-slate-500">
                  Wipes messages, contacts, channels, nodes, attachments and the activity log on the bridge, then
                  generates a <span className="font-medium text-slate-300">new bridge node identity</span>. The
                  Public Mesh channel is re-created and joined automatically. Your bearer token and radio settings
                  are kept. Old contacts and QR codes become invalid.
                </p>
              </div>
              <Button
                variant="danger"
                size="md"
                icon={TriangleAlert}
                onClick={() => {
                  setBridgeResetError(null);
                  setBridgeResetOpen(true);
                }}
                disabled={mode === 'demo'}
                title={mode === 'demo' ? 'Connect to a real bridge to wipe bridge data' : undefined}
              >
                Wipe bridge data
              </Button>
            </div>
            {mode === 'demo' ? (
              <p className="pt-2 text-3xs leading-relaxed text-slate-600">
                Bridge reset is disabled in demo mode. Turn demo mode off and connect to the bridge first.
              </p>
            ) : null}
          </div>

          {bridgeResetResult ? (
            <div className="mt-3 rounded-xl border border-emerald-400/25 bg-emerald-400/[0.05] p-3 text-2xs leading-relaxed text-emerald-100/90">
              <p className="flex items-center gap-1.5 font-medium text-emerald-200">
                <CheckCircle2 className="h-3.5 w-3.5" aria-hidden="true" />
                Bridge reset complete{bridgeResetResult.identity_changed ? ' — new identity' : ''}
              </p>
              <p className="mt-1">
                Cleared {formatBridgeResetCounts(bridgeResetResult.counts)}. Public Mesh is re-created and open.
              </p>
              {bridgeResetResult.node_id ? (
                <p className="mt-1">
                  New node id: <span className="font-mono text-emerald-200">{formatNodeId(bridgeResetResult.node_id)}</span>
                  {bridgeResetResult.old_node_id ? (
                    <span className="text-emerald-200/60"> (was {formatNodeId(bridgeResetResult.old_node_id)})</span>
                  ) : null}
                  . Re-share your new QR; old contact cards and QR codes no longer work.
                </p>
              ) : null}
            </div>
          ) : null}

          <p className="mt-3 text-3xs leading-relaxed text-slate-600">
            The bridge reset is an authenticated operation and is irreversible. It is never performed automatically;
            the Public Mesh channel is public by design and anyone with OpenMesh can read and post there.
          </p>
        </section>

        <div className="flex items-center justify-between border-t border-white/[0.05] py-3 text-3xs text-slate-600">
          <span>OpenMesh web client 0.1.0 · protocol omp/1</span>
          <span className="flex items-center gap-1">
            <ShieldCheck className="h-3 w-3" aria-hidden="true" />
            AGPL-3.0-or-later
          </span>
        </div>
      </div>

      <IdentityQrModal
        open={qrOpen}
        onClose={() => setQrOpen(false)}
        ownNodeId={ownNodeId}
        ownName={savedName}
      />

      <Modal
        open={clientResetOpen}
        onClose={() => {
          if (clientResetting) return;
          setClientResetOpen(false);
          setClientResetText('');
        }}
        title="Reset this client?"
        description="Clears this browser's saved state and reloads. The bridge and its node identity are not touched."
        icon={RotateCcw}
        size="sm"
        dismissible={!clientResetting}
        footer={
          <>
            <Button
              variant="ghost"
              onClick={() => {
                setClientResetOpen(false);
                setClientResetText('');
              }}
              disabled={clientResetting}
            >
              Cancel
            </Button>
            <Button
              variant="danger"
              icon={RotateCcw}
              loading={clientResetting}
              disabled={clientResetText !== 'RESET'}
              onClick={async () => {
                setClientResetting(true);
                try {
                  await resetClient();
                } finally {
                  setClientResetting(false);
                }
              }}
            >
              Reset this client
            </Button>
          </>
        }
      >
        <div className="space-y-3">
          <Callout tone="danger" icon={AlertTriangle} title="This browser starts from scratch">
            Your saved bridge URL/token, drafts, demo flags and read markers are removed. Messages, contacts and the
            node identity on the bridge are unchanged. To create a new identity and wipe bridge history, use the
            bridge wipe action instead.
          </Callout>
          <Field label="Type RESET to confirm">
            {({ id }) => (
              <TextInput
                id={id}
                value={clientResetText}
                onChange={(event) => setClientResetText(event.target.value)}
                placeholder="RESET"
                autoComplete="off"
                className="font-mono"
              />
            )}
          </Field>
        </div>
      </Modal>

      <Modal
        open={bridgeResetOpen}
        onClose={() => {
          if (bridgeResetting) return;
          setBridgeResetOpen(false);
          setBridgeResetText('');
          setBridgeResetError(null);
        }}
        title="Wipe bridge data and create a new identity?"
        description="Irreversible: this clears the bridge's mesh data and regenerates its node identity. Type WIPE to confirm."
        icon={TriangleAlert}
        size="md"
        dismissible={!bridgeResetting}
        footer={
          <>
            <Button
              variant="ghost"
              onClick={() => {
                setBridgeResetOpen(false);
                setBridgeResetText('');
                setBridgeResetError(null);
              }}
              disabled={bridgeResetting}
            >
              Cancel
            </Button>
            <Button
              variant="danger"
              icon={TriangleAlert}
              loading={bridgeResetting}
              disabled={bridgeResetText !== 'WIPE'}
              onClick={async () => {
                setBridgeResetting(true);
                setBridgeResetError(null);
                try {
                  const result = await resetBridgeData({ newIdentity: true });
                  setBridgeResetResult(result);
                  setBridgeResetOpen(false);
                  setBridgeResetText('');
                } catch (cause) {
                  setBridgeResetError(cause instanceof Error ? cause.message : String(cause));
                } finally {
                  setBridgeResetting(false);
                }
              }}
            >
              Wipe and create new identity
            </Button>
          </>
        }
      >
        <div className="space-y-3">
          <Callout tone="danger" icon={AlertTriangle} title="Every contact and QR code will be invalid">
            <ul className="list-disc space-y-1 pl-4">
              <li>Messages, contacts, channels, nodes, attachments and the activity log are wiped on the bridge.</li>
              <li>The bridge gets a new node id and keypair; contacts must re-add and re-verify you.</li>
              <li>Public Mesh is re-created and joined automatically (it cannot be left or deleted).</li>
              <li>The bridge bearer token and radio/region settings are kept.</li>
            </ul>
          </Callout>
          {bridgeResetError ? (
            <p className="flex items-start gap-1.5 text-2xs leading-relaxed text-rose-300">
              <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
              {bridgeResetError}
            </p>
          ) : null}
          <Field label="Type WIPE to confirm">
            {({ id }) => (
              <TextInput
                id={id}
                value={bridgeResetText}
                onChange={(event) => setBridgeResetText(event.target.value)}
                placeholder="WIPE"
                autoComplete="off"
                className="font-mono"
              />
            )}
          </Field>
        </div>
      </Modal>
    </div>
  );
}

function SectionCard({
  icon: Icon,
  title,
  description,
  children,
}: {
  icon: typeof UserRound;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section className="rounded-2xl border border-white/[0.06] bg-white/[0.02] p-4 md:p-5">
      <div className="mb-4 flex items-start gap-2.5">
        <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg border border-white/[0.07] bg-white/[0.03] text-slate-400">
          <Icon className="h-4 w-4" aria-hidden="true" />
        </span>
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-slate-100">{title}</h3>
          <p className="mt-0.5 text-2xs leading-relaxed text-slate-500">{description}</p>
        </div>
      </div>
      <div className="space-y-4">{children}</div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Own contact QR                                                     */
/* ------------------------------------------------------------------ */

function IdentityQrModal({
  open,
  onClose,
  ownNodeId,
  ownName,
}: {
  open: boolean;
  onClose: () => void;
  ownNodeId: string | null;
  ownName: string;
}) {
  const requestContactQr = useAppStore((state) => state.requestContactQr);
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open || !ownNodeId) return;
    let cancelled = false;
    setUrl(null);
    setError(null);
    setLoading(true);
    requestContactQr(ownNodeId)
      .then((invite) => {
        if (!cancelled) setUrl(invite);
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : 'Your bridge did not return a contact invite.');
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, ownNodeId, requestContactQr]);

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={`Your contact card · ${ownName}`}
      description="Another phone or the web client scans this to add you as a contact. Your node id stays self-certifying; the name is only the friendly label."
      size="sm"
    >
      {loading ? (
        <div className="flex items-center justify-center gap-2 py-10 text-xs text-slate-400">
          <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          Asking the bridge for your contact link…
        </div>
      ) : url ? (
        <div className="flex flex-col items-center gap-3">
          <QRCode value={url} size={220} label={`Contact QR for ${ownName}`} />
          <code className="max-w-full break-all rounded-lg border border-white/[0.06] bg-white/[0.02] px-2 py-1.5 text-3xs text-slate-400">
            {url}
          </code>
          <CopyButton value={url} label="Copy link" size="md" variant="surface" />
        </div>
      ) : (
        <p className="py-8 text-center text-xs text-rose-300">
          {error ?? 'Your bridge did not return a contact invite for this node yet.'}
        </p>
      )}
    </Modal>
  );
}
