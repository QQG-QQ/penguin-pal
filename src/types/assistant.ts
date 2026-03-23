export type PetMode = 'idle' | 'listening' | 'thinking' | 'speaking' | 'guarded'
export type PetDockState = 'normal' | 'dockedLeft' | 'dockedRight' | 'dockedTop'
export type AssistantWindowView = 'pet' | 'settings' | 'bubble' | 'research'

export type ProviderKind = 'mock' | 'codexCli' | 'openAi' | 'anthropic' | 'openAiCompatible'
export type ProviderAuthMode = 'apiKey' | 'oauth'
export type VoiceInputMode = 'disabled' | 'continuous' | 'pushToTalk'
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
  voiceInputMode: VoiceInputMode
  pushToTalkShortcut: string
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

export interface ResearchConfig {
  enabled: boolean
  startupPopup: boolean
  bubbleAlerts: boolean
  watchlist: string[]
  funds: string[]
  themes: string[]
  habitNotes: string
  decisionFramework: string
}

export interface AssistantSnapshot {
  mode: PetMode
  messages: ChatMessage[]
  provider: ProviderConfig
  launchAtStartup: boolean
  autoUpdateCodex: boolean
  autoCheckAppUpdate: boolean
  research: ResearchConfig
  workspaceRoot: string | null
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
  launchAtStartup: boolean
  autoUpdateCodex: boolean
  autoCheckAppUpdate: boolean
  research: ResearchConfig
  voiceReply: boolean
  retainHistory: boolean
  voiceInputMode: VoiceInputMode
  pushToTalkShortcut: string
  workspaceRoot: string | null
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

export type AgentRoute = 'chat' | 'control' | 'test' | 'workspace'
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
  pendingShellConfirmation?: PendingShellConfirmation | null
}

export interface PendingShellConfirmation {
  id: string
  command: string
  riskDescription: string
  createdAt: number
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
  credentialPresent: boolean
  authPath: string | null
  runtimePath: string | null
  source: string
  statusKind: string
  statusLabel: string
  reloginRecommended: boolean
  message: string
}

export interface ReplyHistoryEntry {
  id: string
  timestamp: number
  userInput: string
  assistantReply: string
}

export type ManagedMemoryKind = 'semantic' | 'meta'
export type MemoryStatus = 'active' | 'archived' | 'deprecated' | 'conflicted'

export interface ManagedMemoryRecord {
  id: string
  memoryType: ManagedMemoryKind
  title: string
  summary: string
  detail: string
  confidence: number
  explicit: boolean
  mentionCount: number
  status: MemoryStatus
  source: string
  updatedAt: number
  expiresAt: number | null
  tags: string[]
  conflictGroup: string | null
}

export interface MemoryConflictGroup {
  id: string
  memoryType: ManagedMemoryKind
  title: string
  entries: ManagedMemoryRecord[]
}

export interface MemoryManagementStats {
  profileCount: number
  episodicCount: number
  proceduralCount: number
  policyCount: number
  semanticCount: number
  metaCount: number
  stableCount: number
  candidateCount: number
  conflictCount: number
}

export interface MemoryManagementSnapshot {
  stats: MemoryManagementStats
  stableRecords: ManagedMemoryRecord[]
  candidateRecords: ManagedMemoryRecord[]
  conflicts: MemoryConflictGroup[]
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
  inputReady: boolean
  inputMessage: string | null
}

export interface TranscriptionResult {
  text: string
  language: string | null
  durationMs: number
}

export interface WhisperPushToTalkEvent {
  state: 'pressed' | 'released'
  shortcut: string
}

export interface DownloadProgress {
  model: WhisperModel
  downloadedBytes: number
  totalBytes: number
  progressPercent: number
}

// ============================================================================
// Codex 更新类型
// ============================================================================

export interface CodexUpdateStatus {
  currentVersion: string | null
  latestVersion: string | null
  updateAvailable: boolean
  installPath: string | null
  message: string
}

export interface AppUpdateStatus {
  currentVersion: string | null
  latestVersion: string | null
  updateAvailable: boolean
  releaseUrl: string | null
  downloadUrl: string | null
  assetName: string | null
  message: string
}

export interface ResearchBriefSection {
  title: string
  summary: string
  bullets: string[]
}

export interface ResearchBriefAlert {
  id: string
  severity: 'info' | 'watch' | 'urgent'
  title: string
  summary: string
}

export interface ResearchFundQuote {
  assetType: string
  code: string
  name: string
  estimateNav?: number | null
  previousNav?: number | null
  changePercent?: number | null
  estimateTime?: string | null
  note?: string | null
}

export interface ResearchBriefSnapshot {
  generatedAt: number
  dayKey: string
  enabled: boolean
  title: string
  summary: string
  sections: ResearchBriefSection[]
  alerts: ResearchBriefAlert[]
  fundQuotes: ResearchFundQuote[]
  memoryHints: string[]
  alertFingerprint: string
  hasUpdates: boolean
  startupPopupDue: boolean
  updateSummary?: string | null
  analysisStatus: 'disabled' | 'unavailable' | 'error' | 'ready' | string
  analysisProviderLabel?: string | null
  analysisResult?: string | null
  analysisNotice?: string | null
}
