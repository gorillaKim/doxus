import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export default function SchedulerPage() {
  const [jobs, setJobs] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [showAddModal, setShowAddModal] = useState(false);

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
    <div className="max-w-5xl mx-auto space-y-6 pb-20 fade-in">
      <header className="flex justify-between items-center">
        <div>
          <h1 className="text-3xl font-bold bg-clip-text text-transparent bg-gradient-to-r from-blue-400 to-indigo-400">
            자동화 스케줄
          </h1>
          <p className="text-gray-400 mt-2">Doxus 시스템과 Agent가 주기적으로 실행할 작업들을 관리합니다.</p>
        </div>
        
        <button 
          onClick={() => setShowAddModal(true)}
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

function CreateScheduleModal({ onClose, onCreated }: { onClose: () => void, onCreated: () => void }) {
    const [name, setName] = useState("");
    const [action, setAction] = useState("incremental_sync");
    const [type, setType] = useState<"daily" | "weekly" | "interval">("daily");
    
    // Parameters
    const [hour, setHour] = useState(3);
    const [minute, setMinute] = useState(0);
    const [dayOfWeek, setDayOfWeek] = useState(1); // Monday
    const [intervalSeconds, setIntervalSeconds] = useState(3600); // 1 hour
    const [runOnIdle, setRunOnIdle] = useState(false);
    
    const [loading, setLoading] = useState(false);

    async function handleAdd() {
        if (!name) return alert("이름을 입력하세요.");
        setLoading(true);
        
        let scheduleJson: any = { type };
        if (type === 'daily') {
            scheduleJson.hour = hour;
            scheduleJson.minute = minute;
        } else if (type === 'weekly') {
            scheduleJson.day_of_week = dayOfWeek;
            scheduleJson.hour = hour;
            scheduleJson.minute = minute;
        } else if (type === 'interval') {
            scheduleJson.seconds = intervalSeconds;
        }

        try {
            await invoke("create_scheduled_job", {
                jobName: name,
                executor: "system",
                action,
                actionConfig: {},
                scheduleJson,
                runOnIdle
            });
            onCreated();
        } catch (e) {
            alert(e);
        } finally {
            setLoading(false);
        }
    }

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-sm animate-in fade-in duration-200">
            <div className="bg-[#1e1e2e] border border-white/10 rounded-3xl p-8 w-full max-w-md shadow-2xl animate-in zoom-in-95 duration-200">
                <h2 className="text-2xl font-bold text-white mb-6">새 스케줄 만들기</h2>
                
                <div className="space-y-5">
                    <div>
                        <label className="block text-sm font-medium text-gray-400 mb-1.5 font-bold">스케줄 이름</label>
                        <input 
                            value={name}
                            onChange={e => setName(e.target.value)}
                            placeholder="예: 주간 동기화"
                            className="w-full bg-white/5 border border-white/10 rounded-xl px-4 py-2.5 text-white focus:outline-none focus:ring-2 focus:ring-indigo-500/50" 
                        />
                    </div>

                    <div>
                        <label className="block text-sm font-medium text-gray-400 mb-1.5 font-bold">작업 타입</label>
                        <select 
                            value={action}
                            onChange={e => setAction(e.target.value)}
                            className="w-full bg-white/5 border border-white/10 rounded-xl px-4 py-2.5 text-white focus:outline-none focus:ring-2 focus:ring-indigo-500/50 appearance-none cursor-pointer"
                        >
                            <option value="incremental_sync">증분 동기화 (Incremental Sync)</option>
                            <option value="full_index">전체 재인덱싱 (Full Reindex)</option>
                            <option value="freshness_batch">신선도 점수 갱신</option>
                        </select>
                    </div>

                    <div>
                        <label className="block text-sm font-medium text-gray-400 mb-1.5 font-bold">실행 주기 방식</label>
                        <div className="grid grid-cols-3 gap-2">
                            {['daily', 'weekly', 'interval'].map((t) => (
                                <button
                                    key={t}
                                    onClick={() => setType(t as any)}
                                    className={`px-3 py-2 rounded-xl text-xs font-bold transition-all border ${
                                        type === t 
                                        ? 'bg-indigo-500 border-indigo-400 text-white' 
                                        : 'bg-white/5 border-white/10 text-gray-400 hover:bg-white/10'
                                    }`}
                                >
                                    {t === 'daily' ? '매일' : t === 'weekly' ? '매주' : '간격'}
                                </button>
                            ))}
                        </div>
                    </div>

                    <div className="p-4 bg-white/5 border border-white/5 rounded-2xl">
                        {type === 'daily' && (
                            <div>
                                <label className="block text-xs font-bold text-gray-500 mb-2 uppercase tracking-tight">지정 시간</label>
                                <div className="flex gap-2 items-center">
                                    <input 
                                        type="number" value={hour}
                                        onChange={e => setHour(Math.min(23, Math.max(0, parseInt(e.target.value) || 0)))}
                                        className="w-full bg-black/20 border border-white/10 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-indigo-500" 
                                    />
                                    <span className="text-gray-600">:</span>
                                    <input 
                                        type="number" value={minute}
                                        onChange={e => setMinute(Math.min(59, Math.max(0, parseInt(e.target.value) || 0)))}
                                        className="w-full bg-black/20 border border-white/10 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-indigo-500" 
                                    />
                                </div>
                                <p className="text-[10px] text-gray-500 mt-2 font-medium">매일 {hour}시 {minute}분에 작업을 실행합니다.</p>
                            </div>
                        )}

                        {type === 'weekly' && (
                            <div className="space-y-3">
                                <div>
                                    <label className="block text-xs font-bold text-gray-500 mb-2 uppercase tracking-tight">요일 선택</label>
                                    <select 
                                        value={dayOfWeek}
                                        onChange={e => setDayOfWeek(parseInt(e.target.value))}
                                        className="w-full bg-black/20 border border-white/10 rounded-lg px-3 py-2 text-white focus:outline-none"
                                    >
                                        {["일", "월", "화", "수", "목", "금", "토"].map((day, d) => (
                                            <option key={d} value={d}>{day}요일</option>
                                        ))}
                                    </select>
                                </div>
                                <div className="flex gap-2 items-center">
                                    <input 
                                        type="number" value={hour}
                                        onChange={e => setHour(Math.min(23, Math.max(0, parseInt(e.target.value) || 0)))}
                                        className="w-full bg-black/20 border border-white/10 rounded-lg px-3 py-2 text-white focus:outline-none" 
                                    />
                                    <span className="text-gray-600">:</span>
                                    <input 
                                        type="number" value={minute}
                                        onChange={e => setMinute(Math.min(59, Math.max(0, parseInt(e.target.value) || 0)))}
                                        className="w-full bg-black/20 border border-white/10 rounded-lg px-3 py-2 text-white focus:outline-none" 
                                    />
                                </div>
                                <p className="text-[10px] text-gray-500 mt-1 font-medium italic">매주 {["일", "월", "화", "수", "목", "금", "토"][dayOfWeek]}요일 {hour}:{minute.toString().padStart(2, '0')}에 실행합니다.</p>
                            </div>
                        )}

                        {type === 'interval' && (
                            <div>
                                <label className="block text-xs font-bold text-gray-500 mb-2 uppercase tracking-tight">간격 설정 (분)</label>
                                <input 
                                    type="number"
                                    value={intervalSeconds / 60}
                                    onChange={e => setIntervalSeconds(Math.max(1, parseInt(e.target.value) || 1) * 60)}
                                    className="w-full bg-black/20 border border-white/10 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-indigo-500" 
                                />
                                <p className="text-[10px] text-gray-500 mt-2 font-medium">최신 동기화를 위해 {intervalSeconds / 60}분 마다 작업을 반복합니다.</p>
                            </div>
                        )}
                    </div>

                    <div className="flex items-center justify-between p-4 bg-white/5 border border-white/5 rounded-2xl cursor-pointer hover:bg-white/10 transition-all" onClick={() => setRunOnIdle(!runOnIdle)}>
                        <div className="flex-1">
                            <label className="block text-sm font-bold text-white cursor-pointer">유휴 상태에서만 실행</label>
                            <p className="text-[10px] text-gray-500 font-medium">사용자가 자리를 비웠을 때만 작업을 수행하여 부하를 최소화합니다.</p>
                        </div>
                        <div className={`w-10 h-6 rounded-full p-1 transition-all ${runOnIdle ? 'bg-indigo-500' : 'bg-gray-700'}`}>
                            <div className={`w-4 h-4 bg-white rounded-full transition-all transform ${runOnIdle ? 'translate-x-4' : 'translate-x-0'}`}></div>
                        </div>
                    </div>
                </div>

                <div className="flex gap-3 mt-8">
                    <button 
                        onClick={onClose}
                        className="flex-1 px-4 py-2.5 bg-white/5 hover:bg-white/10 text-gray-300 font-semibold rounded-xl transition-all"
                    >
                        취소
                    </button>
                    <button 
                        onClick={handleAdd}
                        disabled={loading}
                        className="flex-1 px-4 py-2.5 bg-indigo-500 hover:bg-indigo-600 text-white font-semibold rounded-xl shadow-lg shadow-indigo-500/20 transition-all disabled:opacity-50 active:scale-95"
                    >
                        {loading ? "생성 중..." : "스케줄 등록"}
                    </button>
                </div>
            </div>
        </div>
    );
}
