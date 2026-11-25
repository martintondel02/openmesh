import { useEffect, useMemo, useState } from 'react';
import {
  MessageSquare,
  Plus,
  QrCode,
  ScanLine,
  Search,
  ShieldCheck,
  Trash2,
  UserPlus,
  Users,
} from 'lucide-react';
import { useAppStore } from '../store/app';
import type { Contact } from '../types/api';
import { buildContactLink, describeLink, isValidIdentity, nodeIdFromIdentity, parseOpenMeshLink } from '../lib/links';
import { formatLastSeen, formatNodeId, shortenNodeId, cn } from '../lib/utils';
import { Avatar, Badge, Button, EmptyState, IconButton, SectionTitle } from '../components/ui/primitives';
import { CopyButton } from '../components/ui/signals';
import { ConfirmDialog, Modal } from '../components/ui/overlay';
import { Callout, Field, TextInput } from '../components/ui/forms';
import { QRCode } from '../components/common/QRCode';

type AddTab = 'link' | 'manual';

export function ContactsScreen() {
  const contacts = useAppStore((state) => state.contacts);
  const nodes = useAppStore((state) => state.nodes);
  const openThread = useAppStore((state) => state.openThread);
  const setScreen = useAppStore((state) => state.setScreen);
  const removeContact = useAppStore((state) => state.removeContact);
  const ownNodeId = useAppStore((state) => state.ownNodeId);
  const ownName = useAppStore((state) => state.ownName);
  const pushToast = useAppStore((state) => state.pushToast);

  const [query, setQuery] = useState('');
  const [addOpen, setAddOpen] = useState(false);
  const [qrOpen, setQrOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Contact | null>(null);
  const [qrContact, setQrContact] = useState<Contact | null>(null);

  const sorted = useMemo(
    () => Object.values(contacts).sort((a, b) => (b.last_seen_ms ?? 0) - (a.last_seen_ms ?? 0)),
    [contacts],
  );
  const filtered = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return sorted;
    return sorted.filter(
      (contact) =>
        contact.name.toLowerCase().includes(normalized) ||
        contact.node_id.toLowerCase().includes(normalized),
    );
  }, [sorted, query]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="border-b border-white/[0.06] px-4 py-3 md:px-6">
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative min-w-[200px] flex-1 md:max-w-md">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-slate-500" aria-hidden="true" />
            <input
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search contacts by name or node id…"
              aria-label="Search contacts"
              className="field-shell h-8 py-0 pl-8 text-xs"
            />
          </div>
          <div className="ml-auto flex items-center gap-2">
            <Button variant="secondary" size="sm" icon={QrCode} onClick={() => setQrOpen(true)}>
              My QR
            </Button>
            <Button variant="primary" size="sm" icon={UserPlus} onClick={() => setAddOpen(true)}>
              Add contact
            </Button>
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4 md:px-6">
        {filtered.length === 0 ? (
          <EmptyState
            icon={UserPlus}
            title={query ? 'No contact matches' : 'No contacts yet'}
            description={
              query
                ? 'Try a different name or the first characters of a node id.'
                : 'Paste an openmesh://contact invite, or add a node by its 72-byte public identity.'
            }
            className="py-16"
            action={
              <Button variant="secondary" icon={Plus} onClick={() => setAddOpen(true)}>
                Add your first contact
              </Button>
            }
          />
        ) : (
          <div className="grid gap-2.5 sm:grid-cols-2 xl:grid-cols-3">
            {filtered.map((contact) => {
              const node = nodes[contact.node_id];
              const online = node ? Date.now() - node.last_seen_ms < 10 * 60_000 : false;
              return (
                <div
                  key={contact.node_id}
                  className="group flex flex-col rounded-xl border border-white/[0.06] bg-white/[0.02] p-3 transition-colors hover:border-white/[0.11] hover:bg-white/[0.03]"
                >
                  <div className="flex items-start gap-2.5">
                    <Avatar name={contact.name} seed={contact.node_id} size="md" online={online} />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5">
                        <p className="truncate text-[13px] font-semibold text-slate-100">{contact.name}</p>
                        <Badge tone="accent">verified</Badge>
                      </div>
                      <p className="mt-0.5 font-mono text-3xs text-slate-500">{shortenNodeId(contact.node_id, 6, 4)}</p>
                      <p className="mt-1 text-3xs text-slate-500">
                        {node ? `${node.hops} hops · ${node.rssi ?? '—'} dBm · ` : ''}
                        seen {formatLastSeen(contact.last_seen_ms)}
                      </p>
                    </div>
                  </div>
                  <div className="mt-2.5 flex items-center gap-1.5 border-t border-white/[0.05] pt-2">
                    <Button
                      size="sm"
                      variant="secondary"
                      icon={MessageSquare}
                      className="flex-1"
                      onClick={() => {
                        openThread({ kind: 'direct', id: contact.node_id });
                        setScreen('chats');
                      }}
                    >
                      Message
                    </Button>
                    <IconButton
                      icon={QrCode}
                      label={`Show QR for ${contact.name}`}
                      size="sm"
                      onClick={() => setQrContact(contact)}
                    />
                    <IconButton
                      icon={Trash2}
                      label={`Delete ${contact.name}`}
                      size="sm"
                      className="hover:text-rose-300"
                      onClick={() => setDeleteTarget(contact)}
                    />
                  </div>
                </div>
              );
            })}
          </div>
        )}

        <div className="mt-5 rounded-xl border border-white/[0.06] bg-white/[0.015] p-4">
          <SectionTitle>How verification works</SectionTitle>
          <p className="mt-1.5 max-w-2xl text-2xs leading-relaxed text-slate-500">
            A contact stores the full 72-byte public identity: an 8-byte self-certifying node id plus the Ed25519
            signing key and X25519 key-exchange key. The node id must hash from both keys, so a relay cannot swap
            identities. Add contacts from an invite link or by pasting the identity out-of-band.
          </p>
        </div>
      </div>

      <AddContactModal
        open={addOpen}
        onClose={() => setAddOpen(false)}
        onAdded={(name) => {
          setAddOpen(false);
          pushToast({ kind: 'success', title: 'Contact added', message: name });
        }}
      />

      <OwnQrModal open={qrOpen} onClose={() => setQrOpen(false)} ownNodeId={ownNodeId} ownName={ownName} />

      <ContactQrDialog contact={qrContact} onClose={() => setQrContact(null)} />

      <ConfirmDialog
        open={Boolean(deleteTarget)}
        onClose={() => setDeleteTarget(null)}
        onConfirm={async () => {
          if (!deleteTarget) return;
          const name = deleteTarget.name;
          await removeContact(deleteTarget.node_id);
          setDeleteTarget(null);
          pushToast({ kind: 'info', title: 'Contact removed', message: name });
        }}
        title={`Delete ${deleteTarget?.name ?? 'contact'}?`}
        description="You will stop verifying this identity. Messages already sent are not affected."
        confirmLabel="Delete contact"
        icon={Trash2}
      />
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Add contact                                                        */
/* ------------------------------------------------------------------ */

function AddContactModal({
  open,
  onClose,
  onAdded,
}: {
  open: boolean;
  onClose: () => void;
  onAdded: (name: string) => void;
}) {
  const addContact = useAppStore((state) => state.addContact);
  const pushToast = useAppStore((state) => state.pushToast);
  const [tab, setTab] = useState<AddTab>('link');
  const [link, setLink] = useState('');
  const [name, setName] = useState('');
  const [identity, setIdentity] = useState('');
  const [nodeId, setNodeId] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setTab('link');
    setLink('');
    setName('');
    setIdentity('');
    setNodeId('');
    setError(null);
    setBusy(false);
  }, [open]);

  const parsed = useMemo(() => (link.trim() ? parseOpenMeshLink(link) : null), [link]);

  const derivedFromIdentity = useMemo(() => {
    if (!identity.trim()) return null;
    return isValidIdentity(identity.trim()) ? nodeIdFromIdentity(identity.trim()) : null;
  }, [identity]);

  const submit = async () => {
    setError(null);
    let finalName = name.trim();
    let finalIdentity: string | null = identity.trim() || null;
    let finalNodeId = nodeId.trim().toLowerCase();

    if (tab === 'link') {
      if (!parsed || parsed.kind === 'error') {
        setError(parsed?.kind === 'error' ? parsed.error : 'Paste an openmesh://contact invite link.');
        return;
      }
      if (parsed.kind !== 'contact') {
        setError('That is a channel invite, not a contact link.');
        return;
      }
      finalName = parsed.name ?? '';
      finalIdentity = parsed.identity;
      finalNodeId = parsed.nodeId ?? parsed.nodeIdFromIdentity ?? '';
      if (!finalName) {
        setError('The invite has no name — add one below first.');
        return;
      }
    }

    if (!finalName) {
      setError('Give the contact a name.');
      return;
    }
    if (finalIdentity && !isValidIdentity(finalIdentity)) {
      setError('The identity must be base64 for 72 bytes (node_id + sign_pk + dh_pk).');
      return;
    }
    const effectiveNodeId = finalNodeId || (finalIdentity ? nodeIdFromIdentity(finalIdentity) ?? '' : '');
    if (!/^[0-9a-f]{16}$/.test(effectiveNodeId)) {
      setError('A valid 16-hex-character node id is required (or an identity to derive it from).');
      return;
    }

    setBusy(true);
    try {
      await addContact({ nodeId: effectiveNodeId, name: finalName, identity: finalIdentity });
      onAdded(finalName);
    } catch (cause) {
      pushToast({ kind: 'error', title: 'Could not add contact', message: cause instanceof Error ? cause.message : String(cause) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Add a contact"
      description="Contacts are identities, not usernames: an invite link or the raw 72-byte public identity."
      icon={UserPlus}
      size="md"
      dismissible={!busy}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" icon={UserPlus} onClick={() => void submit()} loading={busy}>
            Add contact
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <div role="tablist" aria-label="Add contact method" className="flex gap-1 rounded-lg border border-white/[0.07] bg-ink-900/70 p-0.5">
          {[
            { id: 'link' as const, label: 'Invite link', icon: ScanLine },
            { id: 'manual' as const, label: 'Identity', icon: Users },
          ].map((option) => {
            const Icon = option.icon;
            const active = tab === option.id;
            return (
              <button
                key={option.id}
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => setTab(option.id)}
                className={cn(
                  'focus-ring flex flex-1 items-center justify-center gap-1.5 rounded-[7px] px-2 py-1.5 text-xs font-medium transition-colors',
                  active ? 'bg-white/[0.09] text-white' : 'text-slate-400 hover:text-slate-200',
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
            <Field label="Contact invite link" required error={error}>
              {({ id }) => (
                <TextInput
                  id={id}
                  value={link}
                  placeholder="openmesh://contact?node_id=…&name=…&identity=…"
                  className="font-mono text-xs"
                  onChange={(event) => {
                    setLink(event.target.value);
                    setError(null);
                  }}
                />
              )}
            </Field>
            {parsed ? (
              <div className="rounded-xl border border-white/[0.06] bg-white/[0.02] p-3 text-xs">
                <p className="font-medium text-slate-200">{describeLink(parsed)}</p>
                {parsed.kind === 'contact' ? (
                  <div className="mt-1 space-y-0.5 font-mono text-3xs text-slate-500">
                    <p>node_id: {parsed.nodeId ?? parsed.nodeIdFromIdentity ?? '—'}</p>
                    <p className={parsed.identity ? 'text-emerald-300/80' : 'text-amber-300/80'}>
                      identity: {parsed.identity ? 'valid 72-byte identity' : 'missing (cannot verify later)'}
                    </p>
                  </div>
                ) : parsed.kind === 'error' ? (
                  <p className="mt-1 text-2xs text-rose-300">{parsed.error}</p>
                ) : null}
              </div>
            ) : null}
            <Field label="Name override (optional)" hint="Falls back to the name inside the invite.">
              {({ id }) => (
                <TextInput id={id} value={name} onChange={(event) => setName(event.target.value)} placeholder="Anne" />
              )}
            </Field>
          </>
        ) : (
          <div className="space-y-3.5">
            <Field label="Name" required>
              {({ id }) => (
                <TextInput id={id} value={name} onChange={(event) => setName(event.target.value)} placeholder="Anne" />
              )}
            </Field>
            <Field
              label="Node id (16 hex chars)"
              required
              success={derivedFromIdentity ? `Derived from identity: ${derivedFromIdentity}` : undefined}
              hint="The first 8 bytes of the identity. You can paste it on its own if you don't have the full identity."
            >
              {({ id }) => (
                <TextInput
                  id={id}
                  value={nodeId}
                  onChange={(event) => setNodeId(event.target.value.trim())}
                  placeholder="6b1f9a2c4d7e8031"
                  className="font-mono text-xs"
                />
              )}
            </Field>
            <Field
              label="Public identity (base64, 72 bytes)"
              hint="node_id(8) || Ed25519 sign_pk(32) || X25519 dh_pk(32)."
              error={error}
            >
              {({ id }) => (
                <TextInput
                  id={id}
                  value={identity}
                  onChange={(event) => {
                    setIdentity(event.target.value.trim());
                    setError(null);
                  }}
                  placeholder="paste 96 base64 characters"
                  className="font-mono text-xs"
                />
              )}
            </Field>
            <Callout tone="info" icon={ShieldCheck}>
              Without the full identity the contact is addressable but not cryptographically verifiable. Verify the
              identity out-of-band before trusting a node id alone.
            </Callout>
          </div>
        )}
      </div>
    </Modal>
  );
}

/* ------------------------------------------------------------------ */
/* Contact QR                                                         */
/* ------------------------------------------------------------------ */

function ContactQrDialog({ contact, onClose }: { contact: Contact | null; onClose: () => void }) {
  const requestContactQr = useAppStore((state) => state.requestContactQr);
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!contact) return;
    let cancelled = false;
    setUrl(null);
    const fallback =
      contact.identity && isValidIdentity(contact.identity)
        ? buildContactLink({ nodeId: contact.node_id, name: contact.name, identity: contact.identity })
        : null;
    requestContactQr(contact.node_id)
      .then((invite) => {
        if (!cancelled) setUrl(invite);
      })
      .catch(() => {
        if (!cancelled) setUrl(fallback);
      });
    return () => {
      cancelled = true;
    };
  }, [contact, requestContactQr]);

  return (
    <Modal
      open={Boolean(contact)}
      onClose={onClose}
      title={contact ? `Invite for ${contact.name}` : 'Contact invite'}
      description="Share this to let another device verify and message this contact."
      icon={QrCode}
      size="sm"
      footer={
        <Button variant="ghost" onClick={onClose}>
          Close
        </Button>
      }
    >
      <div className="flex flex-col items-center gap-3">
        {url ? (
          <>
            <QRCode value={url} size={196} label="Contact invite QR code" />
            <div className="w-full">
              <div className="mb-1 flex items-center justify-between">
                <p className="text-xs font-medium text-slate-300">{contact?.name}</p>
                <CopyButton value={url} label="Copy invite" size="md" />
              </div>
              <code className="block break-all rounded-lg border border-white/[0.07] bg-ink-950/70 px-2.5 py-2 font-mono text-3xs leading-relaxed text-slate-500">
                {url}
              </code>
            </div>
          </>
        ) : (
          <div className="grid h-[196px] w-[196px] place-items-center rounded-2xl border border-white/[0.07] bg-white/[0.02] text-2xs text-slate-500">
            {contact ? 'No identity to share' : 'Loading invite…'}
          </div>
        )}
      </div>
    </Modal>
  );
}

/* ------------------------------------------------------------------ */
/* Own QR                                                             */
/* ------------------------------------------------------------------ */

function OwnQrModal({
  open,
  onClose,
  ownNodeId,
  ownName,
}: {
  open: boolean;
  onClose: () => void;
  ownNodeId: string | null;
  ownName: string;
}) {
  const requestContactQr = useAppStore((state) => state.requestContactQr);
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open || !ownNodeId) return;
    let cancelled = false;
    setUrl(null);
    setError(null);
    setLoading(true);
    requestContactQr(ownNodeId)
      .then((invite) => {
        if (!cancelled) setUrl(invite);
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setError(
            cause instanceof Error
              ? cause.message
              : 'The bridge does not expose your own contact invite yet.',
          );
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, ownNodeId, requestContactQr]);

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Your contact QR"
      description="Someone can add you by scanning this, or by pasting the invite link."
      icon={QrCode}
      size="sm"
      footer={
        <Button variant="ghost" onClick={onClose}>
          Close
        </Button>
      }
    >
      <div className="flex flex-col items-center gap-3">
        {loading ? (
          <div className="grid h-[196px] w-[196px] place-items-center rounded-2xl border border-white/[0.07] bg-white/[0.02] text-2xs text-slate-500">
            Building invite…
          </div>
        ) : url ? (
          <>
            <QRCode value={url} size={196} label={`Contact QR for ${ownName}`} />
            <div className="w-full">
              <div className="mb-1 flex items-center justify-between">
                <p className="text-xs font-medium text-slate-300">{ownName}</p>
                <CopyButton value={url} label="Copy invite" size="md" />
              </div>
              <code className="block break-all rounded-lg border border-white/[0.07] bg-ink-950/70 px-2.5 py-2 font-mono text-3xs leading-relaxed text-slate-500">
                {url}
              </code>
            </div>
          </>
        ) : (
          <div className="w-full space-y-3 py-4 text-center">
            <p className="text-xs text-slate-400">{error ?? 'No invite available yet.'}</p>
            <p className="text-2xs text-slate-600">
              The web client has no camera scanner — paste the contact link on the receiving device instead.
            </p>
          </div>
        )}
        {ownNodeId ? (
          <div className="w-full border-t border-white/[0.06] pt-3 text-center">
            <p className="text-3xs uppercase tracking-[0.12em] text-slate-600">Your node id</p>
            <p className="mt-1 break-all font-mono text-3xs text-slate-400">{formatNodeId(ownNodeId)}</p>
          </div>
        ) : null}
        <p className="text-center text-3xs leading-relaxed text-slate-600">
          Scan note: the Android app supports QR scanning. Browser builds deliberately ship without camera permissions.
        </p>
      </div>
    </Modal>
  );
}
