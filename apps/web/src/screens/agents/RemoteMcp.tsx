import { useMemo } from 'react';
import {
  Copy,
  ExternalLink,
  Globe2,
  Lock,
  Network,
  ShieldAlert,
  ShieldCheck,
  Terminal,
} from 'lucide-react';
import { useAppStore } from '../../store/app';
import {
  MCP_DOCS_PATH,
  MCP_HTTP_WARNINGS,
  mcpClientCommand,
  mcpConfigSnippet,
  mcpEndpointFromBridgeUrl,
  mcpScopeLabel,
  mcpStatusOrDefault,
  mcpTransportLabel,
} from '../../lib/ops';
import { cn } from '../../lib/utils';
import { Badge } from '../../components/ui/primitives';
import { CopyButton } from '../../components/ui/signals';
import { HelpLink, HelpTip } from '../../components/ui/help';

/**
 * Remote MCP status/guidance card (v0.7, docs/design/remote-operations.md §1/§5).
 *
 * There is no REST API for mcp-http.json in v0.7, so this card never invents one:
 * it shows a bridge/demo status object when available and otherwise renders the file
 * configuration plus a copyable snippet and client command.
 */
export function RemoteMcpCard({ advanced }: { advanced: boolean }) {
  const settings = useAppStore((state) => state.settings);
  const stored = useAppStore((state) => state.mcpStatus);
  const status = useMemo(() => mcpStatusOrDefault(stored, settings.bridgeUrl), [stored, settings.bridgeUrl]);
  const endpoint = status.endpoint_url ?? mcpEndpointFromBridgeUrl(settings.bridgeUrl, status.path);
  const snippet = mcpConfigSnippet(status);
  const command = mcpClientCommand(endpoint);
  const known = stored !== null;
  const readOnly = !status.allow_write;

  return (
    <section id="remote-mcp" className="rounded-2xl border border-white/[0.07] bg-white/[0.02]">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.06] px-4 py-3">
        <div className="min-w-0">
          <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.14em] text-slate-500">
            <Globe2 className="h-3.5 w-3.5 text-violet-300" aria-hidden="true" />
            Remote MCP over HTTP
            <HelpTip helpKey="agents.mcp.remote" label="Remote MCP" />
          </h3>
          <p className="mt-1 max-w-2xl text-2xs leading-relaxed text-slate-500">
            stdio stays the default, fully local path. HTTP is opt-in on the bridge’s existing listener for agents on
            another machine or a tunnel/VPN. It is read-only unless you explicitly allow writes, and the bridge token
            is the control key.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          <Badge tone={status.enabled ? 'violet' : 'slate'} icon={Network}>
            {status.enabled ? 'HTTP enabled' : 'stdio (default)'}
          </Badge>
          <Badge tone={readOnly ? 'accent' : 'rose'} icon={readOnly ? Lock : ShieldAlert}>
            {readOnly ? 'read-only' : 'read + write'}
          </Badge>
        </div>
      </div>

      <div className="grid gap-4 px-4 py-4 lg:grid-cols-[minmax(0,1fr)_360px]">
        <div className="space-y-3">
          <dl className="grid gap-2 text-2xs sm:grid-cols-2">
            <StatusItem label="Transport" value={mcpTransportLabel(status)} detail={status.enabled ? 'HTTP on the bridge listener' : 'Local subprocess; no network listener'} />
            <StatusItem label="Scope" value={mcpScopeLabel(status)} detail={status.allow_write ? 'Write tools allowed (confirm still gates dangerous tools)' : 'Write/confirm-gated tools refused'} />
            <StatusItem label="Rate limit" value={`${status.rate_limit_per_min} / min`} detail={`max ${status.max_sessions} sessions`} />
            <StatusItem
              label={known ? 'Sessions / usage' : 'Status source'}
              value={
                known
                  ? `${status.active_sessions ?? '—'} session${status.active_sessions === 1 ? '' : 's'}${status.requests_per_min !== null ? ` · ${status.requests_per_min}/min` : ''}`
                  : 'file-configured'
              }
              detail={known ? (status.source === 'demo' ? 'demo seed — not a live bridge' : 'reported by bridge') : 'No MCP REST status endpoint in v0.7'}
            />
          </dl>

          <div className="rounded-xl border border-white/[0.07] bg-ink-950/60 px-3 py-2.5">
            <div className="flex items-center justify-between gap-2">
              <p className="flex items-center gap-1 text-3xs font-medium uppercase tracking-[0.1em] text-slate-500">
                <Globe2 className="h-3 w-3" aria-hidden="true" />
                {status.enabled ? 'Endpoint' : 'Endpoint when enabled'}
              </p>
              <CopyButton value={endpoint} label="Copy endpoint" />
            </div>
            <code className="mt-1.5 block break-all font-mono text-3xs text-slate-300">{endpoint}</code>
            <p className="mt-1 text-3xs leading-relaxed text-slate-600">
              POST JSON-RPC to this URL; <code className="font-mono">GET</code> opens the SSE keep-alive stream.{' '}
              {status.enabled ? null : 'This is the expected path from the file configuration, not an active listener.'}
            </p>
          </div>

          <div className="rounded-xl border border-white/[0.07] bg-ink-950/60">
            <div className="flex items-center justify-between gap-2 border-b border-white/[0.06] px-3 py-1.5">
              <p className="font-mono text-3xs text-slate-500">mcp-http.json · mode 0600</p>
              <CopyButton value={snippet} label="Copy config" />
            </div>
            <pre className="max-h-52 overflow-auto px-3 py-2 text-3xs leading-relaxed text-slate-300">
              <code>{snippet}</code>
            </pre>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <CopyButton value={command} label="Copy CLI config command" variant="surface" />
            <HelpLink topic="agents" className="text-3xs">MCP and remote-agent guide</HelpLink>
            <span className="inline-flex items-center gap-1 text-3xs text-slate-600">
              <Terminal className="h-3 w-3" aria-hidden="true" />
              {MCP_DOCS_PATH}
              <ExternalLink className="h-2.5 w-2.5" aria-hidden="true" />
            </span>
          </div>
        </div>

        <div className="space-y-2.5">
          <div className={cn('rounded-xl border px-3 py-2.5', readOnly ? 'border-emerald-400/20 bg-emerald-400/[0.04]' : 'border-amber-400/25 bg-amber-400/[0.05]')}>
            <p className="flex items-center gap-1.5 text-2xs font-semibold text-slate-200">
              {readOnly ? <ShieldCheck className="h-3.5 w-3.5 text-emerald-300" aria-hidden="true" /> : <ShieldAlert className="h-3.5 w-3.5 text-amber-300" aria-hidden="true" />}
              {readOnly ? 'Read-only recommended' : 'Write access is enabled'}
            </p>
            <p className="mt-1 text-3xs leading-relaxed text-slate-500">
              {readOnly
                ? 'Agents can read nodes, messages and status but cannot change configuration, keys, regulatory settings or the backbone.'
                : 'Agents can change your node. Keep require_confirm on, use a dedicated token, and remove write access when the operation is done.'}
            </p>
          </div>

          <div className="rounded-xl border border-rose-400/20 bg-rose-400/[0.04] px-3 py-2.5">
            <p className="flex items-center gap-1.5 text-2xs font-semibold text-rose-200/90">
              <ShieldAlert className="h-3.5 w-3.5" aria-hidden="true" />
              Remote access rules
            </p>
            <ul className="mt-1.5 space-y-1.5">
              {MCP_HTTP_WARNINGS.map((warning) => (
                <li key={warning} className="flex items-start gap-1.5 text-3xs leading-relaxed text-slate-500">
                  <span className="mt-1 h-1 w-1 shrink-0 rounded-full bg-rose-300/60" aria-hidden="true" />
                  {warning}
                </li>
              ))}
            </ul>
          </div>

          {advanced ? (
            <p className="rounded-lg border border-white/[0.06] bg-white/[0.015] px-3 py-2 text-3xs leading-relaxed text-slate-500">
              This panel is intentionally read-only: v0.7 configures MCP HTTP in{' '}
              <code className="font-mono text-slate-300">&lt;data-dir&gt;/mcp-http.json</code> and there is no REST write
              endpoint. Edit the file on the bridge host, restart the bridge, then reconnect the agent.
            </p>
          ) : (
            <p className="rounded-lg border border-white/[0.06] bg-white/[0.015] px-3 py-2 text-3xs leading-relaxed text-slate-500">
              Remote MCP is an advanced, opt-in feature. The card stays visible so you can see whether HTTP is on and
              whether an agent has write access.
            </p>
          )}
          <div className="flex items-center gap-1.5 text-3xs text-slate-600">
            <Copy className="h-3 w-3" aria-hidden="true" />
            Tokens never appear in this panel; copy the token file path with the CLI command above.
          </div>
        </div>
      </div>
    </section>
  );
}

function StatusItem({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-2.5 py-2">
      <dt className="text-3xs uppercase tracking-[0.1em] text-slate-600">{label}</dt>
      <dd className="mt-0.5 text-xs font-medium text-slate-200">{value}</dd>
      <dd className="mt-0.5 text-3xs leading-relaxed text-slate-600">{detail}</dd>
    </div>
  );
}
