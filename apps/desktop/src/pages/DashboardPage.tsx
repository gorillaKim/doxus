import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { useProjectStore } from '../stores/useProjectStore';
import { useSearchStore } from '../stores/useSearchStore';

function StatCard({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="bg-gray-900 rounded-xl p-5 flex flex-col gap-1 border border-gray-800">
      <span className="text-xs text-gray-500 uppercase tracking-wider">{label}</span>
      <span className="text-2xl font-bold text-white">{value}</span>
    </div>
  );
}

export default function DashboardPage() {
  const navigate = useNavigate();
  const { projects } = useProjectStore();
  const { queryHistory } = useSearchStore();
  const [totalDocs, setTotalDocs] = useState<number | null>(null);
  const [topDocs, setTopDocs] = useState<{ document_id: number; title: string; file_path: string; count: number }[]>([]);

  useEffect(() => {
    invoke<{ total_documents: number }>('search_engine_status')
      .then(res => setTotalDocs(res.total_documents))
      .catch(() => {});
    invoke<{ documents: { document_id: number; title: string; file_path: string; count: number }[] }>('get_top_documents', { limit: 5 })
      .then(res => setTopDocs(res.documents))
      .catch(() => {});
  }, []);

  const today = new Date().toLocaleDateString('ko-KR', {
    weekday: 'long',
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  });

  return (
    <div className="flex flex-col gap-8 max-w-3xl">
      {/* 헤더 */}
      <div className="flex flex-col gap-1">
        <h1 className="text-3xl font-bold text-white tracking-tight">doxus에 오신 걸 환영합니다</h1>
        <p className="text-sm text-gray-500">{today}</p>
      </div>

      {/* 통계 */}
      <div className="grid grid-cols-3 gap-4">
        <StatCard label="프로젝트" value={projects.length} />
        <StatCard label="인덱싱된 문서" value={totalDocs !== null ? totalDocs : '—'} />
        <StatCard label="마지막 동기화" value="—" />
      </div>

      {/* 최근 검색 */}
      <div className="flex flex-col gap-3">
        <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider">최근 검색</h2>
        {queryHistory.length === 0 ? (
          <p className="text-gray-600 text-sm">최근 검색 기록이 없습니다</p>
        ) : (
          <ul className="flex flex-col gap-1">
            {queryHistory.map((q, i) => (
              <li
                key={i}
                className="text-sm text-gray-300 bg-gray-900 border border-gray-800 rounded-lg px-4 py-2 hover:border-gray-700 cursor-pointer transition-colors"
                onClick={() => navigate('/search')}
              >
                {q}
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* 자주 찾는 문서 */}
      {topDocs.length > 0 && (
        <div className="flex flex-col gap-3">
          <h2 className="text-sm font-semibold text-gray-400 uppercase tracking-wider">자주 찾는 문서</h2>
          <ul className="flex flex-col gap-1">
            {topDocs.map((doc) => (
              <li
                key={doc.document_id}
                className="flex items-center justify-between text-sm bg-gray-900 border border-gray-800 rounded-lg px-4 py-2 hover:border-gray-700 transition-colors"
              >
                <span className="text-gray-300 truncate flex-1">{doc.title}</span>
                <span className="text-xs text-gray-600 ml-3 shrink-0">{doc.count}회</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* 빠른 실행 */}
      <div className="flex gap-3">
        <button
          onClick={() => navigate('/projects')}
          className="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium rounded-lg transition-colors"
        >
          프로젝트 추가
        </button>
        <button
          onClick={() => navigate('/search')}
          className="px-4 py-2 bg-gray-800 hover:bg-gray-700 text-gray-200 text-sm font-medium rounded-lg border border-gray-700 transition-colors"
        >
          문서 검색
        </button>
      </div>
    </div>
  );
}
