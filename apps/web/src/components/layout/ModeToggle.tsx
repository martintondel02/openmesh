import { Sparkles, SlidersHorizontal } from 'lucide-react';
import { useAppStore } from '../../store/app';
import { cn } from '../../lib/utils';
import { HelpTip } from '../ui/help';

/**
 * Top-bar beginner/advanced switch. Local preference only — switching never
 * changes a setting, and the bridge is told the current mode via features.ui_mode.
 */
export function ModeToggle({ className }: { className?: string }) {
  const uiMode = useAppStore((state) => state.uiMode);
  const setUiMode = useAppStore((state) => state.setUiMode);
  const setScreen = useAppStore((state) => state.setScreen);
  const pushToast = useAppStore((state) => state.pushToast);

  const choose = (mode: 'beginner' | 'advanced') => {
    if (uiMode === mode) return;
    setUiMode(mode);
    if (mode === 'advanced') {
      pushToast({
        kind: 'info',
        title: 'Advanced mode on',
        message: 'Every field and number is now visible. Switching mode did not change any setting.',
        helpTopic: 'getting-started',
      });
    }
  };

  return (
    <div className={cn('inline-flex items-center gap-1', className)}>
      <div
        role="group"
        aria-label="Interface mode"
        className="inline-flex h-7 items-center rounded-full border border-white/[0.08] bg-white/[0.03] p-0.5"
      >
        <button
          type="button"
          onClick={() => choose('beginner')}
          aria-pressed={uiMode === 'beginner'}
          title="Beginner mode: plain language, safe basics"
          className={cn(
            'focus-ring inline-flex h-6 items-center gap-1 rounded-full px-2 text-3xs font-medium transition-colors',
            uiMode === 'beginner' ? 'bg-emerald-400/15 text-emerald-200' : 'text-slate-500 hover:text-slate-300',
          )}
        >
          <Sparkles className="h-3 w-3" aria-hidden="true" />
          <span className="hidden sm:inline">Beginner</span>
        </button>
        <button
          type="button"
          onClick={() => choose('advanced')}
          aria-pressed={uiMode === 'advanced'}
          title="Advanced mode: every control and exact number"
          className={cn(
            'focus-ring inline-flex h-6 items-center gap-1 rounded-full px-2 text-3xs font-medium transition-colors',
            uiMode === 'advanced' ? 'bg-sky-400/15 text-sky-200' : 'text-slate-500 hover:text-slate-300',
          )}
        >
          <SlidersHorizontal className="h-3 w-3" aria-hidden="true" />
          <span className="hidden sm:inline">Advanced</span>
        </button>
      </div>
      <button
        type="button"
        onClick={() => setScreen('settings')}
        className="focus-ring hidden h-7 items-center rounded-full border border-white/[0.08] bg-white/[0.03] px-2 text-3xs text-slate-400 hover:text-slate-200 2xl:inline-flex"
        title="Open mode settings and help"
      >
        Mode & help
      </button>
      <HelpTip helpKey="mode.toggle" label="Interface mode" className="hidden sm:inline-grid" />
    </div>
  );
}
