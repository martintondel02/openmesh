import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  BookOpen,
  Check,
  ChevronRight,
  Clock,
  ExternalLink,
  Link2,
  Loader2,
  Search,
  ThumbsDown,
  ThumbsUp,
  X,
} from 'lucide-react';
import { useAppStore } from '../store/app';
import { Badge, Button, EmptyState, IconButton, SegmentedControl } from '../components/ui/primitives';
import { TextInput } from '../components/ui/forms';
import { copyText } from '../components/ui/signals';
import { HelpLink, HelpTip } from '../components/ui/help';
import { cn } from '../lib/utils';
import {
  audienceLabel,
  extractToc,
  findHelpTopic,
  loadHelpCorpus,
  loadHelpIndex,
  loadHelpTopic,
  readingTimeLabel,
  relatedTopics,
  renderMarkdown,
  searchHelpTopics,
} from '../lib/help';
import type { HelpIndex, HelpTopic, HelpTopicMeta } from '../types/help';
import type { UiMode } from '../lib/mode';

const USEFUL_KEY = 'openmesh.help.useful.v1';

function loadUseful(): Record<string, 'yes' | 'no'> {
  try {
    const raw = window.localStorage.getItem(USEFUL_KEY);
    const parsed = raw ? (JSON.parse(raw) as unknown) : null;
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, 'yes' | 'no'>) : {};
  } catch {
    return {};
  }
}

export function HelpCenterScreen() {
  const uiMode = useAppStore((state) => state.uiMode);
  const mode = useAppStore((state) => state.mode);
  const settings = useAppStore((state) => state.settings);
  const connection = useAppStore((state) => state.connection);
  const helpSlug = useAppStore((state) => state.helpSlug);
  const helpAnchor = useAppStore((state) => state.helpAnchor);
  const setHelpTopic = useAppStore((state) => state.setHelpTopic);
  const pushToast = useAppStore((state) => state.pushToast);

  const [index, setIndex] = useState<HelpIndex | null>(null);
  const [bodies, setBodies] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [audience, setAudience] = useState<'beginner' | 'advanced'>(uiMode === 'advanced' ? 'advanced' : 'beginner');
  const [topic, setTopic] = useState<HelpTopic | null>(null);
  const [topicLoading, setTopicLoading] = useState(false);
  const [topicError, setTopicError] = useState<string | null>(null);
  const [useful, setUseful] = useState<Record<string, 'yes' | 'no'>>(() => (typeof window === 'undefined' ? {} : loadUseful()));
  const articleRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const helpOptions = useMemo(
    () => ({
      mode: mode === 'demo' ? ('demo' as const) : ('live' as const),
      apiBaseUrl: settings.bridgeUrl,
      token: settings.token || null,
      bundleBaseUrl: import.meta.env.BASE_URL,
    }),
    [mode, settings.bridgeUrl, settings.token],
  );

  useEffect(() => {
    // Wait for boot so demo mode never probes the live bridge API.
    if (connection !== 'demo' && connection !== 'connected') return;
    let cancelled = false;
    setLoading(true);
    setLoadError(null);
    loadHelpIndex(helpOptions)
      .then((next) => {
        if (cancelled) return;
        setIndex(next);
        setLoading(false);
        void loadHelpCorpus(next, helpOptions).then((corpus) => {
          if (!cancelled) setBodies((current) => ({ ...current, ...corpus }));
        });
      })
      .catch(() => {
        if (cancelled) return;
        setLoadError('The help index could not be loaded. Check the bridge connection, then try again.');
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [helpOptions, connection]);

  useEffect(() => {
    if (uiMode === 'advanced' && audience === 'beginner') setAudience('advanced');
    if (uiMode === 'beginner' && audience === 'advanced') setAudience('beginner');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [uiMode]);

  const meta = useMemo(() => (index ? findHelpTopic(index.topics, helpSlug) : null), [index, helpSlug]);

  useEffect(() => {
    if (!meta) {
      setTopic(null);
      setTopicError(null);
      return;
    }
    if (bodies[meta.slug]) {
      setTopic({ ...meta, markdown: bodies[meta.slug]! });
      setTopicError(null);
      return;
    }
    let cancelled = false;
    setTopicLoading(true);
    setTopicError(null);
    loadHelpTopic(meta, helpOptions)
      .then((loaded) => {
        if (cancelled) return;
        setTopic(loaded);
        if (loaded) setBodies((current) => ({ ...current, [meta.slug]: loaded.markdown }));
        setTopicLoading(false);
      })
      .catch(() => {
        if (cancelled) return;
        setTopicError('That help topic could not be loaded.');
        setTopicLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [meta, bodies, helpOptions]);

  const filtered = useMemo(() => {
    if (!index) return [];
    return searchHelpTopics(index.topics, { query, audience, bodies });
  }, [index, query, audience, bodies]);

  const visibleTopics = useMemo(() => {
    if (!index) return [];
    return index.topics.filter((entry) => entry.audience === 'both' || entry.audience === audience);
  }, [index, audience]);

  const toc = useMemo(() => (topic ? extractToc(topic.markdown) : []), [topic]);
  const related = useMemo(() => (index && meta ? relatedTopics(index, meta) : []), [index, meta]);
  const position = useMemo(() => {
    if (!index || !meta) return { previous: null as HelpTopicMeta | null, next: null as HelpTopicMeta | null };
    const list = visibleTopics.length ? visibleTopics : index.topics;
    const at = list.findIndex((entry) => entry.slug === meta.slug);
    return { previous: at > 0 ? list[at - 1]! : null, next: at >= 0 && at < list.length - 1 ? list[at + 1]! : null };
  }, [index, meta, visibleTopics]);

  const markUseful = useCallback(
    (value: 'yes' | 'no') => {
      if (!meta) return;
      const next = { ...useful, [meta.slug]: value };
      setUseful(next);
      try {
        window.localStorage.setItem(USEFUL_KEY, JSON.stringify(next));
      } catch {
        // ignore
      }
    },
    [meta, useful],
  );

  useEffect(() => {
    if (!topic || !helpAnchor) return;
    const timer = setTimeout(() => {
      const target = articleRef.current?.querySelector<HTMLElement>(`#${CSS.escape(helpAnchor)}`);
      target?.scrollIntoView({ behavior: 'smooth', block: 'start' });
      target?.focus?.();
    }, 80);
    return () => clearTimeout(timer);
  }, [topic, helpAnchor]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  const openTopic = (slug: string | null, anchor?: string | null) => setHelpTopic(slug, anchor ?? null);

  const onCopyLink = async () => {
    const url = `${window.location.origin}${meta ? `/help/${meta.slug}` : '/help'}`;
    const ok = await copyText(url);
    pushToast({
      kind: ok ? 'success' : 'warn',
      title: ok ? 'Help link copied' : 'Could not copy the link',
      message: ok ? 'Paste it anywhere to open this topic.' : 'Copy the address bar instead.',
    });
  };

  const onMarkdownClick = (event: React.MouseEvent<HTMLDivElement>) => {
    const target = (event.target as HTMLElement).closest<HTMLAnchorElement>('a[data-help-slug]');
    if (!target) return;
    event.preventDefault();
    openTopic(target.dataset.helpSlug ?? null, target.dataset.helpAnchor || null);
  };

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center text-slate-500">
        <span className="inline-flex items-center gap-2 text-xs">
          <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          Loading the handbook…
        </span>
      </div>
    );
  }

  if (loadError || !index) {
    return (
      <div className="flex h-full items-center justify-center">
        <EmptyState
          icon={BookOpen}
          title="The handbook is unavailable"
          description={loadError ?? 'The help bundle could not be read.'}
          action={
            <Button
              variant="primary"
              onClick={() => {
                setLoading(true);
                setLoadError(null);
                window.location.reload();
              }}
            >
              Reload help
            </Button>
          }
          helpTopic="troubleshooting"
        />
      </div>
    );
  }

  const onHelpLink = (topic: HelpTopicMeta) => openTopic(topic.slug);

  return (
    <div className="flex min-h-0 flex-1 flex-col lg:flex-row">
      {/* topic list */}
      <aside
        className={cn(
          'flex min-h-0 shrink-0 flex-col border-b border-white/[0.06] lg:w-[330px] lg:border-b-0 lg:border-r',
          helpSlug ? 'hidden lg:flex' : 'flex',
        )}
        aria-label="Help topics"
      >
        <div className="space-y-3 border-b border-white/[0.06] px-4 py-4">
          <div className="flex items-center justify-between gap-2">
            <div>
              <h2 className="text-sm font-semibold text-white">Help Center</h2>
              <p className="mt-0.5 text-2xs text-slate-500">Works offline. {index.topics.length} topics.</p>
            </div>
            <HelpTip helpKey="common.help" label="Help Center" />
          </div>
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-slate-500" aria-hidden="true" />
            <TextInput
              ref={searchRef}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search help…"
              aria-label="Search help topics"
              className="pl-8 pr-8"
            />
            {query ? (
              <IconButton
                icon={X}
                label="Clear search"
                size="sm"
                onClick={() => setQuery('')}
                className="absolute right-1 top-1/2 -translate-y-1/2"
              />
            ) : (
              <span className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-3xs text-slate-600">⌘K</span>
            )}
          </div>
          <SegmentedControl
            value={audience}
            options={[
              { value: 'beginner', label: 'Beginner' },
              { value: 'advanced', label: 'Advanced' },
            ]}
            onChange={setAudience}
            ariaLabel="Audience filter"
          />
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2">
          {filtered.length === 0 ? (
            <div className="px-3 py-8 text-center">
              <p className="text-xs font-medium text-slate-300">No matching topics</p>
              <p className="mt-1 text-2xs text-slate-500">Try fewer words, or switch the audience filter.</p>
            </div>
          ) : (
            <ul className="space-y-0.5">
              {filtered.map((entry) => {
                const active = entry.slug === helpSlug;
                return (
                  <li key={entry.slug}>
                    <button
                      type="button"
                      onClick={() => onHelpLink(entry)}
                      className={cn(
                        'focus-ring flex w-full items-start gap-2.5 rounded-lg px-2.5 py-2 text-left transition-colors',
                        active ? 'bg-white/[0.07]' : 'hover:bg-white/[0.04]',
                      )}
                    >
                      <BookOpen className={cn('mt-0.5 h-3.5 w-3.5 shrink-0', active ? 'text-emerald-300' : 'text-slate-600')} aria-hidden="true" />
                      <span className="min-w-0 flex-1">
                        <span className={cn('block truncate text-xs font-medium', active ? 'text-white' : 'text-slate-300')}>
                          {entry.title}
                        </span>
                        <span className="mt-0.5 flex items-center gap-1.5 text-3xs text-slate-600">
                          <Clock className="h-2.5 w-2.5" aria-hidden="true" />
                          {readingTimeLabel(entry.minutes)}
                          {entry.audience !== 'both' ? <span>· {audienceLabel(entry.audience)}</span> : null}
                        </span>
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
        <div className="border-t border-white/[0.06] px-4 py-3 text-3xs text-slate-600">
          <span className="inline-flex items-center gap-1.5">
            <ExternalLink className="h-3 w-3" aria-hidden="true" />
            Same content as the docs and AI agents.
          </span>
        </div>
      </aside>

      {/* reader */}
      <section className="min-h-0 flex-1 overflow-y-auto" ref={articleRef} aria-label="Help topic">
        {!meta ? (
          <WelcomeView index={index} audience={audience} onOpen={onHelpLink} />
        ) : topicLoading && !topic ? (
          <div className="flex h-full items-center justify-center text-xs text-slate-500">
            <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden="true" />
            Loading topic…
          </div>
        ) : topicError ? (
          <div className="p-6">
            <EmptyState
              icon={BookOpen}
              title="That topic could not be loaded"
              description="The bundled help may be incomplete. Try another topic, or reload the help bundle."
              action={
                <Button variant="primary" onClick={() => onCopyLink()}>
                  Copy this link
                </Button>
              }
              helpTopic="troubleshooting"
            />
          </div>
        ) : topic ? (
          <div className="mx-auto max-w-3xl px-4 py-5 sm:px-6 lg:py-7">
            <div className="mb-4 flex items-center justify-between gap-2">
              <button
                type="button"
                onClick={() => openTopic(null)}
                className="focus-ring inline-flex items-center gap-1.5 rounded-md text-2xs font-medium text-slate-400 hover:text-slate-200 lg:hidden"
              >
                <ArrowLeft className="h-3.5 w-3.5" aria-hidden="true" />
                All topics
              </button>
              <div className="ml-auto flex items-center gap-1.5">
                <Button variant="ghost" size="sm" icon={Link2} onClick={() => void onCopyLink()}>
                  Copy link
                </Button>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <Badge tone="accent" size="sm">{audienceLabel(topic.audience)}</Badge>
              <Badge tone="neutral" size="sm" icon={Clock}>{readingTimeLabel(topic.minutes)}</Badge>
              {topic.related.length ? <span className="text-3xs text-slate-600">{topic.related.length} related topics</span> : null}
            </div>
            <div className="mt-3 xl:grid xl:grid-cols-[minmax(0,1fr)_170px] xl:gap-6">
              <div
                className="help-markdown min-w-0"
                onClick={onMarkdownClick}
                dangerouslySetInnerHTML={{ __html: renderMarkdown(topic.markdown) }}
              />
              {toc.length ? (
                <nav aria-label="On this page" className="hidden self-start xl:sticky xl:top-4 xl:block">
                  <p className="text-2xs font-semibold uppercase tracking-[0.14em] text-slate-500">On this page</p>
                  <TocList toc={toc} />
                </nav>
              ) : null}
            </div>

            {toc.length >= 3 ? (
              <details className="mt-6 rounded-xl border border-white/[0.07] bg-white/[0.02] px-3 py-2 xl:hidden">
                <summary className="cursor-pointer text-2xs font-semibold text-slate-300">On this page</summary>
                <TocList toc={toc} />
              </details>
            ) : null}

            <div className="mt-6 flex flex-wrap items-center justify-between gap-3 border-t border-white/[0.06] pt-4">
              <div className="flex items-center gap-2">
                <span className="text-2xs text-slate-500">Was this useful?</span>
                <button
                  type="button"
                  onClick={() => markUseful('yes')}
                  aria-pressed={useful[topic.slug] === 'yes'}
                  className={cn(
                    'focus-ring inline-flex items-center gap-1 rounded-md border px-2 py-1 text-2xs',
                    useful[topic.slug] === 'yes'
                      ? 'border-emerald-400/30 bg-emerald-400/10 text-emerald-300'
                      : 'border-white/10 text-slate-400 hover:text-slate-200',
                  )}
                >
                  <ThumbsUp className="h-3 w-3" aria-hidden="true" /> Yes
                </button>
                <button
                  type="button"
                  onClick={() => markUseful('no')}
                  aria-pressed={useful[topic.slug] === 'no'}
                  className={cn(
                    'focus-ring inline-flex items-center gap-1 rounded-md border px-2 py-1 text-2xs',
                    useful[topic.slug] === 'no'
                      ? 'border-amber-400/30 bg-amber-400/10 text-amber-300'
                      : 'border-white/10 text-slate-400 hover:text-slate-200',
                  )}
                >
                  <ThumbsDown className="h-3 w-3" aria-hidden="true" /> Not really
                </button>
                {useful[topic.slug] ? <span className="text-3xs text-slate-600">Thanks — feedback stays on this device.</span> : null}
              </div>
              <HelpLink topic="troubleshooting">Still stuck?</HelpLink>
            </div>

            <div className="mt-5">
              <div>
                <p className="text-2xs font-semibold uppercase tracking-[0.14em] text-slate-500">Related topics</p>
                <div className="mt-2 space-y-1.5">
                  {related.map((entry) => (
                    <button
                      key={entry.slug}
                      type="button"
                      onClick={() => onHelpLink(entry)}
                      className="focus-ring flex w-full items-center justify-between gap-2 rounded-lg border border-white/[0.06] bg-white/[0.02] px-3 py-2 text-left hover:border-white/15"
                    >
                      <span className="min-w-0">
                        <span className="block truncate text-2xs font-medium text-slate-300">{entry.title}</span>
                        <span className="block truncate text-3xs text-slate-600">{entry.summary}</span>
                      </span>
                      <ChevronRight className="h-3.5 w-3.5 shrink-0 text-slate-600" aria-hidden="true" />
                    </button>
                  ))}
                </div>
              </div>
            </div>

            <div className="mt-6 flex items-center justify-between gap-2 border-t border-white/[0.06] pt-4">
              {position.previous ? (
                <Button variant="ghost" size="sm" icon={ArrowLeft} onClick={() => onHelpLink(position.previous!)}>
                  {position.previous.title}
                </Button>
              ) : (
                <span />
              )}
              {position.next ? (
                <Button variant="ghost" size="sm" onClick={() => onHelpLink(position.next!)}>
                  {position.next.title}
                  <ArrowRight className="ml-1 h-3.5 w-3.5" aria-hidden="true" />
                </Button>
              ) : null}
            </div>
          </div>
        ) : null}
      </section>
    </div>
  );
}

function TocList({ toc }: { toc: { id: string; text: string; level: number }[] }) {
  return (
    <ul className="mt-2 space-y-1">
      {toc.map((entry) => (
        <li key={entry.id}>
          <a
            href={`#${entry.id}`}
            onClick={(event) => {
              event.preventDefault();
              const target = document.getElementById(entry.id);
              target?.scrollIntoView({ behavior: 'smooth', block: 'start' });
              window.history.replaceState(null, '', `#${entry.id}`);
            }}
            className={cn(
              'focus-ring block rounded text-2xs text-slate-500 hover:text-slate-300',
              entry.level === 3 ? 'pl-3' : '',
            )}
          >
            {entry.text}
          </a>
        </li>
      ))}
    </ul>
  );
}

function WelcomeView({
  index,
  audience,
  onOpen,
}: {
  index: HelpIndex;
  audience: UiMode;
  onOpen: (topic: HelpTopicMeta) => void;
}) {
  const start = ['getting-started', 'install', 'first-mesh', 'security'];
  const popular = index.topics.filter((topic) => start.includes(topic.slug));
  return (
    <div className="mx-auto max-w-3xl px-4 py-6 sm:px-6 lg:py-10">
      <div className="rounded-2xl border border-white/[0.07] bg-gradient-to-br from-white/[0.045] to-transparent p-5">
        <div className="grid h-10 w-10 place-items-center rounded-xl border border-emerald-400/20 bg-emerald-400/10 text-emerald-300">
          <BookOpen className="h-5 w-5" aria-hidden="true" />
        </div>
        <h2 className="mt-3 text-lg font-semibold tracking-tight text-white">The OpenMesh handbook</h2>
        <p className="mt-1 max-w-xl text-xs leading-relaxed text-slate-400">
          The same help that ships in the docs and feeds AI agents. Read it offline, search it, or jump
          straight to what is on your screen.
        </p>
        <div className="mt-4 flex flex-wrap gap-2">
          {popular.map((topic) => (
            <button
              key={topic.slug}
              type="button"
              onClick={() => onOpen(topic)}
              className="focus-ring inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.04] px-2.5 py-1.5 text-2xs font-medium text-slate-200 hover:border-emerald-400/30 hover:text-emerald-200"
            >
              <Check className="h-3 w-3 text-emerald-300" aria-hidden="true" />
              {topic.title}
            </button>
          ))}
        </div>
      </div>
      <div className="mt-5 grid gap-2 sm:grid-cols-2">
        {index.topics
          .filter((topic) => topic.audience === 'both' || topic.audience === audience)
          .slice(0, 8)
          .map((topic) => (
            <button
              key={topic.slug}
              type="button"
              onClick={() => onOpen(topic)}
              className="focus-ring flex items-start gap-2.5 rounded-xl border border-white/[0.06] bg-white/[0.02] p-3 text-left hover:border-white/15"
            >
              <BookOpen className="mt-0.5 h-3.5 w-3.5 shrink-0 text-slate-600" aria-hidden="true" />
              <span className="min-w-0">
                <span className="block text-2xs font-semibold text-slate-200">{topic.title}</span>
                <span className="mt-0.5 block text-3xs leading-relaxed text-slate-500">{topic.summary}</span>
                <span className="mt-1 flex items-center gap-1 text-3xs text-slate-600">
                  <Clock className="h-2.5 w-2.5" aria-hidden="true" />
                  {readingTimeLabel(topic.minutes)}
                </span>
              </span>
            </button>
          ))}
      </div>
      <p className="mt-5 text-3xs leading-relaxed text-slate-600">
        Tip: press <kbd className="rounded border border-white/10 bg-white/[0.05] px-1 py-0.5 font-mono">⌘K</kbd> to search.
        Every topic has a stable URL, so you can link a teammate straight to the right section.
      </p>
    </div>
  );
}
