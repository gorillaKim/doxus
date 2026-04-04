import { create } from 'zustand';

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'thought';
  content: string;
  timestamp: number;
}

interface ChatState {
  isOpen: boolean;
  messages: ChatMessage[];
  isLoading: boolean;
  open: () => void;
  close: () => void;
  toggle: () => void;
  addMessage: (role: ChatMessage['role'], content: string) => void;
  clear: () => void;
}

export const useChatStore = create<ChatState>((set) => ({
  isOpen: false,
  messages: [],
  isLoading: false,

  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false }),
  toggle: () => set((s) => ({ isOpen: !s.isOpen })),

  addMessage: (role, content) =>
    set((s) => ({
      messages: [
        ...s.messages,
        { id: Date.now().toString(), role, content, timestamp: Date.now() },
      ],
    })),

  clear: () => set({ messages: [] }),
}));
