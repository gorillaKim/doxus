import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { CreateScheduleModal } from '../components/scheduler/CreateScheduleModal';

export default function SchedulerPage() {
  const [jobs, setJobs] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [showAddModal, setShowAddModal] = useState(false);
  const [editingJob, setEditingJob] = useState<any | null>(null);
  const [projects, setProjects] = useState<any[]>([]);

  useEffect(() => {
    fetchJobs();
    fetchProjects();
  }, []);

  async function fetchProjects() {
    try {
      const { projects: data } = await invoke<{ projects: any[] }>("list_projects");
      setProjects(data);
    } catch (e) {
      console.error(e);
    }
  }

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
    <div className="max-w-5xl mx-auto space-y-6 pb-20 fade-in">
      <header className="flex justify-between items-center">
        <div>
          <h1 className="text-3xl font-bold bg-clip-text text-transparent bg-gradient-to-r from-blue-400 to-indigo-400">
            자동화 스케줄
          </h1>
          <p className="text-gray-400 mt-2">Doxus 시스템과 Agent가 주기적으로 실행할 작업들을 관리합니다.</p>
        </div>
        
        <button 
          onClick={() => { setEditingJob(null); setShowAddModal(true); }}
          className="px-5 py-2.5 bg-gradient-to-r from-indigo-500 to-blue-500 hover:from-indigo-600 hover:to-blue-600 text-white font-semibold rounded-xl transition-all shadow-lg shadow-indigo-500/20 flex items-center gap-2 active:scale-95"
        >
          <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
          </svg>
          새 스케줄 생성
        </button>
      </header>

      {loading ? (
        <div className="flex flex-col items-center justify-center py-20 gap-4">
          <div className="w-8 h-8 border-4 border-indigo-500/30 border-t-indigo-500 rounded-full animate-spin"></div>
          <p className="text-gray-500 font-medium">스케줄 정보를 불러오는 중...</p>
        </div>
      ) : jobs.length === 0 ? (
        <div className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-3xl p-16 text-center transform transition-all hover:scale-[1.01]">
          <div className="w-20 h-20 bg-white/5 rounded-full flex items-center justify-center mx-auto mb-6">
            <svg className="w-10 h-10 text-gray-500" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
          </div>
          <h3 className="text-xl font-semibold text-gray-300">등록된 스케줄이 없습니다</h3>
          <p className="text-gray-400 mt-2 max-w-sm mx-auto">새로운 자동화 작업을 생성하여 문서 신선도를 관리하고 지식을 최신으로 유지하세요.</p>
        </div>
      ) : (
        <div className="grid gap-4">
          {jobs.map((job) => (
            <div 
              key={job.id}
              className={`relative group overflow-hidden bg-white/5 backdrop-blur-xl hover:bg-white/10 transition-all border ${job.enabled ? 'border-white/10' : 'border-rose-500/20'} rounded-2xl p-6 shadow-xl hover:shadow-indigo-500/5`}
            >
              <div className="flex justify-between items-start gap-4">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center flex-wrap gap-3 mb-2">
                    <span className={`px-2.5 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider ${job.executor === 'system' ? 'bg-blue-500/20 text-blue-300' : 'bg-amber-500/20 text-amber-300'}`}>
                      {job.executor}
                    </span>
                    <h3 className="text-xl font-bold text-gray-100 truncate">{job.job_name}</h3>
                    {job.is_immutable && (
                      <span className="flex items-center gap-1 text-[10px] bg-white/10 text-gray-400 px-2 py-0.5 rounded-full font-medium">
                        <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
                        </svg>
                        시스템 고정
                      </span>
                    )}
                    {!job.enabled && (
                      <span className="px-2.5 py-0.5 bg-rose-500/20 text-rose-400 text-[10px] font-bold rounded-full border border-rose-500/30">
                        비활성화
                      </span>
                    )}
                  </div>
                  
                  <p className="text-gray-400 text-sm leading-relaxed mb-4 max-w-2xl">
                    {job.description || "상세 설명이 등록되지 않았습니다."}
                  </p>

                  <div className="flex flex-wrap gap-4 items-center text-xs">
                    <div className="flex flex-col gap-1">
                      <span className="text-gray-500 font-medium font-bold">작업 정의</span>
                      <code className="text-indigo-300 bg-indigo-500/10 px-2 py-1 rounded border border-indigo-500/20">
                        {job.action}
                      </code>
                    </div>
                    
                    <div className="flex flex-col gap-1">
                      <span className="text-gray-500 font-medium font-bold">주기</span>
                      <span className="text-gray-300 font-mono">
                        {renderScheduleInfo(job.schedule)}
                      </span>
                    </div>

                    <div className="flex flex-col gap-1">
                      <span className="text-gray-500 font-medium font-bold">생성자</span>
                      <span className="text-gray-300 font-mono uppercase text-[10px]">
                        {job.created_by}
                      </span>
                    </div>
                  </div>
                </div>

                <div className="flex flex-col items-end justify-between self-stretch shrink-0">
                  <div className="text-right space-y-1">
                    <div className="flex items-center gap-2 justify-end">
                       <span className="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse"></span>
                       <span className="text-[10px] font-bold text-gray-500 uppercase tracking-tighter">Next Run</span>
                    </div>
                    <p className="text-sm font-mono text-indigo-400 font-semibold italic">
                         {new Date(job.next_run_at * 1000).toLocaleString('ko-KR')}
                    </p>
                    {job.last_run_at && (
                      <p className="text-[10px] text-gray-500">
                        최근 실행: {new Date(job.last_run_at * 1000).toLocaleString('ko-KR')}
                      </p>
                    )}
                  </div>

                  <div className="flex gap-2 mt-4">
                    <button 
                      disabled={job.is_immutable}
                      onClick={() => { setEditingJob(job); setShowAddModal(true); }}
                      className={`px-4 py-1.5 rounded-lg text-sm font-semibold transition-all ${
                        job.is_immutable 
                        ? 'border border-white/5 text-gray-600 cursor-not-allowed' 
                        : 'bg-white/5 hover:bg-white/20 text-gray-300'
                      }`}
                    >
                      설정
                    </button>
                    {!job.is_immutable && (
                       <button 
                        onClick={() => handleDelete(job.id)}
                        className="px-4 py-1.5 bg-rose-500/10 hover:bg-rose-500 text-rose-400 hover:text-white rounded-lg text-sm font-semibold transition-all"
                      >
                        삭제
                      </button>
                    )}
                  </div>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {showAddModal && (
        <CreateScheduleModal 
          projects={projects}
          editingJob={editingJob}
          onClose={() => setShowAddModal(false)}
          onCreated={() => {
            setShowAddModal(false);
            fetchJobs();
          }}
        />
      )}
    </div>
  );
}

function renderScheduleInfo(schedule: any) {
    if (!schedule) return "-";
    switch (schedule.type) {
        case 'interval': {
            const s = schedule.seconds;
            if (s >= 3600 && s % 3600 === 0) return `${s / 3600}시간 마다`;
            if (s >= 60) return `${Math.floor(s / 60)}분 마다`;
            return `${s}초 간격`;
        }
        case 'daily': return `매일 ${schedule.hour}:${schedule.minute.toString().padStart(2, '0')}`;
        case 'weekly': {
            const days = ["일", "월", "화", "수", "목", "금", "토"];
            return `매주 ${days[schedule.day_of_week]}요일 ${schedule.hour}:${schedule.minute.toString().padStart(2, '0')}`;
        }
        case 'monthly': return `매월 ${schedule.day_of_month}일 ${schedule.hour}:${schedule.minute.toString().padStart(2, '0')}`;
        default: return JSON.stringify(schedule);
    }
}
