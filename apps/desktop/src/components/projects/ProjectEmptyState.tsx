import React from 'react';

interface ProjectEmptyStateProps {
  onAddClick: () => void;
}

export const ProjectEmptyState: React.FC<ProjectEmptyStateProps> = ({ onAddClick }) => {
  return (
    <div className="flex flex-col items-center justify-center py-24 px-6 text-center glass-card border-dashed border-white/5 bg-transparent rounded-[2rem]">
      <div className="w-20 h-20 bg-indigo-500/10 rounded-3xl flex items-center justify-center mb-6 animate-pulse">
        <span className="text-4xl text-indigo-400">📁</span>
      </div>
      <h3 className="text-xl font-bold text-white mb-2">연결된 프로젝트가 없습니다</h3>
      <p className="text-sm text-gray-500 max-w-xs mb-8">
        Obsidian, Confluence 등 지식 소스를 연결하여 Doxus의 AI 검색과 분석 능력을 경험해보세요.
      </p>
      <button
        onClick={onAddClick}
        className="px-8 py-3 bg-white text-gray-900 rounded-2xl font-bold text-sm hover:bg-gray-100 transition-all active:scale-95"
      >
        첫 프로젝트 연결하기
      </button>
    </div>
  );
};
