import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  timestamp: number;
}

export type AiProvider = 'claude' | 'gemini';

export interface ChatSession {
  id: string;
  name: string;
  provider: AiProvider;
  model: string;
  messages: ChatMessage[];
  createdAt: number;
}

export const CLAUDE_MODELS = [
  { id: 'claude-sonnet-4-6', label: 'Claude Sonnet 4.6' },
  { id: 'claude-opus-4-6', label: 'Claude Opus 4.6' },
  { id: 'claude-haiku-4-5-20251001', label: 'Claude Haiku 4.5' },
];

export const GEMINI_MODELS = [
  { id: 'gemini-2.5-pro', label: 'Gemini 2.5 Pro' },
  { id: 'gemini-2.5-flash', label: 'Gemini 2.5 Flash' },
];

function modelLabel(provider: AiProvider, modelId: string): string {
  const list = provider === 'claude' ? CLAUDE_MODELS : GEMINI_MODELS;
  return list.find((m) => m.id === modelId)?.label ?? modelId;
}

interface ChatState {
  isOpen: boolean;
  sessions: ChatSession[];
  activeSessionId: string | null;
  isLoading: boolean;
  toolInfo: string | null;
  /** Session IDs that have been registered with the sidecar in this app run (not persisted) */
  _registeredSessions: Set<string>;

  open: () => void;
  close: () => void;
  toggle: () => void;

  createSession: (provider: AiProvider, model: string) => Promise<void>;
  selectSession: (id: string) => void;
  deleteSession: (id: string) => Promise<void>;

  addMessage: (role: ChatMessage['role'], content: string) => void;
  sendMessage: (content: string) => Promise<void>;
  cancelMessage: () => Promise<void>;
  clear: () => void;
}

/** Detect CLI path for the given provider via Tauri */
async function getCliPath(provider: AiProvider): Promise<{ cliType: string; cliPath: string }> {
  try {
    const result = await invoke<{ found: boolean; cliType: string; cliPath: string }>(
      'detect_cli_path',
      { provider }
    );
    if (result.found) return { cliType: result.cliType, cliPath: result.cliPath };
  } catch {
    // ignore
  }
  return { cliType: provider, cliPath: provider };
}

export const useChatStore = create<ChatState>()(
  persist(
    (set, get) => ({
      isOpen: false,
      sessions: [],
      activeSessionId: null,
      isLoading: false,
      toolInfo: null,
      _registeredSessions: new Set<string>(),

      open: () => set({ isOpen: true }),
      close: () => set({ isOpen: false }),
      toggle: () => set((s) => ({ isOpen: !s.isOpen })),

      createSession: async (provider, model) => {
        const id = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
        const session: ChatSession = {
          id,
          name: modelLabel(provider, model),
          provider,
          model,
          messages: [],
          createdAt: Date.now(),
        };
        set((s) => ({
          sessions: [...s.sessions.slice(-9), session],
          activeSessionId: id,
        }));

        // 사이드카에 세션 등록
        try {
          const { cliType, cliPath } = await getCliPath(provider);
          await invoke('chat_start_session', {
            sessionId: id,
            cliType,
            cliPath,
            model,
          });
          get()._registeredSessions.add(id);
        } catch (e) {
          console.error('[chat] chat_start_session failed:', e);
        }
      },

      selectSession: (id) => set({ activeSessionId: id }),

      deleteSession: async (id) => {
        // 백엔드 정리 먼저 (실패해도 UI는 진행)
        try {
          await invoke('chat_close_session', { sessionId: id });
        } catch {
          // ignore — sidecar가 없을 수도 있음
        }
        get()._registeredSessions.delete(id);
        set((s) => {
          const sessions = s.sessions.filter((sess) => sess.id !== id);
          const activeSessionId =
            s.activeSessionId === id
              ? (sessions[sessions.length - 1]?.id ?? null)
              : s.activeSessionId;
          return { sessions, activeSessionId };
        });
      },

      addMessage: (role, content) => {
        const { activeSessionId } = get();
        if (!activeSessionId) return;
        const msg: ChatMessage = {
          id: Date.now().toString(),
          role,
          content,
          timestamp: Date.now(),
        };
        set((s) => ({
          sessions: s.sessions.map((sess) =>
            sess.id === activeSessionId
              ? { ...sess, messages: [...sess.messages, msg] }
              : sess,
          ),
        }));
      },

      sendMessage: async (content) => {
        const { activeSessionId, sessions } = get();
        if (!activeSessionId) return;
        const session = sessions.find((s) => s.id === activeSessionId);
        if (!session) return;

        // 앱 재시작 후 localStorage 복원 세션은 sidecar에 미등록 → 자동 등록
        if (!get()._registeredSessions.has(activeSessionId)) {
          try {
            const { cliType, cliPath } = await getCliPath(session.provider);
            await invoke('chat_start_session', {
              sessionId: activeSessionId,
              cliType,
              cliPath,
              model: session.model,
            });
            get()._registeredSessions.add(activeSessionId);
          } catch (e) {
            console.error('[chat] auto chat_start_session failed:', e);
          }
        }

        // 1. 유저 메시지 추가
        get().addMessage('user', content);

        // 2. 로딩 상태 + 빈 플레이스홀더
        set({ isLoading: true, toolInfo: null });
        const placeholderId = `ph-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
        const placeholder: ChatMessage = {
          id: placeholderId,
          role: 'assistant',
          content: '',
          timestamp: Date.now(),
        };
        set((s) => ({
          sessions: s.sessions.map((sess) =>
            sess.id === activeSessionId
              ? { ...sess, messages: [...sess.messages, placeholder] }
              : sess,
          ),
        }));

        const updatePlaceholder = (newContent: string) => {
          set((s) => ({
            sessions: s.sessions.map((sess) =>
              sess.id === activeSessionId
                ? {
                    ...sess,
                    messages: sess.messages.map((m) =>
                      m.id === placeholderId ? { ...m, content: newContent } : m,
                    ),
                  }
                : sess,
            ),
          }));
        };

        // 3. 스트리밍 이벤트 구독
        const eventName = `chat-stream:${activeSessionId}`;
        let accumulated = '';
        const unlisten = await listen<{
          type: string;
          content?: string;
          message?: string;
          status?: string;
          toolName?: string;
          input?: Record<string, unknown>;
        }>(eventName, (event) => {
          const payload = event.payload;
          switch (payload.type) {
            case 'text': {
              if (payload.content !== undefined) {
                accumulated += payload.content;
                updatePlaceholder(accumulated);
              }
              break;
            }
            case 'tool_use': {
              if (payload.status === 'running') {
                const inputStr = payload.input ? JSON.stringify(payload.input) : '';
                set({ toolInfo: `${payload.toolName ?? ''} ${inputStr}`.trim() });
              } else {
                set({ toolInfo: null });
              }
              break;
            }
            case 'result': {
              if (payload.content !== undefined && payload.content) {
                updatePlaceholder(payload.content);
              }
              set({ toolInfo: null });
              break;
            }
            case 'error': {
              updatePlaceholder(`오류: ${payload.message ?? 'Unknown error'}`);
              break;
            }
          }
        });

        try {
          await invoke('chat_send_message', {
            sessionId: activeSessionId,
            message: content,
          });
        } catch (e) {
          updatePlaceholder(`오류: ${String(e)}`);
        } finally {
          unlisten();
          set((s) => ({
            isLoading: false,
            toolInfo: null,
            sessions: s.sessions.map((sess) =>
              sess.id === activeSessionId
                ? {
                    ...sess,
                    messages: sess.messages.filter(
                      (m) => !(m.id === placeholderId && m.content === ''),
                    ),
                  }
                : sess,
            ),
          }));
        }
      },

      cancelMessage: async () => {
        const { activeSessionId } = get();
        if (!activeSessionId) return;
        try {
          await invoke('chat_cancel', { sessionId: activeSessionId });
        } catch {
          // ignore
        }
        set({ isLoading: false, toolInfo: null });
      },

      clear: () => {
        const { activeSessionId } = get();
        if (!activeSessionId) return;
        set((s) => ({
          sessions: s.sessions.map((sess) =>
            sess.id === activeSessionId ? { ...sess, messages: [] } : sess,
          ),
        }));
      },
    }),
    {
      name: 'doxus-chat-sessions',
      partialize: (state) => ({
        sessions: state.sessions,
        activeSessionId: state.activeSessionId,
        // _registeredSessions은 앱 런타임 전용 — 저장하지 않음
      }),
    },
  ),
);
