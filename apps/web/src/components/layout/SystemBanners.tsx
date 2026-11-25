import { useState } from 'react';
import { AlertTriangle, BookOpen, KeyRound, ShieldAlert, WifiOff, X } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useAppStore } from '../../store/app';
import { cn } from '../../lib/utils';

interface Banner {
  id: string;
  tone: 'amber' | 'rose';
  icon: LucideIcon;
  title: string;
  body: string;
  helpTopic: string;
  actionLabel?: string;
  onAction?: () => void;
}

function isLoopbackUrl(url: string): boolean {
  try {
    const host = new URL(url).hostname;
    return host === '127.0.0.1' || host === 'localhost' || host === '::1' || host === '[::1]';
  } catch {
    return true;
  }
}

/** Insecurity / radio banners required by the v0.3 UX spec (§4.5). */
export function SystemBanners() {
  const [dismissed, setDismissed] = useState<Record<string, boolean>>({});
  const mode = useAppStore((state) => state.mode);
  const settings = useAppStore((state) => state.settings);
  const connection = useAppStore((state) => state.connection);
  const bridgeStatus = useAppStore((state) => state.bridgeStatus);
  const hello = useAppStore((state) => state.hello);
  const backboneConfig = useAppStore((state) => state.backboneConfig);
  const setScreen = useAppStore((state) => state.setScreen);
  const openHelp = useAppStore((state) => state.openHelp);

  const banners: Banner[] = [];

  if (backboneConfig?.enabled && backboneConfig.auth === 'open') {
    banners.push({
      id: 'backbone-open',
      tone: 'rose',
      icon: ShieldAlert,
      title: 'Backbone accepts open connections',
      body: 'Token auth is off, so any peer that reaches the port may relay traffic. Use a token unless the port is on a trusted private network.',
      helpTopic: 'backbone',
      actionLabel: 'Open Backbone',
      onAction: () => setScreen('backbone'),
    });
  }

  if (mode === 'live' && !settings.token) {
    banners.push({
      id: 'token-missing',
      tone: 'amber',
      icon: KeyRound,
      title: 'No bridge token in this browser',
      body: 'Without a token, the local API is unprotected. Paste the token from the bridge token file in Settings.',
      helpTopic: 'security',
      actionLabel: 'Open Settings',
      onAction: () => setScreen('settings'),
    });
  }

  if (mode === 'live' && !isLoopbackUrl(settings.bridgeUrl)) {
    banners.push({
      id: 'lan-api',
      tone: 'amber',
      icon: AlertTriangle,
      title: 'The bridge API is reachable over the network',
      body: `This panel is talking to ${settings.bridgeUrl.replace(/^https?:\/\//, '')}. Prefer loopback or a private tunnel so other machines cannot reach the API.`,
      helpTopic: 'security',
      actionLabel: 'Connection settings',
      onAction: () => setScreen('settings'),
    });
  }

  const radioMissing = (() => {
    if (mode !== 'live' || connection !== 'connected') return false;
    if (bridgeStatus) return !bridgeStatus.radio.connected;
    if (hello?.radios?.length) return hello.radios.every((radio) => !radio.connected);
    return false;
  })();
  if (radioMissing) {
    banners.push({
      id: 'no-radio',
      tone: 'amber',
      icon: WifiOff,
      title: 'The radio is not answering',
      body: 'Messages queue until a radio is connected. Check the USB device, antenna and power, then retry the connection.',
      helpTopic: 'troubleshooting',
      actionLabel: 'Radio help',
      onAction: () => openHelp('troubleshooting'),
    });
  }

  const visible = banners.filter((banner) => !dismissed[banner.id]);
  if (!visible.length) return null;

  return (
    <div className="relative z-20 shrink-0 border-b border-white/[0.06] bg-ink-925/70">
      {visible.map((banner) => {
        const Icon = banner.icon;
        return (
          <div
            key={banner.id}
            role="status"
            className={cn(
              'flex items-start gap-2 px-3 py-2 text-2xs md:px-5',
              banner.tone === 'rose' ? 'text-rose-200' : 'text-amber-200',
            )}
          >
            <Icon className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
            <div className="min-w-0 flex-1">
              <p className="font-medium leading-snug">{banner.title}</p>
              <p className="mt-0.5 hidden text-3xs leading-relaxed opacity-80 sm:block">{banner.body}</p>
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              <button
                type="button"
                onClick={() => openHelp(banner.helpTopic)}
                className="focus-ring inline-flex h-5 items-center gap-1 rounded px-1.5 text-3xs font-medium hover:bg-white/[0.06]"
              >
                <BookOpen className="h-2.5 w-2.5" aria-hidden="true" />
                Help
              </button>
              {banner.actionLabel && banner.onAction ? (
                <button
                  type="button"
                  onClick={banner.onAction}
                  className="focus-ring inline-flex h-5 items-center rounded px-1.5 text-3xs font-medium hover:bg-white/[0.06]"
                >
                  {banner.actionLabel}
                </button>
              ) : null}
              <button
                type="button"
                aria-label="Dismiss banner"
                onClick={() => setDismissed((current) => ({ ...current, [banner.id]: true }))}
                className="focus-ring grid h-5 w-5 place-items-center rounded text-current/60 hover:bg-white/[0.06]"
              >
                <X className="h-3 w-3" aria-hidden="true" />
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
