import { useState } from 'react';
import {
  useChatStore,
  AiProvider,
  CLAUDE_MODELS,
  GEMINI_MODELS,
} from '../../stores/useChatStore';

// ── New Session Form ──────────────────────────────────────────────────────────

function NewSessionForm({ onDone }: { onDone: () => void }) {
  const createSession = useChatStore((s) => s.createSession);
  const [provider, setProvider] = useState<AiProvider>('claude');
  const models = provider === 'claude' ? CLAUDE_MODELS : GEMINI_MODELS;
  const [model, setModel] = useState(models[0].id);

  const handleProviderChange = (p: AiProvider) => {
    setProvider(p);
    setModel(p === 'claude' ? CLAUDE_MODELS[0].id : GEMINI_MODELS[0].id);
  };

  const handleStart = () => {
    createSession(provider, model);
    onDone();
  };

  return (
    <div className="px-4 py-3 bg-gray-950 border-b border-gray-800 space-y-3">
      {/* Provider toggle */}
      <div>
        <p className="text-xs text-gray-500 mb-1.5 uppercase tracking-wider">Provider</p>
        <div className="flex gap-2">
          {(['claude', 'gemini'] as AiProvider[]).map((p) => (
            <button
              key={p}
              onClick={() => handleProviderChange(p)}
              className={`flex-1 py-1.5 rounded-md text-sm font-medium transition-colors ${
                provider === p
                  ? 'bg-indigo-600 text-white'
                  : 'bg-gray-800 text-gray-400 hover:text-gray-200'
              }`}
            >
              {p === 'claude' ? 'Claude' : 'Gemini'}
            </button>
          ))}
        </div>
      </div>

      {/* Model dropdown */}
      <div>
        <p className="text-xs text-gray-500 mb-1.5 uppercase tracking-wider">Model</p>
        <select
          value={model}
          onChange={(e) => setModel(e.target.value)}
          className="w-full bg-gray-800 border border-gray-700 text-gray-200 text-sm rounded-md px-3 py-1.5 focus:outline-none focus:ring-1 focus:ring-indigo-500"
        >
          {models.map((m) => (
            <option key={m.id} value={m.id}>
              {m.label}
            </option>
          ))}
        </select>
      </div>

      <button
        onClick={handleStart}
        className="w-full py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium rounded-md transition-colors"
      >
        Start Session
      </button>
    </div>
  );
}

// ── Session List ──────────────────────────────────────────────────────────────

function SessionList({ onClose }: { onClose: () => void }) {
  const { sessions, activeSessionId, selectSession, deleteSession } = useChatStore();

  return (
    <div className="px-3 py-2 bg-gray-950 border-b border-gray-800 space-y-1">
      {sessions.length === 0 && (
        <p className="text-xs text-gray-600 text-center py-1">No sessions yet</p>
      )}
      {sessions.map((sess) => (
        <div
          key={sess.id}
          className={`flex items-center justify-between px-3 py-1.5 rounded-lg cursor-pointer transition-colors ${
            sess.id === activeSessionId
              ? 'bg-indigo-600/20 text-indigo-300'
              : 'bg-gray-800 text-gray-400 hover:text-gray-200'
          }`}
          onClick={() => {
            selectSession(sess.id);
            onClose();
          }}
        >
          <span className="text-sm truncate">{sess.name}</span>
          <button
            onClick={(e) => {
              e.stopPropagation();
              deleteSession(sess.id);
            }}
            className="ml-2 text-gray-600 hover:text-gray-300 text-xs shrink-0"
            aria-label="Delete session"
          >
            &#x2715;
          </button>
        </div>
      ))}
    </div>
  );
}

// ── Main ChatDrawer ───────────────────────────────────────────────────────────

export function ChatDrawer() {
  const { isOpen, sessions, activeSessionId, close, addMessage } = useChatStore();
  const [showNewForm, setShowNewForm] = useState(false);
  const [showSessions, setShowSessions] = useState(false);

  const activeSession = sessions.find((s) => s.id === activeSessionId) ?? null;
  const messages = activeSession?.messages ?? [];

  const handleSend = (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (!activeSession) return;
    const fd = new FormData(e.currentTarget);
    const content = (fd.get('message') as string).trim();
    if (!content) return;
    addMessage('user', content);
    e.currentTarget.reset();
    // Stub response until agent sidecar is wired
    setTimeout(() => addMessage('assistant', `Echo: ${content}`), 300);
  };

  if (!isOpen) return null;

  return (
    <div className="fixed right-0 top-0 h-full w-96 bg-gray-900 border-l border-gray-800 shadow-2xl flex flex-col z-50">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 bg-gray-950 border-b border-gray-800 shrink-0">
        <div className="flex items-center gap-2">
          <button
            onClick={() => {
              setShowNewForm((v) => !v);
              setShowSessions(false);
            }}
            className="flex items-center gap-1 px-2.5 py-1 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium rounded-md transition-colors"
            aria-label="New session"
          >
            <span>+</span>
            <span>New</span>
          </button>
          <button
            onClick={() => {
              setShowSessions((v) => !v);
              setShowNewForm(false);
            }}
            className="flex items-center gap-1 px-2.5 py-1 bg-gray-800 hover:bg-gray-700 text-gray-300 text-xs rounded-md transition-colors"
            aria-label="Toggle session list"
          >
            <span>Sessions</span>
            <span>{showSessions ? '▲' : '▼'}</span>
          </button>
        </div>
        <button
          onClick={close}
          className="text-gray-500 hover:text-gray-200 transition-colors"
          aria-label="Close drawer"
        >
          &#x2715;
        </button>
      </div>

      {/* New session form (inline) */}
      {showNewForm && (
        <NewSessionForm onDone={() => setShowNewForm(false)} />
      )}

      {/* Session list */}
      {showSessions && (
        <SessionList onClose={() => setShowSessions(false)} />
      )}

      {/* Active session badge */}
      {activeSession && (
        <div className="px-4 py-2 bg-gray-900 border-b border-gray-800/60 shrink-0">
          <span className="inline-flex items-center gap-1.5 text-xs text-gray-500">
            <span>{activeSession.provider === 'claude' ? '🤖' : '✨'}</span>
            <span>{activeSession.name}</span>
          </span>
        </div>
      )}

      {/* Messages */}
      <div className="flex-1 overflow-auto p-4 space-y-3">
        {!activeSession && (
          <p className="text-sm text-gray-600 text-center py-8">
            Create a session to start chatting.
          </p>
        )}
        {activeSession && messages.length === 0 && (
          <p className="text-sm text-gray-600 text-center py-8">
            Ask the librarian anything about your documents.
          </p>
        )}
        {messages.map((m) => (
          <div
            key={m.id}
            className={`flex ${m.role === 'user' ? 'justify-end' : 'justify-start'}`}
          >
            <div
              className={`max-w-[80%] px-3 py-2 rounded-lg text-sm leading-relaxed ${
                m.role === 'user'
                  ? 'bg-indigo-600 text-white'
                  : m.role === 'thought'
                  ? 'bg-gray-800 text-gray-500 italic text-xs'
                  : 'bg-gray-800 text-gray-100'
              }`}
            >
              {m.content}
            </div>
          </div>
        ))}
      </div>

      {/* Input */}
      <form
        onSubmit={handleSend}
        className="p-3 border-t border-gray-800 flex gap-2 shrink-0"
      >
        <input
          name="message"
          placeholder={activeSession ? 'Ask about your documents…' : 'Start a session first'}
          disabled={!activeSession}
          className="flex-1 px-3 py-2 text-sm bg-gray-800 border border-gray-700 text-gray-200 placeholder-gray-600 rounded-md focus:outline-none focus:ring-1 focus:ring-indigo-500 disabled:opacity-40"
        />
        <button
          type="submit"
          disabled={!activeSession}
          className="px-3 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-sm rounded-md transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        >
          Send
        </button>
      </form>
    </div>
  );
}
