/** Types for the AI-native Agents page (docs/architecture/AI-NATIVE.md §2/§3). */

export type AgentClient = 'claude' | 'cursor' | 'dsh' | 'generic';
export type PromptTier = 'beginner' | 'advanced';

/** One write action from GET /api/activity?limit=100. */
export interface ActivityEntry {
  id: number | string;
  at_ms: number;
  /** `user`, `web`, `cli`, or `agent:<name>`. */
  source: string;
  method: string;
  path: string;
  summary: string;
  ok: boolean;
  error: string | null;
}

export interface ActivityPayloadMeta {
  capacity: number;
  agentConnected: boolean;
  lastAgentAtMs: number | null;
  sources: string[];
}

export interface ActivityPayload extends ActivityPayloadMeta {
  entries: ActivityEntry[];
}

export interface AgentStatus {
  configured: boolean;
  /** Friendly agent name parsed from the activity source, when present. */
  name: string | null;
  lastActionMs: number | null;
  readOnly: boolean;
}

export interface McpConfigOptions {
  bridgeUrl: string;
  /** Token file path; MCP reads it itself so secrets never enter a chat or prompt. */
  tokenFile?: string;
  readOnly?: boolean;
  agentName?: string;
}

export interface McpSnippet {
  client: AgentClient;
  label: string;
  description: string;
  /** Pretty-printed JSON or shell snippet. */
  content: string;
  note: string;
}
