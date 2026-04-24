import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

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

const PERSONA_PRESETS = [
    { id: 'devlog_specialist', name: '🛠️ 개발 로그 분석가', description: '한 주간의 기술적 결정과 버그 해결 패턴을 분석합니다.' },
    { id: 'knowledge_curator', name: '📚 지식 큐레이터', description: '수집된 아티클들에서 핵심 인사이트와 트렌드를 추출합니다.' },
    { id: 'research_assistant', name: '🔍 리서치 어시스턴트', description: '특정 주제에 대해 조사하고 요약 리포트를 생성합니다.' },
    { id: 'custom', name: '✨ 자유 프롬프트', description: '사용자가 직접 정의한 프롬프트로 작업을 수행합니다.' },
];

function CreateScheduleModal({ projects, editingJob, onClose, onCreated }: { projects: any[], editingJob?: any | null, onClose: () => void, onCreated: () => void }) {
    const isEdit = !!editingJob;
    const [name, setName] = useState(editingJob?.job_name || "");
    const [executor, setExecutor] = useState<"system" | "agent">(editingJob?.executor || "system");
    const [action, setAction] = useState(editingJob?.action || "incremental_sync");
    const [scheduleType, setScheduleType] = useState<"daily" | "weekly" | "interval">(editingJob?.schedule?.type || "daily");
    
    const [persona, setPersona] = useState(editingJob?.action_config?.persona || PERSONA_PRESETS[0].id);
    const [selectedProjects, setSelectedProjects] = useState<string[]>(editingJob?.action_config?.scope?.project_names || []);
    const [tags, setTags] = useState(editingJob?.action_config?.scope?.tags?.join(", ") || "");
    const [keywords, setKeywords] = useState(editingJob?.action_config?.scope?.keywords?.join(", ") || "");
    const [outputProject, setOutputProject] = useState(editingJob?.action_config?.output?.project_name || "");
    const [outputDir, setOutputDir] = useState(editingJob?.action_config?.output?.sub_dir || "reports");
    const [summaryStyle, setSummaryStyle] = useState(editingJob?.action_config?.summary_style || "bullet_points");
    const [customPrompt, setCustomPrompt] = useState(editingJob?.action_config?.custom_prompt || "");
    const [description, setDescription] = useState(editingJob?.description || "");
    
    // AI Provider & Model handling
    const initialModel = editingJob?.action_config?.model || "claude-sonnet-4-6";
    const initialProvider = initialModel.includes("gemini") ? "gemini" : "claude";
    const [provider, setProvider] = useState(initialProvider);
    const [model, setModel] = useState(initialModel);

    // Common Parameters
    const [hour, setHour] = useState(editingJob?.schedule?.hour ?? 3);
    const [minute, setMinute] = useState(editingJob?.schedule?.minute ?? 0);
    const [dayOfWeek, setDayOfWeek] = useState(editingJob?.schedule?.day_of_week ?? 1);
    const [intervalSeconds, setIntervalSeconds] = useState(editingJob?.schedule?.seconds ?? 3600);
    const [runOnIdle, setRunOnIdle] = useState(editingJob?.run_on_idle ?? false);
    
    const [loading, setLoading] = useState(false);

    useEffect(() => {
        if (projects.length > 0 && !outputProject) {
            setOutputProject(projects[0].name);
        }
    }, [projects]);

    async function handleSubmit() {
        if (!name) return alert("스케줄 이름을 입력하세요.");
        if (executor === 'agent' && selectedProjects.length === 0) return alert("최소 1개 이상의 탐색 프로젝트를 선택하세요.");
        if (executor === 'agent' && selectedProjects.length > 3) return alert("탐색 프로젝트는 최대 3개까지만 선택 가능합니다.");

        setLoading(true);
        
        let scheduleJson: any = { type: scheduleType };
        if (scheduleType === 'daily') {
            scheduleJson.hour = hour;
            scheduleJson.minute = minute;
        } else if (scheduleType === 'weekly') {
            scheduleJson.day_of_week = dayOfWeek;
            scheduleJson.hour = hour;
            scheduleJson.minute = minute;
        } else if (scheduleType === 'interval') {
            scheduleJson.seconds = intervalSeconds;
        }

        const actionConfig: any = {};
        if (executor === 'agent') {
            actionConfig.model = model;
            actionConfig.persona = persona;
            actionConfig.scope = {
                project_names: selectedProjects,
                tags: tags.split(",").map(t => t.trim()).filter(Boolean),
                keywords: keywords.split(",").map(k => k.trim()).filter(Boolean),
            };
            actionConfig.output = {
                project_name: outputProject,
                sub_dir: outputDir,
            };
            actionConfig.summary_style = summaryStyle;
            actionConfig.custom_prompt = customPrompt;
        }

        try {
            if (isEdit) {
                await invoke("update_scheduled_job", {
                    jobId: editingJob.id,
                    projectId: null,
                    jobName: name,
                    description,
                    executor,
                    action: executor === 'agent' ? 'ai_agent_report' : action,
                    actionConfig,
                    scheduleJson,
                    runOnIdle
                });
            } else {
                await invoke("create_scheduled_job", {
                    projectId: null,
                    jobName: name,
                    description,
                    executor,
                    action: executor === 'agent' ? 'ai_agent_report' : action,
                    actionConfig,
                    scheduleJson,
                    runOnIdle
                });
            }
            onCreated();
        } catch (e) {
            alert(e);
        } finally {
            setLoading(false);
        }
    }

    const toggleProject = (pName: string) => {
        if (selectedProjects.includes(pName)) {
            setSelectedProjects(selectedProjects.filter(id => id !== pName));
        } else {
            if (selectedProjects.length >= 3) return alert("최대 3개까지만 선택 가능합니다.");
            setSelectedProjects([...selectedProjects, pName]);
        }
    };

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-md animate-in fade-in duration-300">
            <div className="bg-[#11111b] border border-white/10 rounded-[2.5rem] w-full max-w-2xl max-h-[90vh] overflow-hidden flex flex-col shadow-2xl animate-in zoom-in-95 duration-300">
                {/* Header with Tabs */}
                <div className="p-8 pb-4">
                    <div className="flex items-center justify-between mb-6">
                        <h2 className="text-2xl font-bold text-white tracking-tight">
                            {isEdit ? '스케줄 설정 수정' : '새 스케줄 만들기'}
                        </h2>
                        <button onClick={onClose} className="text-gray-500 hover:text-white transition-colors">
                            <svg className="w-6 h-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                            </svg>
                        </button>
                    </div>

                    <div className="flex p-1 bg-white/5 rounded-2xl border border-white/5">
                        <button
                            disabled={isEdit}
                            onClick={() => { setExecutor("system"); setAction("incremental_sync"); }}
                            className={`flex-1 flex items-center justify-center gap-2 py-3 rounded-xl text-sm font-bold transition-all ${
                                executor === 'system' ? 'bg-indigo-600 text-white shadow-lg shadow-indigo-500/20' : 'text-gray-500 hover:text-gray-300'
                            } ${isEdit ? 'opacity-50 cursor-not-allowed' : ''}`}
                        >
                            <span>⚙️</span> 시스템 자동화
                        </button>
                        <button
                            disabled={isEdit}
                            onClick={() => { setExecutor("agent"); setAction("ai_agent_report"); }}
                            className={`flex-1 flex items-center justify-center gap-2 py-3 rounded-xl text-sm font-bold transition-all ${
                                executor === 'agent' ? 'bg-indigo-600 text-white shadow-lg shadow-indigo-500/20' : 'text-gray-500 hover:text-gray-300'
                            } ${isEdit ? 'opacity-50 cursor-not-allowed' : ''}`}
                        >
                            <span>🤖</span> AI 인사이트 에이전트
                        </button>
                    </div>
                </div>
                
                <div className="flex-1 overflow-y-auto px-8 pb-8 custom-scrollbar">
                    <div className="space-y-6">
                        {/* 기본 정보 */}
                        <div className="grid grid-cols-2 gap-4">
                            <div className="col-span-2">
                                <label className="block text-xs font-bold text-gray-500 mb-2 uppercase tracking-widest pl-1">스케줄 이름</label>
                                <input 
                                    value={name}
                                    onChange={e => setName(e.target.value)}
                                    placeholder="예: 주간 개발 리포트"
                                    className="w-full bg-white/5 border border-white/10 rounded-2xl px-5 py-3.5 text-white focus:outline-none focus:ring-2 focus:ring-indigo-500/40 transition-all placeholder:text-gray-600" 
                                />
                            </div>
                            <div className="col-span-2">
                                <label className="block text-xs font-bold text-gray-500 mb-2 uppercase tracking-widest pl-1">스케줄 상세 설명</label>
                                <textarea 
                                    value={description}
                                    onChange={e => setDescription(e.target.value)}
                                    placeholder="어떤 목적으로 만들어진 스케줄인지 기록해 두세요."
                                    className="w-full bg-white/5 border border-white/10 rounded-2xl px-5 py-3.5 text-white focus:outline-none focus:ring-2 focus:ring-indigo-500/40 transition-all placeholder:text-gray-600 resize-none h-20" 
                                />
                            </div>
                        </div>

                        {executor === 'system' ? (
                            <div>
                                <label className="block text-xs font-bold text-gray-500 mb-2 uppercase tracking-widest pl-1">작업 유형</label>
                                <div className="grid grid-cols-1 gap-2">
                                    {[
                                        { id: 'incremental_sync', name: '증분 동기화', desc: '변경된 파일만 색인합니다.' },
                                        { id: 'full_index', name: '전체 인덱싱', desc: '모든 데이터를 처음부터 다시 색인합니다.' },
                                        { id: 'freshness_batch', name: '신선도 체크', desc: '문서의 상태를 평가하고 점수를 갱신합니다.' },
                                    ].map(opt => (
                                        <button
                                            key={opt.id}
                                            onClick={() => setAction(opt.id)}
                                            className={`p-4 rounded-2xl border text-left transition-all ${
                                                action === opt.id 
                                                ? 'bg-indigo-500/10 border-indigo-500/50 ring-1 ring-indigo-500/50' 
                                                : 'bg-white/5 border-white/5 hover:border-white/20'
                                            }`}
                                        >
                                            <div className={`font-bold text-sm ${action === opt.id ? 'text-indigo-400' : 'text-white'}`}>{opt.name}</div>
                                            <div className="text-[10px] text-gray-500 mt-0.5">{opt.desc}</div>
                                        </button>
                                    ))}
                                </div>
                            </div>
                        ) : (
                            <div className="space-y-6 animate-in slide-in-from-bottom-2 duration-300">
                                <div className="grid grid-cols-2 gap-4">
                                    <div>
                                        <label className="block text-xs font-bold text-gray-500 mb-2 uppercase tracking-widest pl-1">AI 공급자</label>
                                        <div className="flex gap-2">
                                            {[
                                                { id: 'claude', name: 'Anthropic', icon: '🎨' },
                                                { id: 'gemini', name: 'Google', icon: '💎' },
                                                { id: 'openai', name: 'OpenAI', icon: '🤖' },
                                            ].map(p => (
                                                <button
                                                    key={p.id}
                                                    onClick={() => {
                                                        setProvider(p.id);
                                                        const defaultModels: any = {
                                                            claude: 'claude-sonnet-4-6',
                                                            gemini: 'gemini-2.5-pro',
                                                            openai: 'gpt-4o'
                                                        };
                                                        setModel(defaultModels[p.id]);
                                                    }}
                                                    className={`flex-1 py-3 px-2 rounded-2xl border text-center transition-all ${
                                                        provider === p.id 
                                                        ? 'bg-indigo-500/10 border-indigo-500/50 ring-1 ring-indigo-500/50 text-indigo-400' 
                                                        : 'bg-white/5 border-white/5 text-gray-500 hover:border-white/20'
                                                    }`}
                                                >
                                                    <div className="text-lg mb-1">{p.icon}</div>
                                                    <div className="text-[10px] font-bold">{p.name}</div>
                                                </button>
                                            ))}
                                        </div>
                                    </div>
                                    <div>
                                        <label className="block text-xs font-bold text-gray-500 mb-2 uppercase tracking-widest pl-1">세부 모델 선택</label>
                                        <select 
                                            value={model}
                                            onChange={e => setModel(e.target.value)}
                                            className="w-full bg-white/5 border border-white/10 rounded-2xl px-4 py-4 text-white focus:outline-none transition-all focus:border-indigo-500/50 appearance-none bg-[url('data:image/svg+xml;charset=US-ASCII,%3Csvg%20width%3D%2220%22%20height%3D%2220%22%20viewBox%3D%220%200%2020%2020%22%20fill%3D%22none%22%20xmlns%3D%22http%3A//www.w3.org/2000/svg%22%3E%3Cpath%20d%3D%22M5%208L10%2013L15%208%22%20stroke%3D%22white%22%20stroke-width%3D%222%22%20stroke-linecap%3D%22round%22%20stroke-linejoin%3D%22round%22/%3E%3C/svg%3E')] bg-[length:20px_20px] bg-[right_1rem_center] bg-no-repeat"
                                        >
                                            {provider === 'claude' && (
                                                <>
                                                    <option value="claude-sonnet-4-6">Claude Sonnet 4.6</option>
                                                    <option value="claude-opus-4-6">Claude Opus 4.6</option>
                                                    <option value="claude-haiku-4-5-20251001">Claude Haiku 4.5</option>
                                                </>
                                            )}
                                            {provider === 'gemini' && (
                                                <>
                                                    <option value="gemini-2.5-pro">Gemini 2.5 Pro</option>
                                                    <option value="gemini-2.5-flash">Gemini 2.5 Flash</option>
                                                </>
                                            )}
                                            {provider === 'openai' && (
                                                <>
                                                    <option value="gpt-4o">GPT-4o</option>
                                                    <option value="gpt-4o-mini">GPT-4o Mini</option>
                                                    <option value="o1-preview">o1 Preview</option>
                                                </>
                                            )}
                                        </select>
                                    </div>
                                </div>

                                <div className="grid grid-cols-1 gap-2">
                                    <div>
                                        <label className="block text-xs font-bold text-gray-500 mb-2 uppercase tracking-widest pl-1">페르소나</label>
                                        <div className="grid grid-cols-2 gap-2">
                                            {PERSONA_PRESETS.map(p => (
                                                <button
                                                    key={p.id}
                                                    onClick={() => setPersona(p.id)}
                                                    className={`p-3 rounded-xl border text-left transition-all ${
                                                        persona === p.id 
                                                        ? 'bg-white/10 border-white/20 text-indigo-400' 
                                                        : 'bg-white/5 border-white/5 text-gray-500 hover:text-gray-300'
                                                    }`}
                                                >
                                                    <div className="flex items-center gap-2">
                                                        <span className="text-lg">{p.icon}</span>
                                                        <span className="text-xs font-bold">{p.name}</span>
                                                    </div>
                                                </button>
                                            ))}
                                        </div>
                                    </div>
                                </div>

                                {/* 탐색 범위 */}
                                <div className="bg-white/5 border border-white/5 rounded-[2rem] p-6 space-y-4">
                                    <div>
                                        <label className="block text-xs font-bold text-gray-400 mb-3 uppercase tracking-widest">탐색 대상 프로젝트 (최대 3개)</label>
                                        <div className="flex flex-wrap gap-2">
                                            {projects.map(p => (
                                                <button
                                                    key={p.name}
                                                    onClick={() => toggleProject(p.name)}
                                                    className={`px-4 py-2 rounded-full text-xs font-bold transition-all border ${
                                                        selectedProjects.includes(p.name)
                                                        ? 'bg-indigo-500 border-indigo-400 text-white'
                                                        : 'bg-black/20 border-white/5 text-gray-500 hover:text-gray-400'
                                                    }`}
                                                >
                                                    {p.display_name}
                                                </button>
                                            ))}
                                        </div>
                                    </div>

                                    <div className="grid grid-cols-2 gap-4">
                                        <div>
                                            <label className="block text-[10px] font-bold text-gray-500 mb-1.5 uppercase tracking-widest pl-1">태그 필터</label>
                                            <input 
                                                value={tags}
                                                onChange={e => setTags(e.target.value)}
                                                placeholder="쉼표로 구분 (예: devlog, idea)"
                                                className="w-full bg-black/30 border border-white/5 rounded-xl px-4 py-2.5 text-xs text-white focus:outline-none" 
                                            />
                                        </div>
                                        <div>
                                            <label className="block text-[10px] font-bold text-gray-500 mb-1.5 uppercase tracking-widest pl-1">검색 키워드</label>
                                            <input 
                                                value={keywords}
                                                onChange={e => setKeywords(e.target.value)}
                                                placeholder="예: 신규 기능, 아키텍처"
                                                className="w-full bg-black/30 border border-white/5 rounded-xl px-4 py-2.5 text-xs text-white focus:outline-none" 
                                            />
                                        </div>
                                    </div>
                                </div>

                                {/* 출력 설정 */}
                                <div className="bg-indigo-500/5 border border-indigo-500/10 rounded-[2rem] p-6 space-y-4">
                                    <h4 className="text-xs font-bold text-indigo-400 mb-2 uppercase tracking-widest">산출물 저장 경로</h4>
                                    <div className="grid grid-cols-2 gap-4">
                                        <div>
                                            <label className="block text-[10px] font-bold text-gray-500 mb-1.5 uppercase tracking-widest pl-1">기준 프로젝트</label>
                                            <select 
                                                value={outputProject}
                                                onChange={e => setOutputProject(e.target.value)}
                                                className="w-full bg-black/30 border border-white/5 rounded-xl px-4 py-2.5 text-xs text-white focus:outline-none"
                                            >
                                                {projects.map(p => (
                                                    <option key={p.name} value={p.name}>{p.display_name}</option>
                                                ))}
                                            </select>
                                        </div>
                                        <div>
                                            <label className="block text-[10px] font-bold text-gray-500 mb-1.5 uppercase tracking-widest pl-1">하위 디렉토리</label>
                                            <input 
                                                value={outputDir}
                                                onChange={e => setOutputDir(e.target.value)}
                                                placeholder="예: reports/weekly"
                                                className="w-full bg-black/30 border border-white/5 rounded-xl px-4 py-2.5 text-xs text-white focus:outline-none" 
                                            />
                                        </div>
                                    </div>
                                    <div className="text-[10px] text-gray-500 flex items-center gap-2">
                                        <span className="shrink-0">📍 최종 저장:</span>
                                        <code className="text-indigo-400 truncate">
                                            {outputProject}/{outputDir}/{new Date().toISOString().split('T')[0]}-{name || 'Report'}.md
                                        </code>
                                    </div>
                                </div>

                                {/* 요약 스타일 */}
                                <div>
                                    <label className="block text-xs font-bold text-gray-500 mb-2 uppercase tracking-widest pl-1">결과물 요약 방식</label>
                                    <div className="grid grid-cols-4 gap-2">
                                        {[
                                            { id: 'bullet_points', name: '📌 지표형' },
                                            { id: 'narrative', name: '📖 서사형' },
                                            { id: 'actionable', name: '💡 통찰형' },
                                            { id: 'comparative', name: '🔄 비교형' },
                                        ].map(s => (
                                            <button
                                                key={s.id}
                                                onClick={() => setSummaryStyle(s.id)}
                                                className={`py-2 rounded-xl text-[10px] font-bold transition-all border ${
                                                    summaryStyle === s.id 
                                                    ? 'bg-white/10 border-white/30 text-white' 
                                                    : 'bg-white/5 border-white/5 text-gray-500 hover:text-gray-400'
                                                }`}
                                            >
                                                {s.name}
                                            </button>
                                        ))}
                                    </div>
                                </div>

                                {/* 커스텀 프롬프트 */}
                                <div>
                                    <label className="block text-xs font-bold text-gray-500 mb-2 uppercase tracking-widest pl-1">
                                        {persona === 'custom' ? '✨ 에이전트 명령 프롬프트' : '➕ 추가 지시사항 (선택사항)'}
                                    </label>
                                    <textarea 
                                        value={customPrompt}
                                        onChange={e => setCustomPrompt(e.target.value)}
                                        placeholder={persona === 'custom' 
                                            ? "에이전트가 어떤 작업을 수행해야 하는지 구체적으로 입력하세요.\n예: '최근 인덱싱된 문서들을 분석해서 주요 보안 취약점 대책을 정리해줘.'" 
                                            : "기본 페르소나 작업 외에 특별히 신경 써야 할 점이 있다면 입력하세요."}
                                        className="w-full bg-white/5 border border-white/10 rounded-2xl px-5 py-3.5 text-white focus:outline-none focus:ring-2 focus:ring-indigo-500/40 transition-all placeholder:text-gray-600 resize-none h-32 text-sm" 
                                    />
                                </div>
                            </div>
                        )}

                        {/* 반복 주기 및 유휴 설정 */}
                        <div className="pt-4 border-t border-white/5 space-y-4">
                             <div>
                                <label className="block text-xs font-bold text-gray-500 mb-3 uppercase tracking-widest pl-1">실행 주기 및 스케줄</label>
                                <div className="flex gap-2">
                                    {['daily', 'weekly', 'interval'].map((t) => (
                                        <button
                                            key={t}
                                            onClick={() => setScheduleType(t as any)}
                                            className={`flex-1 py-2.5 rounded-xl text-xs font-bold transition-all border ${
                                                scheduleType === t 
                                                ? 'bg-white/10 border-white/20 text-white' 
                                                : 'bg-transparent border-white/5 text-gray-500 hover:bg-white/5'
                                            }`}
                                        >
                                            {t === 'daily' ? '매일' : t === 'weekly' ? '매주' : '지정 간격'}
                                        </button>
                                    ))}
                                </div>
                            </div>

                            <div className="p-5 bg-black/20 rounded-3xl border border-white/5">
                                {scheduleType === 'daily' && (
                                    <div className="flex items-center justify-between">
                                        <span className="text-xs font-medium text-gray-400">매일 실행 시각</span>
                                        <div className="flex items-center gap-2">
                                            <input 
                                                type="number" value={hour}
                                                onChange={e => setHour(Math.min(23, Math.max(0, parseInt(e.target.value) || 0)))}
                                                className="w-16 bg-white/5 border border-white/10 rounded-lg px-2 py-1.5 text-center text-sm text-white" 
                                            />
                                            <span className="text-gray-600">:</span>
                                            <input 
                                                type="number" value={minute}
                                                onChange={e => setMinute(Math.min(59, Math.max(0, parseInt(e.target.value) || 0)))}
                                                className="w-16 bg-white/5 border border-white/10 rounded-lg px-2 py-1.5 text-center text-sm text-white" 
                                            />
                                        </div>
                                    </div>
                                )}
                                {scheduleType === 'weekly' && (
                                    <div className="space-y-4">
                                        <div className="flex items-center justify-between">
                                            <span className="text-xs font-medium text-gray-400">실행 요일</span>
                                            <select 
                                                value={dayOfWeek}
                                                onChange={e => setDayOfWeek(parseInt(e.target.value))}
                                                className="bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-xs text-white outline-none"
                                            >
                                                {["일", "월", "화", "수", "목", "금", "토"].map((day, d) => (
                                                    <option key={d} value={d}>{day}요일</option>
                                                ))}
                                            </select>
                                        </div>
                                        <div className="flex items-center justify-between border-t border-white/5 pt-3">
                                            <span className="text-xs font-medium text-gray-400">실행 시각</span>
                                            <div className="flex items-center gap-2">
                                                <input type="number" value={hour} onChange={e => setHour(Math.min(23, Math.max(0, parseInt(e.target.value) || 0)))} className="w-14 bg-white/5 border border-white/10 rounded px-1 py-1 text-center text-white" />
                                                <span className="text-gray-600">:</span>
                                                <input type="number" value={minute} onChange={e => setMinute(Math.min(59, Math.max(0, parseInt(e.target.value) || 0)))} className="w-14 bg-white/5 border border-white/10 rounded px-1 py-1 text-center text-white" />
                                            </div>
                                        </div>
                                    </div>
                                )}
                                {scheduleType === 'interval' && (
                                    <div className="flex items-center justify-between">
                                        <span className="text-xs font-medium text-gray-400">반복 간격 (분 단위)</span>
                                        <input 
                                            type="number"
                                            value={intervalSeconds / 60}
                                            onChange={e => setIntervalSeconds(Math.max(1, parseInt(e.target.value) || 1) * 60)}
                                            className="w-24 bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-center text-sm text-white" 
                                        />
                                    </div>
                                )}
                            </div>

                            <button 
                                onClick={() => setRunOnIdle(!runOnIdle)}
                                className={`w-full flex items-center justify-between p-4 rounded-3xl border transition-all ${
                                    runOnIdle ? 'bg-indigo-500/10 border-indigo-500/30' : 'bg-white/5 border-white/5 grayscale opacity-60'
                                }`}
                            >
                                <div className="flex items-center gap-3">
                                    <div className={`w-10 h-10 rounded-2xl flex items-center justify-center text-lg ${runOnIdle ? 'bg-indigo-500 text-white' : 'bg-gray-800 text-gray-500'}`}>
                                        ☕
                                    </div>
                                    <div className="text-left">
                                        <div className={`text-sm font-bold ${runOnIdle ? 'text-indigo-300' : 'text-gray-400'}`}>유휴 상태 대기 모드</div>
                                        <div className="text-[10px] text-gray-500">PC를 사용하지 않을 때만 조용히 실행합니다.</div>
                                    </div>
                                </div>
                                <div className={`w-12 h-6 rounded-full p-1 transition-all ${runOnIdle ? 'bg-indigo-500' : 'bg-gray-800'}`}>
                                    <div className={`w-4 h-4 bg-white rounded-full transition-all transform ${runOnIdle ? 'translate-x-6' : 'translate-x-0'}`}></div>
                                </div>
                            </button>
                        </div>
                    </div>
                </div>

                <div className="p-8 bg-black/40 border-t border-white/5 flex gap-4">
                    <button 
                        onClick={onClose}
                        className="flex-1 px-6 py-4 bg-white/5 hover:bg-white/10 text-gray-400 font-bold rounded-2xl transition-all"
                    >
                        취소
                    </button>
                    <button 
                        onClick={handleSubmit}
                        disabled={loading}
                        className="flex-[2] px-6 py-4 bg-indigo-600 hover:bg-indigo-500 text-white font-bold rounded-2xl shadow-xl shadow-indigo-500/20 transition-all disabled:opacity-50 active:scale-95"
                    >
                        {loading 
                            ? (isEdit ? "스케줄 수정 중..." : "스케줄 생성 중...") 
                            : (isEdit ? "✅ 설정 변경사항 저장" : "🚀 스케줄 등록하기")}
                    </button>
                </div>
            </div>
        </div>
    );
}
