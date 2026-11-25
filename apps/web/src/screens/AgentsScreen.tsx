import { useEffect, useMemo, useState } from 'react';
import {
  Activity,
  Bot,
  CheckCircle2,
  ChevronDown,
  ClipboardList,
  KeyRound,
  Loader2,
  Lock,
  Plug,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Terminal,
  XCircle,
} from 'lucide-react';
import { useAppStore } from '../store/app';
import { Badge, Button, EmptyState, SegmentedControl } from '../components/ui/primitives';
import { Modal } from '../components/ui/overlay';
import { CopyButton } from '../components/ui/signals';
import { HelpLink, HelpTip } from '../components/ui/help';
import { cn } from '../lib/utils';
import {
  ACTIVITY_LIMIT,
  AGENT_CLIENTS,
  AGENT_SAFETY_RULES,
  AGENT_SECURITY_NOTES,
  API_TOKEN_ROTATION_HELP_TOPIC,
  API_TOKEN_ROTATION_STEPS,
  READ_ONLY_EXPLAINER,
  activityEmptyState,
  activitySourceMeta,
  agentPromptTiers,
  buildAgentPrompt,
  buildMcpConfigSnippet,
  formatActivityTime,
} from '../lib/agents';
import type { ActivityEntry, AgentClient, PromptTier } from '../types/agents';

const SOURCE_TONES = {
  agent: 'violet',
  user: 'sky',
  web: 'accent',
  cli: 'amber',
  other: 'neutral',
} as const;

export function AgentsScreen() {
  const uiMode = useAppStore((state) => state.uiMode);
  const mode = useAppStore((state) => state.mode);
  const settings = useAppStore((state) => state.settings);
  const config = useAppStore((state) => state.config);
  const ownName = useAppStore((state) => state.ownName);
  const ownNodeId = useAppStore((state) => state.ownNodeId);
  const activities = useAppStore((state) => state.agentActivities);
  const agentConnected = useAppStore((state) => state.agentConnected);
  const lastAgentAtMs = useAppStore((state) => state.lastAgentAtMs);
  const activityCapacity = useAppStore((state) => state.activityCapacity);
  const activitySources = useAppStore((state) => state.activitySources);
  const agentLoading = useAppStore((state) => state.agentLoading);
  const agentError = useAppStore((state) => state.agentError);
  const loadActivity = useAppStore((state) => state.loadActivity);
  const setUiMode = useAppStore((state) => state.setUiMode);
  const openHelp = useAppStore((state) => state.openHelp);
  const connection = useAppStore((state) => state.connection);

  const [client, setClient] = useState<AgentClient>('claude');
  const [promptTier, setPromptTier] = useState<PromptTier>(uiMode === 'advanced' ? 'advanced' : 'beginner');
  const [revokeOpen, setRevokeOpen] = useState(false);
  const [promptOpen, setPromptOpen] = useState(false);

  useEffect(() => {
    if (mode === 'demo' || connection === 'connected') void loadActivity();
  }, [mode, connection, loadActivity]);

  useEffect(() => {
    if (uiMode === 'advanced' && promptTier === 'beginner') setPromptTier('advanced');
    if (uiMode === 'beginner' && promptTier === 'advanced') setPromptTier('beginner');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [uiMode]);

  const snippet = useMemo(
    () =>
      buildMcpConfigSnippet(client, {
        bridgeUrl: settings.bridgeUrl,
        readOnly: true,
        agentName: client,
      }),
    [client, settings.bridgeUrl],
  );

  const prompt = useMemo(
    () =>
      buildAgentPrompt({
        tier: promptTier,
        nodeName: ownName,
        nodeId: ownNodeId,
        bridgeUrl: settings.bridgeUrl,
        readOnly: true,
        region: config?.radio.region ?? null,
        profile: config?.radio.profile ?? null,
        role: config?.node.class ?? null,
      }),
    [promptTier, ownName, ownNodeId, settings.bridgeUrl, config],
  );

  const latest = useMemo(
    () => [...activities].sort((left, right) => right.at_ms - left.at_ms)[0] ?? null,
    [activities],
  );
  const advanced = uiMode === 'advanced';

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto max-w-6xl px-4 py-4 sm:px-6 lg:py-6">
        {/* status row */}
        <div className="flex flex-wrap items-center gap-2">
          <span
            className={cn(
              'inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-2xs font-medium',
              agentConnected
                ? 'border-violet-400/25 bg-violet-400/[0.08] text-violet-200'
                : 'border-white/10 bg-white/[0.03] text-slate-400',
            )}
          >
            <span className={cn('h-1.5 w-1.5 rounded-full', agentConnected ? 'bg-violet-300' : 'bg-slate-500')} />
            {agentConnected ? 'Agent connected' : 'No agent connected yet'}
          </span>
          {lastAgentAtMs ? (
            <span className="text-3xs text-slate-500">last action {formatActivityTime(lastAgentAtMs)}</span>
          ) : null}
          <Badge tone="accent" icon={Lock}>Read-only recommended</Badge>
          <div className="ml-auto flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              icon={RefreshCw}
              loading={agentLoading}
              onClick={() => void loadActivity()}
            >
              Refresh activity
            </Button>
            <Button variant="ghost" size="sm" icon={Activity} onClick={() => openHelp('agents')}>
              Agent help
            </Button>
          </div>
        </div>

        <div className="mt-4 grid gap-4 xl:grid-cols-[minmax(0,1fr)_340px]">
          <div className="space-y-4">
            {/* intro */}
            <section className="rounded-2xl border border-white/[0.07] bg-gradient-to-br from-violet-400/[0.07] via-white/[0.02] to-transparent p-4">
              <div className="flex items-start gap-3">
                <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl border border-violet-400/25 bg-violet-400/10 text-violet-200">
                  <Sparkles className="h-5 w-5" aria-hidden="true" />
                </span>
                <div className="min-w-0">
                  <h2 className="flex items-center gap-1.5 text-sm font-semibold text-white">
                    Let an AI assistant help with your mesh
                    <HelpTip helpKey="agents.intro" label="Let an AI assistant help" />
                  </h2>
                  <p className="mt-1 max-w-2xl text-2xs leading-relaxed text-slate-400">
                    Connect Claude, Cursor, DSH or any MCP client. The assistant can read your node and — if you
                    allow it — make changes. Every change appears in the activity log below, attributed to the agent.
                  </p>
                  <p className="mt-1 text-3xs text-slate-500">
                    MCP reads the bridge token from a file; the secret never appears in a prompt or this page.
                  </p>
                </div>
              </div>
            </section>

            {/* activity */}
            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02]">
              <div className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.06] px-4 py-3">
                <div>
                  <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.14em] text-slate-500">
                    <ClipboardList className="h-3.5 w-3.5 text-sky-300" aria-hidden="true" />
                    Activity
                    <HelpTip helpKey="agents.activity" label="Activity log" />
                  </h3>
                  <p className="mt-0.5 text-3xs text-slate-600">
                    Write actions only · {activities.length} of {activityCapacity} kept
                    {agentConnected ? ' · shown as agent / you / web / cli' : ''}
                  </p>
                </div>
                {latest ? <span className="text-3xs text-slate-500">latest {formatActivityTime(latest.at_ms)}</span> : null}
              </div>
              {agentError ? (
                <div className="px-4 py-4">
                  <p role="alert" className="rounded-lg border border-rose-400/25 bg-rose-400/[0.06] px-3 py-2 text-2xs leading-relaxed text-rose-200">
                    {agentError}
                  </p>
                  <div className="mt-2">
                    <Button variant="ghost" size="sm" icon={RefreshCw} onClick={() => void loadActivity()}>Retry</Button>
                  </div>
                </div>
              ) : agentLoading && activities.length === 0 ? (
                <div className="flex items-center justify-center py-10 text-xs text-slate-500">
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden="true" /> Loading activity…
                </div>
              ) : activities.length === 0 ? (
                <EmptyState
                  icon={Activity}
                  title={activityEmptyState().title}
                  description={activityEmptyState().body}
                  helpTopic={activityEmptyState().helpTopic}
                />
              ) : (
                <>
                  <div className="hidden overflow-x-auto sm:block">
                    <table className="w-full text-left text-2xs">
                      <thead className="text-3xs uppercase tracking-[0.12em] text-slate-600">
                        <tr className="border-b border-white/[0.06]">
                          <th scope="col" className="px-4 py-2 font-medium">Source</th>
                          <th scope="col" className="px-3 py-2 font-medium">Action</th>
                          <th scope="col" className="px-3 py-2 font-medium">Result</th>
                          <th scope="col" className="whitespace-nowrap px-4 py-2 text-right font-medium">When</th>
                        </tr>
                      </thead>
                      <tbody>
                        {activities.slice(0, ACTIVITY_LIMIT).map((entry) => (
                          <ActivityRow key={entry.id} entry={entry} />
                        ))}
                      </tbody>
                    </table>
                  </div>
                  <ul className="divide-y divide-white/[0.06] sm:hidden">
                    {activities.slice(0, ACTIVITY_LIMIT).map((entry) => (
                      <ActivityMobileRow key={entry.id} entry={entry} />
                    ))}
                  </ul>
                </>
              )}
            </section>

            {/* connect (advanced) */}
            {advanced ? (
              <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
                <div className="flex items-center justify-between gap-2">
                  <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.14em] text-slate-500">
                    <Plug className="h-3.5 w-3.5 text-violet-300" aria-hidden="true" />
                    Connect an assistant
                    <HelpTip helpKey="agents.mcp.claude" label="MCP configuration" />
                  </h3>
                  <span className="text-3xs text-slate-600">stdio · JSON</span>
                </div>
                <div className="mt-3">
                  <SegmentedControl
                    value={client}
                    options={AGENT_CLIENTS.map((entry) => ({ value: entry.id, label: entry.label }))}
                    onChange={setClient}
                    ariaLabel="MCP client"
                    className="w-full sm:w-auto"
                  />
                </div>
                <p className="mt-2 text-2xs text-slate-500">{snippet.description}</p>
                <div className="relative mt-2 rounded-xl border border-white/[0.07] bg-ink-950/70">
                  <div className="flex items-center justify-between border-b border-white/[0.06] px-3 py-1.5">
                    <span className="font-mono text-3xs text-slate-600">mcp.json</span>
                    <CopyButton value={snippet.content} label="Copy config" />
                  </div>
                  <pre className="max-h-64 overflow-auto px-3 py-2 text-3xs leading-relaxed text-slate-300">
                    <code>{snippet.content}</code>
                  </pre>
                </div>
                <p className="mt-2 text-3xs leading-relaxed text-slate-500">{snippet.note}</p>
                <div className="mt-2 rounded-lg border border-white/[0.06] bg-white/[0.02] px-3 py-2 text-3xs leading-relaxed text-slate-500">
                  The exact client command is also available from{' '}
                  <code className="rounded bg-white/[0.05] px-1 py-0.5 font-mono text-slate-300">
                    openmesh-cli agent config --client {client}
                  </code>
                  . <HelpLink topic="agents">Read the agent guide</HelpLink>
                </div>
              </section>
            ) : (
              <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
                <div className="flex flex-wrap items-center gap-3">
                  <span className="grid h-9 w-9 place-items-center rounded-lg border border-white/10 bg-white/[0.04] text-violet-200">
                    <Bot className="h-4 w-4" aria-hidden="true" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="text-xs font-semibold text-slate-200">Advanced connection details are hidden</p>
                    <p className="mt-0.5 text-2xs leading-relaxed text-slate-500">
                      Beginner mode keeps the MCP JSON and system prompts out of the way. The activity log and security
                      notes below still apply.
                    </p>
                  </div>
                  <Button variant="primary" size="sm" onClick={() => setUiMode('advanced')}>
                    Show advanced setup
                  </Button>
                </div>
              </section>
            )}

            {/* prompt (advanced) */}
            {advanced ? (
              <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.14em] text-slate-500">
                    <Terminal className="h-3.5 w-3.5 text-emerald-300" aria-hidden="true" />
                    Give the agent a system prompt
                    <HelpTip helpKey="agents.prompt" label="Agent system prompt" />
                  </h3>
                  <CopyButton value={prompt} label="Copy prompt" />
                </div>
                <div className="mt-3">
                  <SegmentedControl
                    value={promptTier}
                    options={agentPromptTiers().map((tier) => ({ value: tier.id, label: tier.label }))}
                    onChange={setPromptTier}
                    ariaLabel="Prompt tier"
                    className="w-full sm:w-auto"
                  />
                </div>
                <p className="mt-2 text-2xs leading-relaxed text-slate-500">
                  {agentPromptTiers().find((tier) => tier.id === promptTier)?.description}
                </p>
                <div className="mt-2 rounded-xl border border-white/[0.07] bg-ink-950/70">
                  <button
                    type="button"
                    onClick={() => setPromptOpen((open) => !open)}
                    className="focus-ring flex w-full items-center justify-between gap-2 px-3 py-2 text-left"
                    aria-expanded={promptOpen}
                  >
                    <span className="font-mono text-3xs text-slate-500">system-prompt.md · {prompt.split('\n').length} lines</span>
                    <ChevronDown className={cn('h-3.5 w-3.5 text-slate-500 transition-transform', promptOpen ? 'rotate-180' : '')} aria-hidden="true" />
                  </button>
                  {promptOpen ? (
                    <pre className="max-h-72 overflow-auto border-t border-white/[0.06] px-3 py-2 text-3xs leading-relaxed text-slate-300">
                      <code>{prompt}</code>
                    </pre>
                  ) : (
                    <pre className="max-h-24 overflow-hidden border-t border-white/[0.06] px-3 py-2 text-3xs leading-relaxed text-slate-500">
                      <code>{prompt.split('\n').slice(0, 6).join('\n')}</code>
                    </pre>
                  )}
                </div>
                <p className="mt-2 text-3xs text-slate-600">
                  Same text as <code className="font-mono text-slate-400">openmesh-cli agent prompt --tier {promptTier}</code>.
                  Tokens are never included.
                </p>
              </section>
            ) : null}

          </div>

          {/* sidebar */}
          <div className="space-y-4">
            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
              <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.14em] text-slate-500">
                <Lock className="h-3.5 w-3.5 text-emerald-300" aria-hidden="true" />
                {READ_ONLY_EXPLAINER.title}
                <HelpTip helpKey="agents.readonly" label="Read-only mode" />
              </h3>
              <p className="mt-2 text-2xs leading-relaxed text-slate-400">{READ_ONLY_EXPLAINER.body}</p>
              <div className="mt-2 flex items-center gap-1.5 rounded-lg border border-emerald-400/15 bg-emerald-400/[0.05] px-2 py-1.5">
                <code className="min-w-0 flex-1 truncate font-mono text-3xs text-emerald-200">{READ_ONLY_EXPLAINER.env}</code>
                <CopyButton value={READ_ONLY_EXPLAINER.env} label="Copy" />
              </div>
              <p className="mt-2 text-3xs leading-relaxed text-slate-600">{READ_ONLY_EXPLAINER.note}</p>
            </section>

            <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
              <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.14em] text-slate-500">
                <ShieldCheck className="h-3.5 w-3.5 text-sky-300" aria-hidden="true" />
                Security rules
                <HelpTip helpKey="agents.security" label="Agent security rules" />
              </h3>
              <ul className="mt-2 space-y-2">
                {AGENT_SAFETY_RULES.slice(0, 3).map((rule) => (
                  <li key={rule} className="flex items-start gap-1.5 text-3xs leading-relaxed text-slate-500">
                    <CheckCircle2 className="mt-0.5 h-3 w-3 shrink-0 text-emerald-300/70" aria-hidden="true" />
                    {rule}
                  </li>
                ))}
                {AGENT_SECURITY_NOTES.slice(0, 2).map((note) => (
                  <li key={note} className="flex items-start gap-1.5 text-3xs leading-relaxed text-slate-600">
                    <span className="mt-1 h-1 w-1 shrink-0 rounded-full bg-slate-600" aria-hidden="true" />
                    {note}
                  </li>
                ))}
              </ul>
              <HelpLink topic="security" className="mt-2">Read the security guide</HelpLink>
            </section>

            <section className="rounded-2xl border border-amber-400/20 bg-amber-400/[0.04] p-4">
              <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.14em] text-amber-200/80">
                <KeyRound className="h-3.5 w-3.5" aria-hidden="true" />
                Revoke access
                <HelpTip helpKey="agents.revoke" label="Revoke API token" />
              </h3>
              <p className="mt-2 text-2xs leading-relaxed text-slate-400">
                Rotating the bridge token immediately blocks agents and browsers that still use the old one. The
                panel cannot rotate it through the v0.3 API, so we show the exact steps instead of pretending.
              </p>
              <div className="mt-2 flex flex-wrap gap-2">
                <Button variant="danger" size="sm" icon={KeyRound} onClick={() => setRevokeOpen(true)}>
                  Rotate bridge token…
                </Button>
                <Button variant="ghost" size="sm" icon={ShieldAlert} onClick={() => openHelp(API_TOKEN_ROTATION_HELP_TOPIC)}>
                  Guidance
                </Button>
              </div>
            </section>

            {advanced ? (
              <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
                <h3 className="text-xs font-semibold uppercase tracking-[0.14em] text-slate-500">Agent status</h3>
                <dl className="mt-2 space-y-1.5 text-2xs">
                  <div className="flex items-center justify-between gap-3">
                    <dt className="text-slate-500">Connection</dt>
                    <dd className={agentConnected ? 'text-violet-200' : 'text-slate-400'}>
                      {agentConnected ? 'connected' : 'none yet'}
                    </dd>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <dt className="text-slate-500">Last action</dt>
                    <dd className="text-slate-400">{lastAgentAtMs ? formatActivityTime(lastAgentAtMs) : '—'}</dd>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <dt className="text-slate-500">Sources seen</dt>
                    <dd className="truncate text-slate-400">{activitySources.join(', ') || '—'}</dd>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <dt className="text-slate-500">Bridge</dt>
                    <dd className="font-mono text-slate-400">{settings.bridgeUrl}</dd>
                  </div>
                </dl>
              </section>
            ) : null}
          </div>
        </div>
      </div>

      <Modal
        open={revokeOpen}
        onClose={() => setRevokeOpen(false)}
        title="Rotate the bridge API token"
        description="This revokes every browser and agent using the old token. Do it from the bridge host — the v0.3 API has no rotation endpoint."
        icon={KeyRound}
        size="lg"
        footer={
          <>
            <Button variant="ghost" onClick={() => setRevokeOpen(false)}>Close</Button>
            <Button variant="ghost" icon={ShieldAlert} onClick={() => openHelp(API_TOKEN_ROTATION_HELP_TOPIC)}>Open security help</Button>
          </>
        }
      >
        <ol className="space-y-2.5">
          {API_TOKEN_ROTATION_STEPS.map((step, index) => (
            <li key={step} className="flex items-start gap-2.5 rounded-lg border border-white/[0.06] bg-white/[0.02] px-3 py-2">
              <span className="grid h-5 w-5 shrink-0 place-items-center rounded-full border border-amber-400/30 bg-amber-400/10 font-mono text-3xs text-amber-200">
                {index + 1}
              </span>
              <span className="text-2xs leading-relaxed text-slate-400">{step}</span>
            </li>
          ))}
        </ol>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <CopyButton value={API_TOKEN_ROTATION_STEPS.map((step, index) => `${index + 1}. ${step}`).join('\n')} label="Copy steps" variant="surface" />
          <span className="text-3xs text-slate-600">Secrets never appear in this panel or its logs.</span>
        </div>
      </Modal>
    </div>
  );
}

function SourceBadge({ source }: { source: string }) {
  const meta = activitySourceMeta(source);
  return (
    <Badge tone={SOURCE_TONES[meta.kind]} icon={meta.kind === 'agent' ? Bot : meta.kind === 'cli' ? Terminal : undefined}>
      {meta.label}
    </Badge>
  );
}

function ActivityRow({ entry }: { entry: ActivityEntry }) {
  return (
    <tr className="border-b border-white/[0.04] last:border-0">
      <td className="px-4 py-2.5"><SourceBadge source={entry.source} /></td>
      <td className="px-3 py-2.5">
        <p className="max-w-[360px] truncate text-2xs text-slate-300">{entry.summary}</p>
        <p className="mt-0.5 font-mono text-3xs text-slate-600">{entry.method} {entry.path}</p>
      </td>
      <td className="px-3 py-2.5">
        {entry.ok ? (
          <span className="inline-flex items-center gap-1 text-3xs text-emerald-300"><CheckCircle2 className="h-3 w-3" aria-hidden="true" />ok</span>
        ) : (
          <span className="inline-flex max-w-[180px] items-center gap-1 truncate text-3xs text-rose-300" title={entry.error ?? 'failed'}>
            <XCircle className="h-3 w-3 shrink-0" aria-hidden="true" />{entry.error ?? 'failed'}
          </span>
        )}
      </td>
      <td className="whitespace-nowrap px-4 py-2.5 text-right text-3xs text-slate-600">{formatActivityTime(entry.at_ms)}</td>
    </tr>
  );
}

function ActivityMobileRow({ entry }: { entry: ActivityEntry }) {
  return (
    <li className="px-4 py-3">
      <div className="flex items-center justify-between gap-2">
        <SourceBadge source={entry.source} />
        <span className="text-3xs text-slate-600">{formatActivityTime(entry.at_ms)}</span>
      </div>
      <p className="mt-1.5 text-2xs text-slate-300">{entry.summary}</p>
      <p className="mt-0.5 font-mono text-3xs text-slate-600">{entry.method} {entry.path}</p>
      <p className={cn('mt-1 text-3xs', entry.ok ? 'text-emerald-300' : 'text-rose-300')}>
        {entry.ok ? 'ok' : entry.error ?? 'failed'}
      </p>
    </li>
  );
}
