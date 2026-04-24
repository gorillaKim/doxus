import React from 'react';

interface SearchHeaderProps {
  inputValue: string;
  setInputValue: (val: string) => void;
  onSubmit: (e: React.FormEvent) => void;
  onClear: () => void;
  advancedOpen: boolean;
  setAdvancedOpen: (val: boolean) => void;
  activeFilterCount: number;
  isLoading: boolean;
  hasQuery: boolean;
}

export const SearchHeader: React.FC<SearchHeaderProps> = ({
  inputValue,
  setInputValue,
  onSubmit,
  onClear,
  advancedOpen,
  setAdvancedOpen,
  activeFilterCount,
  isLoading,
  hasQuery
}) => {
  return (
    <div className="flex flex-col gap-2 shrink-0">
      <form onSubmit={onSubmit} className="flex gap-3 items-center">
        <div className="relative flex-1 group">
          <input
            type="text"
            placeholder="궁금한 지식을 검색하세요..."
            value={inputValue}
            onChange={(e) => setInputValue(e.target.value)}
            className="w-full bg-white/[0.03] border border-white/5 rounded-3xl px-14 py-4 text-sm text-white focus:outline-none focus:ring-4 focus:ring-indigo-500/10 focus:border-indigo-500/40 transition-all placeholder-gray-600 group-hover:bg-white/[0.05]"
          />
          <div className="absolute left-5 top-1/2 -translate-y-1/2 text-gray-500 group-focus-within:text-indigo-400 transition-colors">
            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
          </div>
          {inputValue && (
            <button
              type="button"
              onClick={() => setInputValue('')}
              className="absolute right-5 top-1/2 -translate-y-1/2 text-gray-600 hover:text-gray-300 transition-colors"
            >
              ✕
            </button>
          )}
        </div>

        <button
          type="button"
          onClick={() => setAdvancedOpen(!advancedOpen)}
          className={`flex items-center gap-2 px-6 py-4 rounded-3xl border transition-all text-sm font-bold ${
            advancedOpen || activeFilterCount > 0
              ? 'bg-indigo-500/10 border-indigo-500/30 text-indigo-400 shadow-[0_0_20px_rgba(99,102,241,0.1)]'
              : 'bg-white/[0.03] border-white/5 text-gray-400 hover:border-white/10 hover:bg-white/5'
          }`}
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/></svg>
          <span>필터</span>
          {activeFilterCount > 0 && (
            <span className="flex items-center justify-center bg-indigo-500 text-white text-[10px] w-4 h-4 rounded-full">
              {activeFilterCount}
            </span>
          )}
        </button>

        <button
          type="submit"
          disabled={isLoading || (!inputValue.trim() && activeFilterCount === 0)}
          className="px-10 py-4 bg-indigo-600 hover:bg-indigo-500 disabled:bg-gray-800 disabled:text-gray-600 text-white rounded-3xl font-black text-sm shadow-xl shadow-indigo-600/20 transition-all active:scale-95 disabled:scale-100 flex items-center gap-2"
        >
          {isLoading ? (
            <div className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
          ) : (
            '검색'
          )}
        </button>

        {(hasQuery) && (
          <button
            type="button"
            onClick={onClear}
            className="px-4 py-4 text-gray-500 hover:text-red-400 text-xs font-bold uppercase tracking-widest transition-colors"
          >
            Clear
          </button>
        )}
      </form>
    </div>
  );
};
