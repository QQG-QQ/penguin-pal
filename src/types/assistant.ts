export type PetMode = 'idle' | 'listening' | 'thinking' | 'speaking' | 'guarded'

export type ProviderKind = 'mock' | 'openAi' | 'anthropic' | 'openAiCompatible'

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

export interface ProviderConfig {
  kind: ProviderKind
  model: string
  baseUrl: string | null
  systemPrompt: string
  allowNetwork: boolean
  voiceReply: boolean
  retainHistory: boolean
  apiKeyLoaded: boolean
}

export interface AssistantSnapshot {
  mode: PetMode
  messages: ChatMessage[]
  provider: ProviderConfig
  permissionLevel: number
  allowedActions: DesktopAction[]
  auditTrail: AuditEntry[]
  audioProfile: AudioProfile
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
  apiKey?: string | null
  clearApiKey?: boolean
}

export interface ChatResponse {
  reply: ChatMessage
  providerLabel: string
  snapshot: AssistantSnapshot
}

export interface ActionExecutionResult {
  status: string
  message: string
  snapshot: AssistantSnapshot
}
