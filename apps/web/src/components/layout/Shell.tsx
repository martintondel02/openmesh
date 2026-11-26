import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import {
  BookOpen,
  Bot,
  ChevronDown,
  Copy,
  Map,
  MessageSquare,
  Radio,
  RadioTower,
  RefreshCw,
  Settings,
  Users,
  Wifi,
  WifiOff,
  X,
  AlertTriangle,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useShallow } from 'zustand/react/shallow';
import { useAppStore } from '../../store/app';
import { selectMeshSummary, selectTotalUnread } from '../../store/selectors';
import { selectBackboneAvailable } from '../../store/backbone-selectors';
import { cn, formatNodeId, shortenNodeId } from '../../lib/utils';
import { Avatar, Badge, Button } from '../ui/primitives';
import { copyText } from '../ui/signals';
import { ModeToggle } from './ModeToggle';
import { SystemBanners } from './SystemBanners';
import { RegulatoryBanner } from '../regulatory/Regulatory';

/* ------------------------------------------------------------------ */
/* Brand mark                                                         */
/* ------------------------------------------------------------------ */

export function LogoMark({ size = 28, className }: { size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      aria-hidden="true"
      className={cn('shrink-0', className)}
    >
      <defs>
        <linearGradient id="om-logo-gradient" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#34d399" />
          <stop offset="1" stopColor="#0ea5e9" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="16" fill="#0d141d" />
      <path d="M32 11 50 22v20L32 53 14 42V22z" fill="none" stroke="url(#om-logo-gradient)" strokeWidth="3.2" />
      <circle cx="32" cy="32" r="5.2" fill="#34d399" />
      <circle cx="32" cy="17" r="2.5" fill="#6ee7b7" />
      <circle cx="45" cy="39" r="2.5" fill="#38bdf8" />
      <circle cx="19" cy="39" r="2.5" fill="#a78bfa" />
      <path d="M32 17v10M45 39l-9-4M19 39l9-4" stroke="#94a3b8" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}

/* ------------------------------------------------------------------ */
/* Nav definitions                                                    */
/* ------------------------------------------------------------------ */

interface NavItem {
  screen: 'chats' | 'map' | 'nodes' | 'contacts' | 'backbone' | 'agents' | 'help' | 'settings';
  label: string;
  icon: LucideIcon;
}

const NAV_ITEMS: NavItem[] = [
  { screen: 'chats', label: 'Chats', icon: MessageSquare },
  { screen: 'map', label: 'Map', icon: Map },
  { screen: 'nodes', label: 'Nodes', icon: Radio },
  { screen: 'contacts', label: 'Contacts', icon: Users },
  { screen: 'backbone', label: 'Backbone', icon: RadioTower },
  { screen: 'agents', label: 'Agents', icon: Bot },
  { screen: 'help', label: 'Help', icon: BookOpen },
  { screen: 'settings', label: 'Settings', icon: Settings },
];

function navItemsFor(mode: 'beginner' | 'advanced', backboneAvailable: boolean, agentsAvailable: boolean): NavItem[] {
  return NAV_ITEMS.filter((item) => {
    if (item.screen === 'backbone') return mode === 'advanced' || backboneAvailable;
    if (item.screen === 'agents') return mode === 'advanced' || agentsAvailable;
    return true;
  });
}

/* ------------------------------------------------------------------ */
/* Connection pill                                                    */
/* ------------------------------------------------------------------ */

const CONNECTION_META = {
  connected: { label: 'Connected', dot: 'bg-emerald-400', text: 'text-emerald-300', ring: 'border-emerald-400/25 bg-emerald-400/[0.08]' },
  demo: { label: 'Demo mesh', dot: 'bg-amber-400', text: 'text-amber-300', ring: 'border-amber-400/25 bg-amber-400/[0.08]' },
  connecting: { label: 'Connecting', dot: 'bg-sky-400', text: 'text-sky-300', ring: 'border-sky-400/25 bg-sky-400/[0.08]' },
  reconnecting: { label: 'Reconnecting', dot: 'bg-amber-400', text: 'text-amber-300', ring: 'border-amber-400/25 bg-amber-400/[0.08]' },
  offline: { label: 'Offline', dot: 'bg-rose-400', text: 'text-rose-300', ring: 'border-rose-400/25 bg-rose-400/[0.08]' },
  idle: { label: 'Offline', dot: 'bg-slate-500', text: 'text-slate-400', ring: 'border-white/10 bg-white/[0.04]' },
} as const;

export function ConnectionPill({ compact: _compact = false }: { compact?: boolean }) {
  const connection = useAppStore((state) => state.connection);
  const detail = useAppStore((state) => state.connectionDetail);
  const bridgeVersion = useAppStore((state) => state.bridgeVersion);
  const mode = useAppStore((state) => state.mode);
  const bridgeStatus = useAppStore((state) => state.bridgeStatus);
  const hello = useAppStore((state) => state.hello);
  const settings = useAppStore((state) => state.settings);
  const config = useAppStore((state) => state.config);
  const overrideActive = Boolean(config?.override_active ?? bridgeStatus?.regulatory_override_active);
  const retryConnection = useAppStore((state) => state.retryConnection);
  const setDemoMode = useAppStore((state) => state.setDemoMode);
  const setScreen = useAppStore((state) => state.setScreen);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  const meta = CONNECTION_META[connection] ?? CONNECTION_META.idle;
  const pulsing = connection === 'connecting' || connection === 'reconnecting';

  useEffect(() => {
    if (!open) return;
    const onClick = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onClick);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onClick);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-haspopup="dialog"
        aria-expanded={open}
        className={cn(
          'focus-ring inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-2xs font-medium transition-colors',
          meta.ring,
          meta.text,
        )}
        title={detail ?? meta.label}
      >
        <span className="relative flex h-1.5 w-1.5">
          {pulsing ? <span className={cn('absolute inset-0 rounded-full opacity-70', meta.dot)} /> : null}
          <span className={cn('h-1.5 w-1.5 rounded-full', pulsing ? 'animate-pulse-soft' : '', meta.dot)} />
        </span>
        <span>{meta.label}</span>
        {bridgeVersion && connection === 'connected' ? (
          <span className="hidden font-mono text-3xs opacity-60 sm:inline">v{bridgeVersion}</span>
        ) : null}
        <ChevronDown className={cn('h-3 w-3 opacity-50 transition-transform', open && 'rotate-180')} aria-hidden="true" />
      </button>

      {open ? (
        <div
          role="dialog"
          aria-label="Connection details"
          className="absolute right-0 top-9 z-50 w-72 overflow-hidden rounded-xl border border-white/[0.09] bg-ink-850/98 shadow-pop backdrop-blur animate-slide-up"
        >
          <div className="flex items-center justify-between border-b border-white/[0.06] px-3.5 py-2.5">
            <p className="text-2xs font-semibold uppercase tracking-[0.12em] text-slate-400">Connection</p>
            <button
              type="button"
              aria-label="Close connection details"
              onClick={() => setOpen(false)}
              className="grid h-5 w-5 place-items-center rounded text-slate-500 hover:bg-white/[0.06] hover:text-slate-300"
            >
              <X className="h-3 w-3" />
            </button>
          </div>
          <dl className="space-y-2 px-3.5 py-3 text-2xs">
            <div className="flex items-center justify-between gap-3">
              <dt className="text-slate-500">Status</dt>
              <dd className={cn('flex items-center gap-1.5 font-medium', meta.text)}>
                <span className={cn('h-1.5 w-1.5 rounded-full', meta.dot)} />
                {meta.label}
                {mode === 'demo' ? ' (seeded)' : ''}
              </dd>
            </div>
            <div className="flex items-center justify-between gap-3">
              <dt className="text-slate-500">Bridge</dt>
              <dd className="truncate font-mono text-3xs text-slate-300">{settings.bridgeUrl.replace(/^https?:\/\//, '')}</dd>
            </div>
            <div className="flex items-center justify-between gap-3">
              <dt className="text-slate-500">Protocol</dt>
              <dd className="font-mono text-3xs text-slate-300">{hello?.protocol ?? 'omp/1'}</dd>
            </div>
            {overrideActive ? (
              <div className="flex items-start justify-between gap-3">
                <dt className="text-slate-500">Regulatory</dt>
                <dd className="max-w-[65%] text-right text-3xs font-medium text-rose-300">⚠ override active</dd>
              </div>
            ) : null}
            <div className="flex items-center justify-between gap-3">
              <dt className="text-slate-500">Radio</dt>
              <dd className="text-right text-3xs text-slate-300">
                {hello?.radios?.[0]
                  ? `${hello.radios[0].profile.replace(/_/g, ' ')} · ${hello.radios[0].region}`
                  : '—'}
              </dd>
            </div>
            <div className="flex items-center justify-between gap-3">
              <dt className="text-slate-500">Known nodes</dt>
              <dd className="tabular-nums text-3xs text-slate-300">{bridgeStatus?.mesh.known_nodes ?? '—'}</dd>
            </div>
            {detail ? <p className="rounded-md bg-white/[0.03] px-2 py-1.5 text-3xs leading-relaxed text-slate-400">{detail}</p> : null}
          </dl>
          <div className="flex gap-2 border-t border-white/[0.06] p-2.5">
            <Button
              size="sm"
              variant="secondary"
              icon={RefreshCw}
              onClick={() => {
                setOpen(false);
                void retryConnection();
              }}
              className="flex-1"
            >
              Retry bridge
            </Button>
            <Button
              size="sm"
              variant="ghost"
              icon={Settings}
              onClick={() => {
                setOpen(false);
                setScreen('settings');
              }}
            >
              Settings
            </Button>
          </div>
          {mode === 'demo' ? (
            <button
              type="button"
              onClick={() => {
                setOpen(false);
                void setDemoMode(false);
              }}
              className="block w-full border-t border-white/[0.06] px-3.5 py-2 text-left text-3xs text-amber-300/80 hover:bg-amber-400/[0.06]"
            >
              Switch to live bridge mode
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Node identity chip                                                 */
/* ------------------------------------------------------------------ */

export function NodeIdentityChip({ className }: { className?: string }) {
  const ownNodeId = useAppStore((state) => state.ownNodeId);
  const ownName = useAppStore((state) => state.ownName);
  const [copied, setCopied] = useState(false);

  if (!ownNodeId) {
    return (
      <div className={cn('flex items-center gap-2 rounded-lg border border-white/[0.07] bg-white/[0.02] px-2 py-1', className)}>
        <span className="h-1.5 w-1.5 animate-pulse-soft rounded-full bg-slate-500" />
        <span className="text-2xs text-slate-500">No identity</span>
      </div>
    );
  }

  return (
    <button
      type="button"
      onClick={async () => {
        if (await copyText(ownNodeId)) {
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        }
      }}
      title={`${formatNodeId(ownNodeId)}\nClick to copy your node id`}
      className={cn(
        'focus-ring group inline-flex items-center gap-2 rounded-lg border border-white/[0.07] bg-white/[0.025] py-1 pl-1 pr-2 text-left transition-colors hover:border-white/[0.14] hover:bg-white/[0.05]',
        className,
      )}
    >
      <Avatar name={ownName} seed={ownNodeId} size="xs" />
      <span className="hidden min-w-0 sm:block">
        <span className="block max-w-[9rem] truncate text-2xs font-medium text-slate-200">{ownName}</span>
        <span className="block font-mono text-3xs text-slate-500">{shortenNodeId(ownNodeId)}</span>
      </span>
      {copied ? (
        <span className="text-3xs font-medium text-emerald-400">copied</span>
      ) : (
        <Copy className="h-3 w-3 text-slate-600 transition-colors group-hover:text-slate-400" aria-hidden="true" />
      )}
      <span className="sr-only">Copy node id {ownNodeId}</span>
    </button>
  );
}

/* ------------------------------------------------------------------ */
/* Sidebar                                                            */
/* ------------------------------------------------------------------ */

function Sidebar() {
  const screen = useAppStore((state) => state.screen);
  const setScreen = useAppStore((state) => state.setScreen);
  const unread = useAppStore((state) => selectTotalUnread(state));
  const connection = useAppStore((state) => state.connection);
  const mode = useAppStore((state) => state.mode);
  const bridgeVersion = useAppStore((state) => state.bridgeVersion);
  const backboneAvailable = useAppStore((state) => selectBackboneAvailable(state));
  const uiMode = useAppStore((state) => state.uiMode);
  const agentsAvailable = useAppStore((state) => state.agentConnected || Boolean(state.setupState?.checks.agent));

  const meta = CONNECTION_META[connection] ?? CONNECTION_META.idle;
  const mainItems = navItemsFor(uiMode, backboneAvailable, agentsAvailable).filter(
    (item) => item.screen !== 'settings' && item.screen !== 'help',
  );

  return (
    <aside className="hidden w-[74px] shrink-0 flex-col border-r border-white/[0.06] bg-ink-925/80 px-2.5 pb-3 pt-3 md:flex xl:w-[232px] xl:px-3.5">
      <div className="mb-5 flex items-center gap-2.5 px-1 xl:px-1.5">
        <LogoMark size={30} />
        <div className="hidden min-w-0 xl:block">
          <p className="text-[13px] font-semibold tracking-tight text-white">OpenMesh</p>
          <p className="truncate text-3xs text-slate-500">LoRa mesh client</p>
        </div>
      </div>

      <nav aria-label="Primary" className="flex flex-col gap-0.5">
        {mainItems.map((item) => {
          const Icon = item.icon;
          const active = screen === item.screen;
          return (
            <button
              key={item.screen}
              type="button"
              onClick={() => setScreen(item.screen)}
              aria-current={active ? 'page' : undefined}
              title={item.label}
              className={cn(
                'focus-ring group relative flex items-center gap-2.5 rounded-lg px-2 py-2 text-left transition-colors',
                active
                  ? 'bg-white/[0.07] text-white'
                  : 'text-slate-400 hover:bg-white/[0.04] hover:text-slate-200',
              )}
            >
              {active ? (
                <span className="absolute -left-2.5 top-1/2 h-4 w-[3px] -translate-y-1/2 rounded-r-full bg-emerald-400 xl:-left-3.5" />
              ) : null}
              <span className="relative grid h-5 w-5 shrink-0 place-items-center">
                <Icon className={cn('h-[17px] w-[17px]', active ? 'text-emerald-300' : 'text-slate-500 group-hover:text-slate-300')} aria-hidden="true" />
              </span>
              <span className="hidden flex-1 truncate text-[13px] font-medium xl:block">{item.label}</span>
              {item.screen === 'chats' && unread > 0 ? (
                <span
                  className={cn(
                    'grid min-w-[18px] place-items-center rounded-full bg-emerald-400 px-1 text-3xs font-semibold tabular-nums text-emerald-950',
                    'xl:static absolute right-0.5 top-0.5 h-4 xl:h-auto',
                  )}
                >
                  {unread > 99 ? '99+' : unread}
                </span>
              ) : null}
            </button>
          );
        })}
      </nav>

      <div className="my-3 h-px bg-white/[0.06]" />

      {[NAV_ITEMS.find((item) => item.screen === 'help')!, NAV_ITEMS.find((item) => item.screen === 'settings')!].map((item) => {
        const Icon = item.icon;
        const active = screen === item.screen;
        return (
          <button
            key={item.screen}
            type="button"
            onClick={() => setScreen(item.screen)}
            aria-current={active ? 'page' : undefined}
            title={item.label}
            className={cn(
              'focus-ring group flex items-center gap-2.5 rounded-lg px-2 py-2 text-left transition-colors',
              active ? 'bg-white/[0.07] text-white' : 'text-slate-400 hover:bg-white/[0.04] hover:text-slate-200',
            )}
          >
            <span className="grid h-5 w-5 shrink-0 place-items-center">
              <Icon className={cn('h-[17px] w-[17px]', active ? 'text-emerald-300' : 'text-slate-500')} aria-hidden="true" />
            </span>
            <span className="hidden text-[13px] font-medium xl:block">{item.label}</span>
          </button>
        );
      })}

      <div className="mt-auto space-y-2 pt-4">
        <div className="hidden rounded-xl border border-white/[0.06] bg-white/[0.02] p-2.5 xl:block">
          <div className="flex items-center gap-2">
            <span className={cn('h-1.5 w-1.5 rounded-full', meta.dot)} />
            <span className={cn('text-2xs font-medium', meta.text)}>{meta.label}</span>
            {mode === 'demo' ? <Badge tone="amber" className="ml-auto">demo</Badge> : null}
          </div>
          <p className="mt-1.5 truncate font-mono text-3xs text-slate-600">{bridgeVersion ? `bridge v${bridgeVersion}` : 'no bridge'}</p>
        </div>
        <div className="flex justify-center xl:hidden">
          <div className="grid h-6 w-6 place-items-center rounded-full border border-white/[0.06] bg-white/[0.02]" title={meta.label}>
            <span className={cn('h-1.5 w-1.5 rounded-full', meta.dot)} />
          </div>
        </div>
        <p className="hidden px-1 text-center text-3xs leading-relaxed text-slate-600 xl:block">
          OpenMesh v0.5.1
          <br />
          AGPL-3.0-or-later
        </p>
      </div>
    </aside>
  );
}

/* ------------------------------------------------------------------ */
/* Top bar                                                            */
/* ------------------------------------------------------------------ */

const SCREEN_TITLES: Record<string, { title: string; subtitle: string }> = {
  chats: { title: 'Chats', subtitle: 'Direct messages and shared channels' },
  map: { title: 'Mesh map', subtitle: 'Live node positions and links' },
  nodes: { title: 'Nodes', subtitle: 'Everything your radio has heard' },
  contacts: { title: 'Contacts', subtitle: 'Verified identities you trust' },
  backbone: { title: 'Backbone', subtitle: 'Always-on relay for distant meshes' },
  agents: { title: 'Agents', subtitle: 'Connect an AI assistant and review what it changes' },
  help: { title: 'Help Center', subtitle: 'The handbook, offline too' },
  settings: { title: 'Settings', subtitle: 'Identity, radio and bridge' },
};

function TopBar() {
  const screen = useAppStore((state) => state.screen);
  const activeThread = useAppStore((state) => state.activeThread);
  const bridgeStatus = useAppStore((state) => state.bridgeStatus);
  const config = useAppStore((state) => state.config);
  const summary = useAppStore(useShallow((state) => selectMeshSummary(state)));
  const backboneConfig = useAppStore((state) => state.backboneConfig);
  const backbonePeers = useAppStore((state) => state.backbonePeers.length);
  const setScreen = useAppStore((state) => state.setScreen);
  const openHelp = useAppStore((state) => state.openHelp);
  const meta = SCREEN_TITLES[screen] ?? SCREEN_TITLES.chats!;
  const threadOpen = screen === 'chats' && Boolean(activeThread);

  return (
    <header
      className={cn(
        'relative z-30 flex h-[52px] shrink-0 items-center gap-3 border-b border-white/[0.06] bg-ink-950/70 px-3 backdrop-blur-md md:px-5',
        threadOpen && 'max-md:hidden',
      )}
    >
      <div className="md:hidden">
        <LogoMark size={26} />
      </div>
      <div className="min-w-0">
        <h1 className="truncate text-[13.5px] font-semibold tracking-tight text-slate-100">{meta.title}</h1>
        <p className="hidden truncate text-3xs text-slate-500 sm:block">{meta.subtitle}</p>
      </div>

      <div className="ml-auto flex min-w-0 items-center gap-1.5 sm:gap-2">
        <div className="hidden items-center gap-3 pr-2 lg:flex">
          <span className="flex items-center gap-1.5 text-3xs text-slate-500" title="Nodes heard by this bridge">
            <Radio className="h-3 w-3 text-slate-600" aria-hidden="true" />
            <span className="tabular-nums text-slate-400">{summary.nodes}</span> nodes
          </span>
          <span className="flex items-center gap-1.5 text-3xs text-slate-500" title="Current duty-cycle usage">
            <Wifi className="h-3 w-3 text-slate-600" aria-hidden="true" />
            <span className="tabular-nums text-slate-400">{bridgeStatus ? bridgeStatus.mesh.duty_cycle_pct.toFixed(1) : '—'}%</span> duty
          </span>
          <span className="hidden items-center gap-1.5 text-3xs text-slate-500 2xl:flex" title="Radio preset">
            <span className="h-1 w-1 rounded-full bg-emerald-400/70" />
            {config?.radio.profile.replace(/_/g, ' ') ?? '—'} · {config?.radio.region ?? '—'}
          </span>
          {backboneConfig?.enabled ? (
            <button
              type="button"
              onClick={() => setScreen('backbone')}
              title="Open the backbone control panel"
              className="focus-ring hidden items-center gap-1.5 rounded-full border border-sky-400/20 bg-sky-400/[0.07] px-2 py-0.5 text-3xs font-medium text-sky-300/90 hover:bg-sky-400/[0.12] xl:flex"
            >
              <RadioTower className="h-3 w-3" aria-hidden="true" />
              backbone · {backbonePeers} peers
            </button>
          ) : null}
        </div>
        <button
          type="button"
          onClick={() => openHelp()}
          aria-label="Open Help Center"
          title="Help Center (works offline)"
          className="focus-ring inline-grid h-7 w-7 shrink-0 place-items-center rounded-full border border-white/[0.08] bg-white/[0.03] text-slate-400 hover:text-emerald-200"
        >
          <BookOpen className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
        <ModeToggle className="hidden md:inline-flex" />
        <ConnectionPill compact />
        <NodeIdentityChip />
      </div>
    </header>
  );
}

/* ------------------------------------------------------------------ */
/* Demo banner                                                        */
/* ------------------------------------------------------------------ */

function DemoBanner() {
  const mode = useAppStore((state) => state.mode);
  const demoReason = useAppStore((state) => state.demoReason);
  const retryConnection = useAppStore((state) => state.retryConnection);
  const setScreen = useAppStore((state) => state.setScreen);
  if (mode !== 'demo') return null;
  return (
    <div className="relative z-20 flex h-8 shrink-0 items-center gap-2 border-b border-amber-400/15 bg-amber-400/[0.055] px-3 text-2xs text-amber-200/90 md:px-5">
      <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-amber-400/80" aria-hidden="true" />
      <p className="truncate">
        <span className="font-medium text-amber-200">Demo data</span>
        <span className="hidden sm:inline">
          {' '}
          — seeded Oslo mesh for review; nothing is transmitted.{' '}
          {demoReason === 'offline' ? 'No bridge answered on the configured URL.' : ''}
        </span>
      </p>
      <div className="ml-auto flex shrink-0 items-center gap-1">
        <button
          type="button"
          onClick={() => void retryConnection()}
          className="focus-ring inline-flex h-5 items-center gap-1 rounded px-1.5 text-3xs font-medium text-amber-200 hover:bg-amber-400/10"
        >
          <RefreshCw className="h-2.5 w-2.5" aria-hidden="true" />
          Retry bridge
        </button>
        <button
          type="button"
          onClick={() => setScreen('settings')}
          className="focus-ring hidden h-5 items-center rounded px-1.5 text-3xs text-amber-200/70 hover:bg-amber-400/10 sm:inline-flex"
        >
          Configure
        </button>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Mobile bottom nav                                                  */
/* ------------------------------------------------------------------ */

function MobileNav() {
  const screen = useAppStore((state) => state.screen);
  const setScreen = useAppStore((state) => state.setScreen);
  const unread = useAppStore((state) => selectTotalUnread(state));
  const backboneAvailable = useAppStore((state) => selectBackboneAvailable(state));
  const uiMode = useAppStore((state) => state.uiMode);
  const agentsAvailable = useAppStore((state) => state.agentConnected || Boolean(state.setupState?.checks.agent));
  const items = navItemsFor(uiMode, backboneAvailable, agentsAvailable);

  return (
    <nav
      aria-label="Primary"
      className="no-scrollbar safe-bottom relative z-30 flex shrink-0 items-stretch overflow-x-auto border-t border-white/[0.07] bg-ink-925/95 backdrop-blur-md md:hidden"
    >
      {items.map((item) => {
        const Icon = item.icon;
        const active = screen === item.screen;
        return (
          <button
            key={item.screen}
            type="button"
            onClick={() => setScreen(item.screen)}
            aria-current={active ? 'page' : undefined}
            className={cn(
              'focus-ring relative flex min-w-[44px] flex-1 flex-col items-center gap-0.5 px-0 py-2 text-[9px] font-medium transition-colors',
              active ? 'text-emerald-300' : 'text-slate-500',
            )}
          >
            {active ? <span className="absolute inset-x-5 top-0 h-[2px] rounded-full bg-emerald-400" /> : null}
            <span className="relative">
              <Icon className="h-[18px] w-[18px]" aria-hidden="true" />
              {item.screen === 'chats' && unread > 0 ? (
                <span className="absolute -right-2 -top-1 grid min-w-[15px] place-items-center rounded-full bg-emerald-400 px-0.5 text-[9px] font-semibold leading-4 text-emerald-950">
                  {unread > 9 ? '9+' : unread}
                </span>
              ) : null}
            </span>
            {item.label}
          </button>
        );
      })}
    </nav>
  );
}

/* ------------------------------------------------------------------ */
/* Screen header (used by Nodes/Contacts/Settings/Map side panels)    */
/* ------------------------------------------------------------------ */

export function ScreenHeader({
  title,
  description,
  actions,
  icon: Icon,
  className,
}: {
  title: string;
  description?: string;
  actions?: ReactNode;
  icon?: LucideIcon;
  className?: string;
}) {
  return (
    <div className={cn('flex flex-wrap items-center gap-3 border-b border-white/[0.06] px-4 py-3.5 md:px-6', className)}>
      {Icon ? (
        <div className="grid h-9 w-9 shrink-0 place-items-center rounded-xl border border-white/[0.07] bg-white/[0.03] text-slate-400">
          <Icon className="h-4 w-4" aria-hidden="true" />
        </div>
      ) : null}
      <div className="min-w-0 flex-1">
        <h2 className="text-[15px] font-semibold tracking-tight text-white">{title}</h2>
        {description ? <p className="mt-0.5 truncate text-xs text-slate-500">{description}</p> : null}
      </div>
      {actions ? <div className="flex shrink-0 items-center gap-2">{actions}</div> : null}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Shell                                                              */
/* ------------------------------------------------------------------ */

export function AppShell({ children }: { children: ReactNode }) {
  const screen = useAppStore((state) => state.screen);
  const activeThread = useAppStore((state) => state.activeThread);
  // Give the chat thread the full height on phones; the back arrow returns to the list.
  const hideMobileNav = screen === 'chats' && Boolean(activeThread);
  return (
    <div className="flex h-full min-h-0 w-full overflow-hidden">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <a
          href="#main-content"
          className="sr-only focus:not-sr-only focus:absolute focus:left-2 focus:top-2 focus:z-[70] focus:rounded-lg focus:bg-emerald-400 focus:px-3 focus:py-2 focus:text-xs focus:font-semibold focus:text-emerald-950"
        >
          Skip to content
        </a>
        <TopBar />
        <DemoBanner />
        <RegulatoryBanner />
        <SystemBanners />
        <main id="main-content" className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
          {children}
        </main>
        {hideMobileNav ? null : <MobileNav />}
      </div>
    </div>
  );
}

/** Tiny helper re-exported for screens that show a live-status dot. */
export function StatusDot({ tone = 'emerald', pulse = false }: { tone?: 'emerald' | 'amber' | 'sky' | 'rose' | 'slate'; pulse?: boolean }) {
  const colors = {
    emerald: 'bg-emerald-400',
    amber: 'bg-amber-400',
    sky: 'bg-sky-400',
    rose: 'bg-rose-400',
    slate: 'bg-slate-500',
  } as const;
  return <span className={cn('inline-block h-1.5 w-1.5 rounded-full', colors[tone], pulse && 'animate-pulse-soft')} aria-hidden="true" />;
}

/** Empty search result / offline hint used by several screens. */
export function WifiUnavailableHint() {
  return (
    <span className="inline-flex items-center gap-1 text-2xs text-slate-500">
      <WifiOff className="h-3 w-3" aria-hidden="true" />
      No bridge connection
    </span>
  );
}
