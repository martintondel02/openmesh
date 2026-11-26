import { useEffect, useMemo, useState } from 'react';
import {
  ArrowRight,
  CheckCircle2,
  Link2,
  Network,
  Plus,
  RefreshCw,
  ShieldAlert,
  Trash2,
  XCircle,
} from 'lucide-react';
import { useAppStore } from '../../store/app';
import type { BackboneLink } from '../../types/ops';
import {
  BACKBONE_LINK_DEDUP_NOTE,
  backboneLinkStatus,
  redactBackboneLinkUrl,
  validateBackboneLinkDraft,
} from '../../lib/ops';
import { formatRelativeTime, cn } from '../../lib/utils';
import { Badge, Button } from '../../components/ui/primitives';
import { Callout, Field, TextInput } from '../../components/ui/forms';
import { CopyButton } from '../../components/ui/signals';
import { Modal } from '../../components/ui/overlay';
import { HelpLink, HelpTip } from '../../components/ui/help';

/** Multi-backbone links card (docs/design/remote-operations.md §3/§5). */
export function BackboneLinksCard() {
  const connection = useAppStore((state) => state.connection);
  const links = useAppStore((state) => state.backboneLinks);
  const loading = useAppStore((state) => state.backboneLinksLoading);
  const error = useAppStore((state) => state.backboneLinksError);
  const unsupported = useAppStore((state) => state.backboneLinksUnsupported);
  const saving = useAppStore((state) => state.backboneLinkSaving);
  const loadLinks = useAppStore((state) => state.loadBackboneLinks);
  const saveLink = useAppStore((state) => state.saveBackboneLink);
  const deleteLink = useAppStore((state) => state.deleteBackboneLink);
  const setLinkEnabled = useAppStore((state) => state.setBackboneLinkEnabled);
  const legacyConfig = useAppStore((state) => state.backboneConfig);

  const [addOpen, setAddOpen] = useState(false);
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [errors, setErrors] = useState<Partial<Record<'name' | 'url', string>>>({});
  const [deleteTarget, setDeleteTarget] = useState<BackboneLink | null>(null);

  useEffect(() => {
    if (connection === 'demo' || connection === 'connected') void loadLinks();
  }, [connection, loadLinks]);

  const connectedCount = useMemo(() => (links ?? []).filter((link) => link.connected).length, [links]);

  const submit = async () => {
    const validation = validateBackboneLinkDraft(
      { name, url },
      { existingNames: (links ?? []).map((link) => link.name) },
    );
    setErrors(validation.errors);
    if (!validation.input) return;
    const ok = await saveLink(validation.input);
    if (ok) {
      setAddOpen(false);
      setName('');
      setUrl('');
      setErrors({});
    }
  };

  return (
    <section id="backbone-links" className="rounded-2xl border border-white/[0.07] bg-white/[0.02]">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.06] px-4 py-3">
        <div className="min-w-0">
          <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.14em] text-slate-500">
            <Network className="h-3.5 w-3.5 text-emerald-300" aria-hidden="true" />
            Backbone links
            <HelpTip helpKey="backbone.links" label="Multiple backbone links" />
          </h3>
          <p className="mt-1 text-2xs leading-relaxed text-slate-500">
            Join several backbones at once for redundancy or separate communities. Each enabled link runs its own
            reconnect loop; OMP dedup suppresses duplicate frames across links.
          </p>
        </div>
        <div className="flex items-center gap-1.5">
          {links?.length ? (
            <Badge tone={connectedCount > 0 ? 'accent' : 'amber'}>{connectedCount}/{links.length} connected</Badge>
          ) : null}
          <Button variant="ghost" size="sm" icon={RefreshCw} loading={loading} onClick={() => void loadLinks()}>
            Refresh
          </Button>
          <Button variant="primary" size="sm" icon={Plus} onClick={() => setAddOpen(true)}>
            Add link
          </Button>
        </div>
      </div>

      <div className="space-y-3 px-4 py-4">
        {unsupported ? (
          <Callout tone="info" icon={Network} title="This bridge uses the legacy single backbone link">
            <p>
              <code className="font-mono">GET /api/backbone/links</code> returned 404, so the bridge predates multiple
              outbound links. The existing join/backbone configuration keeps working and is shown in the connection
              card above; upgrade the bridge to v0.7 to manage several links here.
            </p>
            <div className="mt-2 flex flex-wrap items-center gap-2">
              <HelpLink topic="backbone">How backbone links work</HelpLink>
              {legacyConfig?.enabled ? (
                <span className="inline-flex items-center gap-1 text-3xs text-slate-500">
                  <ArrowRight className="h-2.5 w-2.5" aria-hidden="true" />
                  legacy listener {legacyConfig.enabled ? 'enabled' : 'disabled'}
                </span>
              ) : null}
            </div>
          </Callout>
        ) : null}

        {error ? (
          <Callout tone="danger" icon={ShieldAlert} title="Backbone links API error">
            <div className="flex flex-wrap items-center gap-2">
              <span>{error}</span>
              <Button variant="ghost" size="sm" icon={RefreshCw} onClick={() => void loadLinks()}>Retry</Button>
            </div>
          </Callout>
        ) : null}

        {!unsupported && !links?.length && !loading ? (
          <div className="rounded-xl border border-dashed border-white/10 bg-white/[0.015] px-4 py-5 text-center">
            <Network className="mx-auto h-5 w-5 text-slate-600" aria-hidden="true" />
            <p className="mt-1.5 text-xs font-medium text-slate-300">No outbound links yet</p>
            <p className="mx-auto mt-0.5 max-w-md text-2xs leading-relaxed text-slate-500">
              Add an invite-style <code className="font-mono">openmesh://backbone?…</code> link to bridge this mesh to
              another community or a redundant gateway.
            </p>
            <Button variant="secondary" size="sm" icon={Plus} className="mt-2.5" onClick={() => setAddOpen(true)}>
              Add link
            </Button>
          </div>
        ) : null}

        {links?.map((link) => {
          const view = backboneLinkStatus(link);
          return (
            <article key={link.name} className={cn('rounded-xl border bg-white/[0.02] p-3', view.tone === 'rose' ? 'border-rose-400/25' : 'border-white/[0.07]')}>
              <div className="flex flex-wrap items-start gap-2.5">
                <span className={cn('mt-0.5 grid h-8 w-8 shrink-0 place-items-center rounded-lg border', view.tone === 'accent' ? 'border-emerald-400/20 bg-emerald-400/[0.06] text-emerald-300' : 'border-white/10 bg-white/[0.03] text-slate-400')}>
                  <Network className="h-4 w-4" aria-hidden="true" />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <h4 className="text-sm font-semibold text-slate-100">{link.name}</h4>
                    <Badge tone={link.enabled ? 'accent' : 'slate'}>{link.enabled ? 'enabled' : 'disabled'}</Badge>
                    <Badge tone={view.tone}>{view.label}</Badge>
                    <span className="text-3xs text-slate-600">{link.mode}</span>
                  </div>
                  <p className="mt-1 break-all font-mono text-3xs text-slate-500">{redactBackboneLinkUrl(link.url) || 'link URL not returned'}</p>
                  <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-3xs text-slate-500">
                    {link.stats ? (
                      <>
                        {typeof link.stats.peers === 'number' ? <span>{link.stats.peers} peers</span> : null}
                        {typeof link.stats.uptime_s === 'number' ? <span>uptime {formatRelativeTime(Date.now() - link.stats.uptime_s * 1000)}</span> : null}
                        {typeof link.stats.frames_in === 'number' ? <span>{link.stats.frames_in} in / {link.stats.frames_out ?? 0} out frames</span> : null}
                      </>
                    ) : <span className="text-slate-600">no per-link stats yet</span>}
                    {link.last_error ? (
                      <span className="inline-flex items-center gap-1 text-rose-300/90">
                        <XCircle className="h-3 w-3" aria-hidden="true" />
                        {link.last_error}
                      </span>
                    ) : null}
                  </div>
                </div>
                <div className="flex shrink-0 flex-wrap items-center gap-1">
                  {link.url ? <CopyButton value={link.url} label="Copy link" /> : null}
                  <Button
                    variant="ghost"
                    size="sm"
                    icon={link.enabled ? XCircle : CheckCircle2}
                    loading={saving === link.name}
                    onClick={() => void setLinkEnabled(link.name, !link.enabled)}
                  >
                    {link.enabled ? 'Disable' : 'Enable'}
                  </Button>
                  <Button variant="ghost" size="sm" icon={Trash2} onClick={() => setDeleteTarget(link)}>Remove</Button>
                </div>
              </div>
            </article>
          );
        })}

        <p className="flex items-start gap-1.5 text-3xs leading-relaxed text-slate-600">
          <Link2 className="mt-0.5 h-3 w-3 shrink-0" aria-hidden="true" />
          {BACKBONE_LINK_DEDUP_NOTE}{' '}
          <HelpLink topic="backbone" className="text-3xs">Backbone guide</HelpLink>
        </p>
      </div>

      <Modal
        open={addOpen}
        onClose={() => setAddOpen(false)}
        title="Add a backbone link"
        description="Paste the invite-style openmesh:// URL generated by the other backbone's panel. It contains a token — treat it as a secret."
        icon={Plus}
        size="lg"
        footer={
          <>
            <Button variant="ghost" onClick={() => setAddOpen(false)}>Cancel</Button>
            <Button variant="primary" onClick={() => void submit()} loading={saving !== null}>Add link</Button>
          </>
        }
      >
        <div className="space-y-3">
          <Field label="Link name" required error={errors.name} hint="A short name for this backbone, e.g. oslo or cabin.">
            {({ id, describedBy }) => (
              <TextInput id={id} aria-describedby={describedBy} value={name} placeholder="oslo" onChange={(event) => setName(event.target.value)} />
            )}
          </Field>
          <Field label="openmesh:// invite link" required error={errors.url} hint="From the other bridge's Backbone → Connection card. Includes host, port and token.">
            {({ id, describedBy }) => (
              <TextInput id={id} aria-describedby={describedBy} value={url} placeholder="openmesh://backbone?host=…&port=9443&token=…" onChange={(event) => setUrl(event.target.value)} />
            )}
          </Field>
          <p className="rounded-lg border border-amber-400/20 bg-amber-400/[0.05] px-2.5 py-2 text-3xs leading-relaxed text-amber-200/80">
            The token inside the link authenticates this bridge to the other backbone. Do not paste invite links into
            chats, screenshots or bug reports.
          </p>
        </div>
      </Modal>

      <Modal
        open={Boolean(deleteTarget)}
        onClose={() => setDeleteTarget(null)}
        title={`Remove link · ${deleteTarget?.name ?? ''}`}
        description="The bridge stops dialling this backbone. Messages already delivered are untouched."
        icon={Trash2}
        size="md"
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleteTarget(null)}>Cancel</Button>
            <Button
              variant="danger"
              onClick={() => {
                if (!deleteTarget) return;
                void deleteLink(deleteTarget.name).then((ok) => {
                  if (ok) setDeleteTarget(null);
                });
              }}
            >
              Remove link
            </Button>
          </>
        }
      >
        <p className="text-2xs leading-relaxed text-slate-400">
          Remove <span className="font-mono text-slate-200">{deleteTarget?.name}</span> from the outbound link list?
          Other enabled links keep running and dedup suppresses duplicate frames.
        </p>
      </Modal>
    </section>
  );
}
