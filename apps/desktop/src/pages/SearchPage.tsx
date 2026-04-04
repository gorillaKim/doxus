import { useState } from 'react';
import ReactMarkdown from 'react-markdown';
import { useSearchStore } from '../stores/useSearchStore';

interface Hit {
  title?: string;
  score: number;
  heading_path?: string;
  file_path?: string;
  snippet?: string;
  content?: string;
}

function MarkdownPreview({ content }: { content: string }) {
  return (
    <div className="prose prose-invert prose-sm max-w-none text-gray-300
      prose-headings:text-white prose-headings:font-semibold
      prose-h1:text-lg prose-h2:text-base prose-h3:text-sm
      prose-p:text-gray-300 prose-p:leading-relaxed
      prose-strong:text-white prose-strong:font-semibold
      prose-code:text-indigo-300 prose-code:bg-gray-800 prose-code:px-1 prose-code:py-0.5 prose-code:rounded prose-code:text-xs
      prose-pre:bg-gray-800 prose-pre:border prose-pre:border-gray-700
      prose-blockquote:border-indigo-500 prose-blockquote:text-gray-400
      prose-a:text-indigo-400 prose-a:no-underline hover:prose-a:underline
      prose-li:text-gray-300 prose-ul:text-gray-300 prose-ol:text-gray-300
      prose-hr:border-gray-700">
      <ReactMarkdown>{content}</ReactMarkdown>
    </div>
  );
}

export function SearchPage() {
  const { query, hits, isLoading, error, setQuery, search, clear } = useSearchStore();
  const [inputValue, setInputValue] = useState(query);
  const [selectedHit, setSelectedHit] = useState<Hit | null>(null);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setQuery(inputValue);
    setSelectedHit(null);
    search();
  };

  const handleClear = () => {
    clear();
    setSelectedHit(null);
    setInputValue('');
  };

  return (
    <div className="flex flex-col h-full gap-4">
      {/* 검색 폼 */}
      <form onSubmit={handleSubmit} className="flex gap-2 shrink-0">
        <input
          type="text"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          placeholder="문서를 검색하세요..."
          className="flex-1 px-3 py-2 bg-gray-900 border border-gray-800 rounded-lg text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 text-sm"
        />
        <button
          type="submit"
          disabled={isLoading}
          className="px-4 py-2 bg-indigo-600 text-white rounded-lg hover:bg-indigo-500 disabled:opacity-50 text-sm font-medium transition-colors"
        >
          {isLoading ? '검색 중...' : '검색'}
        </button>
        {hits.length > 0 && (
          <button
            type="button"
            onClick={handleClear}
            className="px-4 py-2 border border-gray-700 text-gray-400 rounded-lg hover:bg-gray-800 hover:text-gray-200 text-sm transition-colors"
          >
            초기화
          </button>
        )}
      </form>

      {error && (
        <div className="p-3 bg-red-950 border border-red-800 rounded-lg text-red-400 text-sm shrink-0">
          {error}
        </div>
      )}

      {/* 결과 영역: 좌측 목록 + 우측 프리뷰 */}
      <div className="flex-1 overflow-hidden flex gap-4">
        {/* 좌측: 검색 결과 목록 */}
        <div className={`flex flex-col gap-2 overflow-auto ${selectedHit ? 'w-80 shrink-0' : 'flex-1'}`}>
          {hits.length === 0 && !isLoading && query && (
            <div className="flex items-center justify-center h-48">
              <p className="text-gray-500 text-sm">"{query}"에 대한 검색 결과가 없습니다</p>
            </div>
          )}
          {hits.length === 0 && !isLoading && !query && (
            <div className="flex flex-col items-center justify-center h-48 gap-2">
              <p className="text-4xl">🔍</p>
              <p className="text-gray-400 font-medium">검색어를 입력하세요</p>
              <p className="text-sm text-gray-600">프로젝트에 인덱싱된 모든 문서를 검색합니다</p>
            </div>
          )}
          {(hits as Hit[]).map((hit, i) => (
            <button
              key={i}
              onClick={() => setSelectedHit(selectedHit === hit ? null : hit)}
              className={`w-full text-left p-4 rounded-xl border transition-colors ${
                selectedHit === hit
                  ? 'bg-indigo-950 border-indigo-700'
                  : 'bg-gray-900 border-gray-800 hover:border-gray-700'
              }`}
            >
              <div className="flex items-start justify-between gap-2">
                <h3 className="font-medium text-white text-sm leading-tight">
                  {hit.title ?? '(제목 없음)'}
                </h3>
                <span className="text-xs text-gray-600 shrink-0">
                  {hit.score.toFixed(2)}
                </span>
              </div>
              {hit.heading_path && (
                <span className="inline-block text-xs text-indigo-400 bg-indigo-950 px-2 py-0.5 rounded mt-1">
                  {hit.heading_path}
                </span>
              )}
              {hit.file_path && (
                <p className="text-xs text-gray-600 mt-1 truncate">{hit.file_path}</p>
              )}
              {hit.snippet && (
                <p className="text-xs text-gray-500 mt-2 line-clamp-2 leading-relaxed">
                  {hit.snippet}
                </p>
              )}
            </button>
          ))}
        </div>

        {/* 우측: 문서 프리뷰 */}
        {selectedHit && (
          <div className="flex-1 bg-gray-900 border border-gray-800 rounded-xl flex flex-col overflow-hidden">
            {/* 프리뷰 헤더 */}
            <div className="px-5 py-3 border-b border-gray-800 flex items-center justify-between shrink-0">
              <div className="min-w-0">
                <h2 className="text-white font-semibold text-sm truncate">
                  {selectedHit.title ?? '(제목 없음)'}
                </h2>
                {selectedHit.file_path && (
                  <p className="text-xs text-gray-600 truncate mt-0.5">{selectedHit.file_path}</p>
                )}
              </div>
              <button
                onClick={() => setSelectedHit(null)}
                className="text-gray-600 hover:text-gray-300 transition-colors shrink-0 ml-3 text-lg"
              >
                ✕
              </button>
            </div>

            {/* 프리뷰 내용 */}
            <div className="flex-1 overflow-auto p-5">
              {selectedHit.content ? (
                <MarkdownPreview content={selectedHit.content} />
              ) : selectedHit.snippet ? (
                <div className="space-y-4">
                  {selectedHit.heading_path && (
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-gray-500">섹션</span>
                      <span className="text-xs text-indigo-400 bg-indigo-950 px-2 py-1 rounded">
                        {selectedHit.heading_path}
                      </span>
                    </div>
                  )}
                  <MarkdownPreview content={selectedHit.snippet} />
                  <div className="pt-4 border-t border-gray-800">
                    <p className="text-xs text-gray-600">
                      전체 내용을 보려면 원본 파일을 열어주세요.
                    </p>
                  </div>
                </div>
              ) : (
                <p className="text-gray-600 text-sm">미리볼 내용이 없습니다.</p>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
