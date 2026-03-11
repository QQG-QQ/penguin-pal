import type { ProviderAuthMode, ProviderKind } from '../types/assistant'

export interface ModelCatalogEntry {
  id: string
  label: string
  kind: ProviderKind
  model: string
  baseUrl: string | null
  authMode: ProviderAuthMode
  aliases: string[]
  presetVisible: boolean
}

const normalizeModelLookup = (value: string) =>
  value
    .trim()
    .toLowerCase()
    .replace(/[\s_-]+/g, '')

export const modelCatalog: ModelCatalogEntry[] = [
  {
    id: 'codex-cli',
    label: 'Codex CLI（官方登录）',
    kind: 'codexCli',
    model: 'gpt-5-codex',
    baseUrl: null,
    authMode: 'oauth',
    aliases: ['codex', 'codexcli', 'codex-cli', 'codex cli', 'gpt-5-codex'],
    presetVisible: true
  },
  {
    id: 'openai',
    label: 'OpenAI 官方',
    kind: 'openAi',
    model: 'gpt-4.1-mini',
    baseUrl: null,
    authMode: 'apiKey',
    aliases: ['openai', 'gpt', 'gpt-4.1-mini'],
    presetVisible: true
  },
  {
    id: 'anthropic',
    label: 'Anthropic 官方',
    kind: 'anthropic',
    model: 'claude-3-5-sonnet-latest',
    baseUrl: null,
    authMode: 'apiKey',
    aliases: ['anthropic', 'claude', 'claude-3-5-sonnet-latest'],
    presetVisible: true
  },
  {
    id: 'openrouter',
    label: 'OpenRouter',
    kind: 'openAiCompatible',
    model: 'openai/gpt-4.1-mini',
    baseUrl: 'https://openrouter.ai/api/v1',
    authMode: 'apiKey',
    aliases: ['openrouter'],
    presetVisible: true
  },
  {
    id: 'deepseek',
    label: 'DeepSeek',
    kind: 'openAiCompatible',
    model: 'deepseek-chat',
    baseUrl: 'https://api.deepseek.com/v1',
    authMode: 'apiKey',
    aliases: ['deepseek', 'deepseek-chat'],
    presetVisible: true
  },
  {
    id: 'ollama',
    label: 'Ollama（本地）',
    kind: 'openAiCompatible',
    model: 'llama3.1',
    baseUrl: 'http://127.0.0.1:11434/v1',
    authMode: 'apiKey',
    aliases: ['ollama', 'llama', 'llama3.1'],
    presetVisible: true
  },
  {
    id: 'mock',
    label: 'Mock',
    kind: 'mock',
    model: 'penguin-guardian',
    baseUrl: null,
    authMode: 'apiKey',
    aliases: ['mock', 'demo', 'penguin-guardian'],
    presetVisible: false
  }
]

export const presetModelCatalog = modelCatalog.filter((entry) => entry.presetVisible)

export const findModelCatalogEntry = (value: string) => {
  const normalized = normalizeModelLookup(value)
  if (!normalized) {
    return null
  }

  return (
    modelCatalog.find((entry) =>
      [entry.id, entry.model, ...entry.aliases].some(
        (candidate) => normalizeModelLookup(candidate) === normalized
      )
    ) ?? null
  )
}
