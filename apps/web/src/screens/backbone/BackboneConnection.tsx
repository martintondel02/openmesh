import { useEffect, useState } from 'react';
import {
  CheckCircle2,
  Eye,
  EyeOff,
  KeyRound,
  Link2,
  Lock,
  QrCode,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
  Unplug,
} from 'lucide-react';
import type { BackboneConfig } from '../../types/backbone';
import { useAppStore } from '../../store/app';
import {
  BACKBONE_INSTALL_URL,
  connectionStringHasToken,
  connectionStringToken,
  formatAuthMode,
  formatListenAddress,
  maskToken,
  redactConnectionString,
} from '../../lib/backbone';
import { Badge, Button, IconButton, Toggle } from '../../components/ui/primitives';
import { CopyButton } from '../../components/ui/signals';
import { HelpTip } from '../../components/ui/help';
import { ConfirmDialog, Modal } from '../../components/ui/overlay';
import { Callout } from '../../components/ui/forms';
import { QRCode } from '../../components/common/QRCode';

/**
 * Connection-string card: endpoint, masked token, opt-in QR, and the token
 * rotation flow (new token is displayed exactly once, with a kick-all option).
 */
export function BackboneConnection({
  config,
  peersConnected,
  maxPeers,
  connectionString,
  demo,
  showAdvanced = true,
}: {
  config: BackboneConfig;
  peersConnected: number;
  maxPeers: number;
  connectionString: string | null;
  demo: boolean;
  showAdvanced?: boolean;
}) {
  const rotateBackboneToken = useAppStore((state) => state.rotateBackboneToken);
  const kickAllBackbonePeers = useAppStore((state) => state.kickAllBackbonePeers);
  const pushToast = useAppStore((state) => state.pushToast);
  const bridgeUrl = useAppStore((state) => state.settings.bridgeUrl);

  const [revealed, setRevealed] = useState(demo && showAdvanced);
  const [rotateOpen, setRotateOpen] = useState(false);
  const [rotating, setRotating] = useState(false);
  const [newToken, setNewToken] = useState<string | null>(null);
  const [kickAll, setKickAll] = useState(true);

  useEffect(() => {
    if (demo) setRevealed(true);
  }, [demo]);

  const token = connectionString ? connectionStringToken(connectionString) : null;
  const qrValue = connectionString ?? '';
  const panelUrl = `${bridgeUrl.replace(/\/+$/, '')}/backbone`;

  const startRotate = async (): Promise<void> => {
    setRotating(true);
    try {
      const tokenValue = await rotateBackboneToken();
      setNewToken(tokenValue);
      setRevealed(true);
      setKickAll(peersConnected > 0);
    } catch {
      // Store already surfaced a toast; keep the confirm open is not useful.
    } finally {
      setRotating(false);
    }
  };

  const finishRotation = async (): Promise<void> => {
    if (kickAll && peersConnected > 0) {
      const count = await kickAllBackbonePeers();
      pushToast({
        kind: 'info',
        title: 'Peers disconnected',
        message: `${count} peer${count === 1 ? '' : 's'} must reconnect with the new token.`,
      });
    }
    setNewToken(null);
  };

  return (
    <section className="rounded-2xl border border-white/[0.06] bg-white/[0.02]">
      <header className="flex items-center gap-2 border-b border-white/[0.06] px-4 py-3">
        <div className="grid h-8 w-8 place-items-center rounded-lg border border-white/[0.07] bg-white/[0.03] text-slate-400">
          <Link2 className="h-4 w-4" aria-hidden="true" />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-slate-100">Connect a gateway</h3>
          <p className="mt-0.5 text-2xs text-slate-500">Share this with the bridge that should join your mesh.</p>
        </div>
        <Badge tone="sky">OBB/1 · TCP</Badge>
      </header>

      <div className="space-y-4 px-4 py-4">
        <div>
          <p className="text-3xs uppercase tracking-[0.12em] text-slate-500">Public endpoint</p>
          <div className="mt-1 flex items-center gap-1.5">
            <code
              className="flex min-w-0 flex-1 items-baseline gap-1 truncate rounded-lg border border-white/[0.07] bg-ink-950/60 px-2.5 py-1.5 font-mono text-2xs leading-relaxed text-emerald-200/90"
              title={`tcp://${config.public_host || config.bind}:${config.port}`}
            >
              <span className="shrink-0 text-slate-600">tcp://</span>
              <span className="truncate">{config.public_host || config.bind}</span>
              <span className="shrink-0">:{config.port}</span>
            </code>
            <CopyButton value={`${config.public_host || config.bind}:${config.port}`} label="Copy" size="md" variant="surface" />
          </div>
          <p className="mt-1 font-mono text-3xs text-slate-600">listening on {formatListenAddress(config)}</p>
        </div>

        <div>
          <div className="mb-1 flex items-center justify-between gap-2">
            <p className="flex items-center gap-1 text-3xs uppercase tracking-[0.12em] text-slate-500">
              Connection string
              {config.auth === 'open' ? <Badge tone="rose">open</Badge> : null}
              <HelpTip helpKey="backbone.connection" label="Connection string" />
            </p>
            <div className="flex items-center gap-1">
              {connectionString ? (
                <>
                  <CopyButton value={connectionString} label="Copy" size="md" variant="surface" />
                  {showAdvanced ? (
                    <IconButton
                      icon={revealed ? EyeOff : Eye}
                      label={revealed ? 'Hide token' : 'Reveal token'}
                      size="sm"
                      onClick={() => setRevealed((value) => !value)}
                    />
                  ) : null}
                </>
              ) : null}
            </div>
          </div>
          {connectionString ? (
            <code className="block max-h-24 overflow-y-auto break-all rounded-lg border border-white/[0.07] bg-ink-950/60 px-2.5 py-2 font-mono text-3xs leading-relaxed text-slate-400">
              {revealed ? connectionString : redactConnectionString(connectionString)}
            </code>
          ) : (
            <Callout tone="warn" icon={Lock} title="Token not available in the panel">
              The bridge never returns the token through GET endpoints. Rotate it below to get a fresh connection
              string, or copy it from the CLI setup output.
            </Callout>
          )}
        </div>

        <div className="flex flex-col items-center gap-2 rounded-xl border border-white/[0.06] bg-white/[0.02] p-3">
          {connectionString && (revealed || demo) ? (
            <>
              <QRCode value={qrValue} size={168} label="Backbone connection string QR code" />
              <p className="text-3xs text-slate-500">Scan with a phone camera to copy the invite to another bridge.</p>
            </>
          ) : (
            <div className="flex w-full flex-col items-center gap-2 py-4">
              <span className="grid h-12 w-12 place-items-center rounded-xl border border-white/[0.07] bg-white/[0.03] text-slate-500">
                <QrCode className="h-5 w-5" aria-hidden="true" />
              </span>
              {showAdvanced ? (
                <>
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={Eye}
                    onClick={() => setRevealed(true)}
                    disabled={!connectionString}
                  >
                    Reveal to show QR
                  </Button>
                  <p className="text-3xs text-slate-600">The QR embeds the bearer token — reveal only when nobody is watching.</p>
                </>
              ) : (
                <p className="text-3xs text-slate-600">Switch to Advanced to reveal the token and QR.</p>
              )}
            </div>
          )}
        </div>

        {showAdvanced ? (
        <div className="rounded-xl border border-white/[0.06] bg-white/[0.015] p-3">
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <KeyRound className="h-3.5 w-3.5 text-slate-500" aria-hidden="true" />
              <div>
                <p className="text-xs font-medium text-slate-300">Backbone token</p>
                <p className="mt-0.5 flex items-center gap-1.5 truncate font-mono text-3xs text-slate-500">
                  <span className="truncate">
                    {token ? maskToken(token) : config.auth === 'open' ? 'no token (open mode)' : config.token_set ? 'set on the bridge · value hidden' : 'not generated yet'}
                  </span>
                  {demo ? <Badge tone="amber">demo token</Badge> : null}
                </p>
              </div>
            </div>
            <Button size="sm" variant="secondary" icon={RefreshCw} onClick={() => setRotateOpen(true)} disabled={rotating}>
              Rotate
            </Button>
          </div>
          <p className="mt-2 text-3xs leading-relaxed text-slate-600">
            {formatAuthMode(config.auth)}. The token is never logged or returned by GET endpoints; rotating it
            invalidates the old value immediately.
          </p>
        </div>

        ) : null}

        <div className="flex items-center justify-between gap-2 border-t border-white/[0.05] pt-3 text-3xs">
          <span className="min-w-0 truncate text-slate-500">
            {peersConnected}/{maxPeers} peers · panel at{' '}
            <span className="font-mono text-slate-400">{panelUrl.replace(/^https?:\/\//, '')}</span>
          </span>
          <CopyButton value={panelUrl} label="Copy panel URL" size="sm" />
        </div>
      </div>

      {showAdvanced ? (
      <ConfirmDialog
        open={rotateOpen}
        onClose={() => setRotateOpen(false)}
        onConfirm={() => {
          setRotateOpen(false);
          void startRotate();
        }}
        title="Rotate the backbone token?"
        description="The old token stops working immediately and existing peers must be updated with the new connection string. The new token is shown exactly once."
        confirmLabel={rotating ? 'Rotating…' : 'Rotate token'}
        icon={ShieldAlert}
        busy={rotating}
        tone="danger"
      >
        <Callout tone="danger" icon={ShieldAlert} title="All gateways will need the new token">
          Current peers stay connected until they reconnect, which is why the panel offers to disconnect them now.
          Copy the new token before closing the next dialog.
        </Callout>
      </ConfirmDialog>
      ) : null}

      {showAdvanced ? (
      <Modal
        open={Boolean(newToken)}
        onClose={() => void finishRotation()}
        title="New backbone token"
        description="This is the only time the bridge will show it. Copy it before closing."
        icon={KeyRound}
        size="sm"
        dismissible={false}
        footer={
          <Button variant="primary" icon={CheckCircle2} onClick={() => void finishRotation()}>
            I've saved the token
          </Button>
        }
      >
        <div className="space-y-3">
          <Callout tone="danger" icon={ShieldAlert} title="Shown once">
            The bridge only returns tokens from rotation and setup; after this dialog closes the panel cannot show it
            again. Update every remote gateway&apos;s connection string.
          </Callout>
          <div>
            <div className="mb-1 flex items-center justify-between">
              <p className="text-2xs font-medium text-slate-300">New token (base64url, 32 bytes)</p>
              <CopyButton value={newToken ?? ''} label="Copy token" size="md" variant="surface" />
            </div>
            <code className="block break-all rounded-lg border border-amber-400/25 bg-amber-400/[0.06] px-2.5 py-2 font-mono text-xs text-amber-200/90">
              {newToken}
            </code>
          </div>
          <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-3 py-2.5">
            <Toggle
              id="rotate-kick-all"
              label={`Disconnect all connected peers${peersConnected > 0 ? ` (${peersConnected})` : ''}`}
              description="Recommended after rotation so stale sessions cannot keep relaying until they reconnect."
              checked={kickAll && peersConnected > 0}
              disabled={peersConnected === 0}
              onChange={setKickAll}
            />
            {peersConnected === 0 ? <p className="mt-1 text-3xs text-slate-600">No peers are connected right now.</p> : null}
          </div>
          {connectionStringHasToken(connectionString ?? '') && !demo ? (
            <p className="flex items-start gap-1.5 text-3xs leading-relaxed text-slate-600">
              <ShieldCheck className="mt-0.5 h-3 w-3 shrink-0 text-slate-700" aria-hidden="true" />
              The connection card now shows the rotated token. The copy button above copies the raw token, not the full
              invite.
            </p>
          ) : null}
        </div>
      </Modal>
      ) : null}
    </section>
  );
}

/** Small install card shown next to the connection card. */
export function BackboneInstallCard({ onOpenSetup }: { onOpenSetup: () => void }) {
  return (
    <section className="rounded-2xl border border-white/[0.06] bg-white/[0.02] px-4 py-4">
      <div className="flex items-center gap-2">
        <span className="grid h-8 w-8 place-items-center rounded-lg border border-white/[0.07] bg-white/[0.03] text-slate-400">
          <Unplug className="h-4 w-4" aria-hidden="true" />
        </span>
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-slate-100">Install OpenMesh</h3>
          <p className="mt-0.5 text-2xs text-slate-500">Linux, macOS and Windows builds for the other end of the link.</p>
        </div>
      </div>
      <div className="mt-3 rounded-lg border border-white/[0.06] bg-ink-950/50 p-2.5">
        <code className="block break-all font-mono text-3xs leading-relaxed text-slate-400">
          {BACKBONE_INSTALL_URL ? `curl -fsSL ${BACKBONE_INSTALL_URL} | sh` : 'curl -fsSL <install.sh> | sh'}
        </code>
      </div>
      <div className="mt-2 flex items-center justify-between gap-2">
        <p className="text-3xs text-slate-600">Installs the bridge binary + web panel, verifies sha256.</p>
        <Button size="sm" variant="secondary" icon={QrCode} onClick={onOpenSetup} className="shrink-0">
          Setup steps
        </Button>
      </div>
    </section>
  );
}
