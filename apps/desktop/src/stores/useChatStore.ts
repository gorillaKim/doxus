import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'thought';
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

  open: () => void;
  close: () => void;
  toggle: () => void;

  createSession: (provider: AiProvider, model: string) => void;
  selectSession: (id: string) => void;
  deleteSession: (id: string) => void;

  addMessage: (role: ChatMessage['role'], content: string) => void;
  sendMessage: (content: string) => Promise<void>;
  clear: () => void;
}

export const useChatStore = create<ChatState>((set, get) => ({
  isOpen: false,
  sessions: [],
  activeSessionId: null,
  isLoading: false,

  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false }),
  toggle: () => set((s) => ({ isOpen: !s.isOpen })),

  createSession: (provider, model) => {
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
      sessions: [...s.sessions.slice(-4), session],
      activeSessionId: id,
    }));
  },

  selectSession: (id) => set({ activeSessionId: id }),

  deleteSession: (id) =>
    set((s) => {
      const sessions = s.sessions.filter((sess) => sess.id !== id);
      const activeSessionId =
        s.activeSessionId === id
          ? (sessions[sessions.length - 1]?.id ?? null)
          : s.activeSessionId;
      return { sessions, activeSessionId };
    }),

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

    get().addMessage('user', content);
    set({ isLoading: true });

    try {
      const result = await invoke<{ text: string; session_id: string; done: boolean }>(
        'agent_send_message',
        {
          sessionId: activeSessionId,
          message: content,
          provider: session.provider,
          model: session.model,
        }
      );
      get().addMessage('assistant', result.text);
    } catch (e) {
      get().addMessage('assistant', `오류: ${String(e)}`);
    } finally {
      set({ isLoading: false });
    }
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
}));
