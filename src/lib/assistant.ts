import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type {
  ActionExecutionResult,
  AssistantSnapshot,
  ChatMessage,
  ChatResponse,
  ProviderConfigInput,
  ProviderKind
} from '../types/assistant'

const providerModels: Record<ProviderKind, string> = {
  mock: 'penguin-guardian',
  openAi: 'gpt-4.1-mini',
  anthropic: 'claude-3-5-sonnet-latest',
  openAiCompatible: 'llama3.1'
}

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
      'PenguinPal 已进入 UI 演示模式。你可以先确认桌宠交互、语音入口和安全动作面板，再接入真实 AI Key。'
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
    apiKeyLoaded: false
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
      summary: '示例级白名单动作，需要人工确认。',
      riskLevel: 2,
      minimumLevel: 2,
      requiresConfirmation: true,
      enabled: false
    },
    {
      id: 'open_calculator',
      title: '打开计算器',
      summary: '示例级白名单动作，需要人工确认。',
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
    inputMode: 'press-to-talk',
    outputMode: 'speech-synthesis',
    stages: [
      {
        id: 'recorder',
        title: '按住说话',
        summary: '前端优先使用 Web Speech 录音入口。',
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
  }
})

let fallbackSnapshot = buildFallbackSnapshot()

const isTauriRuntime = () =>
  typeof window !== 'undefined' && typeof window.__TAURI_INTERNALS__ !== 'undefined'

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
    apiKeyLoaded: Boolean(snapshot.provider.apiKeyLoaded)
  }
})

const nextMockReply = (content: string) => {
  if (content.includes('安全') || content.includes('权限')) {
    return '当前是严格白名单模式。AI 只能建议动作，真正的电脑控制必须走动作面板并经过人工确认。'
  }

  if (content.includes('记事本') || content.includes('计算器') || content.includes('控制电脑')) {
    return '桌面控制入口已经准备好了，但仍然只允许白名单动作。你可以在下方面板里手动确认执行。'
  }

  if (content.includes('语音')) {
    return '按住输入框左侧的语音按钮即可录音，松开后会自动转写并发送。回复也可以由系统语音播报。'
  }

  return 'UI 和安全壳已经搭起来了。下一步只要填入可用的模型配置和 API Key，就能切到真实 AI 对话。'
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
        apiKeyLoaded: Boolean(input.apiKey && input.apiKey.trim())
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
  actionId: string,
  confirmed = false
): Promise<ActionExecutionResult> => {
  try {
    return await safeInvoke<ActionExecutionResult>('request_desktop_action', {
      actionId,
      confirmed
    })
  } catch {
    const selectedAction = fallbackSnapshot.allowedActions.find((action) => action.id === actionId)

    if (!selectedAction) {
      throw new Error('未找到动作定义')
    }

    if (!selectedAction.enabled) {
      throw new Error('当前权限级别不允许执行该动作')
    }

    if (selectedAction.requiresConfirmation && !confirmed) {
      throw new Error('该动作需要人工确认')
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
      snapshot: clone(fallbackSnapshot)
    }
  }
}

export const clearConversation = async (): Promise<AssistantSnapshot> => {
  try {
    const snapshot = await safeInvoke<AssistantSnapshot>('clear_conversation')
    return snapshotWithRuntimeFlags(snapshot)
  } catch {
    fallbackSnapshot = buildFallbackSnapshot()
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
