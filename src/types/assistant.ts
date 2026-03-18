export type PetMode = 'idle' | 'listening' | 'thinking' | 'speaking' | 'guarded'
export type AssistantWindowView = 'pet' | 'settings' | 'bubble'

export type ProviderKind = 'mock' | 'codexCli' | 'openAi' | 'anthropic' | 'openAiCompatible'
export type ProviderAuthMode = 'apiKey' | 'oauth'
export type OAuthStatus = 'signedOut' | 'pending' | 'authorized' | 'error'
export type VisionChannelKind = 'disabled' | 'openAi' | 'openAiCompatible'
export type VisionProviderStatusKind =
  | 'supported'
  | 'unknown'
  | 'unsupported'
  | 'timeout'
  | 'disabledOffline'
  | 'analysisFailed'

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

export interface VisionProviderStatus {
  kind: VisionProviderStatusKind
  message: string
}

export interface VisionChannelConfig {
  enabled: boolean
  kind: VisionChannelKind
  model: string
  baseUrl: string | null
  allowNetwork: boolean
  apiKeyLoaded: boolean
  timeoutMs: number
  maxImageBytes: number
  maxImageWidth: number
  maxImageHeight: number
  lastError: string | null
}

export interface ShellPermissionSettings {
  enabled: boolean
  allowExecute: boolean
  allowFileModify: boolean
  allowFileDelete: boolean
  allowNetwork: boolean
  allowSystem: boolean
  durationHours: number
}

export interface AssistantSnapshot {
  mode: PetMode
  messages: ChatMessage[]
  provider: ProviderConfig
  visionChannel: VisionChannelConfig
  visionChannelStatus: VisionProviderStatus
  permissionLevel: number
  allowedActions: DesktopAction[]
  auditTrail: AuditEntry[]
  audioProfile: AudioProfile
  aiConstraints: AiConstraintProfile
  shellPermissions: ShellPermissionSettings
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
  visionChannel: VisionChannelConfigInput
  shellPermissions: ShellPermissionSettings
}

export interface VisionChannelConfigInput {
  enabled: boolean
  kind: VisionChannelKind
  model: string
  baseUrl: string | null
  allowNetwork: boolean
  timeoutMs: number
  maxImageBytes: number
  maxImageWidth: number
  maxImageHeight: number
  apiKey?: string | null
  clearApiKey?: boolean
}

export type AgentRoute = 'chat' | 'control' | 'test'
export type AgentTaskStatus = 'running' | 'waitingConfirmation' | 'completed' | 'failed' | 'cancelled'

export interface AgentTaskProgress {
  taskId: string
  taskTitle: string
  stepIndex: number
  stepCount: number
  status: AgentTaskStatus
  stepSummary?: string | null
  detail?: string | null
}

export interface AgentMessageMeta {
  route: AgentRoute
  plannedTools: string[]
  pendingRequest?: ControlPendingRequest | null
  task?: AgentTaskProgress | null
}

export interface ChatResponse {
  reply: ChatMessage
  providerLabel: string
  snapshot: AssistantSnapshot
  agent?: AgentMessageMeta | null
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

export interface ControlServiceStatus {
  running: boolean
  baseUrl: string | null
  toolCount: number
  message: string
}

export interface ControlErrorPayload {
  code: string
  message: string
  detail?: string | null
  retryable: boolean
}

export type ControlRiskLevel = 'readOnly' | 'writeLow' | 'writeHigh'

export interface ControlPendingRequest {
  id: string
  tool: string
  title: string
  prompt: string
  preview: Record<string, unknown>
  args: Record<string, unknown>
  createdAt: number
  expiresAt: number
  minimumPermissionLevel: number
  riskLevel: ControlRiskLevel
}

export interface ControlToolInvokeResponse {
  status: 'success' | 'pending_confirmation' | 'error'
  result?: Record<string, unknown> | unknown[] | null
  message?: string | null
  pendingRequest?: ControlPendingRequest | null
  error?: ControlErrorPayload | null
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

export type BubbleMessageTier = 'short' | 'medium' | 'long' | 'pinned'

export interface BubbleLayoutMetrics {
  messageId: number
  charCount: number
  scrollHeight: number
  clientHeight: number
  contentHeight: number
  isScrollable: boolean
}

export interface BubbleWindowState {
  messageId: number
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

// ============================================================================
// Whisper 语音识别类型
// ============================================================================

export type WhisperModel = 'tiny' | 'base' | 'small' | 'medium' | 'large'
export type RecordingState = 'idle' | 'recording' | 'processing'

export interface ModelInfo {
  model: WhisperModel
  label: string
  sizeBytes: number
  downloaded: boolean
}

export interface WhisperStatus {
  modelLoaded: boolean
  currentModel: WhisperModel | null
  availableModels: ModelInfo[]
  recordingState: RecordingState
}

export interface TranscriptionResult {
  text: string
  language: string | null
  durationMs: number
}

export interface DownloadProgress {
  model: WhisperModel
  downloadedBytes: number
  totalBytes: number
  progressPercent: number
}
