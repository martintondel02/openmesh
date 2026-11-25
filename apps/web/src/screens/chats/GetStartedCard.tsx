import { useMemo } from 'react';
import { ArrowRight, Check, ChevronRight, Rocket, X } from 'lucide-react';
import { useAppStore } from '../../store/app';
import { ProgressBar } from '../../components/ui/primitives';
import { HelpLink } from '../../components/ui/help';
import { checklistForMode, checklistProgress, isSetupStateComplete } from '../../lib/setup';

/**
 * Persistent "Get started" checklist on Chats (UX spec §4.5). It reappears until done
 * or dismissed for the session; Advanced mode gets the operator checklist instead.
 */
export function GetStartedCard() {
  const uiMode = useAppStore((state) => state.uiMode);
  const setupState = useAppStore((state) => state.setupState);
  const ownNodeId = useAppStore((state) => state.ownNodeId);
  const bridgeStatus = useAppStore((state) => state.bridgeStatus);
  const settings = useAppStore((state) => state.settings);
  const agentConnected = useAppStore((state) => state.agentConnected);
  const setupCardDismissed = useAppStore((state) => state.setupCardDismissed);
  const forceSetupCard = useAppStore((state) => state.forceSetupCard);
  const dismissSetupCard = useAppStore((state) => state.dismissSetupCard);
  const openOnboarding = useAppStore((state) => state.openOnboarding);
  const toggleAdvancedChecklist = useAppStore((state) => state.toggleAdvancedChecklist);
  const advancedChecklistDone = useAppStore((state) => state.advancedChecklistDone);
  const openHelp = useAppStore((state) => state.openHelp);
  const setScreen = useAppStore((state) => state.setScreen);

  const checks = useMemo(() => {
    if (!setupState) return null;
    const merged = {
      ...setupState.checks,
      identity: setupState.checks.identity || Boolean(ownNodeId),
      radio: setupState.checks.radio || Boolean(bridgeStatus?.radio.connected),
      api_token: setupState.checks.api_token || Boolean(settings.token),
      agent: setupState.checks.agent || agentConnected,
    };
    return checklistForMode(uiMode, merged);
  }, [setupState, ownNodeId, bridgeStatus, settings.token, agentConnected, uiMode]);

  if (!setupState && !forceSetupCard) return null;
  if (setupState && isSetupStateComplete(setupState) && !forceSetupCard && uiMode === 'beginner') return null;
  if (setupCardDismissed && !forceSetupCard) return null;
  if (!checks) return null;

  const progress = checklistProgress(checks);
  const done = progress.done === progress.total;

  return (
    <div className="mb-2 rounded-xl border border-emerald-400/15 bg-emerald-400/[0.045] p-3">
      <div className="flex items-start gap-2">
        <span className="grid h-7 w-7 shrink-0 place-items-center rounded-lg border border-emerald-400/20 bg-emerald-400/10 text-emerald-300">
          <Rocket className="h-3.5 w-3.5" aria-hidden="true" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="flex items-center gap-1.5 text-2xs font-semibold text-slate-100">
            {uiMode === 'advanced' ? 'Operator checklist' : 'Get started'}
            <span className="text-3xs font-normal text-slate-500">
              {done ? 'all done' : `${progress.done} of ${progress.total}`}
            </span>
          </p>
          <ProgressBar value={progress.pct} className="mt-1.5" tone={done ? 'accent' : 'sky'} />
          <ul className="mt-2 space-y-0.5">
            {checks.slice(0, 4).map((item) => {
              const checked = uiMode === 'advanced' && item.key !== 'backbone' && item.key !== 'agent' && item.key !== 'airtime' && item.key !== 'backup'
                ? item.ok
                : item.key === 'airtime' || item.key === 'backup'
                  ? Boolean(advancedChecklistDone[item.key])
                  : item.ok;
              return (
                <li key={item.key}>
                  <button
                    type="button"
                    onClick={() => {
                      if (item.key === 'airtime' || item.key === 'backup') toggleAdvancedChecklist(item.key);
                      else openHelp(item.helpTopic);
                    }}
                    className="focus-ring flex w-full items-center gap-1.5 rounded px-0.5 py-0.5 text-left text-3xs"
                  >
                    {checked ? (
                      <Check className="h-3 w-3 shrink-0 text-emerald-300" aria-hidden="true" />
                    ) : (
                      <ChevronRight className="h-3 w-3 shrink-0 text-slate-500" aria-hidden="true" />
                    )}
                    <span className={checked ? 'text-slate-500 line-through decoration-slate-600' : 'text-slate-300'}>{item.label}</span>
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            {uiMode === 'beginner' ? (
              <button
                type="button"
                onClick={openOnboarding}
                className="focus-ring inline-flex items-center gap-1 rounded-md bg-emerald-400/15 px-2 py-1 text-3xs font-semibold text-emerald-200 hover:bg-emerald-400/20"
              >
                Resume setup
                <ArrowRight className="h-3 w-3" aria-hidden="true" />
              </button>
            ) : (
              <button
                type="button"
                onClick={() => setScreen('backbone')}
                className="focus-ring inline-flex items-center gap-1 rounded-md bg-emerald-400/15 px-2 py-1 text-3xs font-semibold text-emerald-200 hover:bg-emerald-400/20"
              >
                Open backbone
                <ArrowRight className="h-3 w-3" aria-hidden="true" />
              </button>
            )}
            <HelpLink topic="getting-started" className="text-3xs">What is this?</HelpLink>
          </div>
        </div>
        <button
          type="button"
          onClick={dismissSetupCard}
          aria-label="Dismiss the Get started card"
          className="focus-ring -mr-1 -mt-1 grid h-5 w-5 shrink-0 place-items-center rounded text-slate-500 hover:bg-white/[0.06] hover:text-slate-300"
        >
          <X className="h-3 w-3" aria-hidden="true" />
        </button>
      </div>
    </div>
  );
}
