/**
 * Plain-language copy used by beginner surfaces (UX spec §6). Keep protocol jargon
 * and unexplained numbers out of these strings — the unit test enforces it.
 */
export const BEGINNER_COPY = {
  modeLabel: 'Beginner',
  modeSwitchHint: 'Switch to Advanced when you want to see every number and field.',
  settingsIntro: 'Only the choices most people need. Nothing here can break your mesh.',
  secureCardTitle: 'Secure my node',
  secureCardBody: 'Use safe defaults: a private channel, token-protected access and conservative radio settings.',
  secureCardAction: 'Check my security',
  nodeNameLabel: 'What should we call this device?',
  nodeNameHelp: 'Other people see this name. A nickname is fine.',
  roleLabel: 'What will this device do?',
  roleHelp: 'We use this to choose power and radio defaults that fit the job.',
  radioLabel: 'Radio coverage',
  radioHelp: 'Longer range is slower. The default is a good balance for most homes.',
  positionLabel: 'Share my location',
  positionHelp: 'When off, your position stays on your device until you share it once.',
  storeForwardLabel: 'Hold messages for people who are offline',
  storeForwardHelp: 'Encrypted messages wait on this device and arrive when the person is back in range.',
  bridgeLabel: 'Connect to your node',
  bridgeHelp: 'This is the local program that talks to your radio. Keep the default unless you moved it.',
  showAdvanced: 'Show advanced settings',
  advancedHint: 'You can switch back to Beginner at any time. Switching never changes a setting by itself.',
  emptyChatsHint: 'Start a conversation, or invite someone to a channel to see messages here.',
  inviteLabel: 'Invite someone',
  inviteHelp: 'The link includes the channel key, so only share it with people you trust.',
  nextStepsTitle: 'You are ready',
  nextStepsBody: 'Send a message, add a contact or connect an AI assistant. You can change anything later.',
} as const;

export type BeginnerCopyKey = keyof typeof BEGINNER_COPY;
