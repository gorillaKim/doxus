import React from 'react';
import { Project } from '../../stores/useProjectStore';

interface ProjectCardProps {
  project: Project;
  isBusy: boolean;
  isIndexing: boolean;
  isToggling: boolean;
  isUpdating: boolean;
  isRemoving: boolean;
  onIndex: () => void;
  onToggle: () => void;
  onSensitivityChange: (val: string) => void;
  onRemove: () => void;
}

export const ProjectCard: React.FC<ProjectCardProps> = ({
  project,
  isBusy,
  isIndexing,
  isToggling,
  isUpdating,
  isRemoving,
  onIndex,
  onToggle,
  onSensitivityChange,
  onRemove
}) => {
  return (
    <div
      className={`glass-card border-white/5 rounded-2xl p-5 flex flex-col gap-5 hover:border-white/10 transition-all duration-300 group relative ${
        isBusy ? 'opacity-60 grayscale-[0.5] pointer-events-none' : ''
      }`}
    >
      {isBusy && (
        <div className="absolute inset-0 flex items-center justify-center z-10 bg-gray-950/20 rounded-2xl backdrop-blur-[1px]">
          <div className="flex items-center gap-2 px-3 py-1.5 bg-gray-900/80 border border-white/10 rounded-full shadow-2xl">
            <div className="w-3 h-3 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
            <span className="text-[10px] font-bold text-indigo-300 uppercase tracking-widest">Processing</span>
          </div>
        </div>
      )}

      <div className="flex items-start justify-between">
        <div className="flex flex-col gap-1 min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="font-bold text-gray-100 truncate">{project.display_name}</h3>
            <span className={`text-[9px] px-1.5 py-0.5 rounded uppercase font-bold tracking-tighter ${
              project.status === 'active'
                ? 'bg-emerald-500/10 text-emerald-400 ring-1 ring-emerald-500/20'
                : 'bg-gray-800 text-gray-500'
            }`}>
              {project.status}
            </span>
          </div>
          <p className="text-xs text-gray-500 font-mono truncate">{project.path}</p>
        </div>
        
        <div className="flex items-center gap-1.5">
          <button
            onClick={onIndex}
            disabled={isBusy}
            className="px-3 py-1.5 bg-white/5 hover:bg-indigo-500 text-gray-300 hover:text-white rounded-lg text-[11px] font-bold transition-all duration-300 disabled:opacity-30"
          >
            {isIndexing ? 'Indexing...' : 'Index Now'}
          </button>
        </div>
      </div>

      <div className="flex items-center justify-between pt-4 border-t border-white/5">
        <div className="flex items-center gap-4">
          <button onClick={onToggle} disabled={isBusy}
            className="text-[11px] font-semibold text-gray-500 hover:text-gray-200 transition-colors disabled:opacity-30">
            {isToggling ? '...' : project.status === 'active' ? '비활성화' : '활성화'}
          </button>
          
          <div className="flex items-center gap-2">
            <label className="text-[10px] text-gray-500 font-semibold uppercase tracking-widest">민감도:</label>
            <select
              value={(() => {
                try {
                  if (project.freshness_policy_json) {
                    const parsed = JSON.parse(project.freshness_policy_json);
                    return parsed.sensitivity_mode || 'normal';
                  }
                } catch { /* ignore */ }
                return 'normal';
              })()}
              onChange={(e) => onSensitivityChange(e.target.value)}
              disabled={isBusy}
              className="bg-transparent text-[11px] font-medium text-indigo-300 focus:outline-none border-none cursor-pointer hover:text-indigo-200 uppercase disabled:opacity-30"
            >
              <option className="bg-gray-800" value="strict">Strict</option>
              <option className="bg-gray-800" value="normal">Normal</option>
              <option className="bg-gray-800" value="relaxed">Relaxed</option>
            </select>
          </div>
        </div>
        <button onClick={onRemove} disabled={isBusy}
          className="text-[11px] font-semibold text-gray-600 hover:text-red-400 transition-colors disabled:opacity-30">
          {isRemoving ? '...' : '삭제'}
        </button>
      </div>
    </div>
  );
};
