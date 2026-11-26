import { useEffect, useMemo, useState } from 'react';
import {
  ArrowUpRight,
  CheckCircle2,
  Link2,
  Lock,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  Webhook,
  XCircle,
} from 'lucide-react';
import { useAppStore } from '../../store/app';
import type { GatewayConnector } from '../../types/ops';
import {
  GATEWAY_ADAPTERS,
  GATEWAYS_HELP_TOPIC,
  deriveInboundUrl,
  gatewayDraftFromConnector,
  gatewayStatusView,
  gatewayTestSummary,
  validateGatewayDraft,
  type GatewayDraft,
  type GatewayDraftErrors,
} from '../../lib/ops';
import { formatRelativeTime, cn } from '../../lib/utils';
import { Badge, Button } from '../../components/ui/primitives';
import { Callout, Checkbox, Field, TextInput } from '../../components/ui/forms';
import { CopyButton } from '../../components/ui/signals';
import { Modal } from '../../components/ui/overlay';
import { HelpLink, HelpTip } from '../../components/ui/help';
import type { GatewayTestResult } from '../../types/ops';

const EMPTY_DRAFT: GatewayDraft = {
  id: '',
  url: '',
  outSecretEnv: '',
  inSecretEnv: '',
  channelIds: '',
  includeDirect: false,
  enabled: true,
  inboundEnabled: false,
  inboundChannelId: '',
  inboundMaxChars: '400',
};

/** Gateways card for the Backbone panel (docs/design/remote-operations.md §2/§5). */
export function GatewaysCard({ advanced }: { advanced: boolean }) {
  const connection = useAppStore((state) => state.connection);
  const settings = useAppStore((state) => state.settings);
  const gateways = useAppStore((state) => state.gateways);
  const loading = useAppStore((state) => state.gatewaysLoading);
  const error = useAppStore((state) => state.gatewaysError);
  const unsupported = useAppStore((state) => state.gatewaysUnsupported);
  const saving = useAppStore((state) => state.gatewaySaving);
  const testing = useAppStore((state) => state.gatewayTesting);
  const loadGateways = useAppStore((state) => state.loadGateways);
  const testGateway = useAppStore((state) => state.testGateway);
  const deleteGateway = useAppStore((state) => state.deleteGateway);
  const setGatewayEnabled = useAppStore((state) => state.setGatewayEnabled);
  const setUiMode = useAppStore((state) => state.setUiMode);
  const channels = useAppStore((state) => state.channels);

  const [editorOpen, setEditorOpen] = useState(false);
  const [editing, setEditing] = useState<GatewayConnector | null>(null);
  const [draft, setDraft] = useState<GatewayDraft>(EMPTY_DRAFT);
  const [fieldErrors, setFieldErrors] = useState<GatewayDraftErrors>({});
  const [deleteTarget, setDeleteTarget] = useState<GatewayConnector | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState('');
  const [testResults, setTestResults] = useState<Record<string, GatewayTestResult>>({});

  useEffect(() => {
    if (connection === 'demo' || connection === 'connected') void loadGateways();
  }, [connection, loadGateways]);

  const channelNames = useMemo(() => {
    const map = new Map<string, string>();
    for (const channel of Object.values(channels)) map.set(channel.channel_id, channel.name);
    return map;
  }, [channels]);

  const openAdd = () => {
    setEditing(null);
    setDraft(EMPTY_DRAFT);
    setFieldErrors({});
    setEditorOpen(true);
  };
  const openEdit = (connector: GatewayConnector) => {
    setEditing(connector);
    setDraft(gatewayDraftFromConnector(connector));
    setFieldErrors({});
    setEditorOpen(true);
  };

  const submit = async () => {
    const validation = validateGatewayDraft(draft, {
      existingIds: (gateways ?? []).map((connector) => connector.id),
      editing: Boolean(editing),
    });
    setFieldErrors(validation.errors);
    if (!validation.config) return;
    const ok = await useAppStore.getState().saveGateway(validation.config);
    if (ok) setEditorOpen(false);
  };

  const runTest = async (connector: GatewayConnector) => {
    const result = await testGateway(connector.id);
    if (result) setTestResults((current) => ({ ...current, [connector.id]: result }));
  };

  if (!advanced) {
    return (
      <section id="gateways" className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-4">
        <div className="flex flex-wrap items-start gap-3">
          <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl border border-sky-400/20 bg-sky-400/[0.08] text-sky-200">
            <Webhook className="h-5 w-5" aria-hidden="true" />
          </span>
          <div className="min-w-0 flex-1">
            <h3 className="flex items-center gap-1.5 text-sm font-semibold text-slate-100">
              Gateways connect your mesh to other apps
              <HelpTip helpKey="backbone.gateways" label="Gateways" />
            </h3>
            <p className="mt-1 max-w-2xl text-2xs leading-relaxed text-slate-500">
              A gateway is an authenticated webhook in both directions: channel messages can go out to an SMS,
              Matrix, email or satellite adapter, and that adapter can inject replies back into a channel. Only
              administrator-set environment-variable names are stored — never the secrets themselves.
            </p>
            <div className="mt-3 flex flex-wrap items-center gap-2">
              <Button variant="primary" size="sm" onClick={() => setUiMode('advanced')}>
                Show gateway settings
              </Button>
              <HelpLink topic={GATEWAYS_HELP_TOPIC}>How gateways and remote agents work</HelpLink>
            </div>
          </div>
        </div>
      </section>
    );
  }

  return (
    <section id="gateways" className="rounded-2xl border border-white/[0.07] bg-white/[0.02]">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.06] px-4 py-3">
        <div className="min-w-0">
          <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.14em] text-slate-500">
            <Webhook className="h-3.5 w-3.5 text-sky-300" aria-hidden="true" />
            Gateways
            <HelpTip helpKey="backbone.gateways" label="Gateways" />
          </h3>
          <p className="mt-1 max-w-2xl text-2xs leading-relaxed text-slate-500">
            Authenticated webhooks in both directions. Adapters handle SMS, Matrix, email or satellite services;
            secrets live in environment variables on the bridge host and are referenced by name only.
          </p>
        </div>
        <div className="flex items-center gap-1.5">
          <CopyButton value={GATEWAY_ADAPTERS.map((adapter) => `${adapter.label}: ${adapter.hint}`).join('\n')} label="Copy adapter ideas" />
          <Button variant="ghost" size="sm" icon={RefreshCw} loading={loading} onClick={() => void loadGateways()}>
            Refresh
          </Button>
          <Button variant="primary" size="sm" icon={Plus} onClick={openAdd}>
            Add gateway
          </Button>
        </div>
      </div>

      <div className="space-y-3 px-4 py-4">
        <div className="flex flex-wrap gap-1.5">
          {GATEWAY_ADAPTERS.slice(0, 4).map((adapter) => (
            <button
              key={adapter.id}
              type="button"
              onClick={() => useAppStore.getState().openHelp(GATEWAYS_HELP_TOPIC)}
              title={adapter.hint}
              className="focus-ring rounded-full border border-white/[0.08] bg-white/[0.03] px-2 py-0.5 text-3xs text-slate-400 hover:border-sky-400/30 hover:text-sky-200"
            >
              {adapter.label}
            </button>
          ))}
          <HelpLink topic={GATEWAYS_HELP_TOPIC} className="text-3xs">Adapter examples and signing</HelpLink>
        </div>

        {unsupported ? (
          <Callout tone="warn" icon={ShieldAlert} title="This bridge build does not expose gateways">
            <p>
              <code className="font-mono">GET /api/gateways</code> returned 404, so this UI cannot list or edit
              connectors. Upgrade the bridge to v0.7 or manage gateways with{' '}
              <code className="font-mono">openmesh-cli gateways</code>. The contract and adapter examples are in the
              help topic below.
            </p>
            <div className="mt-2">
              <HelpLink topic={GATEWAYS_HELP_TOPIC}>Open the gateways guide</HelpLink>
            </div>
          </Callout>
        ) : null}

        {error ? (
          <Callout tone="danger" icon={ShieldAlert} title="Gateway API error">
            <div className="flex flex-wrap items-center gap-2">
              <span>{error}</span>
              <Button variant="ghost" size="sm" icon={RefreshCw} onClick={() => void loadGateways()}>Retry</Button>
            </div>
          </Callout>
        ) : null}

        {!unsupported && !gateways?.length && !loading ? (
          <div className="rounded-xl border border-dashed border-white/10 bg-white/[0.015] px-4 py-5 text-center">
            <Webhook className="mx-auto h-5 w-5 text-slate-600" aria-hidden="true" />
            <p className="mt-1.5 text-xs font-medium text-slate-300">No gateways yet</p>
            <p className="mx-auto mt-0.5 max-w-md text-2xs leading-relaxed text-slate-500">
              Add your first adapter to move channel traffic to SMS, Matrix, email or a satellite service — and let
              replies come back into a channel.
            </p>
            <Button variant="secondary" size="sm" icon={Plus} className="mt-2.5" onClick={openAdd}>
              Add gateway
            </Button>
          </div>
        ) : null}

        {gateways?.map((connector) => {
          const view = gatewayStatusView(connector);
          const inbound = connector.inbound;
          const inboundUrl = deriveInboundUrl(settings.bridgeUrl, connector.id);
          const result = testResults[connector.id];
          return (
            <article
              key={connector.id}
              className={cn(
                'rounded-xl border bg-white/[0.02] p-3',
                view.tone === 'rose' ? 'border-rose-400/25' : 'border-white/[0.07]',
              )}
            >
              <div className="flex flex-wrap items-start gap-2.5">
                <span className={cn('mt-0.5 grid h-8 w-8 shrink-0 place-items-center rounded-lg border', view.tone === 'rose' ? 'border-rose-400/20 bg-rose-400/[0.06] text-rose-300' : 'border-white/10 bg-white/[0.03] text-slate-400')}>
                  <Webhook className="h-4 w-4" aria-hidden="true" />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <h4 className="text-sm font-semibold text-slate-100">{connector.id}</h4>
                    <Badge tone={view.tone}>{view.label}</Badge>
                    <Badge tone={connector.enabled ? 'accent' : 'slate'}>{connector.enabled ? 'enabled' : 'disabled'}</Badge>
                    {inbound.enabled ? <Badge tone="sky">inbound → {channelNames.get(inbound.channel_id ?? '') ?? inbound.channel_id}</Badge> : null}
                  </div>
                  <p className="mt-1 break-all font-mono text-3xs text-slate-500">{connector.outbound.url || 'no outbound URL'}
                  </p>
                  <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-3xs text-slate-500">
                    <span className="inline-flex items-center gap-1">
                      {connector.status.out_secret_present ? (
                        <ShieldCheck className="h-3 w-3 text-emerald-300" aria-hidden="true" />
                      ) : (
                        <ShieldAlert className="h-3 w-3 text-amber-300" aria-hidden="true" />
                      )}
                      out secret {connector.status.out_secret_present ? 'present' : 'missing'}
                    </span>
                    <span className="inline-flex items-center gap-1">
                      {!inbound.enabled ? (
                        <span className="text-slate-600">inbound off</span>
                      ) : connector.status.in_secret_present ? (
                        <><ShieldCheck className="h-3 w-3 text-emerald-300" aria-hidden="true" /> in secret present</>
                      ) : (
                        <><ShieldAlert className="h-3 w-3 text-amber-300" aria-hidden="true" /> in secret missing</>
                      )}
                    </span>
                    {connector.status.last_success_ms ? (
                      <span className="inline-flex items-center gap-1 text-emerald-300/80">
                        <CheckCircle2 className="h-3 w-3" aria-hidden="true" />
                        last success {formatRelativeTime(connector.status.last_success_ms)}
                      </span>
                    ) : null}
                    {connector.status.last_failure_ms ? (
                      <span className="inline-flex items-center gap-1 text-rose-300/90" title={connector.status.last_error ?? undefined}>
                        <XCircle className="h-3 w-3" aria-hidden="true" />
                        last failure {formatRelativeTime(connector.status.last_failure_ms)}
                      </span>
                    ) : null}
                    {connector.status.retries > 0 ? <span>{connector.status.retries} retries</span> : null}
                  </div>
                  {connector.status.last_error ? (
                    <p className="mt-1 rounded border border-rose-400/20 bg-rose-400/[0.05] px-2 py-1 text-3xs text-rose-200/80">
                      {connector.status.last_error}
                    </p>
                  ) : null}
                  {inbound.enabled ? (
                    <div className="mt-2 rounded-lg border border-sky-400/15 bg-sky-400/[0.04] px-2.5 py-1.5">
                      <p className="flex items-center gap-1 text-3xs font-medium text-sky-200/90">
                        <Link2 className="h-3 w-3" aria-hidden="true" />
                        Inbound URL · channel {channelNames.get(inbound.channel_id ?? '') ?? inbound.channel_id} · max {inbound.max_chars} chars
                      </p>
                      <div className="mt-1 flex items-center gap-1.5">
                        <code className="min-w-0 flex-1 truncate font-mono text-3xs text-slate-300">{inboundUrl}</code>
                        <CopyButton value={inboundUrl} label="Copy inbound URL" />
                      </div>
                    </div>
                  ) : null}
                  {result ? (
                    <p className={cn('mt-1.5 flex items-center gap-1 text-3xs', result.ok ? 'text-emerald-300' : 'text-rose-300')}>
                      {result.ok ? <CheckCircle2 className="h-3 w-3" aria-hidden="true" /> : <XCircle className="h-3 w-3" aria-hidden="true" />}
                      {gatewayTestSummary(result)}
                    </p>
                  ) : null}
                </div>
                <div className="flex shrink-0 flex-wrap items-center gap-1">
                  <Button variant="ghost" size="sm" icon={Play} loading={testing === connector.id} onClick={() => void runTest(connector)}>
                    Test
                  </Button>
                  <Button variant="ghost" size="sm" icon={Pencil} onClick={() => openEdit(connector)}>Edit</Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    icon={connector.enabled ? XCircle : CheckCircle2}
                    loading={saving === connector.id}
                    onClick={() => void setGatewayEnabled(connector.id, !connector.enabled)}
                  >
                    {connector.enabled ? 'Disable' : 'Enable'}
                  </Button>
                  <Button variant="ghost" size="sm" icon={Trash2} onClick={() => { setDeleteTarget(connector); setDeleteConfirm(''); }}>
                    Delete
                  </Button>
                </div>
              </div>
            </article>
          );
        })}

        <p className="text-3xs leading-relaxed text-slate-600">
          Attachment bytes are not pushed through webhooks in v0.7 — messages carry attachment metadata only.
          Adapters should treat the sender name as display text, never identity. Secrets are read from
          environment variables on the bridge host; this panel only ever shows their names.
        </p>
      </div>

      <Modal
        open={editorOpen}
        onClose={() => setEditorOpen(false)}
        title={editing ? `Edit gateway · ${editing.id}` : 'Add gateway'}
        description="Secrets stay on the bridge host. Enter the names of environment variables, never the secret values."
        icon={Webhook}
        size="lg"
        footer={
          <>
            <Button variant="ghost" onClick={() => setEditorOpen(false)}>Cancel</Button>
            <Button variant="primary" onClick={() => void submit()} loading={saving !== null}>
              {editing ? 'Save gateway' : 'Add gateway'}
            </Button>
          </>
        }
      >
        <div className="space-y-3.5">
          <div className="grid gap-3 sm:grid-cols-2">
            <Field label="Connector id" required error={fieldErrors.id} hint="Lowercase letters, digits, dash or underscore.">
              {({ id, describedBy }) => (
                <TextInput
                  id={id}
                  aria-describedby={describedBy}
                  value={draft.id}
                  disabled={Boolean(editing)}
                  placeholder="sms-ops"
                  onChange={(event) => setDraft({ ...draft, id: event.target.value })}
                />
              )}
            </Field>
            <Field label="Adapter webhook URL" required error={fieldErrors.url} hint="HTTPS endpoint run by your adapter.">
              {({ id, describedBy }) => (
                <TextInput
                  id={id}
                  aria-describedby={describedBy}
                  value={draft.url}
                  placeholder="https://gateway.example/openmesh"
                  onChange={(event) => setDraft({ ...draft, url: event.target.value })}
                />
              )}
            </Field>
            <Field label="Outbound secret env name" error={fieldErrors.outSecretEnv} hint="Read from the bridge host environment; never pasted here.">
              {({ id, describedBy }) => (
                <TextInput
                  id={id}
                  aria-describedby={describedBy}
                  value={draft.outSecretEnv}
                  placeholder="OPENMESH_SMS_OUT_SECRET"
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(event) => setDraft({ ...draft, outSecretEnv: event.target.value })}
                />
              )}
            </Field>
            <Field label="Inbound secret env name" error={fieldErrors.inSecretEnv} hint="Required only when inbound is enabled.">
              {({ id, describedBy }) => (
                <TextInput
                  id={id}
                  aria-describedby={describedBy}
                  value={draft.inSecretEnv}
                  placeholder="OPENMESH_SMS_IN_SECRET"
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(event) => setDraft({ ...draft, inSecretEnv: event.target.value })}
                />
              )}
            </Field>
          </div>

          <Field label="Channel filters" error={fieldErrors.channelIds} hint="Optional comma-separated channel ids. Empty = every channel.">
            {({ id, describedBy }) => (
              <TextInput
                id={id}
                aria-describedby={describedBy}
                value={draft.channelIds}
                placeholder={(Object.values(channels)[0]?.channel_id ?? 'dba1337a96acc686')}
                onChange={(event) => setDraft({ ...draft, channelIds: event.target.value })}
              />
            )}
          </Field>

          <Checkbox
            checked={draft.includeDirect}
            onChange={(checked) => setDraft({ ...draft, includeDirect: checked })}
            label="Include direct messages"
            description="Outbound sends channel messages; direct messages are excluded unless this is on."
          />
          <Checkbox
            checked={draft.enabled}
            onChange={(checked) => setDraft({ ...draft, enabled: checked })}
            label="Connector enabled"
          />

          <div className="rounded-xl border border-white/[0.07] bg-white/[0.015] p-3">
            <Checkbox
              checked={draft.inboundEnabled}
              onChange={(checked) => setDraft({ ...draft, inboundEnabled: checked })}
              label="Accept inbound messages"
              description="The adapter can POST messages into one channel, attributed to the gateway as display text."
            />
            {draft.inboundEnabled ? (
              <div className="mt-3 space-y-3">
                <div className="grid gap-3 sm:grid-cols-2">
                  <Field label="Inbound channel id" required error={fieldErrors.inboundChannelId} hint="16 hex characters. Messages appear in this channel.">
                    {({ id, describedBy }) => (
                      <>
                        <TextInput
                          id={id}
                          aria-describedby={describedBy}
                          value={draft.inboundChannelId}
                          placeholder={Object.values(channels)[0]?.channel_id ?? 'dba1337a96acc686'}
                          onChange={(event) => setDraft({ ...draft, inboundChannelId: event.target.value })}
                        />
                        <select
                          className="field-shell mt-1.5 h-8 py-0 text-2xs"
                          value=""
                          aria-label="Pick a channel"
                          onChange={(event) => {
                            if (event.target.value) setDraft({ ...draft, inboundChannelId: event.target.value });
                          }}
                        >
                          <option value="">Pick a channel…</option>
                          {Object.values(channels).map((channel) => (
                            <option key={channel.channel_id} value={channel.channel_id}>{channel.name}</option>
                          ))}
                        </select>
                      </>
                    )}
                  </Field>
                  <Field label="Max characters" required error={fieldErrors.inboundMaxChars} hint="1–4000; longer inbound text is rejected.">
                    {({ id, describedBy }) => (
                      <TextInput
                        id={id}
                        aria-describedby={describedBy}
                        value={draft.inboundMaxChars}
                        inputMode="numeric"
                        onChange={(event) => setDraft({ ...draft, inboundMaxChars: event.target.value })}
                      />
                    )}
                  </Field>
                </div>
                <div className="rounded-lg border border-sky-400/15 bg-sky-400/[0.04] px-2.5 py-1.5">
                  <p className="flex items-center gap-1 text-3xs font-medium text-sky-200/90">
                    <Lock className="h-3 w-3" aria-hidden="true" />
                    Generated inbound URL
                  </p>
                  <div className="mt-1 flex items-center gap-1.5">
                    <code className="min-w-0 flex-1 truncate font-mono text-3xs text-slate-300">
                      {deriveInboundUrl(settings.bridgeUrl, editing?.id || draft.id || 'CONNECTOR')}
                    </code>
                    <CopyButton value={deriveInboundUrl(settings.bridgeUrl, editing?.id || draft.id || 'CONNECTOR')} label="Copy inbound URL" />
                  </div>
                </div>
              </div>
            ) : null}
          </div>

          <p className="flex items-start gap-1.5 text-3xs leading-relaxed text-slate-500">
            <ArrowUpRight className="mt-0.5 h-3 w-3 shrink-0" aria-hidden="true" />
            Adapter examples for Matrix, SMS, email, satellite and ntfy are in{' '}
            <HelpLink topic={GATEWAYS_HELP_TOPIC} className="text-3xs">the gateways guide</HelpLink>.
            <HelpTip helpKey="backbone.gateways" label="Gateway signature" />
          </p>
        </div>
      </Modal>

      <Modal
        open={Boolean(deleteTarget)}
        onClose={() => setDeleteTarget(null)}
        title={`Delete gateway · ${deleteTarget?.id ?? ''}`}
        description="This removes the connector and its inbound endpoint. Adapter configuration on the other side is untouched."
        icon={Trash2}
        size="md"
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleteTarget(null)}>Cancel</Button>
            <Button
              variant="danger"
              disabled={deleteConfirm !== deleteTarget?.id}
              onClick={() => {
                if (!deleteTarget) return;
                void deleteGateway(deleteTarget.id).then((ok) => {
                  if (ok) setDeleteTarget(null);
                });
              }}
            >
              Delete gateway
            </Button>
          </>
        }
      >
        <Field
          label={`Type “${deleteTarget?.id ?? ''}” to confirm`}
          required
          error={deleteConfirm.length > 0 && deleteConfirm !== deleteTarget?.id ? 'The id does not match.' : null}
          hint="Deleting is immediate. Outbound messages stop and the inbound URL starts returning 404."
        >
          {({ id, describedBy }) => (
            <TextInput
              id={id}
              aria-describedby={describedBy}
              value={deleteConfirm}
              autoComplete="off"
              onChange={(event) => setDeleteConfirm(event.target.value)}
            />
          )}
        </Field>
      </Modal>
    </section>
  );
}
