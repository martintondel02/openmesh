import { useEffect, useMemo, useState } from 'react';
import {
  ArrowUpRight,
  Check,
  CheckCircle2,
  ClipboardCopy,
  Download,
  ExternalLink,
  Flame,
  Globe,
  KeyRound,
  ListChecks,
  Loader2,
  Lock,
  ShieldAlert,
  Terminal,
} from 'lucide-react';
import type { BackboneSetup, BackboneSetupStep } from '../../types/backbone';
import { useAppStore } from '../../store/app';
import {
  INSTALL_OPTIONS,
  formatAuthMode,
  isFirewallStep,
  isGatewayStep,
  panelUrlHint,
  redactConnectionString,
} from '../../lib/backbone';
import { cn } from '../../lib/utils';
import { Badge, Button, IconButton, SegmentedControl } from '../../components/ui/primitives';
import { CopyButton, copyText } from '../../components/ui/signals';
import { Drawer } from '../../components/ui/overlay';
import { Callout } from '../../components/ui/forms';

type PlatformId = (typeof INSTALL_OPTIONS)[number]['id'];
type FirewallId = 'firewalld' | 'ufw';

/**
 * "Set up your backbone" wizard. Renders the bridge-provided setup payload
 * (BACKBONE.md §8) with copy buttons, firewall tabs, systemd unit and a
 * platform-aware install section. Falls back to a local template offline.
 */
export function BackboneSetupDrawer({
  open,
  onClose,
  setup: setupProp,
  demo,
  peersConnected,
}: {
  open: boolean;
  onClose: () => void;
  setup: BackboneSetup | null;
  demo: boolean;
  peersConnected: number;
}) {
  const loadBackboneSetup = useAppStore((state) => state.loadBackboneSetup);
  const pushToast = useAppStore((state) => state.pushToast);
  const [setup, setSetup] = useState<BackboneSetup | null>(setupProp);
  const [loading, setLoading] = useState(false);
  const [revealed, setRevealed] = useState(demo);
  const [platform, setPlatform] = useState<PlatformId>('linux');
  const [firewall, setFirewall] = useState<FirewallId>('firewalld');

  useEffect(() => {
    if (!open) return;
    setRevealed(demo);
    setLoading(true);
    void loadBackboneSetup()
      .then((loaded) => {
        if (loaded) setSetup(loaded);
      })
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => {
    if (setupProp) setSetup(setupProp);
  }, [setupProp]);

  const install = INSTALL_OPTIONS.find((option) => option.id === platform) ?? INSTALL_OPTIONS[0]!;
  const panelUrl = panelUrlHint(setup?.panel_url);
  const connectionString = setup?.connection_string ?? '';
  const isOpenAuth = setup?.auth === 'open';

  const copyAll = async (): Promise<void> => {
    if (!setup) return;
    const text = [
      `# OpenMesh backbone setup — ${setup.listen}`,
      `# Panel: ${panelUrl}`,
      '',
      `Connection string: ${connectionString}`,
      '',
      ...setup.steps.flatMap((step, index) => [`${index + 1}. ${step.title}`, ...step.commands.map((command) => `   ${command}`), '']),
      '## systemd unit',
      setup.systemd_unit,
      '',
      `## Install (${install.label})`,
      ...install?.commands.map((command) => `   ${command}`) ?? [],
    ].join('\n');
    const ok = await copyText(text);
    pushToast(
      ok
        ? { kind: 'success', title: 'Setup copied', message: 'All steps are on your clipboard.' }
        : { kind: 'error', title: 'Could not copy', message: 'Clipboard access was denied.' },
    );
  };

  const extraVerify: BackboneSetupStep | null = useMemo(() => {
    if (!setup) return null;
    if (setup.steps.some((step) => /verify|test/i.test(step.title))) return null;
    return {
      title: 'Verify it works',
      commands: ['curl -fsS -H "Authorization: Bearer $OPENMESH_TOKEN" http://127.0.0.1:8787/api/backbone/peers'],
      note: 'The peer appears in this panel within ~30 s. A JSON array means the control API and listener are healthy.',
    };
  }, [setup]);

  return (
    <Drawer
      open={open}
      onClose={onClose}
      title="Set up your backbone"
      description="Everything needed to stand up the relay on a VPS or Raspberry Pi and join a gateway."
      widthClassName="sm:max-w-[660px]"
      footer={
        <div className="flex items-center gap-2">
          <p className="hidden flex-1 truncate text-3xs text-slate-500 sm:block">
            {demo ? 'Demo setup — commands are illustrative until you run a real bridge.' : 'Commands are generated from your live backbone configuration.'}
          </p>
          <Button variant="ghost" size="sm" onClick={onClose}>
            Close
          </Button>
          <Button variant="secondary" size="sm" icon={ClipboardCopy} onClick={() => void copyAll()} disabled={!setup}>
            Copy all steps
          </Button>
        </div>
      }
    >
      {loading && !setup ? (
        <div className="flex items-center justify-center gap-2 py-16 text-xs text-slate-400">
          <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          Asking the bridge for setup instructions…
        </div>
      ) : !setup ? (
        <Callout tone="warn" icon={ShieldAlert} title="No setup payload">
          The bridge did not return a setup guide. Check the bridge version and try again, or use the manual commands
          below.
        </Callout>
      ) : (
        <div className="space-y-5 px-4 py-4 md:px-5">
          {/* Summary */}
          <div className="rounded-xl border border-white/[0.07] bg-white/[0.025] p-3.5">
            <div className="flex flex-wrap items-center gap-2">
              <Badge tone={isOpenAuth ? 'rose' : 'accent'} icon={isOpenAuth ? ShieldAlert : Lock}>
                {formatAuthMode(setup.auth)}
              </Badge>
              <Badge tone="slate">{setup.listen}</Badge>
              <Badge tone="sky">{setup.public_host || 'public host not set'}</Badge>
              {peersConnected > 0 ? <Badge tone="accent">{peersConnected} peers online</Badge> : null}
            </div>
            <div className="mt-2.5">
              <div className="mb-1 flex items-center justify-between">
                <p className="text-2xs font-medium text-slate-300">Connection string</p>
                <div className="flex items-center gap-1">
                  <CopyButton value={connectionString} label="Copy" size="sm" variant="surface" />
                  <IconButton
                    icon={revealed ? Lock : KeyRound}
                    label={revealed ? 'Hide token' : 'Reveal token'}
                    size="sm"
                    onClick={() => setRevealed((value) => !value)}
                  />
                </div>
              </div>
              <code className="block max-h-24 overflow-y-auto break-all rounded-lg border border-white/[0.07] bg-ink-950/60 px-2.5 py-2 font-mono text-3xs leading-relaxed text-slate-400">
                {revealed ? connectionString : redactConnectionString(connectionString)}
              </code>
            </div>
          </div>

          {isOpenAuth ? (
            <Callout tone="danger" icon={ShieldAlert} title="This backbone is running with open authentication">
              Any peer that can reach {setup.listen} is accepted. Keep the port behind a firewall, VPN or private LAN
              until you switch to token mode.
            </Callout>
          ) : null}

          {/* Numbered steps */}
          <div>
            <div className="mb-2 flex items-center gap-2">
              <ListChecks className="h-4 w-4 text-emerald-300/80" aria-hidden="true" />
              <h3 className="text-sm font-semibold text-slate-100">Onboarding steps</h3>
              <span className="text-3xs text-slate-500">{setup.steps.length + (extraVerify ? 1 : 0)} steps</span>
            </div>
            <ol className="space-y-3">
              {setup.steps.map((step, index) => (
                <SetupStep
                  key={`${step.title}-${index}`}
                  index={index + 1}
                  step={step}
                  firewall={firewall}
                  onFirewallChange={setFirewall}
                  firewallCommands={isFirewallStep(step) ? setup.firewall : null}
                  highlight={isGatewayStep(step)}
                />
              ))}
              {extraVerify ? (
                <SetupStep
                  index={setup.steps.length + 1}
                  step={extraVerify}
                  firewall={firewall}
                  onFirewallChange={setFirewall}
                  firewallCommands={null}
                  highlight={false}
                />
              ) : null}
            </ol>
          </div>

          {/* systemd unit */}
          <details className="rounded-xl border border-white/[0.07] bg-white/[0.02]">
            <summary className="flex cursor-pointer items-center gap-2 px-3.5 py-2.5 text-xs font-medium text-slate-300">
              <Terminal className="h-3.5 w-3.5 text-slate-500" aria-hidden="true" />
              systemd unit (optional, if you skip install-service)
              <span className="ml-auto"><CopyButton value={setup.systemd_unit} label="Copy unit" size="sm" /></span>
            </summary>
            <pre className="max-h-64 overflow-auto border-t border-white/[0.05] bg-ink-950/60 px-3.5 py-3 font-mono text-3xs leading-relaxed text-slate-400">
              {setup.systemd_unit}
            </pre>
          </details>

          {/* Install */}
          <div className="rounded-xl border border-white/[0.07] bg-white/[0.02] p-3.5">
            <div className="flex flex-wrap items-center gap-2">
              <Download className="h-4 w-4 text-slate-500" aria-hidden="true" />
              <h3 className="text-sm font-semibold text-slate-100">Install OpenMesh</h3>
              <SegmentedControl<PlatformId>
                ariaLabel="Install platform"
                value={platform}
                onChange={setPlatform}
                className="ml-auto"
                options={INSTALL_OPTIONS.map((option) => ({ value: option.id, label: option.label }))}
              />
            </div>
            <p className="mt-2 text-2xs text-slate-500">{install?.hint}</p>
            <div className="mt-2 space-y-1.5">
              {install?.commands.map((command) => (
                <CommandRow key={command} command={command} />
              ))}
            </div>
            <p className="mt-2 flex items-start gap-1.5 text-3xs leading-relaxed text-slate-600">
              <CheckCircle2 className="mt-0.5 h-3 w-3 shrink-0 text-slate-700" aria-hidden="true" />
              {install?.note}
            </p>
            <a
              href="https://github.com/martintondel02/openmesh/releases/latest"
              target="_blank"
              rel="noreferrer noopener"
              className="mt-2 inline-flex items-center gap-1 text-3xs font-medium text-sky-300/80 hover:text-sky-200"
            >
              Browse all release artifacts
              <ExternalLink className="h-2.5 w-2.5" aria-hidden="true" />
            </a>
          </div>

          {/* Panel + notes */}
          <div className="rounded-xl border border-white/[0.07] bg-white/[0.02] p-3.5">
            <div className="flex items-center gap-2">
              <Globe className="h-4 w-4 text-slate-500" aria-hidden="true" />
              <h3 className="text-sm font-semibold text-slate-100">Your panel URL</h3>
              <Badge tone="slate" className="ml-auto">loopback</Badge>
            </div>
            <div className="mt-2 flex items-center gap-1.5">
              <code className="flex min-w-0 flex-1 items-center gap-1.5 truncate rounded-lg border border-white/[0.07] bg-ink-950/60 px-2.5 py-1.5 font-mono text-3xs text-slate-400">
                {panelUrl}
              </code>
              <CopyButton value={panelUrl} label="Copy" size="md" variant="surface" />
              <a
                href={panelUrl}
                target="_blank"
                rel="noreferrer noopener"
                className="focus-ring inline-flex h-8 items-center gap-1 rounded-lg border border-white/10 bg-white/[0.04] px-2 text-2xs text-slate-300 hover:bg-white/[0.08]"
              >
                Open
                <ArrowUpRight className="h-3 w-3" aria-hidden="true" />
              </a>
            </div>
            <p className="mt-1.5 text-3xs text-slate-600">
              The API and panel bind to loopback by default; reach it over SSH port-forwarding or a reverse proxy.
            </p>
          </div>

          <div className="rounded-xl border border-white/[0.06] bg-white/[0.015] p-3.5">
            <div className="flex items-center gap-2">
              <Flame className="h-4 w-4 text-slate-500" aria-hidden="true" />
              <h3 className="text-xs font-semibold text-slate-200">Security notes</h3>
            </div>
            <ul className="mt-2 space-y-1.5">
              {setup.notes.map((note) => (
                <li key={note} className="flex items-start gap-1.5 text-3xs leading-relaxed text-slate-500">
                  <Check className="mt-0.5 h-3 w-3 shrink-0 text-emerald-400/60" aria-hidden="true" />
                  {note}
                </li>
              ))}
              <li className="flex items-start gap-1.5 text-3xs leading-relaxed text-slate-500">
                <Check className="mt-0.5 h-3 w-3 shrink-0 text-emerald-400/60" aria-hidden="true" />
                Rotate the token from the panel if it leaks; disconnected peers must be updated with the new invite.
              </li>
            </ul>
          </div>
        </div>
      )}
    </Drawer>
  );
}

function SetupStep({
  index,
  step,
  firewall,
  onFirewallChange,
  firewallCommands,
  highlight,
}: {
  index: number;
  step: BackboneSetupStep;
  firewall: FirewallId;
  onFirewallChange: (value: FirewallId) => void;
  firewallCommands: BackboneSetup['firewall'] | null;
  highlight: boolean;
}) {
  const commands = firewallCommands ? firewallCommands[firewall] : step.commands;
  return (
    <li
      className={cn(
        'rounded-xl border p-3',
        highlight ? 'border-sky-400/25 bg-sky-400/[0.045]' : 'border-white/[0.06] bg-white/[0.02]',
      )}
    >
      <div className="flex items-start gap-2.5">
        <span
          className={cn(
            'grid h-6 w-6 shrink-0 place-items-center rounded-full border text-2xs font-semibold tabular-nums',
            highlight ? 'border-sky-400/30 bg-sky-400/10 text-sky-300' : 'border-emerald-400/25 bg-emerald-400/10 text-emerald-300',
          )}
        >
          {index}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h4 className="text-xs font-semibold text-slate-200">{step.title}</h4>
            {highlight ? <Badge tone="sky">join step</Badge> : null}
          </div>
          {step.note ? <p className="mt-0.5 text-2xs leading-relaxed text-slate-500">{step.note}</p> : null}

          {firewallCommands ? (
            <div className="mt-2 inline-flex items-center gap-0.5 rounded-lg border border-white/[0.07] bg-ink-900/70 p-0.5">
              {(['firewalld', 'ufw'] as const).map((option) => (
                <button
                  key={option}
                  type="button"
                  onClick={() => onFirewallChange(option)}
                  className={cn(
                    'focus-ring rounded-[7px] px-2 py-1 text-2xs font-medium transition-colors',
                    firewall === option ? 'bg-white/[0.09] text-white' : 'text-slate-400 hover:text-slate-200',
                  )}
                >
                  {option === 'firewalld' ? 'firewalld' : 'ufw'}
                </button>
              ))}
            </div>
          ) : null}

          <div className="mt-2 space-y-1.5">
            {commands.map((command) => (
              <CommandRow key={command} command={command} />
            ))}
          </div>
        </div>
      </div>
    </li>
  );
}

function CommandRow({ command }: { command: string }) {
  return (
    <div className="flex items-start gap-1.5">
      <code className="min-w-0 flex-1 break-all rounded-md border border-white/[0.06] bg-ink-950/70 px-2 py-1.5 font-mono text-3xs leading-relaxed text-emerald-100/70">
        <span className="select-none text-slate-600">$ </span>
        {command}
      </code>
      <CopyButton value={command} label="" size="md" variant="surface" className="h-[26px] px-1.5" />
    </div>
  );
}
