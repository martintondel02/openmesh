import { useEffect } from 'react';
import { AppShell } from './components/layout/Shell';
import { ToastStack } from './components/ui/overlay';
import { useAppStore } from './store/app';
import { ChatsScreen } from './screens/ChatsScreen';

import { NodesScreen } from './screens/NodesScreen';
import { ContactsScreen } from './screens/ContactsScreen';
import { SettingsScreen } from './screens/SettingsScreen';
import { MapScreen } from './screens/MapScreen';


const TITLES: Record<string, string> = {
  chats: 'Chats',
  map: 'Mesh map',
  nodes: 'Nodes',
  contacts: 'Contacts',
  settings: 'Settings',
};

export default function App() {
  const screen = useAppStore((state) => state.screen);
  const mode = useAppStore((state) => state.mode);
  const toasts = useAppStore((state) => state.toasts);
  const dismissToast = useAppStore((state) => state.dismissToast);
  const bootstrap = useAppStore((state) => state.bootstrap);

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

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
      </AppShell>
      <ToastStack toasts={toasts} onDismiss={dismissToast} />
    </>
  );
}
