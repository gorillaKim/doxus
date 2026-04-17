import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useProjectStore } from '../stores/useProjectStore';
import { useSearchStore } from '../stores/useSearchStore';

function StatCard({ label, value, icon }: { label: string; value: string | number; icon?: React.ReactNode }) {
  return (
    <div className="glass-card rounded-2xl p-6 flex flex-col gap-3 relative overflow-hidden group hover:ring-1 hover:ring-indigo-500/30 transition-all duration-300">
      <div className="flex items-center justify-between">
        <span className="text-[10px] text-gray-500 uppercase tracking-widest font-semibold">{label}</span>
        {icon && <div className="text-gray-600 group-hover:text-indigo-400 transition-colors">{icon}</div>}
      </div>
      <span className="text-3xl font-bold text-white tracking-tight">{value}</span>
      <div className="absolute -right-4 -bottom-4 w-20 h-20 bg-indigo-500/5 blur-3xl rounded-full" />
    </div>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="text-[10px] font-bold text-gray-500 uppercase tracking-[0.2em] mb-4 flex items-center gap-2">
      <span className="w-1.5 h-1.5 rounded-full bg-indigo-500/50" />
      {children}
    </h2>
  );
}

export default function DashboardPage() {
  const navigate = useNavigate();
  const { projects, fetch: fetchProjects } = useProjectStore();
  const { queryHistory } = useSearchStore();
  const [totalDocs, setTotalDocs] = useState<number | null>(null);
  const [lastSync, setLastSync] = useState<string>('—');
  const [topDocs, setTopDocs] = useState<{ document_id: number; title: string; count: number }[]>([]);

  useEffect(() => {
    fetchProjects();
    
    // 문서 총합 및 상위 문서 조회
    invoke<{ documents: any[] }>('list_all_documents')
      .then(res => setTotalDocs(res.documents.length))
      .catch(() => setTotalDocs(0));

    invoke<{ documents: any[] }>('get_top_documents', { limit: 5 })
      .then(res => setTopDocs(res.documents))
      .catch(() => setTopDocs([]));
      
    setLastSync('방금 전');
  }, [fetchProjects]);

  const today = new Date().toLocaleDateString('ko-KR', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    weekday: 'long',
  });

  return (
    <div className="flex flex-col gap-10 max-w-5xl mx-auto py-6 animate-in fade-in slide-in-from-bottom-4 duration-700">
      {/* 헤더 */}
      <div className="flex flex-col gap-2">
        <h1 className="text-4xl font-extrabold text-white tracking-tight bg-clip-text text-transparent bg-gradient-to-br from-white to-gray-500">
          안녕하세요, {projects.length > 0 ? '전략적인 사서님' : 'Doxus에 오신 것을 환영합니다'}
        </h1>
        <p className="text-gray-500 text-sm font-medium">{today}</p>
      </div>

      {/* 통계 */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-5">
        <StatCard 
          label="프로젝트 매니저" 
          value={projects.length} 
          icon={<svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" /></svg>}
        />
        <StatCard 
          label="인덱싱된 지식" 
          value={totalDocs !== null ? totalDocs.toLocaleString() : '—'} 
          icon={<svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" /></svg>}
        />
        <StatCard 
          label="시스템 동기화" 
          value={lastSync} 
          icon={<svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" /></svg>}
        />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-10">
        {/* 최근 검색 */}
        <div className="flex flex-col">
          <SectionTitle>최근 탐색 기록</SectionTitle>
          {queryHistory.length === 0 ? (
            <div className="glass-card rounded-2xl p-8 flex flex-col items-center justify-center text-center gap-2">
               <span className="text-2xl opacity-20">🔍</span>
               <p className="text-gray-500 text-sm">아직 검색 기록이 없습니다</p>
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              {queryHistory.map((q, i) => (
                <button
                  key={i}
                  className="group flex items-center justify-between text-sm text-gray-300 glass-card rounded-xl px-4 py-3 hover:bg-white/5 border border-white/5 transition-all duration-200"
                  onClick={() => navigate('/search')}
                >
                  <span className="truncate">{q}</span>
                  <span className="opacity-0 group-hover:opacity-100 transition-opacity text-indigo-400">→</span>
                </button>
              ))}
            </div>
          )}
        </div>

        {/* 자주 찾는 문서 */}
        <div className="flex flex-col">
          <SectionTitle>인기 있는 문서</SectionTitle>
          {topDocs.length === 0 ? (
            <div className="glass-card rounded-2xl p-8 flex flex-col items-center justify-center text-center gap-2">
               <span className="text-2xl opacity-20">📄</span>
               <p className="text-gray-500 text-sm">데이터가 충분하지 않습니다</p>
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              {topDocs.map((doc) => (
                <div
                  key={doc.document_id}
                  className="group flex items-center justify-between text-sm glass-card rounded-xl px-4 py-3 hover:bg-white/5 border border-white/5 transition-all duration-200"
                >
                  <div className="flex items-center gap-3 truncate">
                    <span className="text-indigo-400/50">#</span>
                    <span className="text-gray-300 truncate font-medium">{doc.title}</span>
                  </div>
                  <div className="flex items-center gap-2 shrink-0">
                    <span className="px-1.5 py-0.5 rounded bg-gray-950/50 text-[10px] text-gray-500 font-mono">{doc.count} Views</span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* 빠른 실행 */}
      <div className="flex items-center gap-4 pt-4">
        <button
          onClick={() => navigate('/projects')}
          className="px-6 py-3 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-semibold rounded-2xl shadow-lg shadow-indigo-600/20 transition-all duration-300 transform hover:-translate-y-0.5 active:translate-y-0"
        >
          새 프로젝트 추가
        </button>
        <button
          onClick={() => navigate('/search')}
          className="px-6 py-3 glass-card hover:bg-white/10 text-gray-200 text-sm font-semibold rounded-2xl border border-white/10 transition-all duration-300 transform hover:-translate-y-0.5 active:translate-y-0"
        >
          통합 검색 실행
        </button>
      </div>
    </div>
  );
}
// Force Vite refresh: Fri Apr 17 20:18:33 KST 2026
// Fix topDocs mapping: Fri Apr 17 20:35:01 KST 2026
