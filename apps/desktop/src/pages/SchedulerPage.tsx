import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export default function SchedulerPage() {
  const [jobs, setJobs] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetchJobs();
  }, []);

  async function fetchJobs() {
    setLoading(true);
    try {
      const data = await invoke<any[]>("list_scheduled_jobs", { projectId: null });
      setJobs(data);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  }

  async function handleDelete(id: number) {
    if (!confirm("정말 삭제하시겠습니까?")) return;
    try {
      await invoke("delete_scheduled_job", { jobId: id, disableOnly: false });
      fetchJobs();
    } catch (e) {
      alert(e);
    }
  }

  return (
    <div className="max-w-5xl mx-auto space-y-6">
      <header className="flex justify-between items-center">
        <div>
          <h1 className="text-3xl font-bold bg-clip-text text-transparent bg-gradient-to-r from-blue-400 to-indigo-400">
            자동화 스케줄
          </h1>
          <p className="text-gray-400 mt-2">Doxus 시스템과 Agent가 주기적으로 실행할 작업들을 확인합니다.</p>
        </div>
        <button className="px-4 py-2 bg-indigo-500 hover:bg-indigo-600 active:bg-indigo-700 text-white font-medium rounded-xl transition-all shadow-lg shadow-indigo-500/20">
          새 스케줄 생성
        </button>
      </header>

      {loading ? (
        <div className="text-gray-500 animate-pulse">스케줄 로딩 중...</div>
      ) : jobs.length === 0 ? (
        <div className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-10 text-center">
          <p className="text-gray-400">등록된 스케줄이 없습니다.</p>
        </div>
      ) : (
        <div className="space-y-4">
          {jobs.map((job) => (
            <div key={job.id} className="relative overflow-hidden bg-white/5 backdrop-blur-xl hover:bg-white/10 transition-colors border border-white/10 rounded-2xl p-5 shadow-lg group">
              <div className="flex justify-between items-start">
                  <div>
                    <div className="flex items-center gap-3">
                        <span className={`px-2 py-0.5 rounded text-xs font-semibold ${job.executor === 'system' ? 'bg-blue-500/20 text-blue-300' : 'bg-amber-500/20 text-amber-300'}`}>
                            {job.executor.toUpperCase()}
                        </span>
                        <h3 className="text-lg font-medium text-gray-100">{job.job_name}</h3>
                        {!job.enabled && <span className="text-xs text-rose-400 border border-rose-400/30 px-2 py-0.5 rounded-full">비활성화됨</span>}
                    </div>
                    <p className="text-gray-400 text-sm mt-2 font-mono bg-black/20 p-2 rounded-lg inline-block">
                       Action: {job.action}
                    </p>
                  </div>
                  <div className="flex flex-col items-end gap-2">
                      <div className="text-xs text-gray-400 text-right">
                          <p>다음 실행: {new Date(job.next_run_at * 1000).toLocaleString()}</p>
                          {job.last_run_at && <p>마지막 실행: {new Date(job.last_run_at * 1000).toLocaleString()}</p>}
                      </div>
                      <div className="opacity-0 group-hover:opacity-100 transition-opacity flex gap-2">
                          <button 
                             onClick={() => handleDelete(job.id)}
                             className="px-3 py-1 bg-rose-500/20 text-rose-300 hover:bg-rose-500 hover:text-white rounded transition-colors text-sm"
                          >
                             삭제
                          </button>
                      </div>
                  </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
