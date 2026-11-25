import { ArrowUpRight, ExternalLink, Globe, Network, ShieldCheck } from 'lucide-react';
import type { BackboneExposure } from '../../types/backbone';
import {
  BACKBONE_EXPOSURE_GUIDE_URL,
  EXPOSURE_ALTERNATIVES,
  OPENFLARE_LAUNCH_NOTE,
  OPENFLARE_URL,
  buildExposureAdvice,
  buildLoopbackBindCommand,
  normalizeBackboneExposure,
  portFromListen,
} from '../../lib/backbone';
import { Badge } from '../../components/ui/primitives';
import { CopyButton } from '../../components/ui/signals';
import { HelpTip } from '../../components/ui/help';
import { cn } from '../../lib/utils';

const TONE_BY_STATE = {
  public: 'accent',
  tunnel: 'sky',
  bound: 'amber',
} as const;

function ExposureProviderCard({ exposure, compact = false }: { exposure: BackboneExposure; compact?: boolean }) {
  const provider = exposure.provider ?? {
    name: 'Openflare',
    url: OPENFLARE_URL,
    description:
      'Self-hosted tunnel, reverse proxy and control plane with outbound-only connectors and one public edge.',
  };
  return (
    <div className={cn('rounded-xl border border-sky-400/20 bg-sky-400/[0.045]', compact ? 'p-2.5' : 'p-3')}>
      <div className="flex items-start gap-2.5">
        <span className="mt-0.5 grid h-7 w-7 shrink-0 place-items-center rounded-lg border border-sky-400/25 bg-sky-400/10 text-sky-300">
          <Network className="h-3.5 w-3.5" aria-hidden="true" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <p className="text-2xs font-semibold text-slate-100">{provider.name}</p>
            <Badge tone="sky">outbound-only tunnel · no open port</Badge>
          </div>
          <p className="mt-0.5 text-3xs leading-relaxed text-slate-500">{provider.description}</p>
          <a
            href={provider.url || OPENFLARE_URL}
            target="_blank"
            rel="noreferrer noopener"
            className="focus-ring mt-1.5 inline-flex items-center gap-1 rounded text-3xs font-medium text-sky-300 hover:text-sky-200"
          >
            Openflare on GitHub
            <ExternalLink className="h-2.5 w-2.5" aria-hidden="true" />
          </a>
          <p className="mt-1 text-3xs leading-relaxed text-slate-600">{OPENFLARE_LAUNCH_NOTE}</p>
        </div>
      </div>
    </div>
  );
}

function AlternativeChips({ alternatives, guideUrl }: { alternatives: string[]; guideUrl: string }) {
  const chips = alternatives.length ? alternatives : [...EXPOSURE_ALTERNATIVES];
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {chips.map((alternative) => (
        <a
          key={alternative}
          href={guideUrl}
          target="_blank"
          rel="noreferrer noopener"
          className="focus-ring inline-flex items-center gap-1 rounded-full border border-white/[0.08] bg-white/[0.03] px-2 py-0.5 text-3xs text-slate-400 hover:border-white/20 hover:text-slate-200"
        >
          {alternative}
          <ArrowUpRight className="h-2.5 w-2.5" aria-hidden="true" />
        </a>
      ))}
      <a
        href={guideUrl}
        target="_blank"
        rel="noreferrer noopener"
        className="focus-ring inline-flex items-center gap-1 rounded-full px-1.5 py-0.5 text-3xs font-medium text-emerald-300 hover:text-emerald-200"
      >
        Exposure guide
        <ExternalLink className="h-2.5 w-2.5" aria-hidden="true" />
      </a>
    </div>
  );
}

function BindCommand({ command }: { command: string }) {
  return (
    <div>
      <div className="mb-1 flex items-center justify-between gap-2">
        <p className="text-3xs uppercase tracking-[0.12em] text-slate-500">Bind to loopback</p>
        <CopyButton value={command} label="Copy command" size="sm" />
      </div>
      <code className="block break-all rounded-lg border border-white/[0.07] bg-ink-950/70 px-2.5 py-1.5 font-mono text-3xs leading-relaxed text-emerald-100/70">
        <span className="select-none text-slate-600">$ </span>
        {command}
      </code>
    </div>
  );
}

/**
 * Compact "Exposure" card for the backbone panel (visible in both modes).
 * Driven by the optional setup exposure object with a local fallback.
 */
export function BackboneExposureCard({
  exposure,
  bind,
  port,
  publicHost,
}: {
  exposure?: BackboneExposure | null;
  bind: string;
  port: number;
  publicHost: string;
}) {
  const guidance = normalizeBackboneExposure(exposure, undefined);
  const advice = buildExposureAdvice({ bind, port, publicHost, exposure: guidance });
  const bindCommand =
    guidance.bind_command && guidance.bind_command.trim()
      ? guidance.bind_command.trim()
      : buildLoopbackBindCommand({ port, publicHost });

  return (
    <section className="rounded-2xl border border-white/[0.06] bg-white/[0.02] px-4 py-4" aria-label="Public exposure">
      <div className="flex items-center gap-2">
        <Globe className="h-4 w-4 text-sky-300/80" aria-hidden="true" />
        <h3 className="flex items-center gap-1.5 text-xs font-semibold text-slate-200">
          Exposure
          <HelpTip helpKey="backbone.exposure" label="Public exposure" />
        </h3>
        <Badge tone={TONE_BY_STATE[advice.state]} className="ml-auto">
          {advice.badge}
        </Badge>
      </div>

      <p className="mt-2 text-xs font-medium text-slate-200">{advice.title}</p>
      <p className="mt-1 text-2xs leading-relaxed text-slate-500">{advice.body}</p>
      {advice.publicEndpoint ? (
        <p className="mt-1.5 font-mono text-3xs text-sky-300/80">{advice.publicEndpoint}</p>
      ) : null}

      <div className="mt-2.5 space-y-2.5">
        <ExposureProviderCard exposure={guidance} compact />
        <BindCommand command={bindCommand} />
        <p className="text-3xs leading-relaxed text-slate-600">
          Set <span className="font-mono text-slate-500">public_host</span> to the tunnel hostname; the invite and QR
          update automatically. The panel/API stays on loopback.
        </p>
        <AlternativeChips alternatives={guidance.alternatives ?? [...EXPOSURE_ALTERNATIVES]} guideUrl={guidance.guide_url ?? BACKBONE_EXPOSURE_GUIDE_URL} />
      </div>
    </section>
  );
}

/**
 * Numbered setup-wizard step rendered after the firewall step. It is part of the
 * ordered list rendered by `BackboneSetupDrawer`; the number is supplied by that list.
 */
export function BackboneExposureSetupStep({
  index,
  exposure,
  listen,
  publicHost,
}: {
  index: number;
  exposure?: BackboneExposure | null;
  listen: string;
  publicHost: string;
}) {
  const guidance = normalizeBackboneExposure(exposure, undefined);
  const port = portFromListen(listen);
  const advice = buildExposureAdvice({ bind: listen, port, publicHost, exposure: guidance });
  const bindCommand =
    guidance.bind_command && guidance.bind_command.trim()
      ? guidance.bind_command.trim()
      : buildLoopbackBindCommand({ port, publicHost });
  const steps = guidance.steps ?? [];

  return (
    <li className="rounded-xl border border-sky-400/25 bg-sky-400/[0.045] p-3" aria-label="Expose it publicly (optional)">
      <div className="flex items-start gap-2.5">
        <span className="grid h-6 w-6 shrink-0 place-items-center rounded-full border border-sky-400/30 bg-sky-400/10 text-2xs font-semibold tabular-nums text-sky-300">
          {index}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h4 className="flex items-center gap-1.5 text-xs font-semibold text-slate-200">
              Expose it publicly (optional)
              <HelpTip helpKey="backbone.exposure" label="Public exposure" />
            </h4>
            <Badge tone="sky">optional</Badge>
            {publicHost ? <Badge tone="accent">{publicHost}</Badge> : null}
          </div>
          <p className="mt-0.5 text-2xs leading-relaxed text-slate-500">
            {guidance.when_to_use || advice.body}
          </p>

          <div className="mt-2 space-y-2.5">
            <ExposureProviderCard exposure={guidance} />
            {steps.length ? (
              <ol className="space-y-1">
                {steps.map((step, stepIndex) => (
                  <li key={step} className="flex items-start gap-1.5 text-3xs leading-relaxed text-slate-500">
                    <span className="mt-0.5 grid h-4 w-4 shrink-0 place-items-center rounded-full border border-white/[0.08] bg-white/[0.03] font-mono text-[9px] text-slate-500">
                      {stepIndex + 1}
                    </span>
                    {step}
                  </li>
                ))}
              </ol>
            ) : null}
            <BindCommand command={bindCommand} />
            <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-2.5 py-2 text-3xs leading-relaxed text-slate-500">
              <p className="flex items-center gap-1.5 font-medium text-slate-300">
                <ShieldCheck className="h-3 w-3 text-emerald-300/80" aria-hidden="true" />
                Public host note
              </p>
              <p className="mt-1">
                After the tunnel is live, set{' '}
                <span className="font-mono text-slate-400">public_host</span> to the tunnel hostname
                {publicHost ? (
                  <> (currently <span className="font-mono text-slate-300">{publicHost}</span>)</>
                ) : (
                  <> (currently not set — invites will use the detected address)</>
                )}
                . Keep only the backbone port reachable; the panel and API stay loopback-only.
              </p>
            </div>
            <AlternativeChips
              alternatives={guidance.alternatives ?? [...EXPOSURE_ALTERNATIVES]}
              guideUrl={guidance.guide_url ?? BACKBONE_EXPOSURE_GUIDE_URL}
            />
          </div>
        </div>
      </div>
    </li>
  );
}

/** One-line summary used by copy-all in the setup drawer. */
export function exposureCopyLines(exposure: BackboneExposure | null | undefined, listen: string, publicHost: string): string[] {
  const guidance = normalizeBackboneExposure(exposure, undefined);
  const port = portFromListen(listen);
  const provider = guidance.provider?.name ?? 'Openflare';
  const bindCommand =
    guidance.bind_command?.trim() || buildLoopbackBindCommand({ port, publicHost });
  return [
    '## Expose it publicly (optional)',
    `${provider}: ${guidance.provider?.url ?? OPENFLARE_URL}`,
    guidance.when_to_use ?? '',
    ...(guidance.steps ?? []).map((step, index) => `   ${index + 1}. ${step}`),
    `   $ ${bindCommand}`,
    `   Guide: ${guidance.guide_url ?? BACKBONE_EXPOSURE_GUIDE_URL}`,
    '',
  ].filter((line) => line !== '');
}
