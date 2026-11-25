import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import {
  Globe,
  KeyRound,
  Lock,
  Plus,
  RadioTower,
  RotateCcw,
  Save,
  ShieldAlert,
  ShieldCheck,
  SlidersHorizontal,
  Unlock,
  X,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import type { BackboneConfig, BackboneConfigPatch } from '../../types/backbone';
import {
  addAllowlistEntry,
  diffBackboneConfig,
  formatAuthMode,
  hasBackboneChanges,
  removeAllowlistEntry,
} from '../../lib/backbone';
import { normalizeHex, cn } from '../../lib/utils';
import { Badge, Button, SegmentedControl, Toggle } from '../../components/ui/primitives';
import { Callout, Field, Slider, TextInput } from '../../components/ui/forms';
import { ConfirmDialog } from '../../components/ui/overlay';
import { HelpTip } from '../../components/ui/help';

function cloneConfig(config: BackboneConfig): BackboneConfig {
  return { ...config, allowlist: [...config.allowlist] };
}

/**
 * Live backbone configuration form. Edits are local until Save, which sends only
 * a diff via POST /api/backbone/config (live) or set_backbone_config (demo).
 */
export function BackboneControls({
  config,
  saving,
  demo,
  sessionTokenSet,
  onSave,
  onRotateToken,
}: {
  config: BackboneConfig;
  saving: boolean;
  demo: boolean;
  sessionTokenSet: boolean;
  onSave: (patch: BackboneConfigPatch) => Promise<boolean>;
  onRotateToken: () => void;
}) {
  const [draft, setDraft] = useState<BackboneConfig>(() => cloneConfig(config));
  const [allowlistInput, setAllowlistInput] = useState('');
  const [allowlistError, setAllowlistError] = useState<string | null>(null);
  const [openConfirm, setOpenConfirm] = useState(false);
  const [openConfirmText, setOpenConfirmText] = useState('');

  const dirty = hasBackboneChanges(config, draft);

  useEffect(() => {
    if (dirty) return;
    setDraft(cloneConfig(config));
    // Only re-sync when the authoritative config changes and there are no local edits.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [config]);

  const patchField = <K extends keyof BackboneConfig>(key: K, value: BackboneConfig[K]): void => {
    setDraft((current) => ({ ...current, [key]: value }));
  };

  const addAllowlist = (): void => {
    const result = addAllowlistEntry(draft.allowlist, allowlistInput);
    if (result.error) {
      setAllowlistError(result.error);
      return;
    }
    setAllowlistError(null);
    setAllowlistInput('');
    patchField('allowlist', result.allowlist);
  };

  const save = async (): Promise<void> => {
    const patch = diffBackboneConfig(config, draft);
    if (Object.keys(patch).length === 0) return;
    await onSave(patch);
  };

  return (
    <section className="overflow-hidden rounded-2xl border border-white/[0.06] bg-white/[0.02]">
      <header className="flex flex-wrap items-center gap-2 border-b border-white/[0.06] px-4 py-3">
        <div className="grid h-8 w-8 place-items-center rounded-lg border border-white/[0.07] bg-white/[0.03] text-slate-400">
          <SlidersHorizontal className="h-4 w-4" aria-hidden="true" />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-slate-100">Backbone configuration</h3>
          <p className="mt-0.5 text-2xs text-slate-500">
            {demo ? 'Changes are simulated locally in demo mode.' : 'Saved with POST /api/backbone/config · listener restarts live when bind, port or enabled changes.'}
          </p>
        </div>
        {dirty ? <Badge tone="amber">unsaved changes</Badge> : <Badge tone="accent">saved</Badge>}
      </header>

      <div className="space-y-5 px-4 py-4 md:px-5">
        {/* Listener */}
        <FormSection icon={Globe} title="Public listener" description="Where remote gateways dial in. Applies live without dropping the process.">
          <div className="grid gap-3.5 sm:grid-cols-2">
            <Field label="Bind address" hint="0.0.0.0 listens on every interface; 127.0.0.1 keeps it loopback-only." labelAccessory={<HelpTip helpKey="backbone.bind" label="Bind address" />}>
              {({ id }) => (
                <div className="space-y-1.5">
                  <TextInput
                    id={id}
                    value={draft.bind}
                    className="font-mono text-xs"
                    onChange={(event) => patchField('bind', event.target.value.trim())}
                    placeholder="0.0.0.0"
                  />
                  <div className="flex gap-1">
                    {['0.0.0.0', '127.0.0.1', '::'].map((value) => (
                      <button
                        key={value}
                        type="button"
                        onClick={() => patchField('bind', value)}
                        className={cn(
                          'focus-ring rounded border px-1.5 py-0.5 font-mono text-3xs',
                          draft.bind === value
                            ? 'border-emerald-400/40 bg-emerald-400/10 text-emerald-300'
                            : 'border-white/[0.07] text-slate-500 hover:text-slate-300',
                        )}
                      >
                        {value}
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </Field>
            <div className="grid grid-cols-2 gap-3">
              <Field label="TCP port" hint="1–65535" labelAccessory={<HelpTip helpKey="backbone.port" label="TCP port" />}>
                {({ id }) => (
                  <TextInput
                    id={id}
                    type="number"
                    min={1}
                    max={65535}
                    value={draft.port}
                    className="font-mono text-xs"
                    onChange={(event) => {
                      const parsed = Number(event.target.value);
                      if (Number.isFinite(parsed)) patchField('port', Math.max(1, Math.min(65535, Math.round(parsed))));
                    }}
                  />
                )}
              </Field>
              <div className="pt-4">
                <Slider
                  value={draft.max_peers}
                  min={1}
                  max={256}
                  step={1}
                  label="Max peers"
                  labelAccessory={<HelpTip helpKey="backbone.maxPeers" label="Max peers" />}
                  formatValue={(value) => String(value)}
                  onChange={(value) => patchField('max_peers', value)}
                  markers={[
                    { value: 1, label: '1' },
                    { value: 256, label: '256' },
                  ]}
                />
              </div>
            </div>
          </div>

          <div className="space-y-3">
            <Toggle
              id="backbone-enabled"
              label="Enable backbone listener"
              labelAccessory={<HelpTip helpKey="backbone.enabled" label="Enable backbone listener" />}
              description="Turn off to close the public port without losing configuration."
              checked={draft.enabled}
              onChange={(checked) => patchField('enabled', checked)}
            />
            <Toggle
              id="backbone-store-forward"
              label="Store-and-forward relay"
              labelAccessory={<HelpTip helpKey="backbone.storeForward" label="Store-and-forward relay" />}
              description="Hold encrypted envelopes for absent unicast destinations and offer them when the addressee appears."
              checked={draft.store_forward}
              onChange={(checked) => patchField('store_forward', checked)}
            />
          </div>
        </FormSection>

        <div className="h-px bg-white/[0.05]" />

        {/* Identity */}
        <FormSection icon={RadioTower} title="Identity & sharing" description="How this backbone presents itself in connection strings and peer tables.">
          <div className="grid gap-3.5 sm:grid-cols-2">
            <Field label="Backbone name" hint="Max 32 characters, shown to peers." labelAccessory={<HelpTip helpKey="backbone.name" label="Backbone name" />}>
              {({ id }) => (
                <TextInput
                  id={id}
                  value={draft.name}
                  maxLength={32}
                  onChange={(event) => patchField('name', event.target.value)}
                  placeholder="oslo-backbone"
                />
              )}
            </Field>
            <Field label="Public host" hint="Used to build the shareable connection string; blank falls back to the detected address." labelAccessory={<HelpTip helpKey="backbone.publicHost" label="Public host" />}>
              {({ id }) => (
                <TextInput
                  id={id}
                  value={draft.public_host}
                  className="font-mono text-xs"
                  onChange={(event) => patchField('public_host', event.target.value.trim())}
                  placeholder="mesh.example.org"
                />
              )}
            </Field>
          </div>
          <Slider
            value={draft.ping_interval_s}
            min={10}
            max={300}
            step={5}
            label="Peer ping interval"
            labelAccessory={<HelpTip helpKey="backbone.ping" label="Peer ping interval" />}
            formatValue={(value) => `${value} s`}
            onChange={(value) => patchField('ping_interval_s', value)}
            markers={[
              { value: 10, label: '10 s' },
              { value: 300, label: '5 min' },
            ]}
          />
        </FormSection>

        <div className="h-px bg-white/[0.05]" />

        {/* Security */}
        <FormSection icon={ShieldCheck} title="Transport security" description="OMP payloads are end-to-end encrypted; this authenticates the transport link only.">
          <div className="flex flex-wrap items-center gap-2">
            <SegmentedControl
              ariaLabel="Backbone authentication mode"
              value={draft.auth}
              onChange={(value) => {
                if (value === 'open' && config.auth !== 'open') {
                  setOpenConfirmText('');
                  setOpenConfirm(true);
                  return;
                }
                patchField('auth', value);
              }}
              options={[
                { value: 'token', label: 'Token', icon: KeyRound },
                { value: 'open', label: 'Open', icon: Unlock },
              ]}
            />
            <Badge tone={config.token_set ? 'accent' : 'amber'} icon={config.token_set ? Lock : ShieldAlert}>
              {config.token_set ? 'token set' : 'no token yet'}
            </Badge>
            {sessionTokenSet ? <Badge tone="sky">new token in memory</Badge> : null}
            <div className="ml-auto flex items-center gap-2">
              <Button size="sm" variant="secondary" icon={KeyRound} onClick={onRotateToken}>
                Rotate token
              </Button>
            </div>
          </div>
          <p className="text-2xs leading-relaxed text-slate-500">
            {formatAuthMode(draft.auth)}.{' '}
            {draft.auth === 'open'
              ? 'Anyone who can reach this port may inject traffic — only use this on a trusted LAN or behind WireGuard.'
              : 'Peers prove knowledge of the token with HMAC-SHA256; the token itself never crosses the wire.'}
          </p>

          {draft.auth === 'open' ? (
            <Callout tone="danger" icon={ShieldAlert} title="Open authentication — red warning">
              Frames from any peer that can reach <span className="font-mono">{draft.bind}:{draft.port}</span> will be
              accepted. Put this port behind a VPN or firewall allowlist before enabling open mode.
            </Callout>
          ) : null}

          {draft.auth === 'token' && !config.token_set ? (
            <Callout tone="warn" icon={KeyRound} title="No token has been generated">
              Rotate the token before enabling token auth, otherwise no peer can complete a handshake.
            </Callout>
          ) : null}

          <div>
            <div className="mb-1.5 flex items-center justify-between">
              <p className="text-xs font-medium text-slate-300">Peer allowlist</p>
              <span className="text-3xs text-slate-500">
                {draft.allowlist.length === 0 ? 'empty — any authenticated peer may connect' : `${draft.allowlist.length} node id(s)`}
              </span>
            </div>
            {draft.allowlist.length > 0 ? (
              <div className="mb-2 flex flex-wrap gap-1.5">
                {draft.allowlist.map((nodeId) => (
                  <span
                    key={nodeId}
                    className="inline-flex items-center gap-1 rounded-md border border-white/[0.08] bg-white/[0.04] py-0.5 pl-2 pr-1 font-mono text-3xs text-slate-300"
                  >
                    {normalizeHex(nodeId)}
                    <button
                      type="button"
                      aria-label={`Remove ${nodeId} from allowlist`}
                      onClick={() => patchField('allowlist', removeAllowlistEntry(draft.allowlist, nodeId))}
                      className="grid h-4 w-4 place-items-center rounded text-slate-500 hover:bg-white/[0.08] hover:text-rose-300"
                    >
                      <X className="h-2.5 w-2.5" aria-hidden="true" />
                    </button>
                  </span>
                ))}
              </div>
            ) : null}
            <div className="flex gap-1.5">
              <TextInput
                value={allowlistInput}
                onChange={(event) => {
                  setAllowlistInput(event.target.value);
                  setAllowlistError(null);
                }}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') {
                    event.preventDefault();
                    addAllowlist();
                  }
                }}
                placeholder="Transport node id (16 hex characters)"
                aria-label="Allowlist node id"
                className="font-mono text-xs"
              />
              <Button variant="secondary" size="md" icon={Plus} onClick={addAllowlist} disabled={!allowlistInput.trim()}>
                Add
              </Button>
            </div>
            {allowlistError ? <p className="mt-1 text-2xs text-rose-300">{allowlistError}</p> : null}
            <p className="mt-1 text-2xs text-slate-600">
              Matches the peer bridge&apos;s identity, not the source of every frame it relays.
            </p>
          </div>
        </FormSection>
      </div>

      <footer className="flex flex-wrap items-center gap-2 border-t border-white/[0.06] bg-ink-900/50 px-4 py-3">
        <p className="flex items-center gap-1.5 text-2xs text-slate-500">
          {dirty ? (
            <>
              <span className="h-1.5 w-1.5 rounded-full bg-amber-400" />
              Unsaved changes
            </>
          ) : (
            <>
              <ShieldCheck className="h-3 w-3 text-emerald-400/70" aria-hidden="true" />
              Panel is in sync with the bridge
            </>
          )}
        </p>
        <div className="ml-auto flex items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            icon={RotateCcw}
            disabled={!dirty || saving}
            onClick={() => {
              setDraft(cloneConfig(config));
              setAllowlistInput('');
              setAllowlistError(null);
            }}
          >
            Reset
          </Button>
          <Button variant="primary" size="sm" icon={Save} loading={saving} disabled={!dirty} onClick={() => void save()}>
            Save changes
          </Button>
        </div>
      </footer>

      <ConfirmDialog
        open={openConfirm}
        onClose={() => setOpenConfirm(false)}
        onConfirm={() => {
          if (openConfirmText.trim().toUpperCase() !== 'OPEN') return;
          patchField('auth', 'open');
          setOpenConfirm(false);
          setOpenConfirmText('');
        }}
        title="Switch to open authentication?"
        description="No token, no allowlist check — any peer that can reach the port is accepted. This is only safe on a trusted LAN, VPN or behind a strict firewall."
        confirmLabel="Enable open mode"
        icon={ShieldAlert}
        tone="danger"
      >
        <div className="space-y-3">
          <Callout tone="danger" icon={ShieldAlert} title="Transport authentication will be disabled">
            Message contents stay encrypted, but unauthorised peers could inject traffic and fill queues. Rotate back to
            token mode as soon as possible.
          </Callout>
          <Field label="Type OPEN to confirm">
            {({ id }) => (
              <TextInput
                id={id}
                value={openConfirmText}
                onChange={(event) => setOpenConfirmText(event.target.value)}
                placeholder="OPEN"
                autoComplete="off"
                className="font-mono"
              />
            )}
          </Field>
        </div>
      </ConfirmDialog>
    </section>
  );
}

function FormSection({
  icon: Icon,
  title,
  description,
  children,
}: {
  icon: LucideIcon;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div>
      <div className="mb-3 flex items-start gap-2">
        <Icon className="mt-0.5 h-4 w-4 shrink-0 text-slate-500" aria-hidden="true" />
        <div className="min-w-0">
          <h4 className="text-xs font-semibold text-slate-200">{title}</h4>
          <p className="mt-0.5 text-2xs leading-relaxed text-slate-500">{description}</p>
        </div>
      </div>
      <div className="space-y-3.5 pl-0.5">{children}</div>
    </div>
  );
}
