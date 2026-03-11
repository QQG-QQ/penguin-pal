export type PetMode = 'idle' | 'listening' | 'thinking' | 'speaking' | 'guarded'
export type AssistantWindowView = 'pet' | 'settings' | 'bubble'

export type ProviderKind = 'mock' | 'codexCli' | 'openAi' | 'anthropic' | 'openAiCompatible'
export type ProviderAuthMode = 'apiKey' | 'oauth'
export type OAuthStatus = 'signedOut' | 'pending' | 'authorized' | 'error'

export interface ChatMessage {
  id: string
  role: 'system' | 'user' | 'assistant'
  content: string
  createdAt: number
}

export interface DesktopAction {
  id: string
  title: string
  summary: string
  riskLevel: number
  minimumLevel: number
  requiresConfirmation: boolean
  enabled: boolean
}

export interface AuditEntry {
  id: string
  action: string
  outcome: string
  detail: string
  createdAt: number
  riskLevel: number
}

export interface AudioStage {
  id: string
  title: string
  summary: string
  status: string
}

export interface AudioProfile {
  inputMode: string
  outputMode: string
  stages: AudioStage[]
}

export interface AiConstraintItem {
  id: string
  title: string
  summary: string
  status: string
}

export interface AiConstraintProfile {
  label: string
  version: string
  summary: string
  immutableRules: AiConstraintItem[]
  capabilityGates: AiConstraintItem[]
  runtimeBoundaries: AiConstraintItem[]
}

export interface OAuthState {
  status: OAuthStatus
  authorizeUrl: string | null
  tokenUrl: string | null
  clientId: string | null
  redirectUrl: string | null
  scopes: string[]
  accountHint: string | null
  pendingAuthUrl: string | null
  accessTokenLoaded: boolean
  lastError: string | null
  startedAt: number | null
  expiresAt: number | null
}

export interface ProviderConfig {
  kind: ProviderKind
  model: string
  baseUrl: string | null
  systemPrompt: string
  allowNetwork: boolean
  voiceReply: boolean
  retainHistory: boolean
  apiKeyLoaded: boolean
  authMode: ProviderAuthMode
  oauth: OAuthState
}

export interface AssistantSnapshot {
  mode: PetMode
  messages: ChatMessage[]
  provider: ProviderConfig
  permissionLevel: number
  allowedActions: DesktopAction[]
  auditTrail: AuditEntry[]
  audioProfile: AudioProfile
  aiConstraints: AiConstraintProfile
}

export interface ProviderConfigInput {
  kind: ProviderKind
  model: string
  baseUrl: string | null
  systemPrompt: string
  allowNetwork: boolean
  voiceReply: boolean
  retainHistory: boolean
  permissionLevel: number
  authMode: ProviderAuthMode
  oauthAuthorizeUrl: string | null
  oauthTokenUrl: string | null
  oauthClientId: string | null
  oauthRedirectUrl: string | null
  oauthScopes: string
  apiKey?: string | null
  clearApiKey?: boolean
  clearOAuthToken?: boolean
}

export interface ChatResponse {
  reply: ChatMessage
  providerLabel: string
  snapshot: AssistantSnapshot
}

export interface ActionApprovalCheck {
  id: string
  label: string
}

export interface ActionApprovalRequest {
  id: string
  action: DesktopAction
  prompt: string
  requiredPhrase: string
  checks: ActionApprovalCheck[]
  createdAt: number
  expiresAt: number
}

export interface ActionExecutionResult {
  status: string
  message: string
  snapshot: AssistantSnapshot
  approvalRequest?: ActionApprovalRequest | null
}

export interface OAuthFlowResult {
  message: string
  authorizationUrl: string | null
  snapshot: AssistantSnapshot
}

export interface CodexCliStatus {
  installed: boolean
  version: string | null
  loggedIn: boolean
  authPath: string | null
  runtimePath: string | null
  source: string
  message: string
}

export interface ReplyHistoryEntry {
  id: string
  timestamp: number
  userInput: string
  assistantReply: string
}

export interface PetLayoutMetrics {
  anchorX: number
  anchorY: number
  petLeft: number
  petTop: number
  petRight: number
  petBottom: number
  faceLeft: number
  faceTop: number
  faceRight: number
  faceBottom: number
}

export interface BubbleWindowState {
  visible: boolean
  text: string
  anchorX: number
  anchorY: number
  petLeft: number
  petTop: number
  petRight: number
  petBottom: number
  faceLeft: number
  faceTop: number
  faceRight: number
  faceBottom: number
}
