import { useState } from 'react';
import { useSearchStore } from '../stores/useSearchStore';

export function SearchPage() {
  const { query, hits, isLoading, error, setQuery, search, clear } = useSearchStore();
  const [inputValue, setInputValue] = useState(query);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setQuery(inputValue);
    search();
  };

  return (
    <div className="flex flex-col h-full p-4 gap-4">
      <form onSubmit={handleSubmit} className="flex gap-2">
        <input
          type="text"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          placeholder="Search documents..."
          className="flex-1 px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
        />
        <button
          type="submit"
          disabled={isLoading}
          className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 disabled:opacity-50"
        >
          {isLoading ? 'Searching...' : 'Search'}
        </button>
        {hits.length > 0 && (
          <button
            type="button"
            onClick={clear}
            className="px-4 py-2 border border-gray-300 rounded-md hover:bg-gray-50"
          >
            Clear
          </button>
        )}
      </form>

      {error && (
        <div className="p-3 bg-red-50 border border-red-200 rounded-md text-red-700 text-sm">
          {error}
        </div>
      )}

      <div className="flex-1 overflow-auto space-y-2">
        {hits.length === 0 && !isLoading && query && (
          <p className="text-gray-500 text-center py-8">No results for &ldquo;{query}&rdquo;</p>
        )}
        {hits.map((hit, i) => (
          <div key={i} className="p-4 bg-white border border-gray-200 rounded-lg shadow-sm">
            <div className="flex items-start justify-between gap-2">
              <h3 className="font-medium text-gray-900">{hit.title ?? '(untitled)'}</h3>
              <span className="text-xs text-gray-400 shrink-0">score: {hit.score.toFixed(2)}</span>
            </div>
            {hit.heading_path && (
              <span className="text-xs text-blue-600 bg-blue-50 px-2 py-0.5 rounded">{hit.heading_path}</span>
            )}
            {hit.file_path && (
              <p className="text-xs text-gray-400 mt-1 truncate">{hit.file_path}</p>
            )}
            {hit.snippet && (
              <p className="text-sm text-gray-600 mt-2 line-clamp-3">{hit.snippet}</p>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
