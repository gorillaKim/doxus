import { useEffect, useRef, useState } from 'react';
import { Markdown } from '../common/Markdown';
import {
  useChatStore,
  AiProvider,
  CLAUDE_MODELS,
  GEMINI_MODELS,
} from '../../stores/useChatStore';

// ── 새 세션 폼 ──────────────────────────────────────────────────────────────

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
      <div>
        <p className="text-xs text-gray-500 mb-1.5 uppercase tracking-wider">AI 제공자</p>
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

      <div>
        <p className="text-xs text-gray-500 mb-1.5 uppercase tracking-wider">모델 선택</p>
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
        세션 시작
      </button>
    </div>
  );
}

// ── 세션 목록 ──────────────────────────────────────────────────────────────

function SessionList({ onClose }: { onClose: () => void }) {
  const { sessions, activeSessionId, selectSession, deleteSession } = useChatStore();

  return (
    <div className="px-3 py-2 bg-gray-950 border-b border-gray-800 space-y-1">
      {sessions.length === 0 && (
        <p className="text-xs text-gray-600 text-center py-1">세션이 없습니다</p>
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
            aria-label="세션 삭제"
          >
            &#x2715;
          </button>
        </div>
      ))}
    </div>
  );
}

// ── 마크다운 메시지 ──────────────────────────────────────────────────────

function MarkdownMessage({ content }: { content: string }) {
  return (
    <div className="
      text-sm text-gray-100 leading-relaxed
      [&_h1]:text-base [&_h1]:font-bold [&_h1]:text-white [&_h1]:mt-3 [&_h1]:mb-1
      [&_h2]:text-sm [&_h2]:font-semibold [&_h2]:text-white [&_h2]:mt-3 [&_h2]:mb-1
      [&_h3]:text-sm [&_h3]:font-semibold [&_h3]:text-gray-200 [&_h3]:mt-2 [&_h3]:mb-1
      [&_p]:my-1.5 [&_p]:text-gray-200
      [&_strong]:text-white [&_strong]:font-semibold
      [&_em]:text-gray-300 [&_em]:italic
      [&_a]:text-indigo-400 [&_a]:underline [&_a]:underline-offset-2 [&_a:hover]:text-indigo-300
      [&_ul]:my-1.5 [&_ul]:pl-4 [&_ul]:list-disc [&_ul]:space-y-0.5
      [&_ol]:my-1.5 [&_ol]:pl-4 [&_ol]:list-decimal [&_ol]:space-y-0.5
      [&_li]:text-gray-200
      [&_blockquote]:border-l-2 [&_blockquote]:border-indigo-500 [&_blockquote]:pl-3 [&_blockquote]:my-2 [&_blockquote]:text-gray-400 [&_blockquote]:italic
      [&_code]:text-indigo-300 [&_code]:bg-gray-700/60 [&_code]:px-1 [&_code]:py-0.5 [&_code]:rounded [&_code]:text-xs [&_code]:font-mono
      [&_pre]:my-2 [&_pre]:rounded-lg [&_pre]:overflow-x-auto [&_pre]:border [&_pre]:border-gray-700
      [&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_pre_code]:text-xs
      [&_hr]:border-gray-700 [&_hr]:my-3
      [&_table]:w-full [&_table]:my-2 [&_table]:text-xs [&_table]:border-collapse [&_table]:block [&_table]:overflow-x-auto
      [&_thead]:bg-gray-700/50
      [&_th]:px-3 [&_th]:py-2 [&_th]:text-left [&_th]:text-gray-300 [&_th]:font-semibold [&_th]:border [&_th]:border-gray-600
      [&_td]:px-3 [&_td]:py-1.5 [&_td]:text-gray-300 [&_td]:border [&_td]:border-gray-700
      [&_tr:nth-child(even)]:bg-gray-700/20
    ">
      <Markdown content={content} />
    </div>
  );
}

// ── 에이전트 상태 인디케이터 ──────────────────────────────────────────────

function StatusIndicator({ provider, toolInfo }: { provider: string; toolInfo: string | null }) {
  return (
    <div className="flex items-center gap-2 text-xs text-gray-500 py-1">
      {/* 스피너 */}
      <div className="w-3.5 h-3.5 border-2 border-gray-700 border-t-indigo-400 rounded-full animate-spin shrink-0" />
      <span>
        {toolInfo
          ? toolInfo
          : `${provider === 'claude' ? 'Claude' : 'Gemini'} 답변 생성 중...`}
      </span>
    </div>
  );
}

// ── 채팅 드로어 ──────────────────────────────────────────────────────────

export function ChatDrawer() {
  const {
    isOpen,
    sessions,
    activeSessionId,
    close,
    sendMessage,
    cancelMessage,
    isLoading,
    toolInfo,
  } = useChatStore();
  const [showNewForm, setShowNewForm] = useState(false);
  const [showSessions, setShowSessions] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const [drawerWidth, setDrawerWidth] = useState<number>(() => {
    const stored = localStorage.getItem('doxus-drawer-width');
    const val = stored ? parseInt(stored, 10) : 384;
    return isNaN(val) ? 384 : Math.min(700, Math.max(280, val));
  });
  const isDragging = useRef(false);

  const handleDragStart = (e: React.MouseEvent) => {
    e.preventDefault();
    isDragging.current = true;
    document.body.style.userSelect = 'none';

    const onMove = (ev: MouseEvent) => {
      if (!isDragging.current) return;
      const newWidth = Math.min(700, Math.max(280, window.innerWidth - ev.clientX));
      setDrawerWidth(newWidth);
      localStorage.setItem('doxus-drawer-width', String(newWidth));
    };

    const onUp = () => {
      isDragging.current = false;
      document.body.style.userSelect = '';
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
    };

    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  };

  const activeSession = sessions.find((s) => s.id === activeSessionId) ?? null;
  const messages = activeSession?.messages ?? [];

  // 새 메시지 / 로딩 상태 변경 시 자동 스크롤
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages.length, isLoading]);

  const [messageInput, setMessageInput] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const autoResize = (el: HTMLTextAreaElement) => {
    el.style.height = 'auto';
    el.style.height = Math.min(el.scrollHeight, 160) + 'px';
  };

  const handleSend = async (e?: React.FormEvent<HTMLFormElement>) => {
    e?.preventDefault();
    if (!activeSession || isLoading) return;
    const content = messageInput.trim();
    if (!content) return;
    setMessageInput('');
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
    await sendMessage(content);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  if (!isOpen) return null;

  return (
    <div
      className="fixed right-0 top-0 h-full bg-gray-900 border-l border-gray-800 shadow-2xl flex flex-col z-50"
      style={{ width: drawerWidth }}
    >
      {/* Drag handle - 좌측 경계 */}
      <div
        onMouseDown={handleDragStart}
        className="absolute left-0 top-0 h-full w-1 cursor-col-resize hover:bg-indigo-500/50 transition-colors"
        style={{ zIndex: 1 }}
      />
      {/* 헤더 */}
      <div className="flex items-center justify-between px-4 py-3 bg-gray-950 border-b border-gray-800 shrink-0">
        <div className="flex items-center gap-2">
          <button
            onClick={() => {
              setShowNewForm((v) => !v);
              setShowSessions(false);
            }}
            className="flex items-center gap-1 px-2.5 py-1 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium rounded-md transition-colors"
          >
            <span>+</span>
            <span>새 세션</span>
          </button>
          <button
            onClick={() => {
              setShowSessions((v) => !v);
              setShowNewForm(false);
            }}
            className="flex items-center gap-1 px-2.5 py-1 bg-gray-800 hover:bg-gray-700 text-gray-300 text-xs rounded-md transition-colors"
          >
            <span>세션 목록</span>
            <span>{showSessions ? '▲' : '▼'}</span>
          </button>
        </div>
        <button
          onClick={close}
          className="text-gray-500 hover:text-gray-200 transition-colors"
          aria-label="닫기"
        >
          &#x2715;
        </button>
      </div>

      {showNewForm && (
        <NewSessionForm onDone={() => setShowNewForm(false)} />
      )}

      {showSessions && (
        <SessionList onClose={() => setShowSessions(false)} />
      )}

      {activeSession && (
        <div className="px-4 py-2 bg-gray-900 border-b border-gray-800/60 shrink-0">
          <span className="inline-flex items-center gap-1.5 text-xs text-gray-500">
            <span>{activeSession.provider === 'claude' ? '🤖' : '✨'}</span>
            <span>{activeSession.name}</span>
          </span>
        </div>
      )}

      {/* 메시지 목록
          key={activeSessionId}로 세션 전환 시 DOM을 강제 재마운트 → 메시지 올바르게 갱신 */}
      <div
        key={activeSessionId ?? 'none'}
        className="flex-1 overflow-auto p-4 space-y-3"
      >
        {!activeSession && (
          <p className="text-sm text-gray-600 text-center py-8">
            세션을 만들어 사서 에이전트와 대화하세요.
          </p>
        )}
        {activeSession && messages.length === 0 && !isLoading && (
          <p className="text-sm text-gray-600 text-center py-8">
            문서에 대해 무엇이든 물어보세요.
          </p>
        )}

        {messages.map((m) => (
          <div
            key={m.id}
            className={`flex ${m.role === 'user' ? 'justify-end' : 'justify-start'}`}
          >
            <div
              className={`max-w-[85%] min-w-0 px-3 py-2 rounded-lg text-sm leading-relaxed overflow-x-auto ${
                m.role === 'user'
                  ? 'bg-indigo-600 text-white break-words'
                  : 'bg-gray-800 text-gray-100'
              }`}
            >
              {m.role === 'assistant' ? (
                <MarkdownMessage content={m.content} />
              ) : (
                m.content
              )}
            </div>
          </div>
        ))}

        {/* 에이전트 동작 상태 인디케이터 (도구 호출 / 답변 생성 중) */}
        {isLoading && activeSession && (
          <div className="flex justify-start">
            <div className="bg-gray-800 px-3 py-2 rounded-lg max-w-[85%]">
              <StatusIndicator
                provider={activeSession.provider}
                toolInfo={toolInfo}
              />
            </div>
          </div>
        )}

        <div ref={messagesEndRef} />
      </div>

      {/* 입력 */}
      <form
        onSubmit={handleSend}
        className="p-3 border-t border-gray-800 flex flex-col gap-2 shrink-0"
      >
        <textarea
          ref={textareaRef}
          name="message"
          rows={1}
          value={messageInput}
          onChange={(e) => {
            setMessageInput(e.target.value);
            autoResize(e.target);
          }}
          onKeyDown={handleKeyDown}
          placeholder={activeSession ? '문서에 대해 질문하세요… (Shift+Enter로 줄바꿈)' : '먼저 세션을 시작하세요'}
          disabled={!activeSession || isLoading}
          className="w-full px-3 py-2 text-sm bg-gray-800 border border-gray-700 text-gray-200 placeholder-gray-600 rounded-md focus:outline-none focus:ring-1 focus:ring-indigo-500 disabled:opacity-40 resize-none overflow-hidden"
        />
        <div className="flex justify-end">
        {isLoading ? (
          <button
            type="button"
            onClick={() => cancelMessage()}
            className="px-3 py-1.5 bg-red-700 hover:bg-red-600 text-white text-sm rounded-md transition-colors"
            title="답변 중지"
          >
            ■ 중지
          </button>
        ) : (
          <button
            type="submit"
            disabled={!activeSession}
            className="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white text-sm rounded-md transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
          >
            전송
          </button>
        )}
        </div>
      </form>
    </div>
  );
}
