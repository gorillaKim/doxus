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
  icon?: string;
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
  onRemove,
  icon = '📁'
}) => {
  const isActive = project.status === 'active';

  const sensitivity = (() => {
    try {
      if (project.freshness_policy_json) {
        const parsed = JSON.parse(project.freshness_policy_json);
        return parsed.sensitivity_mode || 'normal';
      }
    } catch { /* ignore */ }
    return 'normal';
  })();

  return (
    <div
      className={`relative group bg-white/[0.03] hover:bg-white/[0.05] border border-white/5 hover:border-white/10 rounded-3xl p-6 transition-all duration-500 overflow-hidden ${
        !isActive ? 'opacity-70 grayscale-[0.3]' : ''
      } ${isBusy ? 'pointer-events-none' : ''}`}
    >
      {/* Background Decor */}
      <div className="absolute top-0 right-0 w-32 h-32 bg-indigo-500/5 blur-3xl rounded-full translate-x-10 -translate-y-10 group-hover:bg-indigo-500/10 transition-colors duration-500" />

      {/* Loading Overlay */}
      {isBusy && (
        <div className="absolute inset-0 z-20 bg-gray-950/20 backdrop-blur-[2px] rounded-3xl flex items-center justify-center">
          <div className="px-4 py-2 bg-gray-900 border border-white/10 rounded-2xl shadow-2xl flex items-center gap-3">
            <div className="w-4 h-4 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin" />
            <span className="text-[11px] font-bold text-indigo-300 uppercase tracking-widest">Processing...</span>
          </div>
        </div>
      )}

      {/* Card Header */}
      <div className="flex items-start justify-between mb-6 relative z-10">
        <div className="flex items-center gap-4 min-w-0">
          <div className={`w-14 h-14 rounded-2xl flex items-center justify-center text-2xl shadow-inner transition-all duration-500 ${
            isActive ? 'bg-white/5 border border-white/10' : 'bg-gray-800/50 border border-transparent'
          }`}>
            {icon}
          </div>
          <div className="flex flex-col gap-1 min-w-0">
            <div className="flex items-center gap-2">
              <h3 className="font-bold text-lg text-white truncate leading-tight">{project.display_name}</h3>
              <span className={`px-2 py-0.5 rounded-full text-[9px] font-bold tracking-tight uppercase border ${
                isActive 
                  ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20 shadow-[0_0_10px_rgba(16,185,129,0.1)]' 
                  : 'bg-gray-800 text-gray-500 border-gray-700'
              }`}>
                {project.status}
              </span>
            </div>
            <p className="text-xs text-gray-500 font-mono truncate max-w-[200px] hover:text-gray-400 transition-colors cursor-help" title={project.path}>
              {project.path}
            </p>
          </div>
        </div>

        <button
          onClick={onIndex}
          disabled={isBusy}
          className={`px-4 py-2 rounded-xl text-xs font-bold transition-all duration-300 border ${
            isIndexing 
              ? 'bg-indigo-500 text-white border-indigo-400 cursor-default' 
              : 'bg-white/5 text-gray-300 border-white/5 hover:bg-white/10 hover:border-white/10 active:scale-95'
          }`}
        >
          {isIndexing ? 'Indexing...' : 'Index Now'}
        </button>
      </div>

      {/* Card Footer */}
      <div className="flex items-center justify-between pt-5 border-t border-white/5 relative z-10">
        <div className="flex items-center gap-6">
          <div className="flex flex-col gap-1">
            <label className="text-[9px] text-gray-600 font-bold uppercase tracking-widest">Sensitivity</label>
            <select
              value={sensitivity}
              onChange={(e) => onSensitivityChange(e.target.value)}
              disabled={isBusy}
              className="bg-transparent text-sm font-bold text-indigo-400 focus:outline-none border-none cursor-pointer hover:text-indigo-300 transition-colors appearance-none"
            >
              <option className="bg-gray-900" value="strict">Strict</option>
              <option className="bg-gray-900" value="normal">Normal</option>
              <option className="bg-gray-900" value="relaxed">Relaxed</option>
            </select>
          </div>

          <div className="w-px h-6 bg-white/5" />

          <button 
            onClick={onToggle} 
            disabled={isBusy}
            className={`text-xs font-bold transition-colors ${
              isActive ? 'text-gray-500 hover:text-gray-300' : 'text-indigo-400 hover:text-indigo-300'
            }`}
          >
            {isActive ? 'Disable' : 'Enable'}
          </button>
        </div>

        <button 
          onClick={onRemove} 
          disabled={isBusy}
          className="w-8 h-8 rounded-xl bg-red-500/0 hover:bg-red-500/10 text-gray-600 hover:text-red-400 flex items-center justify-center transition-all duration-300 opacity-0 group-hover:opacity-100"
          title="Remove project"
        >
          <span className="text-sm">✕</span>
        </button>
      </div>
    </div>
  );
};
