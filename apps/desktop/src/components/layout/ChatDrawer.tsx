import { useChatStore } from '../../stores/useChatStore';

export function ChatDrawer() {
  const { isOpen, messages, close, addMessage, clear } = useChatStore();

  const handleSend = (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const fd = new FormData(e.currentTarget);
    const content = fd.get('message') as string;
    if (!content.trim()) return;
    addMessage('user', content);
    e.currentTarget.reset();
    // Stub response until agent sidecar is wired
    setTimeout(() => addMessage('assistant', `Echo: ${content}`), 300);
  };

  if (!isOpen) return null;

  return (
    <div className="fixed right-0 top-0 h-full w-96 bg-white border-l border-gray-200 shadow-lg flex flex-col z-50">
      <div className="flex items-center justify-between px-4 py-3 border-b">
        <h2 className="font-semibold text-gray-900">Librarian</h2>
        <div className="flex gap-2">
          <button onClick={clear} className="text-xs text-gray-400 hover:text-gray-600">
            Clear
          </button>
          <button onClick={close} className="text-gray-400 hover:text-gray-600">
            &#x2715;
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-auto p-4 space-y-3">
        {messages.length === 0 && (
          <p className="text-sm text-gray-400 text-center py-4">
            Ask the librarian anything about your documents.
          </p>
        )}
        {messages.map((m) => (
          <div key={m.id} className={`flex ${m.role === 'user' ? 'justify-end' : 'justify-start'}`}>
            <div
              className={`max-w-[80%] px-3 py-2 rounded-lg text-sm ${
                m.role === 'user'
                  ? 'bg-blue-600 text-white'
                  : m.role === 'thought'
                  ? 'bg-gray-100 text-gray-400 italic text-xs'
                  : 'bg-gray-100 text-gray-800'
              }`}
            >
              {m.content}
            </div>
          </div>
        ))}
      </div>

      <form onSubmit={handleSend} className="p-3 border-t flex gap-2">
        <input
          name="message"
          placeholder="Ask about your documents..."
          className="flex-1 px-3 py-2 text-sm border border-gray-300 rounded-md focus:outline-none focus:ring-1 focus:ring-blue-500"
        />
        <button
          type="submit"
          className="px-3 py-2 bg-blue-600 text-white text-sm rounded-md hover:bg-blue-700"
        >
          Send
        </button>
      </form>
    </div>
  );
}
