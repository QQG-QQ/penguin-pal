import { invoke } from '@tauri-apps/api/core'
import { emit, emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type {
  ActionApprovalRequest,
  AiConstraintProfile,
  ActionExecutionResult,
  AssistantSnapshot,
  ChatMessage,
  ChatResponse,
  OAuthFlowResult,
  OAuthState,
  ProviderConfigInput,
  ProviderKind
} from '../types/assistant'

const providerModels: Record<ProviderKind, string> = {
  mock: 'penguin-guardian',
  openAi: 'gpt-4.1-mini',
  anthropic: 'claude-3-5-sonnet-latest',
  openAiCompatible: 'llama3.1'
}

const defaultOAuthState = (): OAuthState => ({
  status: 'signedOut',
  authorizeUrl: null,
  tokenUrl: null,
  clientId: null,
  redirectUrl: 'http://127.0.0.1:8976/oauth/callback',
  scopes: [],
  accountHint: null,
  pendingAuthUrl: null,
  accessTokenLoaded: false,
  lastError: null,
  startedAt: null,
  expiresAt: null
})

const defaultConstraintsProfile = (): AiConstraintProfile => ({
  label: 'Codex Guardrails',
  version: '2026-03-10',
  summary: '这套约束由后端强制执行，角色设定只能补充风格，不能覆盖安全边界。',
  immutableRules: [
    {
      id: 'no-freeform-exec',
      title: '禁止自由执行',
      summary: 'AI 不能直接执行 shell、脚本、下载、安装、浏览器自动化或任意软件控制。',
      status: '硬限制'
    },
    {
      id: 'whitelist-only-actions',
      title: '只允许白名单动作',
      summary: '任何电脑控制都必须走后端白名单，高风险动作还要经过一次性确认。',
      status: '硬限制'
    },
    {
      id: 'privacy-first',
      title: '禁止隐私外泄',
      summary: 'AI 不能请求、上传、整理或暴露密钥、令牌、密码、私人文件和聊天隐私。',
      status: '硬限制'
    }
  ],
  capabilityGates: [
    {
      id: 'chat',
      title: '对话陪伴',
      summary: '允许正常对话、提醒、解释风险和引导用户使用受控入口。',
      status: '可用'
    },
    {
      id: 'model-gateway',
      title: '模型网关访问',
      summary: '当前演示模式不连接外部 AI 网关。',
      status: '已阻止'
    },
    {
      id: 'desktop-actions',
      title: '桌面动作申请',
      summary: '仅允许白名单动作，而且高风险动作仍然需要人工确认。',
      status: '需审批'
    }
  ],
  runtimeBoundaries: [
    {
      id: 'permission-level',
      title: '权限等级',
      summary: '当前演示快照默认处于 L1。',
      status: 'L1'
    },
    {
      id: 'auth-mode',
      title: '认证门禁',
      summary: '演示模式不持有 API Key 或 OAuth 令牌。',
      status: '演示'
    }
  ]
})

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T
const now = () => Date.now()

const fallbackMessage = (role: ChatMessage['role'], content: string): ChatMessage => ({
  id: `${role}-${now()}`,
  role,
  content,
  createdAt: now()
})

const buildFallbackSnapshot = (): AssistantSnapshot => ({
  mode: 'idle',
  messages: [
    fallbackMessage(
      'assistant',
      'PenguinPal 已进入 UI 演示模式。你可以先确认桌宠交互、语音入口、OAuth 设置和严格动作确认流，再接入真实 AI Key 或 OAuth 网关。'
    )
  ],
  provider: {
    kind: 'mock',
    model: providerModels.mock,
    baseUrl: null,
    systemPrompt:
      '你是一只严格遵循白名单和人工确认流程的管理员企鹅助手。永远不要执行未授权的电脑操作。',
    allowNetwork: false,
    voiceReply: true,
    retainHistory: true,
    apiKeyLoaded: false,
    authMode: 'apiKey',
    oauth: defaultOAuthState()
  },
  permissionLevel: 1,
  allowedActions: [
    {
      id: 'show_window',
      title: '显示主面板',
      summary: '重新显示桌宠控制面板。',
      riskLevel: 0,
      minimumLevel: 0,
      requiresConfirmation: false,
      enabled: true
    },
    {
      id: 'hide_window',
      title: '收起主面板',
      summary: '保留托盘驻留，仅隐藏主窗口。',
      riskLevel: 0,
      minimumLevel: 0,
      requiresConfirmation: false,
      enabled: true
    },
    {
      id: 'open_notepad',
      title: '打开记事本',
      summary: '示例级白名单动作，需要更严格的一次性确认。',
      riskLevel: 2,
      minimumLevel: 2,
      requiresConfirmation: true,
      enabled: false
    },
    {
      id: 'open_calculator',
      title: '打开计算器',
      summary: '示例级白名单动作，需要更严格的一次性确认。',
      riskLevel: 2,
      minimumLevel: 2,
      requiresConfirmation: true,
      enabled: false
    }
  ],
  auditTrail: [
    {
      id: `audit-${now()}`,
      action: 'ui_bootstrap',
      outcome: 'demo',
      detail: '当前运行在浏览器/无 Tauri 后端的演示模式。',
      createdAt: now(),
      riskLevel: 0
    }
  ],
  audioProfile: {
    inputMode: 'auto-listen',
    outputMode: 'speech-synthesis',
    stages: [
      {
        id: 'recorder',
        title: '自动语音监听',
        summary: '检测到麦克风后，前端会优先使用 Web Speech 自动进入监听。',
        status: 'ready'
      },
      {
        id: 'transcribe',
        title: '语音转写',
        summary: '识别完成后自动回填到对话框。',
        status: 'ready'
      },
      {
        id: 'tts',
        title: '语音播报',
        summary: '助手回复可使用系统语音播报。',
        status: 'ready'
      }
    ]
  },
  aiConstraints: defaultConstraintsProfile()
})

let fallbackSnapshot = buildFallbackSnapshot()
let fallbackPendingApproval: ActionApprovalRequest | null = null
let fallbackOAuthStateValue = 'demo-oauth-state'

const isTauriRuntime = () =>
  typeof window !== 'undefined' && typeof window.__TAURI_INTERNALS__ !== 'undefined'

export type SettingsSection = 'settings' | 'actions'

const SETTINGS_WINDOW_LABEL = 'settings'
const SNAPSHOT_UPDATED_EVENT = 'penguinpal://assistant-snapshot'
const SETTINGS_SECTION_EVENT = 'penguinpal://settings-section'

let browserSettingsWindow: Window | null = null

const normalizeSettingsSection = (value: string | null | undefined): SettingsSection =>
  value === 'actions' ? 'actions' : 'settings'

const settingsWindowUrl = (section: SettingsSection) =>
  `/?view=settings&section=${section}`

export const isSettingsWindowView = (): boolean => {
  if (typeof window === 'undefined') {
    return false
  }

  return new URL(window.location.href).searchParams.get('view') === 'settings'
}

export const readRequestedSettingsSection = (): SettingsSection => {
  if (typeof window === 'undefined') {
    return 'settings'
  }

  return normalizeSettingsSection(new URL(window.location.href).searchParams.get('section'))
}

export const publishAssistantSnapshot = async (snapshot: AssistantSnapshot): Promise<void> => {
  if (!isTauriRuntime()) {
    return
  }

  await emit(SNAPSHOT_UPDATED_EVENT, snapshot)
}

export const listenForAssistantSnapshot = async (
  handler: (snapshot: AssistantSnapshot) => void
): Promise<UnlistenFn | null> => {
  if (!isTauriRuntime()) {
    return null
  }

  return listen<AssistantSnapshot>(SNAPSHOT_UPDATED_EVENT, (event) => {
    handler(event.payload)
  })
}

export const listenForSettingsSectionChange = async (
  handler: (section: SettingsSection) => void
): Promise<UnlistenFn | null> => {
  if (!isTauriRuntime()) {
    return null
  }

  return listen<{ section?: string }>(SETTINGS_SECTION_EVENT, (event) => {
    handler(normalizeSettingsSection(event.payload?.section))
  })
}

export const openSettingsWindow = async (section: SettingsSection = 'settings'): Promise<boolean> => {
  const url = settingsWindowUrl(section)

  if (!isTauriRuntime()) {
    if (typeof window === 'undefined') {
      return false
    }

    browserSettingsWindow = window.open(url, 'PenguinPalSettings', 'width=860,height=760')
    browserSettingsWindow?.focus()
    return browserSettingsWindow !== null
  }

  const existing = await WebviewWindow.getByLabel(SETTINGS_WINDOW_LABEL)
  if (existing) {
    try {
      await existing.unminimize()
    } catch {
      // no-op
    }
    await existing.show()
    await existing.setFocus()
    await emitTo(SETTINGS_WINDOW_LABEL, SETTINGS_SECTION_EVENT, { section })
    return true
  }

  return await new Promise<boolean>((resolve) => {
    const settingsWindow = new WebviewWindow(SETTINGS_WINDOW_LABEL, {
      url,
      title: 'PenguinPal 设置',
      width: 820,
      height: 760,
      minWidth: 680,
      minHeight: 620,
      resizable: true,
      decorations: true,
      transparent: false,
      center: true,
      focus: true,
      alwaysOnTop: false,
      skipTaskbar: false
    })

    void settingsWindow.once('tauri://created', async () => {
      await emitTo(SETTINGS_WINDOW_LABEL, SETTINGS_SECTION_EVENT, { section })
      resolve(true)
    })

    void settingsWindow.once('tauri://error', () => {
      resolve(false)
    })
  })
}

export const closeSettingsWindow = async (): Promise<boolean> => {
  if (!isTauriRuntime()) {
    if (browserSettingsWindow && !browserSettingsWindow.closed) {
      browserSettingsWindow.close()
      browserSettingsWindow = null
      return true
    }

    return false
  }

  const currentWindow = getCurrentWindow()
  if (currentWindow.label === SETTINGS_WINDOW_LABEL) {
    await currentWindow.hide()
    return true
  }

  const existing = await WebviewWindow.getByLabel(SETTINGS_WINDOW_LABEL)
  if (!existing) {
    return false
  }

  await existing.hide()
  return true
}

const safeInvoke = async <T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> => {
  if (!isTauriRuntime()) {
    throw new Error('Tauri backend unavailable')
  }

  return invoke<T>(command, args)
}

const snapshotWithRuntimeFlags = (snapshot: AssistantSnapshot): AssistantSnapshot => ({
  ...snapshot,
  provider: {
    ...snapshot.provider,
    apiKeyLoaded: Boolean(snapshot.provider.apiKeyLoaded),
    oauth: {
      ...snapshot.provider.oauth,
      scopes: [...snapshot.provider.oauth.scopes],
      accessTokenLoaded: Boolean(snapshot.provider.oauth.accessTokenLoaded)
    }
  }
})

const nextMockReply = (content: string) => {
  if (content.includes('安全') || content.includes('权限')) {
    return '当前是严格白名单模式。AI 只能建议动作，真正的电脑控制必须走一次性授权票据并经过人工逐项确认。'
  }

  if (content.includes('OAuth') || content.includes('登录')) {
    return '现在的设置里已经有 OAuth 准备流。它默认采用 PKCE 授权码思路，但前提是你的上游模型网关真的支持 OAuth bearer token。'
  }

  if (content.includes('记事本') || content.includes('计算器') || content.includes('控制电脑')) {
    return '桌面控制入口已经准备好了，但仍然只允许白名单动作。高风险动作会弹出逐项确认清单和确认短语输入。'
  }

  if (content.includes('语音')) {
    return '检测到麦克风后会自动进入语音监听，文字输入仍然随时可用。回复也会默认进行系统语音播报，并同步显示头顶气泡。'
  }

  return 'UI、安全壳、OAuth 准备流和更严格的动作确认协议已经就位。下一步可以继续接入真实模型网关和 Windows 真机验证。'
}

const createFallbackApproval = (actionId: string): ActionApprovalRequest => {
  const action = fallbackSnapshot.allowedActions.find((item) => item.id === actionId)
  if (!action) {
    throw new Error('未找到动作定义')
  }

  return {
    id: `approval-${now()}`,
    action,
    prompt: `你即将执行“${action.title}”。这次授权只对本次动作生效，不会开放后续自由控制。`,
    requiredPhrase: `确认执行 ${action.title}`,
    checks: [
      {
        id: 'one_time',
        label: '我确认这是一次性授权，不会放开自由控制电脑的权限'
      },
      {
        id: 'visible_effect',
        label: '我知道这个动作会直接影响当前 Windows 软件或窗口状态'
      },
      {
        id: 'privacy_boundary',
        label: '我确认本次动作不应读取、上传或暴露我的隐私数据'
      }
    ],
    createdAt: now(),
    expiresAt: now() + 2 * 60 * 1000
  }
}

export const getAssistantSnapshot = async (): Promise<AssistantSnapshot> => {
  try {
    const snapshot = await safeInvoke<AssistantSnapshot>('get_assistant_snapshot')
    return snapshotWithRuntimeFlags(snapshot)
  } catch {
    return clone(fallbackSnapshot)
  }
}

export const saveProviderConfig = async (
  input: ProviderConfigInput
): Promise<AssistantSnapshot> => {
  try {
    const snapshot = await safeInvoke<AssistantSnapshot>('save_provider_config', { input })
    return snapshotWithRuntimeFlags(snapshot)
  } catch {
    const oauth = {
      ...fallbackSnapshot.provider.oauth,
      authorizeUrl: input.oauthAuthorizeUrl,
      tokenUrl: input.oauthTokenUrl,
      clientId: input.oauthClientId,
      redirectUrl: input.oauthRedirectUrl || fallbackSnapshot.provider.oauth.redirectUrl,
      scopes: input.oauthScopes
        .split(/[\s,]+/)
        .map((value) => value.trim())
        .filter(Boolean)
    }

    fallbackSnapshot = {
      ...fallbackSnapshot,
      provider: {
        ...fallbackSnapshot.provider,
        kind: input.kind,
        model: input.model || providerModels[input.kind],
        baseUrl: input.baseUrl,
        systemPrompt: input.systemPrompt,
        allowNetwork: input.allowNetwork,
        voiceReply: input.voiceReply,
        retainHistory: input.retainHistory,
        apiKeyLoaded: Boolean(input.apiKey && input.apiKey.trim()),
        authMode: input.authMode,
        oauth: {
          ...oauth,
          accessTokenLoaded: input.clearOAuthToken ? false : oauth.accessTokenLoaded,
          status: input.clearOAuthToken ? 'signedOut' : oauth.status,
          pendingAuthUrl: input.clearOAuthToken ? null : oauth.pendingAuthUrl,
          accountHint: input.clearOAuthToken ? null : oauth.accountHint,
          lastError: input.clearOAuthToken ? null : oauth.lastError,
          startedAt: input.clearOAuthToken ? null : oauth.startedAt,
          expiresAt: input.clearOAuthToken ? null : oauth.expiresAt
        }
      },
      permissionLevel: input.permissionLevel,
      allowedActions: fallbackSnapshot.allowedActions.map((action) => ({
        ...action,
        enabled: input.permissionLevel >= action.minimumLevel
      }))
    }
    return clone(fallbackSnapshot)
  }
}

export const startOAuthSignIn = async (): Promise<OAuthFlowResult> => {
  try {
    return await safeInvoke<OAuthFlowResult>('start_oauth_sign_in')
  } catch {
    if (fallbackSnapshot.provider.authMode !== 'oauth') {
      throw new Error('请先在设置中把认证方式切换到 OAuth。')
    }

    const oauth = fallbackSnapshot.provider.oauth
    if (!oauth.authorizeUrl || !oauth.clientId || !oauth.redirectUrl) {
      throw new Error('OAuth 配置不完整：至少需要 Client ID、Authorize URL 和 Redirect URL。')
    }

    fallbackOAuthStateValue = `demo-state-${now()}`
    const url = new URL(oauth.authorizeUrl)
    url.searchParams.set('response_type', 'code')
    url.searchParams.set('client_id', oauth.clientId)
    url.searchParams.set('redirect_uri', oauth.redirectUrl)
    url.searchParams.set('state', fallbackOAuthStateValue)
    url.searchParams.set('code_challenge_method', 'S256')
    url.searchParams.set('code_challenge', 'demo-code-challenge')
    if (oauth.scopes.length > 0) {
      url.searchParams.set('scope', oauth.scopes.join(' '))
    }

    fallbackSnapshot = {
      ...fallbackSnapshot,
      provider: {
        ...fallbackSnapshot.provider,
        oauth: {
          ...oauth,
          status: 'pending',
          pendingAuthUrl: url.toString(),
          startedAt: now(),
          expiresAt: now() + 5 * 60 * 1000,
          lastError: null
        }
      },
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: 'oauth_login_started',
          outcome: 'demo',
          detail: '浏览器演示模式仅生成 OAuth 授权链接，不会真正访问远端登录。',
          createdAt: now(),
          riskLevel: 1
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }

    return {
      message: '已生成 OAuth 授权链接。登录完成后，把浏览器回调地址粘贴回来。',
      authorizationUrl: fallbackSnapshot.provider.oauth.pendingAuthUrl,
      snapshot: clone(fallbackSnapshot)
    }
  }
}

export const completeOAuthSignIn = async (callbackUrl: string): Promise<OAuthFlowResult> => {
  try {
    return await safeInvoke<OAuthFlowResult>('complete_oauth_sign_in', { callbackUrl })
  } catch {
    if (!callbackUrl.trim()) {
      throw new Error('请先粘贴浏览器回调地址。')
    }

    const url = new URL(callbackUrl.trim())
    const returnedState = url.searchParams.get('state')
    const code = url.searchParams.get('code')

    if (returnedState !== fallbackOAuthStateValue) {
      throw new Error('OAuth 状态校验失败，请重新生成授权链接。')
    }

    if (!code) {
      throw new Error('回调地址中没有 code，无法完成登录。')
    }

    fallbackSnapshot = {
      ...fallbackSnapshot,
      provider: {
        ...fallbackSnapshot.provider,
        oauth: {
          ...fallbackSnapshot.provider.oauth,
          status: 'authorized',
          pendingAuthUrl: null,
          accessTokenLoaded: true,
          accountHint: 'demo-oauth-user',
          lastError: null,
          startedAt: null,
          expiresAt: now() + 60 * 60 * 1000
        }
      },
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: 'oauth_login_completed',
          outcome: 'demo',
          detail: '浏览器演示模式已在内存中标记 OAuth 登录成功。',
          createdAt: now(),
          riskLevel: 1
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }

    return {
      message: 'OAuth 演示登录成功。当前仅把访问令牌状态保留在运行内存中。',
      authorizationUrl: null,
      snapshot: clone(fallbackSnapshot)
    }
  }
}

export const disconnectOAuthSignIn = async (): Promise<OAuthFlowResult> => {
  try {
    return await safeInvoke<OAuthFlowResult>('disconnect_oauth_sign_in')
  } catch {
    fallbackSnapshot = {
      ...fallbackSnapshot,
      provider: {
        ...fallbackSnapshot.provider,
        oauth: {
          ...fallbackSnapshot.provider.oauth,
          status: 'signedOut',
          pendingAuthUrl: null,
          accessTokenLoaded: false,
          accountHint: null,
          lastError: null,
          startedAt: null,
          expiresAt: null
        }
      },
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: 'oauth_logout',
          outcome: 'demo',
          detail: '浏览器演示模式已清空 OAuth 登录状态。',
          createdAt: now(),
          riskLevel: 0
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }

    return {
      message: '已退出 OAuth 登录，并清空内存中的令牌状态。',
      authorizationUrl: null,
      snapshot: clone(fallbackSnapshot)
    }
  }
}

export const sendChatMessage = async (content: string): Promise<ChatResponse> => {
  try {
    return await safeInvoke<ChatResponse>('send_chat_message', { content })
  } catch {
    const userMessage = fallbackMessage('user', content)
    const replyMessage = fallbackMessage('assistant', nextMockReply(content))
    fallbackSnapshot = {
      ...fallbackSnapshot,
      mode: 'idle',
      messages: [...fallbackSnapshot.messages, userMessage, replyMessage],
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: 'chat_completion',
          outcome: 'mock',
          detail: '当前为本地 UI 演示回复。',
          createdAt: now(),
          riskLevel: 0
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }
    return {
      reply: replyMessage,
      providerLabel: 'Mock Assistant',
      snapshot: clone(fallbackSnapshot)
    }
  }
}

export const requestDesktopAction = async (
  actionId: string
): Promise<ActionExecutionResult> => {
  try {
    return await safeInvoke<ActionExecutionResult>('request_desktop_action', {
      actionId
    })
  } catch {
    const selectedAction = fallbackSnapshot.allowedActions.find((action) => action.id === actionId)

    if (!selectedAction) {
      throw new Error('未找到动作定义')
    }

    if (!selectedAction.enabled) {
      throw new Error('当前权限级别不允许执行该动作')
    }

    if (selectedAction.requiresConfirmation) {
      const approvalRequest = createFallbackApproval(actionId)
      fallbackPendingApproval = approvalRequest
      fallbackSnapshot = {
        ...fallbackSnapshot,
        auditTrail: [
          {
            id: `audit-${now()}`,
            action: 'action_approval_requested',
            outcome: 'demo',
            detail: `${selectedAction.title} 已进入一次性授权确认阶段。`,
            createdAt: now(),
            riskLevel: selectedAction.riskLevel
          },
          ...fallbackSnapshot.auditTrail
        ].slice(0, 8)
      }

      return {
        status: 'needs_confirmation',
        message: `${selectedAction.title} 需要逐项确认后才能执行。`,
        snapshot: clone(fallbackSnapshot),
        approvalRequest
      }
    }

    fallbackSnapshot = {
      ...fallbackSnapshot,
      mode: 'idle',
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: actionId,
          outcome: 'demo',
          detail: '浏览器演示模式未真正调用系统能力。',
          createdAt: now(),
          riskLevel: selectedAction.riskLevel
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }

    return {
      status: 'demo',
      message: `${selectedAction.title} 已通过演示模式记录审计，但未真正执行系统操作。`,
      snapshot: clone(fallbackSnapshot),
      approvalRequest: null
    }
  }
}

export const confirmDesktopAction = async (
  approvalId: string,
  typedPhrase: string,
  acknowledgedChecks: string[]
): Promise<ActionExecutionResult> => {
  try {
    return await safeInvoke<ActionExecutionResult>('confirm_desktop_action', {
      approvalId,
      typedPhrase,
      acknowledgedChecks
    })
  } catch {
    if (!fallbackPendingApproval || fallbackPendingApproval.id !== approvalId) {
      throw new Error('未找到待确认的动作授权。')
    }

    if (fallbackPendingApproval.expiresAt < now()) {
      fallbackPendingApproval = null
      throw new Error('这次动作授权已经过期，请重新发起。')
    }

    if (typedPhrase.trim() !== fallbackPendingApproval.requiredPhrase) {
      throw new Error(`请完整输入确认短语：${fallbackPendingApproval.requiredPhrase}`)
    }

    const acknowledged = new Set(acknowledgedChecks)
    const missing = fallbackPendingApproval.checks.find((check) => !acknowledged.has(check.id))
    if (missing) {
      throw new Error('请先完成所有确认项。')
    }

    const action = fallbackPendingApproval.action
    fallbackPendingApproval = null
    fallbackSnapshot = {
      ...fallbackSnapshot,
      mode: 'idle',
      auditTrail: [
        {
          id: `audit-${now()}`,
          action: action.id,
          outcome: 'demo',
          detail: '演示模式已通过更严格的确认流记录本次动作，但未真正执行系统操作。',
          createdAt: now(),
          riskLevel: action.riskLevel
        },
        ...fallbackSnapshot.auditTrail
      ].slice(0, 8)
    }

    return {
      status: 'demo',
      message: `${action.title} 已通过演示模式完成更严格的确认流。`,
      snapshot: clone(fallbackSnapshot),
      approvalRequest: null
    }
  }
}

export const cancelDesktopActionApproval = async (
  approvalId: string
): Promise<AssistantSnapshot> => {
  try {
    const snapshot = await safeInvoke<AssistantSnapshot>('cancel_desktop_action_approval', {
      approvalId
    })
    return snapshotWithRuntimeFlags(snapshot)
  } catch {
    if (fallbackPendingApproval?.id === approvalId) {
      fallbackSnapshot = {
        ...fallbackSnapshot,
        auditTrail: [
          {
            id: `audit-${now()}`,
            action: 'action_approval_cancelled',
            outcome: 'demo',
            detail: `${fallbackPendingApproval.action.title} 的一次性授权已被取消。`,
            createdAt: now(),
            riskLevel: fallbackPendingApproval.action.riskLevel
          },
          ...fallbackSnapshot.auditTrail
        ].slice(0, 8)
      }
      fallbackPendingApproval = null
    }

    return clone(fallbackSnapshot)
  }
}

export const clearConversation = async (): Promise<AssistantSnapshot> => {
  try {
    const snapshot = await safeInvoke<AssistantSnapshot>('clear_conversation')
    return snapshotWithRuntimeFlags(snapshot)
  } catch {
    fallbackSnapshot = buildFallbackSnapshot()
    fallbackPendingApproval = null
    return clone(fallbackSnapshot)
  }
}

export const hideAssistantWindow = async (): Promise<boolean> => {
  if (!isTauriRuntime()) {
    return false
  }

  try {
    await getCurrentWindow().hide()
    return true
  } catch (error) {
    throw error instanceof Error ? error : new Error('桌宠隐藏失败，请改用托盘菜单恢复或退出。')
  }
}
