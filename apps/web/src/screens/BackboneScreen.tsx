import { useEffect, useMemo, useState } from 'react';
import {
  ArrowRight,
  CheckCircle2,
  ListChecks,
  Lock,
  RadioTower,
  RefreshCw,
  RotateCcw,
  Server,
  Settings2,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Waypoints,
} from 'lucide-react';
import { useAppStore } from '../store/app';
import {
  selectBackboneAvailable,
  selectBackboneConnectionString,
  selectBackboneHeader,
  selectBackbonePeers,
  selectBackboneSetup,
  selectBackboneStats,
} from '../store/backbone-selectors';
import { formatBackboneUptime } from '../lib/backbone';
import { formatRelativeTime, cn } from '../lib/utils';
import { Badge, Button } from '../components/ui/primitives';
import { Callout } from '../components/ui/forms';
import { BackboneStats } from './backbone/BackboneStats';
import { BackbonePeers } from './backbone/BackbonePeers';
import { BackboneControls } from './backbone/BackboneControls';
import { BackboneConnection, BackboneInstallCard } from './backbone/BackboneConnection';
import { BackboneSetupDrawer } from './backbone/BackboneSetup';

/**
 * Backbone control panel (OBB/1, v0.2).
 *
 * Reachable from the nav rail, `?screen=backbone` and the `/backbone` route.
 * Live data comes from the WebSocket events plus the REST control API; demo mode
 * hydrates a seeded mesh and the mock transport keeps its counters ticking.
 */
export function BackboneScreen() {
  const mode = useAppStore((state) => state.mode);
  const config = useAppStore((state) => state.backboneConfig);
  const status = useAppStore((state) => state.backboneStatus);
  const hello = useAppStore((state) => state.hello);
  const peers = useAppStore((state) => state.backbonePeers);
  const setup = useAppStore((state) => state.backboneSetup);
  const sessionToken = useAppStore((state) => state.backboneSessionToken);
  const tokenFresh = useAppStore((state) => state.backboneTokenFresh);
  const ownNodeId = useAppStore((state) => state.ownNodeId);
  const loading = useAppStore((state) => state.backboneLoading);
  const saving = useAppStore((state) => state.backboneSaving);
  const error = useAppStore((state) => state.backboneError);
  const lastUpdated = useAppStore((state) => state.backboneLastUpdated);
  const connection = useAppStore((state) => state.connection);
  const loadBackbone = useAppStore((state) => state.loadBackbone);
  const saveBackboneConfig = useAppStore((state) => state.saveBackboneConfig);
  const setBackboneEnabled = useAppStore((state) => state.setBackboneEnabled);
  const rotateBackboneToken = useAppStore((state) => state.rotateBackboneToken);
  const setScreen = useAppStore((state) => state.setScreen);

  const [setupOpen, setSetupOpen] = useState(false);
  const [enabling, setEnabling] = useState(false);
  const demo = mode === 'demo';

  useEffect(() => {
    // Wait for the transport to settle so demo mode never probes the live REST API.
    if (connection !== 'demo' && connection !== 'connected') return;
    void loadBackbone();
  }, [connection, loadBackbone]);

  const stateSlice = useMemo(
    () => ({
      mode,
      hello,
      ownNodeId,
      backboneConfig: config,
      backboneStatus: status,
      backbonePeers: peers,
      backboneSetup: setup,
      backboneSessionToken: sessionToken,
    }),
    [mode, hello, ownNodeId, config, status, peers, setup, sessionToken],
  );

  const stats = useMemo(() => selectBackboneStats(stateSlice), [stateSlice]);
  const header = useMemo(() => selectBackboneHeader(stateSlice), [stateSlice]);
  const sortedPeers = useMemo(() => selectBackbonePeers({ backbonePeers: peers }), [peers]);
  const connectionString = useMemo(() => selectBackboneConnectionString(stateSlice), [stateSlice]);
  const effectiveSetup = useMemo(() => selectBackboneSetup(stateSlice), [stateSlice]);
  const backboneAvailable = useMemo(
    () => selectBackboneAvailable({ mode, hello, backboneConfig: config }),
    [mode, hello, config],
  );

  const enableBackbone = async (): Promise<void> => {
    setEnabling(true);
    const ok = await setBackboneEnabled(true);
    if (ok) await loadBackbone();
    setEnabling(false);
  };

  if (loading && !config) {
    return (
      <div className="mx-auto w-full max-w-[1240px] space-y-4 px-4 py-5 md:px-6">
        <div className="h-8 w-64 animate-pulse rounded-lg bg-white/[0.04]" />
        <div className="grid grid-cols-2 gap-2.5 md:grid-cols-3 xl:grid-cols-6">
          {Array.from({ length: 6 }).map((_, index) => (
            <div key={index} className="h-[86px] animate-pulse rounded-xl border border-white/[0.05] bg-white/[0.02]" />
          ))}
        </div>
        <div className="h-64 animate-pulse rounded-2xl border border-white/[0.05] bg-white/[0.02]" />
      </div>
    );
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto w-full max-w-[1240px] space-y-4 px-4 py-5 md:px-6">
        {/* Header */}
        <div className="flex flex-wrap items-start gap-3">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <StatusPill pill={header.pill} />
              {demo ? <Badge tone="amber">demo data</Badge> : null}
              {config?.enabled && config.auth === 'open' ? (
                <Badge tone="rose" icon={ShieldAlert}>open LAN auth</Badge>
              ) : null}
              {config && !config.enabled ? <Badge tone="slate">listener off</Badge> : null}
              {status?.last_error ? <Badge tone="rose">last error</Badge> : null}
            </div>
            <p className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-slate-500">
              <span className="min-w-0 break-all font-mono text-slate-400">tcp://{header.endpoint}</span>
              <span className="text-slate-700">·</span>
              <span>uptime {formatBackboneUptime(stats.uptimeS)}</span>
              <span className="text-slate-700">·</span>
              <span>
                {stats.peers}/{stats.maxPeers} peers
              </span>
              {lastUpdated ? (
                <>
                  <span className="hidden text-slate-700 sm:inline">·</span>
                  <span className="hidden sm:inline" title={new Date(lastUpdated).toLocaleString()}>
                    updated {formatRelativeTime(lastUpdated)}
                  </span>
                </>
              ) : null}
              {config ? (
                <>
                  <span className="hidden text-slate-700 sm:inline">·</span>
                  <span className="hidden font-mono sm:inline">{header.listen}</span>
                </>
              ) : null}
            </p>
          </div>
          <div className="flex w-full shrink-0 items-center justify-end gap-2 sm:w-auto">
            <Button variant="ghost" size="sm" icon={RefreshCw} loading={loading} onClick={() => void loadBackbone()}>
              Refresh
            </Button>
            <Button variant="primary" size="sm" icon={ListChecks} onClick={() => setSetupOpen(true)}>
              Set up your backbone
            </Button>
          </div>
        </div>

        {error ? (
          <Callout tone="danger" icon={ShieldAlert} title="Backbone control API error">
            <div className="flex flex-wrap items-center gap-2">
              <span>{error}</span>
              <div className="flex gap-1.5">
                <button
                  type="button"
                  onClick={() => void loadBackbone()}
                  className="focus-ring rounded border border-rose-300/30 px-1.5 py-0.5 text-3xs font-medium text-rose-200 hover:bg-rose-400/10"
                >
                  Retry
                </button>
                <button
                  type="button"
                  onClick={() => setScreen('settings')}
                  className="focus-ring rounded border border-rose-300/30 px-1.5 py-0.5 text-3xs font-medium text-rose-200 hover:bg-rose-400/10"
                >
                  Open Settings
                </button>
              </div>
            </div>
          </Callout>
        ) : null}

        {!config ? (
          <DisabledHero
            demo={demo}
            enabling={enabling}
            onEnable={() => void enableBackbone()}
            onOpenSetup={() => setSetupOpen(true)}
            error={error}
            available={backboneAvailable}
          />
        ) : !config.enabled ? (
          <DisabledHero
            demo={demo}
            enabling={enabling}
            onEnable={() => void enableBackbone()}
            onOpenSetup={() => setSetupOpen(true)}
            error={error}
            available={backboneAvailable}
          />
        ) : (
          <>
            {!stats.running ? (
              <Callout tone="warn" icon={RadioTower} title="Listener is stopped">
                <div className="flex flex-wrap items-center gap-2">
                  <span>The configuration is enabled but the backbone TCP listener is not running. Start it to accept peers.</span>
                  <Button variant="secondary" size="sm" icon={RotateCcw} loading={enabling} onClick={() => void enableBackbone()}>
                    Start listener
                  </Button>
                </div>
              </Callout>
            ) : null}

            {config.auth === 'open' ? (
              <Callout tone="danger" icon={ShieldAlert} title="Open authentication is enabled">
                Any peer that can reach <span className="font-mono">{header.listen}</span> is accepted without a token.
                Put the port behind WireGuard/Tailscale, a firewall allowlist or private LAN before leaving this on.
              </Callout>
            ) : null}

            <BackboneStats stats={stats} />

            <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_366px]">
              <div className="min-w-0 space-y-4">
                <BackbonePeers
                  peers={sortedPeers}
                  maxPeers={stats.maxPeers}
                  demo={demo}
                  onKick={(nodeId) => void useAppStore.getState().kickBackbonePeer(nodeId)}
                  onOpenSetup={() => setSetupOpen(true)}
                />
                <BackboneControls
                  config={config}
                  saving={saving}
                  demo={demo}
                  sessionTokenSet={Boolean(sessionToken) && tokenFresh}
                  onSave={(patch) => saveBackboneConfig(patch)}
                  onRotateToken={() => {
                    void rotateBackboneToken().catch(() => undefined);
                  }}
                />
              </div>
              <div className="min-w-0 space-y-4">
                <BackboneConnection
                  config={config}
                  peersConnected={stats.peers}
                  maxPeers={stats.maxPeers}
                  connectionString={connectionString}
                  demo={demo}
                />
                <BackboneInstallCard onOpenSetup={() => setSetupOpen(true)} />
                <div className="rounded-2xl border border-white/[0.06] bg-white/[0.015] px-4 py-3.5">
                  <div className="flex items-center gap-2">
                    <ShieldCheck className="h-4 w-4 text-emerald-400/70" aria-hidden="true" />
                    <p className="text-xs font-semibold text-slate-200">Ciphertext relay</p>
                  </div>
                  <p className="mt-1.5 text-2xs leading-relaxed text-slate-500">
                    OMP frames stay end-to-end encrypted. The backbone only relays and stores ciphertext, and can see
                    source/destination node ids, sizes and timing — never message content from other nodes.
                  </p>
                </div>
              </div>
            </div>

            <div className="flex flex-wrap items-center justify-between gap-2 border-t border-white/[0.05] pt-3 text-3xs text-slate-600">
              <span className="flex items-center gap-1.5">
                <Waypoints className="h-3 w-3" aria-hidden="true" />
                OBB/1 · HMAC-SHA256 transport auth · {config.store_forward ? 'store-and-forward on' : 'store-and-forward off'}
                {config.allowlist.length > 0 ? ` · allowlist ${config.allowlist.length}` : ' · open to any authenticated peer'}
              </span>
              <span>{demo ? 'Demo backbone — no traffic leaves this machine.' : 'Backbone control API v0.2'}</span>
            </div>
          </>
        )}
      </div>

      <BackboneSetupDrawer
        open={setupOpen}
        onClose={() => setSetupOpen(false)}
        setup={effectiveSetup}
        demo={demo}
        peersConnected={stats.peers}
      />
    </div>
  );
}

function StatusPill({ pill }: { pill: ReturnType<typeof selectBackboneHeader>['pill'] }) {
  const tone = {
    accent: 'border-emerald-400/30 bg-emerald-400/10 text-emerald-300',
    amber: 'border-amber-400/30 bg-amber-400/10 text-amber-300',
    rose: 'border-rose-400/30 bg-rose-400/10 text-rose-300',
    slate: 'border-white/10 bg-white/[0.04] text-slate-400',
    sky: 'border-sky-400/30 bg-sky-400/10 text-sky-300',
  } as const;
  const dot = {
    accent: 'bg-emerald-400',
    amber: 'bg-amber-400',
    rose: 'bg-rose-400',
    slate: 'bg-slate-500',
    sky: 'bg-sky-400',
  } as const;
  return (
    <span className={cn('inline-flex h-7 items-center gap-1.5 whitespace-nowrap rounded-full border px-2.5 text-2xs font-semibold', tone[pill.tone])}>
      <span className={cn('h-1.5 w-1.5 rounded-full', dot[pill.tone], pill.phase.startsWith('running') && 'animate-pulse-soft')} />
      {pill.label}
    </span>
  );
}

function DisabledHero({
  demo,
  enabling,
  onEnable,
  onOpenSetup,
  error,
  available,
}: {
  demo: boolean;
  enabling: boolean;
  onEnable: () => void;
  onOpenSetup: () => void;
  error: string | null;
  available: boolean;
}) {
  return (
    <div className="overflow-hidden rounded-2xl border border-white/[0.08] bg-white/[0.025]">
      <div className="relative p-5 md:p-7">
        <div className="pointer-events-none absolute -right-10 -top-16 h-56 w-56 rounded-full bg-emerald-400/[0.06] blur-2xl" />
        <div className="relative grid gap-5 lg:grid-cols-[minmax(0,1fr)_300px]">
          <div>
            <div className="flex items-center gap-2">
              <span className="grid h-11 w-11 place-items-center rounded-2xl border border-emerald-400/20 bg-emerald-400/[0.08] text-emerald-300">
                <RadioTower className="h-5 w-5" aria-hidden="true" />
              </span>
              <div>
                <p className="text-3xs uppercase tracking-[0.16em] text-emerald-300/70">OpenMesh Backbone</p>
                <h2 className="text-lg font-semibold tracking-tight text-white">Turn this bridge into a backbone</h2>
              </div>
            </div>
            <p className="mt-3 max-w-xl text-xs leading-relaxed text-slate-400">
              An always-on backbone is the tower-free tower: one VPS or Raspberry Pi with a public TCP port that joins
              distant meshes into one network. OMP frames stay end-to-end encrypted — the backbone relays ciphertext
              and keeps store-and-forward envelopes for nodes that are offline.
            </p>
            <div className="mt-4 grid gap-2 sm:grid-cols-2">
              {[
                { icon: Lock, title: 'HMAC token auth', text: 'Peers prove knowledge of a 32-byte token; nothing is sent in the clear.' },
                { icon: Waypoints, title: 'Cross-mesh relay', text: 'Any bridge or repeater can join over TCP and bridge two meshes.' },
                { icon: Server, title: 'Store & forward', text: 'Holds encrypted envelopes for absent nodes for up to 72 hours.' },
                { icon: ShieldCheck, title: 'Live control panel', text: 'Stats, peers, kicks and token rotation — all from this panel.' },
              ].map((item) => {
                const Icon = item.icon;
                return (
                  <div key={item.title} className="flex gap-2.5 rounded-xl border border-white/[0.06] bg-white/[0.02] p-2.5">
                    <Icon className="mt-0.5 h-4 w-4 shrink-0 text-slate-500" aria-hidden="true" />
                    <div>
                      <p className="text-2xs font-semibold text-slate-200">{item.title}</p>
                      <p className="mt-0.5 text-3xs leading-relaxed text-slate-500">{item.text}</p>
                    </div>
                  </div>
                );
              })}
            </div>
            {error ? (
              <p className="mt-3 rounded-lg border border-rose-400/20 bg-rose-400/[0.06] px-2.5 py-2 text-3xs text-rose-200/80">
                {error}
              </p>
            ) : null}
            <div className="mt-4 flex flex-wrap items-center gap-2">
              <Button variant="primary" icon={Sparkles} loading={enabling} onClick={onEnable}>
                Enable backbone
              </Button>
              <Button variant="secondary" icon={ListChecks} onClick={onOpenSetup}>
                Preview setup steps
              </Button>
              {!available ? (
                <span className="text-3xs text-slate-600">
                  This bridge does not advertise backbone support (hello.features) — update the bridge first.
                </span>
              ) : null}
            </div>
          </div>

          <div className="rounded-xl border border-white/[0.07] bg-ink-950/50 p-4">
            <div className="flex items-center gap-2">
              <Settings2 className="h-3.5 w-3.5 text-slate-500" aria-hidden="true" />
              <p className="text-2xs font-semibold uppercase tracking-[0.12em] text-slate-400">What you need</p>
            </div>
            <ul className="mt-2.5 space-y-2">
              {[
                'A VPS or Raspberry Pi that stays online',
                'A public TCP port (default 9443) reachable by gateways',
                'A token (generated on first run) shared only with your gateways',
                'The OpenMesh release installed on both ends',
              ].map((line) => (
                <li key={line} className="flex items-start gap-1.5 text-2xs leading-relaxed text-slate-500">
                  <CheckCircle2 className="mt-0.5 h-3 w-3 shrink-0 text-emerald-400/60" aria-hidden="true" />
                  {line}
                </li>
              ))}
            </ul>
            <div className="mt-3 border-t border-white/[0.06] pt-2.5">
              <p className="text-3xs leading-relaxed text-slate-600">
                Enabling starts the listener with the defaults; tune port, public host and token in the panel.
              </p>
              <p className="mt-1 flex items-center gap-1 text-3xs text-slate-500">
                <ArrowRight className="h-2.5 w-2.5" aria-hidden="true" />
                {demo ? 'Demo mode: enabling is simulated locally.' : 'One click posts enabled:true to /api/backbone/config.'}
              </p>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
