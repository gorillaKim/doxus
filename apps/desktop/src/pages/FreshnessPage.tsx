import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface FreshnessReport {
  total_docs: number;
  fresh_docs: number;
  aging_docs: number;
  stale_docs: number;
  obsolete_docs: number;
  average_score: number;
}

interface StaleDocument {
  title: string;
  source_doc_id: string;
  project_name: string;
  freshness_score: number;
  updated_at: number;
}

export default function FreshnessPage() {
  const [report, setReport] = useState<FreshnessReport | null>(null);
  const [staleDocs, setStaleDocs] = useState<StaleDocument[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetchReport();
  }, []);

  async function fetchReport() {
    setLoading(true);
    try {
      const data = await invoke<FreshnessReport>("get_freshness_dashboard", { projectId: null });
      setReport(data);
      const staleData = await invoke<{ documents: StaleDocument[] }>("get_stale_documents", { projectId: null, limit: 10 });
      setStaleDocs(staleData.documents);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <header>
        <h1 className="text-3xl font-bold bg-clip-text text-transparent bg-gradient-to-r from-emerald-400 to-indigo-400">
          신선도 대시보드
        </h1>
        <p className="text-gray-400 mt-2">문서의 노화 상태를 시각적으로 확인하고 유지보수를 진행합니다.</p>
      </header>

      {loading ? (
        <div className="text-gray-500 animate-pulse">데이터를 불러오는 중...</div>
      ) : report ? (
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <div className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6 shadow-xl relative overflow-hidden group">
            <div className="absolute inset-0 bg-gradient-to-br from-emerald-500/10 to-transparent opacity-0 group-hover:opacity-100 transition-opacity"></div>
            <h2 className="text-emerald-400 font-semibold mb-2">🟢 Fresh (100~70)</h2>
            <div className="text-4xl font-bold text-white">{report.fresh_docs}</div>
            <div className="text-sm text-gray-400 mt-2">최신 상태의 문서</div>
          </div>
          
          <div className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6 shadow-xl relative overflow-hidden group">
            <div className="absolute inset-0 bg-gradient-to-br from-amber-500/10 to-transparent opacity-0 group-hover:opacity-100 transition-opacity"></div>
            <h2 className="text-amber-400 font-semibold mb-2">🟡 Aging (70~40)</h2>
            <div className="text-4xl font-bold text-white">{report.aging_docs}</div>
            <div className="text-sm text-gray-400 mt-2">주의가 필요한 문서</div>
          </div>
          
          <div className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6 shadow-xl relative overflow-hidden group">
            <div className="absolute inset-0 bg-gradient-to-br from-rose-500/10 to-transparent opacity-0 group-hover:opacity-100 transition-opacity"></div>
            <h2 className="text-rose-400 font-semibold mb-2">🔴 Stale (&lt;40)</h2>
            <div className="text-4xl font-bold text-white">{report.stale_docs}</div>
            <div className="text-sm text-gray-400 mt-2">유지보수가 필요한 낡은 문서</div>
          </div>

          <div className="col-span-1 sm:col-span-3 bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6 mt-4 shadow-xl">
             <div className="flex justify-between items-center">
                 <div>
                    <h3 className="text-lg font-medium text-white">전체 신선도 평균</h3>
                    <p className="text-gray-400 text-sm">현재 등록된 {report.total_docs}개 문서의 평균 점수입니다.</p>
                 </div>
                 <div className="text-5xl font-bold text-indigo-400">{report.average_score.toFixed(1)}점</div>
             </div>
          </div>
        </div>
      ) : (
        <div className="text-gray-500">리포트를 불러오는데 실패했습니다.</div>
      )}

      <div className="mt-8 bg-gray-900 border border-gray-800 rounded-2xl p-6 shadow-xl relative overflow-hidden">
         <h3 className="text-lg font-semibold text-rose-400 mb-4 flex items-center gap-2">🚧 주의가 필요한 낡은 문서 Top 10</h3>
         {staleDocs.length > 0 ? (
           <div className="overflow-x-auto">
              <table className="w-full text-left text-sm text-gray-400">
                  <thead className="text-xs text-gray-500 uppercase bg-gray-800/50">
                      <tr>
                          <th className="px-4 py-3 font-medium">문서 제목</th>
                          <th className="px-4 py-3 font-medium">프로젝트</th>
                          <th className="px-4 py-3 font-medium text-right">신선도 점수</th>
                          <th className="px-4 py-3 font-medium text-right">마지막 갱신</th>
                      </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-800/50">
                      {staleDocs.map(doc => (
                          <tr key={doc.source_doc_id} className="hover:bg-gray-800/30 transition-colors">
                              <td className="px-4 py-3 font-medium text-gray-300">
                                <div className="flex items-center gap-2">
                                  <span>{doc.title}</span>
                                </div>
                                <div className="text-[10px] text-gray-600 font-mono mt-0.5">{doc.source_doc_id}</div>
                              </td>
                              <td className="px-4 py-3">{doc.project_name}</td>
                              <td className="px-4 py-3 text-right font-bold text-rose-400/90">{doc.freshness_score.toFixed(1)}</td>
                              <td className="px-4 py-3 text-right">
                                {doc.updated_at ? new Date(doc.updated_at * 1000).toLocaleDateString('ko-KR') : '알 수 없음'}
                              </td>
                          </tr>
                      ))}
                  </tbody>
              </table>
           </div>
         ) : (
           <div className="py-12 flex flex-col items-center justify-center text-gray-500 border border-dashed border-gray-700 rounded-xl">
             <span className="text-4xl mb-2">🎉</span>
             <p className="text-sm font-medium text-gray-400">주의가 필요한 낡은 문서가 없습니다.</p>
             <p className="text-xs mt-1">모든 문서가 훌륭하게 관리되고 갱신 중입니다!</p>
           </div>
         )}
      </div>
    </div>
  );
}
