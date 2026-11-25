import { useEffect, useMemo, useState } from 'react';
import {
  CheckCircle2,
  Hash,
  KeyRound,
  Link2,
  Loader2,
  QrCode,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  TriangleAlert,
  Users,
} from 'lucide-react';
import { useAppStore } from '../../store/app';
import type { Channel } from '../../types/api';
import { buildChannelLink, isValidPsk, parseOpenMeshLink } from '../../lib/links';
import { cn, formatNodeId } from '../../lib/utils';
import { shortChannelId } from './chat-utils';
import { Badge, Button } from '../../components/ui/primitives';
import { Modal } from '../../components/ui/overlay';
import { Callout, Field, TextInput } from '../../components/ui/forms';
import { CopyButton } from '../../components/ui/signals';
import { QRCode } from '../../components/common/QRCode';

/* ------------------------------------------------------------------ */
/* Create channel                                                     */
/* ------------------------------------------------------------------ */

export function CreateChannelModal({
  open,
  onClose,
  onCreated,
}: {
  open: boolean;
  onClose: () => void;
  onCreated: (channelId: string) => void;
}) {
  const createChannel = useAppStore((state) => state.createChannel);
  const requestChannelInvite = useAppStore((state) => state.requestChannelInvite);
  const channels = useAppStore((state) => state.channels);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [created, setCreated] = useState<{ channelId: string; name: string; psk: string | null; inviteUrl: string } | null>(null);

  useEffect(() => {
    if (!open) return;
    setName('');
    setError(null);
    setBusy(false);
    setCreated(null);
  }, [open]);

  const duplicate = useMemo(
    () => Object.values(channels).some((channel) => channel.name.toLowerCase() === name.trim().toLowerCase()),
    [channels, name],
  );

  const submit = async () => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError('Give the channel a name.');
      return;
    }
    if (trimmed.length > 48) {
      setError('Keep the name under 48 characters.');
      return;
    }
    if (duplicate) {
      setError('You already have a channel with that name.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const result = await createChannel(trimmed);
      let inviteUrl = buildChannelLink({ channelId: result.channel_id, name: result.name, psk: result.psk ?? null });
      try {
        inviteUrl = await requestChannelInvite(result.channel_id);
      } catch {
        // The bridge may not support channel_invite yet; the client-built link is equivalent.
      }
      setCreated({
        channelId: result.channel_id,
        name: result.name,
        psk: result.psk ?? null,
        inviteUrl,
      });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not create the channel');
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={created ? 'Channel created' : 'Create a channel'}
      description={
        created
          ? 'Share the invite now — the channel key is only shown once.'
          : 'Channels are group chats encrypted with a shared 256-bit key (PSK). Every message is Ed25519-signed.'
      }
      icon={created ? CheckCircle2 : Users}
      size="md"
      dismissible={!busy}
      footer={
        created ? (
          <>
            <Button variant="secondary" onClick={() => void navigator.clipboard?.writeText(created.inviteUrl).catch(() => undefined)}>
              Copy invite
            </Button>
            <Button variant="primary" onClick={() => onCreated(created.channelId)}>
              Open channel
            </Button>
          </>
        ) : (
          <>
            <Button variant="ghost" onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" icon={Sparkles} onClick={() => void submit()} loading={busy} disabled={!name.trim()}>
              Create channel
            </Button>
          </>
        )
      }
    >
      {created ? (
        <div className="space-y-4">
          <div className="flex items-center gap-3 rounded-xl border border-emerald-400/20 bg-emerald-400/[0.06] px-3.5 py-3">
            <span className="grid h-10 w-10 place-items-center rounded-xl bg-emerald-400/15 text-emerald-300">
              <Hash className="h-5 w-5" aria-hidden="true" />
            </span>
            <div className="min-w-0">
              <p className="text-sm font-semibold text-white">{created.name}</p>
              <p className="font-mono text-3xs text-slate-400">{formatNodeId(created.channelId)}</p>
            </div>
            <div className="ml-auto grid h-[88px] w-[88px] place-items-center rounded-lg bg-slate-50 p-1">
              <QRCode value={created.inviteUrl} size={80} label="Channel invite QR code" />
            </div>
          </div>

          <div>
            <div className="mb-1 flex items-center justify-between">
              <p className="text-xs font-medium text-slate-300">Channel invite link</p>
              <CopyButton value={created.inviteUrl} label="Copy link" size="md" />
            </div>
            <code className="block break-all rounded-lg border border-white/[0.07] bg-ink-950/70 px-2.5 py-2 font-mono text-3xs leading-relaxed text-slate-400">
              {created.inviteUrl}
            </code>
            <p className="mt-1 text-2xs text-slate-500">
              Paste this into another OpenMesh client. It contains the channel key — treat it like a password.
            </p>
          </div>

          {created.psk ? (
            <div>
              <div className="mb-1 flex items-center justify-between">
                <p className="flex items-center gap-1.5 text-xs font-medium text-slate-300">
                  <KeyRound className="h-3.5 w-3.5 text-amber-300" aria-hidden="true" />
                  Pre-shared key
                  <Badge tone="amber">shown once</Badge>
                </p>
                <CopyButton value={created.psk} label="Copy PSK" size="md" />
              </div>
              <code className="block break-all rounded-lg border border-amber-400/20 bg-amber-400/[0.06] px-2.5 py-2 font-mono text-3xs text-amber-200/90">
                {created.psk}
              </code>
            </div>
          ) : null}

          <Callout tone="warn" icon={ShieldAlert} title="Store the invite somewhere safe">
            The bridge never returns the PSK again. If you lose the invite link, members can keep chatting but new
            devices cannot join.
          </Callout>
        </div>
      ) : (
        <div className="space-y-4">
          <Field
            label="Channel name"
            required
            hint="Visible to anyone who can read the channel. Max 48 characters."
            error={error}
          >
            {({ id, describedBy }) => (
              <div className="relative">
                <span className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-slate-500">
                  <Hash className="h-3.5 w-3.5" aria-hidden="true" />
                </span>
                <TextInput
                  id={id}
                  aria-describedby={describedBy}
                  value={name}
                  autoFocus
                  maxLength={48}
                  placeholder="nordmarka-tur"
                  className="pl-8"
                  onChange={(event) => {
                    setName(event.target.value);
                    setError(null);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') void submit();
                  }}
                />
              </div>
            )}
          </Field>

          <div className="rounded-xl border border-white/[0.06] bg-white/[0.02] p-3">
            <p className="flex items-center gap-1.5 text-xs font-medium text-slate-300">
              <ShieldCheck className="h-3.5 w-3.5 text-emerald-300" aria-hidden="true" />
              A 256-bit PSK will be generated on the bridge
            </p>
            <p className="mt-1 text-2xs leading-relaxed text-slate-500">
              The key is derived with HKDF-SHA256 from your PSK and the channel name. The bridge will show it exactly
              once, right after creation — we&apos;ll also hand you an <code className="font-mono">openmesh://channel</code>{' '}
              invite with the key embedded.
            </p>
          </div>

          <Callout tone="info" icon={KeyRound}>
            Group messages are signed by the sender&apos;s Ed25519 identity, so relays cannot forge authorship even
            though they can see that a channel is active.
          </Callout>
        </div>
      )}
    </Modal>
  );
}

/* ------------------------------------------------------------------ */
/* Join channel                                                       */
/* ------------------------------------------------------------------ */

type JoinTab = 'link' | 'manual';

export function JoinChannelModal({
  open,
  onClose,
  onJoined,
}: {
  open: boolean;
  onClose: () => void;
  onJoined: (channelId: string) => void;
}) {
  const joinChannel = useAppStore((state) => state.joinChannel);
  const [tab, setTab] = useState<JoinTab>('link');
  const [link, setLink] = useState('');
  const [name, setName] = useState('');
  const [psk, setPsk] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setTab('link');
    setLink('');
    setName('');
    setPsk('');
    setError(null);
    setBusy(false);
  }, [open]);

  const parsed = useMemo(() => (link.trim() ? parseOpenMeshLink(link) : null), [link]);

  const submit = async () => {
    setError(null);
    let joinName = name.trim();
    let joinPsk = psk.trim();
    if (tab === 'link') {
      if (!parsed || parsed.kind === 'error') {
        setError(parsed?.kind === 'error' ? parsed.error : 'Paste an openmesh://channel invite link.');
        return;
      }
      if (parsed.kind !== 'channel') {
        setError('That is a contact link, not a channel invite.');
        return;
      }
      joinName = parsed.name ?? '';
      joinPsk = parsed.psk ?? '';
      if (!joinName) {
        setError('The invite has no channel name — ask for a name and PSK instead.');
        return;
      }
      if (!joinPsk) {
        setError('The invite has no PSK. Ask the owner for a link with the key, or use the manual tab.');
        return;
      }
    }
    if (!joinName) {
      setError('Enter the channel name.');
      return;
    }
    if (!isValidPsk(joinPsk)) {
      setError('The PSK must be base64 for 32 bytes (256 bits).');
      return;
    }
    setBusy(true);
    try {
      const result = await joinChannel(joinName, joinPsk);
      onJoined(result.channel_id);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not join the channel');
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Join a channel"
      description="Paste an invite link, or enter the channel name and its 256-bit key."
      icon={Link2}
      size="md"
      dismissible={!busy}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" icon={Hash} onClick={() => void submit()} loading={busy}>
            Join channel
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <div role="tablist" aria-label="Join method" className="flex gap-1 rounded-lg border border-white/[0.07] bg-ink-900/70 p-0.5">
          {(
            [
              { id: 'link' as const, label: 'Invite link', icon: Link2 },
              { id: 'manual' as const, label: 'Name + PSK', icon: KeyRound },
            ]
          ).map((option) => {
            const Icon = option.icon;
            const selected = tab === option.id;
            return (
              <button
                key={option.id}
                type="button"
                role="tab"
                aria-selected={selected}
                onClick={() => setTab(option.id)}
                className={cn(
                  'focus-ring flex flex-1 items-center justify-center gap-1.5 rounded-[7px] px-2 py-1.5 text-xs font-medium transition-colors',
                  selected ? 'bg-white/[0.09] text-white' : 'text-slate-400 hover:text-slate-200',
                )}
              >
                <Icon className="h-3.5 w-3.5" aria-hidden="true" />
                {option.label}
              </button>
            );
          })}
        </div>

        {tab === 'link' ? (
          <>
            <Field label="Invite link" required error={error}>
              {({ id }) => (
                <TextInput
                  id={id}
                  value={link}
                  placeholder="openmesh://channel?channel_id=…&name=…&psk=…"
                  onChange={(event) => {
                    setLink(event.target.value);
                    setError(null);
                  }}
                  className="font-mono text-xs"
                />
              )}
            </Field>
            {parsed && parsed.kind === 'channel' ? (
              <div className="rounded-xl border border-white/[0.06] bg-white/[0.02] p-3 text-xs">
                <p className="font-medium text-slate-200">
                  {parsed.name ?? 'Unnamed channel'}
                  <span className="ml-2 font-mono text-3xs text-slate-500">
                    {parsed.channelId ? formatNodeId(parsed.channelId) : ''}
                  </span>
                </p>
                <p className="mt-1 text-2xs text-slate-500">
                  {parsed.psk ? (
                    <span className="flex items-center gap-1 text-emerald-300/80">
                      <ShieldCheck className="h-3 w-3" aria-hidden="true" />
                      Contains a valid 256-bit channel key
                    </span>
                  ) : (
                    <span className="flex items-center gap-1 text-amber-300/80">
                      <TriangleAlert className="h-3 w-3" aria-hidden="true" />
                      No PSK in this link — you will need the key from the channel owner
                    </span>
                  )}
                </p>
              </div>
            ) : null}
            <Callout tone="info" icon={QrCode}>
              Invite links embed the channel key. Only share them over a channel you trust (in person, or encrypted
              direct message).
            </Callout>
          </>
        ) : null}

        {tab === 'manual' ? (
          <div className="space-y-3.5">
            <Field label="Channel name" required hint="Must match the name the channel was created with.">
              {({ id }) => (
                <TextInput
                  id={id}
                  value={name}
                  maxLength={48}
                  placeholder="nordmarka-tur"
                  onChange={(event) => {
                    setName(event.target.value);
                    setError(null);
                  }}
                />
              )}
            </Field>
            <Field label="Pre-shared key (base64, 32 bytes)" required error={error} hint="Example: 32 bytes encoded as 44 base64 characters.">
              {({ id, describedBy }) => (
                <TextInput
                  id={id}
                  aria-describedby={describedBy}
                  value={psk}
                  placeholder="q83v…"
                  className="font-mono text-xs"
                  onChange={(event) => {
                    setPsk(event.target.value.trim());
                    setError(null);
                  }}
                />
              )}
            </Field>
          </div>
        ) : null}
      </div>
    </Modal>
  );
}

/* ------------------------------------------------------------------ */
/* Channel invite (share existing channel)                            */
/* ------------------------------------------------------------------ */

export function ChannelInviteModal({
  open,
  channel,
  onClose,
}: {
  open: boolean;
  channel: Channel | null;
  onClose: () => void;
}) {
  const requestChannelInvite = useAppStore((state) => state.requestChannelInvite);
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [shared, setShared] = useState(false);

  useEffect(() => {
    if (!open || !channel) return;
    let cancelled = false;
    setUrl(null);
    setError(null);
    setShared(false);
    setLoading(true);
    requestChannelInvite(channel.channel_id)
      .then((invite) => {
        if (!cancelled) setUrl(invite);
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(cause instanceof Error ? cause.message : 'Could not fetch an invite');
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, channel, requestChannelInvite]);

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Invite to channel"
      description={channel ? `Share “${channel.name}” — the link contains the channel key.` : undefined}
      icon={QrCode}
      size="md"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Close
          </Button>
          {url ? (
            <Button
              variant="primary"
              icon={CheckCircle2}
              onClick={async () => {
                if (!url) return;
                try {
                  if (navigator.share) {
                    await navigator.share({ title: `Join ${channel?.name ?? 'channel'} on OpenMesh`, text: url });
                  } else {
                    await navigator.clipboard?.writeText(url);
                  }
                  setShared(true);
                } catch {
                  // User cancelled the share sheet.
                }
              }}
            >
              {shared ? 'Shared' : 'Share invite'}
            </Button>
          ) : null}
        </>
      }
    >
      {loading ? (
        <div className="flex items-center justify-center gap-2 py-10 text-xs text-slate-400">
          <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          Asking the bridge for an invite…
        </div>
      ) : error ? (
        <Callout tone="danger" icon={TriangleAlert} title="No invite available">
          {error}
        </Callout>
      ) : url ? (
        <div className="flex flex-col items-center gap-4">
          <QRCode value={url} size={196} label={`Invite QR for channel ${channel?.name ?? ''}`} />
          <div className="w-full">
            <div className="mb-1 flex items-center justify-between">
              <p className="text-xs font-medium text-slate-300">
                Invite link
                {channel ? <span className="ml-2 font-mono text-3xs text-slate-500">{shortChannelId(channel.channel_id)}</span> : null}
              </p>
              <CopyButton value={url} label="Copy" size="md" />
            </div>
            <code className="block break-all rounded-lg border border-white/[0.07] bg-ink-950/70 px-2.5 py-2 font-mono text-3xs leading-relaxed text-slate-400">
              {url}
            </code>
          </div>
          <Callout tone="warn" icon={ShieldAlert}>
            The link embeds the PSK. Anyone who gets it can read and write to this channel — send it over a trusted
            path only.
          </Callout>
        </div>
      ) : null}
    </Modal>
  );
}
