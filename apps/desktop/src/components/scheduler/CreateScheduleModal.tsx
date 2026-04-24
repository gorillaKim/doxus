import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface Project {
    name: string;
    display_name: string;
}

interface CreateScheduleModalProps {
    projects: Project[];
    editingJob?: any | null;
    onClose: () => void;
    onCreated: () => void;
}

export const PERSONA_PRESETS = [
    { id: 'devlog_specialist', name: '🛠️ 개발 로그 분석가', description: '한 주간의 기술적 결정과 버그 해결 패턴을 분석합니다.' },
    { id: 'knowledge_curator', name: '📚 지식 큐레이터', description: '수집된 아티클들에서 핵심 인사이트와 트렌드를 추출합니다.' },
    { id: 'research_assistant', name: '🔍 리서치 어시스턴트', description: '특정 주제에 대해 조사하고 요약 리포트를 생성합니다.' },
    { id: 'custom', name: '✨ 자유 프롬프트', description: '사용자가 직접 정의한 프롬프트로 작업을 수행합니다.' },
];

export function CreateScheduleModal({ projects, editingJob, onClose, onCreated }: CreateScheduleModalProps) {
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
    const initialProvider = initialModel.includes("gemini") ? "gemini" : initialModel.includes("gpt") ? "openai" : "claude";
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
            <div className="bg-[#0b0b13] border border-white/10 rounded-[2.5rem] w-full max-w-2xl max-h-[90vh] overflow-hidden flex flex-col shadow-[0_0_50px_-12px_rgba(79,70,229,0.2)] animate-in zoom-in-95 duration-300">
                {/* Header Section */}
                <div className="px-8 pt-8 pb-4 border-b border-white/5">
                    <div className="flex items-center justify-between mb-6">
                        <div>
                            <h2 className="text-2xl font-bold text-white tracking-tight leading-none mb-1">
                                {isEdit ? '스케줄 설정 수정' : '새 자동화 스케줄'}
                            </h2>
                            <p className="text-[10px] text-gray-500 font-medium uppercase tracking-[0.2em]">
                                {executor === 'agent' ? 'AI Insight Agent Task' : 'System Automation Task'}
                            </p>
                        </div>
                        <button 
                            onClick={onClose} 
                            className="w-10 h-10 flex items-center justify-center rounded-full bg-white/5 border border-white/5 text-gray-500 hover:text-white hover:bg-white/10 transition-all"
                        >
                            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                            </svg>
                        </button>
                    </div>

                    <div className="flex p-1.5 bg-black/40 rounded-2xl border border-white/5">
                        {[
                            { id: 'system', icon: '⚙️', label: '시스템 자동화' },
                            { id: 'agent', icon: '🤖', label: 'AI 인사이트' }
                        ].map(opt => (
                            <button
                                key={opt.id}
                                disabled={isEdit}
                                onClick={() => { 
                                    setExecutor(opt.id as any); 
                                    setAction(opt.id === 'agent' ? "ai_agent_report" : "incremental_sync"); 
                                }}
                                className={`flex-1 flex items-center justify-center gap-2.5 py-3 rounded-xl text-xs font-bold transition-all ${
                                    executor === opt.id 
                                    ? 'bg-indigo-600 text-white shadow-[0_0_20px_rgba(79,70,229,0.3)]' 
                                    : 'text-gray-500 hover:text-gray-300 hover:bg-white/5'
                                } ${isEdit ? 'opacity-50 cursor-not-allowed' : ''}`}
                            >
                                <span className="text-base">{opt.icon}</span>
                                {opt.label}
                            </button>
                        ))}
                    </div>
                </div>
                
                {/* Scrollable Content Area */}
                <div className="flex-1 overflow-y-auto px-8 py-6 space-y-8 custom-scrollbar">
                    {/* Section 1: Basic Info */}
                    <div className="space-y-5">
                        <div className="flex items-center gap-2 px-1">
                            <div className="w-1.5 h-1.5 rounded-full bg-indigo-500 shadow-[0_0_8px_rgba(79,70,229,0.8)]" />
                            <h3 className="text-[11px] font-black text-gray-400 uppercase tracking-widest">기본 정보 설정</h3>
                        </div>
                        
                        <div className="grid grid-cols-1 gap-4">
                            <div>
                                <input 
                                    value={name}
                                    onChange={e => setName(e.target.value)}
                                    placeholder="스케줄 이름을 입력하세요 (예: 주간 개발 로그 리포팅)"
                                    className="w-full bg-white/5 border border-white/10 rounded-2xl px-6 py-4 text-sm text-white focus:outline-none focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500/50 transition-all placeholder:text-gray-600" 
                                />
                            </div>
                            <div>
                                <textarea 
                                    value={description}
                                    onChange={e => setDescription(e.target.value)}
                                    placeholder="작업에 대한 상세 설명을 기록해 두세요."
                                    className="w-full bg-white/5 border border-white/10 rounded-2xl px-6 py-4 text-sm text-white focus:outline-none focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500/50 transition-all placeholder:text-gray-600 resize-none h-24" 
                                />
                            </div>
                        </div>
                    </div>

                    {/* Section 2: Action Specifics */}
                    <div className="space-y-5">
                        <div className="flex items-center gap-2 px-1">
                            <div className="w-1.5 h-1.5 rounded-full bg-indigo-500 shadow-[0_0_8px_rgba(79,70,229,0.8)]" />
                            <h3 className="text-[11px] font-black text-gray-400 uppercase tracking-widest">
                                {executor === 'system' ? '실행 작업 및 동작' : '에이전트 지능 및 페르소나'}
                            </h3>
                        </div>

                        {executor === 'system' ? (
                            <div className="grid grid-cols-1 gap-3">
                                {[
                                    { id: 'incremental_sync', name: '증분 동기화', desc: '변경된 파일만 선별하여 색인합니다.' },
                                    { id: 'full_index', name: '전체 인덱싱', desc: '모든 데이터를 깨끗하게 다시 색인합니다.' },
                                    { id: 'freshness_batch', name: '신선도 분석', desc: '문서의 상태를 정밀 진단하고 점수를 갱신합니다.' },
                                ].map(opt => (
                                    <button
                                        key={opt.id}
                                        onClick={() => setAction(opt.id)}
                                        className={`p-5 rounded-2xl border text-left transition-all group ${
                                            action === opt.id 
                                            ? 'bg-indigo-500/10 border-indigo-500/40 ring-1 ring-indigo-500/20' 
                                            : 'bg-white/5 border-white/5 hover:border-white/10 hover:bg-white/[0.07]'
                                        }`}
                                    >
                                        <div className="flex items-center justify-between">
                                            <div className={`font-bold text-sm ${action === opt.id ? 'text-indigo-300' : 'text-gray-200 group-hover:text-white'}`}>{opt.name}</div>
                                            {action === opt.id && <div className="w-2 h-2 rounded-full bg-indigo-500" />}
                                        </div>
                                        <div className="text-[11px] text-gray-500 mt-1 font-medium">{opt.desc}</div>
                                    </button>
                                ))}
                            </div>
                        ) : (
                            <div className="space-y-6">
                                {/* AI Provider Selection */}
                                <div className="grid grid-cols-3 gap-3">
                                    {[
                                        { id: 'claude', name: 'Anthropic', icon: '🎨' },
                                        { id: 'gemini', name: 'Google AI', icon: '✨' },
                                        { id: 'openai', name: 'OpenAI', icon: '🦾' },
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
                                            className={`flex flex-col items-center justify-center p-4 rounded-2xl border transition-all ${
                                                provider === p.id 
                                                ? 'bg-indigo-500/10 border-indigo-500/50 ring-1 ring-indigo-500/20 text-indigo-300' 
                                                : 'bg-white/5 border-white/5 text-gray-500 hover:border-white/20 hover:text-gray-300'
                                            }`}
                                        >
                                            <div className="text-xl mb-1.5">{p.icon}</div>
                                            <div className="text-[10px] font-black uppercase tracking-wider">{p.name}</div>
                                        </button>
                                    ))}
                                </div>

                                {/* Model Select */}
                                <div className="relative group">
                                    <select 
                                        value={model}
                                        onChange={e => setModel(e.target.value)}
                                        className="w-full bg-white/5 border border-white/10 rounded-2xl px-6 py-4 text-sm text-white focus:outline-none transition-all focus:border-indigo-500/50 appearance-none bg-[url('data:image/svg+xml;charset=US-ASCII,%3Csvg%20width%3D%2220%22%20height%3D%2220%22%20viewBox%3D%220%200%2020%2020%22%20fill%3D%22none%22%20xmlns%3D%22http%3A//www.w3.org/2000/svg%22%3E%3Cpath%20d%3D%22M5%208L10%2013L15%208%22%20stroke%3D%22%23666%22%20stroke-width%3D%222%22%20stroke-linecap%3D%22round%22%20stroke-linejoin%3D%22round%22/%3E%3C/svg%3E')] bg-[length:18px_18px] bg-[right_1.5rem_center] bg-no-repeat pr-12 focus:bg-white/[0.08]"
                                    >
                                        <optgroup label="Model Selection" className="bg-[#11111b] text-gray-400">
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
                                        </optgroup>
                                    </select>
                                </div>

                                {/* Persona Grid */}
                                <div className="grid grid-cols-2 gap-3">
                                    {PERSONA_PRESETS.map(p => (
                                        <button
                                            key={p.id}
                                            onClick={() => setPersona(p.id)}
                                            className={`p-4 rounded-2xl border text-left transition-all relative group/persona ${
                                                persona === p.id 
                                                ? 'bg-indigo-500/10 border-indigo-500/40 text-indigo-300' 
                                                : 'bg-white/5 border-white/5 text-gray-500 hover:text-gray-300 hover:border-white/10'
                                            }`}
                                        >
                                            <div className="flex items-center gap-3">
                                                <span className="text-xl filter drop-shadow-[0_0_8px_rgba(255,255,255,0.1)]">{p.name.split(' ')[0]}</span>
                                                <span className="text-[11px] font-bold">{p.name.split(' ').slice(1).join(' ')}</span>
                                            </div>

                                            {/* Tooltip */}
                                            <div className="absolute bottom-full left-0 mb-3 w-64 p-4 bg-gray-950 border border-white/10 rounded-2xl shadow-2xl opacity-0 invisible group-hover/persona:opacity-100 group-hover/persona:visible transition-all z-30 pointer-events-none backdrop-blur-xl">
                                                <div className="flex flex-col gap-1.5">
                                                    <span className="text-white font-bold text-xs">{p.name}</span>
                                                    <span className="text-gray-500 text-[10px] leading-relaxed font-medium">{p.description}</span>
                                                </div>
                                                <div className="absolute top-full left-6 border-8 border-transparent border-t-gray-950" />
                                            </div>
                                        </button>
                                    ))}
                                </div>

                                {/* Summary Style (Interactive Strip) */}
                                <div className="bg-black/30 p-2 rounded-[1.5rem] border border-white/5">
                                    <div className="grid grid-cols-4 gap-1.5">
                                        {[
                                            { id: 'bullet_points', name: '지표형', icon: '📌', desc: '핵심 실적과 정량 지표 위주의 간결한 불렛 포인트 요약' },
                                            { id: 'narrative', name: '서사형', icon: '📖', desc: '작업의 맥락과 기술적 결정 과정을 이야기 형태로 기록' },
                                            { id: 'actionable', name: '통찰형', icon: '💡', desc: '데이터를 분석하여 향후 시도할 만한 개선 아이디어와 리스크 도출' },
                                            { id: 'comparative', name: '비교형', icon: '🔄', desc: '과거 대비 변경점과 진척도를 대조하여 시각화' },
                                        ].map(s => (
                                            <div key={s.id} className="relative group/style">
                                                <button
                                                    onClick={() => setSummaryStyle(s.id)}
                                                    className={`w-full py-3 rounded-xl flex flex-col items-center gap-1 transition-all ${
                                                        summaryStyle === s.id 
                                                        ? 'bg-indigo-600 text-white shadow-lg' 
                                                        : 'text-gray-500 hover:text-gray-300 hover:bg-white/5'
                                                    }`}
                                                >
                                                    <span className="text-sm">{s.icon}</span>
                                                    <span className="text-[9px] font-black uppercase tracking-tighter">{s.name}</span>
                                                </button>
                                                
                                                <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-3 w-56 p-4 bg-gray-950 border border-white/10 rounded-2xl shadow-2xl opacity-0 invisible group-hover/style:opacity-100 group-hover/style:visible transition-all z-30 pointer-events-none backdrop-blur-xl">
                                                    <div className="flex flex-col gap-1.5">
                                                        <span className="text-white font-bold text-xs">{s.icon} {s.name}</span>
                                                        <span className="text-gray-500 text-[10px] leading-relaxed font-medium">{s.desc}</span>
                                                    </div>
                                                    <div className="absolute top-full left-1/2 -translate-x-1/2 border-8 border-transparent border-t-gray-950" />
                                                </div>
                                            </div>
                                        ))}
                                    </div>
                                </div>
                            </div>
                        )}
                    </div>

                    {/* Section 3: Scope & Storage (Conditional for Agent) */}
                    {executor === 'agent' && (
                        <div className="animate-in fade-in slide-in-from-bottom-4 duration-500 space-y-5">
                            <div className="flex items-center gap-2 px-1">
                                <div className="w-1.5 h-1.5 rounded-full bg-indigo-500 shadow-[0_0_8px_rgba(79,70,229,0.8)]" />
                                <h3 className="text-[11px] font-black text-gray-400 uppercase tracking-widest">분석 대상 및 저장 설정</h3>
                            </div>

                            <div className="bg-white/[0.03] border border-white/5 rounded-[2rem] p-7 space-y-6">
                                <div>
                                    <label className="block text-[10px] font-black text-gray-500 mb-3 uppercase tracking-[0.2em] pl-1">대상 프로젝트 (최대 3개)</label>
                                    <div className="flex flex-wrap gap-2.5">
                                        {projects.map(p => (
                                            <button
                                                key={p.name}
                                                onClick={() => toggleProject(p.name)}
                                                className={`px-4 py-2.5 rounded-full text-[11px] font-bold transition-all border ${
                                                    selectedProjects.includes(p.name)
                                                    ? 'bg-indigo-600 border-indigo-500 text-white shadow-[0_0_15px_rgba(79,70,229,0.2)]'
                                                    : 'bg-black/40 border-white/5 text-gray-500 hover:text-gray-300 hover:border-white/10'
                                                }`}
                                            >
                                                {p.display_name}
                                            </button>
                                        ))}
                                    </div>
                                </div>

                                <div className="grid grid-cols-2 gap-4">
                                    <div className="space-y-2">
                                        <label className="block text-[10px] font-black text-gray-600 uppercase tracking-widest pl-1">태그 필터</label>
                                        <input 
                                            value={tags}
                                            onChange={e => setTags(e.target.value)}
                                            placeholder="예: devlog, api"
                                            className="w-full bg-black/40 border border-white/5 rounded-2xl px-5 py-3.5 text-xs text-white focus:outline-none focus:border-indigo-500/40 transition-colors" 
                                        />
                                    </div>
                                    <div className="space-y-2">
                                        <label className="block text-[10px] font-black text-gray-600 uppercase tracking-widest pl-1">검색 키워드</label>
                                        <input 
                                            value={keywords}
                                            onChange={e => setKeywords(e.target.value)}
                                            placeholder="핵심 단어 입력.."
                                            className="w-full bg-black/40 border border-white/5 rounded-2xl px-5 py-3.5 text-xs text-white focus:outline-none focus:border-indigo-500/40 transition-colors" 
                                        />
                                    </div>
                                </div>

                                <div className="pt-6 border-t border-white/5">
                                    <div className="flex items-center gap-3 mb-4">
                                        <div className="w-8 h-8 rounded-lg bg-indigo-500/10 border border-indigo-500/20 flex items-center justify-center text-indigo-400">📁</div>
                                        <h4 className="text-[11px] font-black text-indigo-400 uppercase tracking-widest">산출물 저장 위치</h4>
                                    </div>
                                    
                                    <div className="grid grid-cols-2 gap-4 mb-4">
                                        <div className="space-y-2">
                                            <label className="block text-[10px] font-black text-gray-600 uppercase tracking-widest pl-1">저장 프로젝트</label>
                                            <select 
                                                value={outputProject}
                                                onChange={e => setOutputProject(e.target.value)}
                                                className="w-full bg-black/40 border border-white/5 rounded-2xl px-5 py-3.5 text-xs text-white focus:outline-none appearance-none cursor-pointer"
                                            >
                                                {projects.map(p => (
                                                    <option key={p.name} value={p.name} className="bg-[#11111b]">{p.display_name}</option>
                                                ))}
                                            </select>
                                        </div>
                                        <div className="space-y-2">
                                            <label className="block text-[10px] font-black text-gray-600 uppercase tracking-widest pl-1">하위 폴더</label>
                                            <input 
                                                value={outputDir}
                                                onChange={e => setOutputDir(e.target.value)}
                                                placeholder="reports/weekly"
                                                className="w-full bg-black/40 border border-white/5 rounded-2xl px-5 py-3.5 text-xs text-white focus:outline-none focus:border-indigo-500/40" 
                                            />
                                        </div>
                                    </div>
                                    
                                    <div className="flex items-start gap-3 bg-indigo-500/5 p-4 rounded-2xl border border-indigo-500/10">
                                        <span className="text-base">📍</span>
                                        <div className="flex flex-col gap-1 min-w-0">
                                            <span className="text-[9px] font-black text-indigo-400/60 uppercase tracking-widest leading-none">최종 파일 예시 경로</span>
                                            <code className="text-[10px] text-indigo-300 font-mono truncate">
                                                {outputProject}/{outputDir}/{new Date().toISOString().split('T')[0]}-{name.replace(/\s+/g, '_') || 'Report'}.md
                                            </code>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>
                    )}

                    {/* Section 4: Schedule Settings */}
                    <div className="space-y-5">
                        <div className="flex items-center gap-2 px-1">
                            <div className="w-1.5 h-1.5 rounded-full bg-indigo-500 shadow-[0_0_8px_rgba(79,70,229,0.8)]" />
                            <h3 className="text-[11px] font-black text-gray-400 uppercase tracking-widest">반복 주기 및 스케줄</h3>
                        </div>

                        <div className="space-y-4">
                            <div className="flex p-1.5 bg-black/30 rounded-2xl border border-white/5">
                                {[
                                    { id: 'daily', label: '매일 한 번' },
                                    { id: 'weekly', label: '매주 지정' },
                                    { id: 'interval', label: '시간 간격' }
                                ].map(t => (
                                    <button
                                        key={t.id}
                                        onClick={() => setScheduleType(t.id as any)}
                                        className={`flex-1 py-3 rounded-xl text-xs font-bold transition-all ${
                                            scheduleType === t.id 
                                            ? 'bg-white/10 text-white shadow-sm ring-1 ring-white/10' 
                                            : 'text-gray-500 hover:text-gray-400'
                                        }`}
                                    >
                                        {t.label}
                                    </button>
                                ))}
                            </div>

                            <div className="p-7 bg-white/[0.02] rounded-[2rem] border border-white/5">
                                {scheduleType === 'daily' && (
                                    <div className="flex items-center justify-between">
                                        <div className="flex flex-col">
                                            <span className="text-[11px] font-bold text-gray-300">작업 실행 시각</span>
                                            <span className="text-[9px] text-gray-500">24시간 단위 (예: 23:00)</span>
                                        </div>
                                        <div className="flex items-center gap-3">
                                            <input 
                                                type="number" value={hour}
                                                onChange={e => setHour(Math.min(23, Math.max(0, parseInt(e.target.value) || 0)))}
                                                className="w-16 bg-black/40 border border-white/10 rounded-xl px-2 py-3 text-center text-sm text-white font-mono focus:border-indigo-500/50 outline-none" 
                                            />
                                            <span className="text-gray-700 font-bold">:</span>
                                            <input 
                                                type="number" value={minute}
                                                onChange={e => setMinute(Math.min(59, Math.max(0, parseInt(e.target.value) || 0)))}
                                                className="w-16 bg-black/40 border border-white/10 rounded-xl px-2 py-3 text-center text-sm text-white font-mono focus:border-indigo-500/50 outline-none" 
                                            />
                                        </div>
                                    </div>
                                )}
                                {scheduleType === 'weekly' && (
                                    <div className="space-y-6">
                                        <div className="flex items-center justify-between">
                                            <span className="text-xs font-bold text-gray-300">실행 요일</span>
                                            <div className="flex gap-1.5">
                                                {["일", "월", "화", "수", "목", "금", "토"].map((day, d) => (
                                                    <button
                                                        key={d}
                                                        onClick={() => setDayOfWeek(d)}
                                                        className={`w-10 h-10 rounded-xl text-[11px] font-bold transition-all ${
                                                            dayOfWeek === d 
                                                            ? 'bg-indigo-600 text-white shadow-lg shadow-indigo-500/20' 
                                                            : 'bg-black/40 text-gray-500 hover:text-gray-300'
                                                        }`}
                                                    >
                                                        {day}
                                                    </button>
                                                ))}
                                            </div>
                                        </div>
                                        <div className="flex items-center justify-between pt-6 border-t border-white/5">
                                            <span className="text-xs font-bold text-gray-300">실행 시각</span>
                                            <div className="flex items-center gap-3">
                                                <input type="number" value={hour} onChange={e => setHour(Math.min(23, Math.max(0, parseInt(e.target.value) || 0)))} className="w-14 bg-black/40 border border-white/10 rounded-xl py-3 text-center text-sm text-white font-mono outline-none" />
                                                <span className="text-gray-700 font-bold">:</span>
                                                <input type="number" value={minute} onChange={e => setMinute(Math.min(59, Math.max(0, parseInt(e.target.value) || 0)))} className="w-14 bg-black/40 border border-white/10 rounded-xl py-3 text-center text-sm text-white font-mono outline-none" />
                                            </div>
                                        </div>
                                    </div>
                                )}
                                {scheduleType === 'interval' && (
                                    <div className="flex items-center justify-between">
                                        <div className="flex flex-col">
                                            <span className="text-[11px] font-bold text-gray-300">반복 실행 간격</span>
                                            <span className="text-[9px] text-gray-500">분 단위로 설정 (최소 1분)</span>
                                        </div>
                                        <div className="flex items-center gap-3">
                                            <input 
                                                type="number"
                                                value={intervalSeconds / 60}
                                                onChange={e => setIntervalSeconds(Math.max(1, parseInt(e.target.value) || 1) * 60)}
                                                className="w-28 bg-black/40 border border-white/10 rounded-xl px-4 py-3 text-center text-sm text-white font-mono focus:border-indigo-500/50 outline-none" 
                                            />
                                            <span className="text-gray-500 text-[10px] font-black uppercase tracking-widest">Mins</span>
                                        </div>
                                    </div>
                                )}
                            </div>

                            <button 
                                onClick={() => setRunOnIdle(!runOnIdle)}
                                className={`w-full p-6 rounded-[2rem] border flex items-center justify-between transition-all group ${
                                    runOnIdle 
                                    ? 'bg-indigo-500/10 border-indigo-500/50 ring-1 ring-indigo-500/10' 
                                    : 'bg-white/[0.02] border-white/5 hover:border-white/10'
                                }`}
                            >
                                <div className="flex flex-col text-left">
                                    <span className={`text-[11px] font-black uppercase tracking-widest ${runOnIdle ? 'text-indigo-400' : 'text-gray-400 opacity-60'}`}>유휴 상태 시 자동 실행</span>
                                    <span className="text-[9px] text-gray-500 mt-1 leading-relaxed">자리를 비웠을 때만 작업을 백그라운드에서 진행하여 업무를 방해하지 않습니다.</span>
                                </div>
                                <div className={`w-14 h-7 rounded-full relative transition-all duration-500 ${runOnIdle ? 'bg-indigo-600 shadow-[0_0_15px_rgba(79,70,229,0.4)]' : 'bg-gray-800'}`}>
                                    <div className={`absolute top-1 w-5 h-5 rounded-full bg-white transition-all duration-500 ease-spring ${runOnIdle ? 'left-8 shadow-[0_0_10px_white]' : 'left-1'}`} />
                                </div>
                            </button>
                        </div>
                    </div>

                    {/* Section 5: Custom Prompt / Master Instruction (Sticky footer style extension) */}
                    <div className="animate-in fade-in slide-in-from-bottom-4 duration-1000 space-y-5">
                        <div className="flex items-center gap-2 px-1">
                            <div className="w-1.5 h-1.5 rounded-full bg-indigo-500 shadow-[0_0_8px_rgba(79,70,229,0.8)]" />
                            <h3 className="text-[11px] font-black text-gray-400 uppercase tracking-widest">
                                {executor === 'agent' ? (persona === 'custom' ? '✨ 마스터 에이전트 지시' : '➕ 추가 강조 지시사항') : '🔔 알림 및 특이사항 (선택)'}
                            </h3>
                        </div>
                        <textarea 
                            value={customPrompt}
                            onChange={e => setCustomPrompt(e.target.value)}
                            placeholder={executor === 'agent' 
                                ? (persona === 'custom' 
                                    ? "에이전트가 완벽히 수행해야 할 명령을 구체적으로 입력하세요." 
                                    : "기본 작업 외에 특별히 분석에 포함하거나 강조해줬으면 하는 내용을 입력하세요.")
                                : "자동화 실행 시 참고할 만한 내용을 입력하세요."}
                            className="w-full bg-white/[0.03] border border-white/10 border-dashed rounded-[2.5rem] px-8 py-7 text-sm text-white focus:outline-none focus:ring-2 focus:ring-indigo-500/30 transition-all placeholder:text-gray-600 resize-none h-44 hover:border-indigo-500/40" 
                        />
                    </div>
                </div>

                {/* Footer Footer Footer */}
                <div className="p-8 bg-black/60 border-t border-white/5 backdrop-blur-xl">
                    <div className="flex gap-4">
                        <button 
                            onClick={onClose}
                            className="flex-1 py-5 bg-white/5 hover:bg-white/10 text-gray-400 hover:text-white rounded-[1.5rem] font-bold text-sm transition-all border border-white/5 active:scale-[0.98]"
                        >
                            취소
                        </button>
                        <button 
                            disabled={loading}
                            onClick={handleSubmit}
                            className="flex-[2] group relative flex items-center justify-center gap-3 py-5 bg-indigo-600 hover:bg-indigo-500 disabled:bg-gray-800 text-white rounded-[1.5rem] font-black text-sm transition-all shadow-[0_0_40px_-10px_rgba(79,70,229,0.6)] active:scale-[0.98] overflow-hidden"
                        >
                            {loading ? (
                                <div className="w-5 h-5 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                            ) : (
                                <>
                                    <span className="absolute inset-0 bg-gradient-to-r from-white/0 via-white/10 to-white/0 -translate-x-full group-hover:translate-x-full transition-transform duration-1000" />
                                    <span className="text-xl leading-none">{isEdit ? '💾' : '🚀'}</span>
                                    {isEdit ? '설정 완료 및 저장' : '공정 자동화 스케줄 등록'}
                                </>
                            )}
                        </button>
                    </div>
                    <p className="text-[9px] text-gray-600 text-center mt-7 uppercase tracking-[0.4em] font-black opacity-40">
                        Doxus Scheduling Engine Platform v1.1.2
                    </p>
                </div>
            </div>
        </div>
    );
}
