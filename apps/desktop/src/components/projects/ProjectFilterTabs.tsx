import React from 'react';
import { usePluginStore } from '../../stores/usePluginStore';

interface ProjectFilterTabsProps {
  activeTab: string;
  tabs: { id: string; label: string; icon: string; count: number }[];
  onTabChange: (id: string) => void;
}

export const ProjectFilterTabs: React.FC<ProjectFilterTabsProps> = ({ activeTab, tabs, onTabChange }) => {
  return (
    <div className="flex items-center gap-2 mb-6 p-1 bg-white/[0.02] border border-white/5 rounded-2xl overflow-x-auto no-scrollbar">
      <button
        onClick={() => onTabChange('all')}
        className={`px-4 py-2 rounded-xl text-xs font-bold transition-all duration-300 flex items-center gap-2 whitespace-nowrap ${
          activeTab === 'all'
            ? 'bg-white/10 text-white shadow-sm'
            : 'text-gray-500 hover:text-gray-300 hover:bg-white/[0.02]'
        }`}
      >
        <span>🌐</span>
        전체 보기
      </button>
      
      <div className="w-px h-4 bg-white/10 mx-1" />

      {tabs.map((tab) => (
        <button
          key={tab.id}
          onClick={() => onTabChange(tab.id)}
          className={`px-4 py-2 rounded-xl text-xs font-bold transition-all duration-300 flex items-center gap-2 whitespace-nowrap ${
            activeTab === tab.id
              ? 'bg-indigo-500/10 text-indigo-400 border border-indigo-500/20'
              : 'text-gray-500 hover:text-gray-300 hover:bg-white/[0.02] border border-transparent'
          }`}
        >
          <span>{tab.icon}</span>
          {tab.label}
          <span className={`px-1.5 py-0.5 rounded-md text-[10px] ${
            activeTab === tab.id ? 'bg-indigo-500/20 text-indigo-300' : 'bg-white/5 text-gray-600'
          }`}>
            {tab.count}
          </span>
        </button>
      ))}
    </div>
  );
};
