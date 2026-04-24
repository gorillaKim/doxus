import React from 'react';
import { SearchFilters } from '../../stores/useSearchStore';

interface AdvancedSearchPanelProps {
  filters: SearchFilters;
  availablePlugins: { id: string; label: string; icon: string }[];
  availableProjects: string[];
  onChange: (f: Partial<SearchFilters>) => void;
}

export const AdvancedSearchPanel: React.FC<AdvancedSearchPanelProps> = ({
  filters,
  availablePlugins,
  availableProjects,
  onChange,
}) => {
  return (
    <div className="glass-card border-white/10 rounded-3xl px-8 py-6 flex flex-col gap-6 shrink-0 shadow-2xl animate-in fade-in zoom-in-95 duration-300">
      <div className="grid grid-cols-2 gap-10">
        <div className="flex flex-col gap-4">
          <span className="text-[10px] text-gray-500 font-black uppercase tracking-[0.2em] px-1">소스 플러그인</span>
          <div className="flex flex-wrap gap-2">
            {availablePlugins.map((p) => {
              const isActive = filters.sourceTypes.includes(p.id);
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => {
                    const val = isActive
                      ? filters.sourceTypes.filter((v) => v !== p.id)
                      : [...filters.sourceTypes, p.id];
                    onChange({ sourceTypes: val });
                  }}
                  className={`flex items-center gap-2 px-4 py-2 rounded-2xl border transition-all duration-300 text-xs font-bold ${
                    isActive
                      ? 'bg-indigo-500/10 border-indigo-500/50 text-indigo-400 shadow-[0_0_20px_rgba(99,102,241,0.1)]'
                      : 'bg-white/[0.03] border-white/5 text-gray-500 hover:border-white/20 hover:bg-white/5'
                  }`}
                >
                  <span className="text-lg">{p.icon}</span>
                  <span>{p.label}</span>
                </button>
              );
            })}
          </div>
        </div>

        <div className="flex flex-col gap-4">
          <span className="text-[10px] text-gray-500 font-black uppercase tracking-[0.2em] px-1">프로젝트 선택</span>
          <div className="flex flex-wrap gap-2 max-h-32 overflow-y-auto no-scrollbar">
            {availableProjects.map((projectName) => {
              const isActive = filters.projectNames.includes(projectName);
              return (
                <button
                  key={projectName}
                  type="button"
                  onClick={() => {
                    const val = isActive
                      ? filters.projectNames.filter((v) => v !== projectName)
                      : [...filters.projectNames, projectName];
                    onChange({ projectNames: val });
                  }}
                  className={`px-4 py-2 rounded-2xl border transition-all duration-300 text-xs font-bold ${
                    isActive
                      ? 'bg-emerald-500/10 border-emerald-500/50 text-emerald-400 shadow-[0_0_20px_rgba(16,185,129,0.1)]'
                      : 'bg-white/[0.03] border-white/5 text-gray-400 hover:border-white/20 hover:bg-white/5'
                  }`}
                >
                  {projectName}
                </button>
              );
            })}
          </div>
        </div>
      </div>

      <div className="flex flex-col gap-4 pt-4 border-t border-white/5">
        <span className="text-[10px] text-gray-500 font-black uppercase tracking-[0.2em] px-1">태그 검색</span>
        <div className="relative group">
          <input
            type="text"
            placeholder="태그 입력 (콤마로 구분 가능)"
            value={filters.tagQuery}
            onChange={(e) => onChange({ tagQuery: e.target.value })}
            className="w-full bg-white/[0.03] border border-white/5 rounded-2xl px-5 py-3 text-sm text-white placeholder-gray-600 focus:outline-none focus:border-indigo-500/50 transition-all"
          />
          <span className="absolute right-5 top-1/2 -translate-y-1/2 text-gray-600 text-xs font-mono group-focus-within:text-indigo-500 transition-colors">TAGS</span>
        </div>
      </div>

      {(filters.sourceTypes.length > 0 || filters.projectNames.length > 0 || filters.tagQuery) && (
        <div className="flex items-center gap-3 pt-6 border-t border-indigo-500/10">
          <span className="text-[10px] text-gray-600 font-bold uppercase tracking-widest">Active Filters</span>
          <div className="flex flex-wrap gap-2 flex-1">
            {filters.sourceTypes.map((t) => (
              <span key={t} className="text-[10px] font-bold text-indigo-400 bg-indigo-500/10 px-2.5 py-1 rounded-lg border border-indigo-500/10">{t}</span>
            ))}
            {filters.projectNames.map((n) => (
              <span key={n} className="text-[10px] font-bold text-emerald-400 bg-emerald-500/10 px-2.5 py-1 rounded-lg border border-emerald-500/10">{n}</span>
            ))}
            {filters.tagQuery && (
              <span key={filters.tagQuery} className="text-[10px] font-bold text-amber-400 bg-amber-500/10 px-2.5 py-1 rounded-lg border border-amber-500/10">{filters.tagQuery}</span>
            )}
          </div>
          <button
            type="button"
            onClick={() => onChange({ sourceTypes: [], projectNames: [], tagQuery: '' })}
            className="text-xs font-bold text-gray-500 hover:text-red-400 transition-colors"
          >
            모든 필터 초기화
          </button>
        </div>
      )}
    </div>
  );
};
