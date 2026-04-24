import React from 'react';

interface ProjectHeaderProps {
  onAddClick: () => void;
  projectCount: number;
}

export const ProjectHeader: React.FC<ProjectHeaderProps> = ({ onAddClick, projectCount }) => {
  return (
    <div className="flex items-center justify-between mb-8">
      <div className="flex flex-col gap-1">
        <h1 className="text-3xl font-extrabold text-white tracking-tight bg-clip-text text-transparent bg-gradient-to-r from-white to-gray-500">
          프로젝트 매니저
        </h1>
        <div className="flex items-center gap-2">
          <span className="flex h-2 w-2 rounded-full bg-emerald-500 animate-pulse" />
          <p className="text-xs text-gray-500 font-medium">
            현재 {projectCount}개의 지식 소스가 연결되어 있습니다.
          </p>
        </div>
      </div>
      <button 
        onClick={onAddClick}
        className="group relative px-6 py-3 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-bold rounded-2xl shadow-lg shadow-indigo-600/20 transition-all duration-300 transform hover:-translate-y-1 active:translate-y-0 overflow-hidden"
      >
        <div className="absolute inset-0 w-full h-full bg-gradient-to-r from-transparent via-white/10 to-transparent -translate-x-full group-hover:translate-x-full transition-transform duration-1000" />
        <span className="relative flex items-center gap-2">
          <span className="text-lg">+</span>
          소스 추가하기
        </span>
      </button>
    </div>
  );
};
