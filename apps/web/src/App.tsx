import { useEffect } from 'react';
import { AppShell } from './components/layout/Shell';
import { ToastStack } from './components/ui/overlay';
import { useAppStore } from './store/app';
import { ChatsScreen } from './screens/ChatsScreen';

import { NodesScreen } from './screens/NodesScreen';
import { ContactsScreen } from './screens/ContactsScreen';
import { SettingsScreen } from './screens/SettingsScreen';
import { BackboneScreen } from './screens/BackboneScreen';
import { HelpCenterScreen } from './screens/HelpCenterScreen';
import { AgentsScreen } from './screens/AgentsScreen';
import { OnboardingWizard } from './components/onboarding/OnboardingWizard';
import { parseHelpRoute } from './lib/help';
import { MapScreen } from './screens/MapScreen';


const TITLES: Record<string, string> = {
  chats: 'Chats',
  map: 'Mesh map',
  nodes: 'Nodes',
  contacts: 'Contacts',
  settings: 'Settings',
  backbone: 'Backbone',
  agents: 'Agents',
  help: 'Help Center',
};

export default function App() {
  const screen = useAppStore((state) => state.screen);
  const mode = useAppStore((state) => state.mode);
  const toasts = useAppStore((state) => state.toasts);
  const dismissToast = useAppStore((state) => state.dismissToast);
  const bootstrap = useAppStore((state) => state.bootstrap);
  const connection = useAppStore((state) => state.connection);
  const config = useAppStore((state) => state.config);
  const uiMode = useAppStore((state) => state.uiMode);
  const updateConfig = useAppStore((state) => state.updateConfig);

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

  // Support browser back/forward for /backbone, /agents and /help/:slug#anchor.
  useEffect(() => {
    const onPopState = () => {
      const path = window.location.pathname;
      const help = parseHelpRoute(path, window.location.search, window.location.hash);
      if (help) {
        useAppStore.setState({ screen: 'help', helpSlug: help.slug, helpAnchor: help.anchor });
        return;
      }
      if (/\/backbone\/?$/.test(path)) {
        useAppStore.setState({ screen: 'backbone' });
        return;
      }
      if (/\/agents\/?$/.test(path)) {
        useAppStore.setState({ screen: 'agents' });
        return;
      }
      useAppStore.setState({ screen: 'chats', helpSlug: null, helpAnchor: null });
    };
    window.addEventListener('popstate', onPopState);
    return () => window.removeEventListener('popstate', onPopState);
  }, []);

  // Keep the bridge's view of features.ui_mode in sync; the local layout preference wins.
  useEffect(() => {
    if (connection !== 'connected' || !config) return;
    if (config.features.ui_mode === uiMode) return;
    updateConfig({ features: { ...config.features, ui_mode: uiMode } });
  }, [connection, config, uiMode, updateConfig]);

  useEffect(() => {
    document.title = `${TITLES[screen] ?? 'OpenMesh'} · OpenMesh${mode === 'demo' ? ' (demo)' : ''}`;
  }, [screen, mode]);

  useEffect(() => {
    document.documentElement.dataset.mode = mode;
  }, [mode]);

  return (
    <>
      <AppShell>
        {screen === 'chats' ? <ChatsScreen /> : null}
        {screen === 'map' ? <MapScreen /> : null}
        {screen === 'nodes' ? <NodesScreen /> : null}
        {screen === 'contacts' ? <ContactsScreen /> : null}
        {screen === 'settings' ? <SettingsScreen /> : null}
        {screen === 'backbone' ? <BackboneScreen /> : null}
        {screen === 'agents' ? <AgentsScreen /> : null}
        {screen === 'help' ? <HelpCenterScreen /> : null}
      </AppShell>
      <OnboardingWizard />
      <ToastStack toasts={toasts} onDismiss={dismissToast} />
    </>
  );
}
