import { useState, useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

interface AppSettings {
  embedding_model: 'onnx' | 'ollama';
  language: 'ko' | 'en';
  theme: 'light' | 'dark' | 'system';
}

interface SystemStatus {
  app: { version: string; status: string };
  database: { path: string; exists: boolean; status: string };
  mcp: { status: string; note: string };
  cli: { status: string; path: string };
  agent: { status: string; note: string };
}

interface EmbeddingStatus {
  model: string;
  model_loaded: boolean;
  dimension: number;
  total_documents: number;
  embedded_chunks: number;
  status: string;
  path?: string;
}

interface SyncTriggerSummary {
  trigger_type: string;
  project_name?: string;
  details?: string;
  timestamp: number;
}

interface SyncStatus {
  active_tasks: ActiveTaskSummary[];
  recent_triggers: SyncTriggerSummary[];
}

interface ActiveTaskSummary {
  project_name: string;
  started_at: number;
}

type StatusLevel = 'ok' | 'warn' | 'error' | 'unknown';

function statusLevel(status: string): StatusLevel {
  const s = status.toLowerCase();
  if (['ok', 'running', 'connected', 'installed', 'active', 'ready'].includes(s)) return 'ok';
  if (['warn', 'not started', 'unknown'].includes(s)) return 'warn';
  if (['error', 'not found', 'not installed', 'inactive'].includes(s)) return 'error';
  return 'unknown';
}

function StatusBadge({ status }: { status: string }) {
  const level = statusLevel(status);
  const cls = {
    ok: 'bg-emerald-950 text-emerald-400 border-emerald-800',
    warn: 'bg-yellow-950 text-yellow-400 border-yellow-800',
    error: 'bg-red-950 text-red-400 border-red-800',
    unknown: 'bg-gray-800 text-gray-400 border-gray-700',
  }[level];

  const icon = { ok: '●', warn: '◐', error: '○', unknown: '?' }[level];
  const label = { ok: '정상', warn: '경고', error: '오류', unknown: '미확인' }[level];

  return (
    <span className={`inline-flex items-center gap-1.5 text-xs px-2.5 py-1 rounded-full font-medium border ${cls}`}>
      <span>{icon}</span>
      {label}
    </span>
  );
}

interface StatusCardProps {
  title: string;
  status: string;
  detail?: string;
  note?: string;
  onTest?: () => void;
  testLabel?: string;
  testLoading?: boolean;
  testResult?: string | null;
}

function StatusCard({
  title, status, detail, note, onTest, testLabel, testLoading, testResult,
}: StatusCardProps) {
  return (
    <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-3">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-white font-semibold">{title}</h3>
        <StatusBadge status={status} />
      </div>
      {detail && (
        <p className="text-xs text-gray-500 font-mono break-all">{detail}</p>
      )}
      {note && (
        <p className="text-xs text-gray-600">{note}</p>
      )}
      {onTest && (
        <div className="flex items-center gap-3 pt-1 w-full overflow-hidden">
          <button
            onClick={onTest}
            disabled={testLoading}
            className="shrink-0 px-3 py-1.5 text-xs bg-gray-800 hover:bg-gray-700 text-gray-300 rounded-lg border border-gray-700 transition-colors disabled:opacity-50 whitespace-nowrap"
          >
            {testLoading ? '테스트 중...' : (testLabel ?? '연결 테스트')}
          </button>
          {testResult && (
            <span className="text-xs text-gray-400 truncate" title={testResult}>{testResult}</span>
          )}
        </div>
      )}
    </div>
  );
}

export default function SettingsPage() {
  const [activeTab, setActiveTab] = useState<'general' | 'diagnostics'>('general');
  const [sysStatus, setSysStatus] = useState<SystemStatus | null>(null);
  const [embeddingStatus, setEmbeddingStatus] = useState<EmbeddingStatus | null>(null);
  const [syncStatus, setSyncStatus] = useState<SyncStatus | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  // App settings state
  const [appSettings, setAppSettings] = useState<AppSettings>({
    embedding_model: 'onnx',
    language: 'ko',
    theme: 'system',
  });
  const [settingsSaving, setSettingsSaving] = useState(false);
  const [settingsResult, setSettingsResult] = useState<string | null>(null);
  const [modelExists, setModelExists] = useState<boolean | null>(null);
  const [dbTestResult, setDbTestResult] = useState<string | null>(null);
  const [dbTestLoading, setDbTestLoading] = useState(false);
  const [mcpTestResult, setMcpTestResult] = useState<string | null>(null);
  const [mcpTestLoading, setMcpTestLoading] = useState(false);
  const [claudeStatus, setClaudeStatus] = useState<'ok' | 'warn' | 'unknown'>('unknown');
  const [claudeTestResult, setClaudeTestResult] = useState<string | null>(null);
  const [claudeTestLoading, setClaudeTestLoading] = useState(false);
  const [geminiStatus, setGeminiStatus] = useState<'ok' | 'warn' | 'unknown'>('unknown');
  const [geminiTestResult, setGeminiTestResult] = useState<string | null>(null);
  const [geminiTestLoading, setGeminiTestLoading] = useState(false);

  const fetchStatus = async () => {
    setIsLoading(true);
    try {
      const [s, e, sync] = await Promise.all([
        invoke<SystemStatus>('get_system_status'),
        invoke<EmbeddingStatus>('get_embedding_status'),
        invoke<SyncStatus>('get_sync_status'),
      ]);
      setSysStatus(s);
      setEmbeddingStatus(e);
      setSyncStatus(sync);
    } catch (err) {
      console.error('Status fetch failed', err);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    invoke<AppSettings>('load_settings')
      .then(setAppSettings)
      .catch(() => { /* use defaults */ });

    invoke<{ exists: boolean; path: string | null }>('check_model_status')
      .then((s) => setModelExists(s.exists))
      .catch(() => setModelExists(false));

    fetchStatus();

    // Auto-detect Claude / Gemini on mount
    invoke<{ status: string; message: string }>('check_claude_status')
      .then((res) => setClaudeStatus(res.status as 'ok' | 'warn' | 'unknown'))
      .catch(() => setClaudeStatus('warn'));

    invoke<{ status: string; message: string }>('check_gemini_status')
      .then((res) => setGeminiStatus(res.status as 'ok' | 'warn' | 'unknown'))
      .catch(() => setGeminiStatus('warn'));
  }, []);

  const handleSaveSettings = async () => {
    setSettingsSaving(true);
    setSettingsResult(null);
    try {
      await invoke('save_settings', { settings: appSettings });
      setSettingsResult('✓ 저장됨');
    } catch (e) {
      setSettingsResult(`✗ ${String(e)}`);
    } finally {
      setSettingsSaving(false);
    }
  };

  const handleRefresh = async () => {
    await fetchStatus();
  };

  const handleDbTest = async () => {
    setDbTestLoading(true);
    setDbTestResult(null);
    try {
      await invoke('list_projects');
      setDbTestResult('✓ 연결 성공');
    } catch (e) {
      setDbTestResult(`✗ ${String(e)}`);
    } finally {
      setDbTestLoading(false);
    }
  };

  const handleMcpTest = async () => {
    setMcpTestLoading(true);
    setMcpTestResult(null);
    try {
      const res = await invoke<SystemStatus>('get_system_status');
      setMcpTestResult(res.mcp.status === 'running' ? '✓ 연결됨' : '✗ 실행되지 않음');
    } catch (e) {
      setMcpTestResult(`✗ ${String(e)}`);
    } finally {
      setMcpTestLoading(false);
    }
  };

  const handleClaudeTest = async () => {
    setClaudeTestLoading(true);
    setClaudeTestResult(null);
    try {
      const res = await invoke<{ status: string; message: string }>('check_claude_status');
      setClaudeStatus(res.status as 'ok' | 'warn' | 'unknown');
      setClaudeTestResult(res.message);
    } catch (e) {
      setClaudeTestResult(`✗ ${String(e)}`);
    } finally {
      setClaudeTestLoading(false);
    }
  };

  const handleGeminiTest = async () => {
    setGeminiTestLoading(true);
    setGeminiTestResult(null);
    try {
      const res = await invoke<{ status: string; message: string }>('check_gemini_status');
      setGeminiStatus(res.status as 'ok' | 'warn' | 'unknown');
      setGeminiTestResult(res.message);
    } catch (e) {
      setGeminiTestResult(`✗ ${String(e)}`);
    } finally {
      setGeminiTestLoading(false);
    }
  };

  return (
    <div className="flex flex-col gap-6 max-w-3xl">
      {/* 헤더 */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-white tracking-tight">설정</h1>
          <p className="text-sm text-gray-400 mt-0.5">앱 환경 설정 및 시스템 진단 도구</p>
        </div>
        <button
          onClick={handleRefresh}
          disabled={isLoading}
          className="px-3 py-1.5 text-sm border border-gray-700 text-gray-400 rounded-lg hover:bg-gray-800 hover:text-gray-200 disabled:opacity-50 transition-colors"
        >
          {isLoading ? '새로고침 중...' : '상태 새로고침'}
        </button>
      </div>

      {/* 탭 네비게이션 */}
      <div className="flex items-center border-b border-gray-800">
        <button
          onClick={() => setActiveTab('general')}
          className={`px-6 py-3 text-sm font-medium transition-colors border-b-2 ${
            activeTab === 'general' ? 'border-primary text-white' : 'border-transparent text-gray-500 hover:text-gray-300'
          }`}
        >
          일반 설정
        </button>
        <button
          onClick={() => setActiveTab('diagnostics')}
          className={`px-6 py-3 text-sm font-medium transition-colors border-b-2 ${
            activeTab === 'diagnostics' ? 'border-primary text-white' : 'border-transparent text-gray-500 hover:text-gray-300'
          }`}
        >
          진단 및 디버깅
        </button>
      </div>

      {activeTab === 'general' ? (
        <div className="flex flex-col gap-8 animate-in fade-in duration-300">
          {/* 앱 설정 */}
          <section className="flex flex-col gap-4">
            <h2 className="text-sm font-semibold text-gray-400 tracking-tight">기본 환경 설정</h2>
            <div className="bg-gray-900/50 border border-gray-800/80 rounded-xl p-5 flex flex-col gap-5">
              <div className="flex items-center justify-between gap-4">
                <div className="flex flex-col gap-0.5">
                  <label className="text-sm font-medium text-gray-300">임베딩 모델</label>
                  <p className="text-xs text-gray-500">지식 인덱싱 시 사용할 모델 엔진을 선택합니다.</p>
                </div>
                <select
                  value={appSettings.embedding_model}
                  onChange={(e) => setAppSettings({ ...appSettings, embedding_model: e.target.value as AppSettings['embedding_model'] })}
                  className="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded-lg px-3 py-1.5 focus:outline-none focus:border-gray-500 min-w-[140px]"
                >
                  <option value="onnx">ONNX (내장형)</option>
                  <option value="ollama">Ollama (외부 서버)</option>
                </select>
              </div>
              <div className="flex items-center justify-between gap-4 pt-2 border-t border-gray-800/50">
                <div className="flex flex-col gap-0.5">
                  <label className="text-sm font-medium text-gray-300">언어</label>
                  <p className="text-xs text-gray-500">인터페이스의 기본 언어를 설정합니다.</p>
                </div>
                <select
                  value={appSettings.language}
                  onChange={(e) => setAppSettings({ ...appSettings, language: e.target.value as AppSettings['language'] })}
                  className="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded-lg px-3 py-1.5 focus:outline-none focus:border-gray-500 min-w-[140px]"
                >
                  <option value="ko">한국어</option>
                  <option value="en">English</option>
                </select>
              </div>
              <div className="flex items-center justify-between gap-4 pt-2 border-t border-gray-800/50">
                <div className="flex flex-col gap-0.5">
                  <label className="text-sm font-medium text-gray-300">테마</label>
                  <p className="text-xs text-gray-500">앱의 시각적 테마를 변경합니다.</p>
                </div>
                <select
                  value={appSettings.theme}
                  onChange={(e) => setAppSettings({ ...appSettings, theme: e.target.value as AppSettings['theme'] })}
                  className="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded-lg px-3 py-1.5 focus:outline-none focus:border-gray-500 min-w-[140px]"
                >
                  <option value="system">시스템 설정에 따름</option>
                  <option value="light">라이트 (밝은 배경)</option>
                  <option value="dark">다크 (어두운 배경)</option>
                </select>
              </div>

              <div className="flex items-center gap-3 pt-4 border-t border-gray-800/50">
                <button
                  onClick={handleSaveSettings}
                  disabled={settingsSaving}
                  className="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-500 text-white font-medium rounded-lg transition-all shadow-sm active:scale-[0.98] disabled:opacity-50"
                >
                  {settingsSaving ? '저장 중...' : '설정 내용 저장'}
                </button>
                {settingsResult && (
                  <span className={`text-sm font-medium ${settingsResult.startsWith('✓') ? 'text-emerald-400' : 'text-red-400'}`}>
                    {settingsResult}
                  </span>
                )}
              </div>
            </div>
          </section>

          {/* 임베딩 모델 파일 */}
          <section className="flex flex-col gap-4">
            <h2 className="text-sm font-semibold text-gray-400 tracking-tight">임베딩 모델 파일</h2>
            <div className="bg-gray-900/50 border border-gray-800/80 rounded-xl p-5 flex items-center justify-between gap-4">
              <div className="flex flex-col gap-1">
                <span className="text-sm font-medium text-gray-300">multilingual-e5-small</span>
                <span className="text-xs text-gray-500">
                  의미 기반 벡터 검색용 ONNX 모델 (~110MB)
                  <code className="ml-1 text-gray-600">~/.doxus/models/</code>
                </span>
              </div>
              {modelExists === null ? (
                <span className="text-xs text-gray-500">확인 중...</span>
              ) : modelExists ? (
                <span className="inline-flex items-center gap-1.5 text-xs px-2.5 py-1 rounded-full font-medium border bg-emerald-950 text-emerald-400 border-emerald-800">
                  ● 설치됨
                </span>
              ) : (
                <button
                  onClick={() => window.dispatchEvent(new CustomEvent("doxus:open-model-download"))}
                  className="px-3 py-1.5 rounded-lg bg-indigo-500 hover:bg-indigo-400 text-white text-xs font-medium transition"
                >
                  다운로드
                </button>
              )}
            </div>
          </section>

          {/* 앱 정보 */}
          <section className="flex flex-col gap-4">
            <h2 className="text-sm font-semibold text-gray-400 tracking-tight">앱 정보</h2>
            <div className="bg-gray-900/50 border border-gray-800 rounded-xl p-5 flex flex-col gap-3">
              <InfoRow label="애플리케이션 명칭" value="Doxus Desktop" />
              <InfoRow label="현재 버전" value={sysStatus?.app.version ?? '0.1.0'} />
              <InfoRow label="구동 환경" value="macOS (Tauri v2 + Rust)" />
              <InfoRow label="데이터베이스(SQLite) 위치" value={sysStatus?.database.path ?? '—'} mono />
            </div>
          </section>
        </div>
      ) : (
        <div className="flex flex-col gap-8 animate-in slide-in-from-right-4 fade-in duration-300">
          {/* 동기화 및 큐 상태 */}
          <section className="flex flex-col gap-4">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-semibold text-gray-400 tracking-tight">동기화 및 와쳐 상태</h2>
              {syncStatus?.active_tasks && syncStatus.active_tasks.length > 0 && (
                <span className="flex items-center gap-1.5 text-xs text-blue-400 font-medium">
                  <span className="relative flex h-2 w-2">
                    <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-blue-400 opacity-75"></span>
                    <span className="relative inline-flex rounded-full h-2 w-2 bg-blue-500"></span>
                  </span>
                  진행 중인 작업: {syncStatus.active_tasks.length}개
                </span>
              )}
            </div>
            <div className="grid grid-cols-1 gap-3">
              <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-4">
                <div className="flex flex-col gap-1.5">
                  <h3 className="text-sm font-semibold text-white">처리 중인 프로젝트</h3>
                  {syncStatus?.active_tasks && syncStatus.active_tasks.length > 0 ? (
                    <div className="flex flex-col gap-2 pt-1">
                      {syncStatus.active_tasks.map(task => {
                        const elapsed = Math.max(0, Math.floor(Date.now() / 1000) - task.started_at);
                        return (
                          <div key={task.project_name} className="flex items-center justify-between bg-blue-950/20 border border-blue-900/30 p-2 rounded-lg">
                            <span className="text-xs font-mono text-blue-300">{task.project_name}</span>
                            <span className="text-[10px] text-blue-500 font-medium whitespace-nowrap">
                              {elapsed > 60 ? `${Math.floor(elapsed / 60)}분 ${elapsed % 60}초` : `${elapsed}초`} 전 시작
                            </span>
                          </div>
                        );
                      })}
                    </div>
                  ) : (
                    <p className="text-xs text-gray-500 italic mt-1">현재 활발히 동기화 중인 작업이 없습니다.</p>
                  )}
                </div>
                
                <div className="flex flex-col gap-2 pt-2 border-t border-gray-800/50">
                  <h3 className="text-sm font-semibold text-white">최근 작업 이력</h3>
                  {syncStatus?.recent_triggers && syncStatus.recent_triggers.length > 0 ? (
                    <div className="flex flex-col gap-1.5 mt-1">
                      {syncStatus.recent_triggers.slice(0, 5).map((tr, i) => (
                        <div key={i} className="flex flex-col gap-1 bg-gray-800/30 px-3 py-2 rounded border border-gray-800/30">
                          <div className="flex items-center justify-between">
                            <div className="flex items-center gap-2">
                              <span className="text-[10px] font-bold text-blue-400 uppercase tracking-tight">[{tr.trigger_type}]</span>
                              <span className="text-xs font-semibold text-gray-300">{tr.project_name || 'Global'}</span>
                            </div>
                            <span className="text-[10px] text-gray-500 font-mono">{new Date(tr.timestamp * 1000).toLocaleTimeString()}</span>
                          </div>
                          {tr.details && (
                            <p className="text-[11px] text-gray-500 leading-relaxed border-t border-gray-800/20 pt-1 mt-0.5 italic">
                              ↳ {tr.details}
                            </p>
                          )}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <p className="text-xs text-gray-600 italic">감지된 기록이 없습니다.</p>
                  )}
                </div>
              </div>
            </div>
          </section>

          {/* 임베딩 세부 상태 */}
          <section className="flex flex-col gap-4">
            <h2 className="text-sm font-semibold text-gray-400 tracking-tight">임베딩 엔진(Embedding) 정보</h2>
            <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-4">
              <div className="flex items-center justify-between gap-3">
                <div className="flex flex-col gap-1">
                  <span className="text-xs text-gray-500">현재 활성 모델</span>
                  <span className="text-sm font-semibold text-white">{embeddingStatus?.model || '미확인'}</span>
                </div>
                <StatusBadge status={embeddingStatus?.status || 'unknown'} />
              </div>

              <div className="grid grid-cols-2 gap-4 py-3 border-y border-gray-800/50">
                <div className="flex flex-col gap-1">
                  <span className="text-[10px] text-gray-500 uppercase font-semibold">벡터 차원</span>
                  <span className="text-sm font-mono text-gray-300">{embeddingStatus?.dimension || 0}d</span>
                </div>
                <div className="flex flex-col gap-1">
                  <span className="text-[10px] text-gray-500 uppercase font-semibold">색인된 청크</span>
                  <span className="text-sm font-mono text-gray-300">{embeddingStatus?.embedded_chunks || 0} / {embeddingStatus?.total_documents || 0} 청크</span>
                </div>
              </div>

              {embeddingStatus?.path && (
                <div className="flex flex-col gap-1.5">
                  <span className="text-[10px] text-gray-500 uppercase font-semibold tracking-wider">물리적 모델 경로</span>
                  <div className="bg-black/20 p-2 rounded border border-gray-800 font-mono text-[10px] text-gray-400 break-all select-all hover:bg-black/30 transition-colors">
                    {embeddingStatus.path}
                  </div>
                </div>
              )}
            </div>
          </section>

          {/* 시스템 진단 */}
          <section className="flex flex-col gap-4">
            <h2 className="text-sm font-semibold text-gray-400 tracking-tight">시스템 레벨 진단</h2>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <StatusCard
                title="데이터베이스 (SQLite)"
                status={sysStatus?.database.status || 'unknown'}
                detail={sysStatus?.database.path}
                onTest={handleDbTest}
                testLabel="DB 연결 테스트"
                testLoading={dbTestLoading}
                testResult={dbTestResult}
              />
              <StatusCard
                title="MCP 서버 (doxus-mcp)"
                status={sysStatus?.mcp.status || 'unknown'}
                note={sysStatus?.mcp.note}
                onTest={handleMcpTest}
                testLabel="MCP 연결 테스트"
                testLoading={mcpTestLoading}
                testResult={mcpTestResult}
              />
              <StatusCard
                title="Claude (Agent)"
                status={claudeStatus}
                onTest={handleClaudeTest}
                testLabel="연결 테스트"
                testLoading={claudeTestLoading}
                testResult={claudeTestResult}
              />
              <StatusCard
                title="Gemini (Agent)"
                status={geminiStatus}
                onTest={handleGeminiTest}
                testLabel="연결 테스트"
                testLoading={geminiTestLoading}
                testResult={geminiTestResult}
              />
            </div>
          </section>

          {/* 개발 도구 */}
          <DevToolsSection />
        </div>
      )}
    </div>
  );
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <span className="text-xs text-gray-500 shrink-0">{label}</span>
      <span className={`text-sm text-gray-300 truncate ${mono ? 'font-mono text-xs' : ''}`}>
        {value}
      </span>
    </div>
  );
}

interface PluginLogEntry {
  id: number;
  project_id: number | null;
  event_type: string;
  payload: string | null;
  occurred_at: number;
}

const EVENT_TYPE_OPTIONS = ['전체', 'index_start', 'index_complete', 'sync_start', 'sync_complete', 'plugin_error', 'document_fetch_error', 'system_error'];

function PluginLogModal({ initialLogs, onClose }: { initialLogs: PluginLogEntry[]; onClose: () => void }) {
  const [logs, setLogs] = useState<PluginLogEntry[]>(initialLogs);
  const [filter, setFilter] = useState('전체');
  const [clearing, setClearing] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  const fetchLogs = async () => {
    try {
      const res = await invoke<{ logs: PluginLogEntry[] }>('get_plugin_logs');
      setLogs(res.logs);
    } catch (e) {
      console.error('Failed to fetch logs', e);
    }
  };

  // Tauri event push: audit:new 이벤트 수신 시 목록에 prepend
  useEffect(() => {
    const unlisten = listen<PluginLogEntry>('audit:new', event => {
      setLogs(prev => [event.payload, ...prev].slice(0, 100));
    });
    return () => { unlisten.then(f => f()); };
  }, []);

  const filtered = filter === '전체' ? logs : logs.filter(l => l.event_type === filter);

  const handleClear = async () => {
    setClearing(true);
    try {
      await invoke('clear_audit_log');
      setLogs([]);
    } finally {
      setClearing(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60" onClick={onClose}>
      <div
        className="bg-gray-950 border border-gray-800 rounded-xl w-[720px] max-h-[75vh] flex flex-col shadow-2xl"
        onClick={e => e.stopPropagation()}
      >
        {/* 헤더 */}
        <div className="flex items-center justify-between px-5 py-3 border-b border-gray-800">
          <div className="flex items-center gap-3">
            <span className="text-sm font-semibold text-gray-200">플러그인 로그 ({filtered.length}건)</span>
            <button
              onClick={fetchLogs}
              className="p-1 text-gray-500 hover:text-gray-300 transition-colors"
              title="새로고침"
            >
              <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 2v6h-6"/><path d="M3 12a9 9 0 0 1 15-6.7L21 8"/><path d="M3 22v-6h6"/><path d="M21 12a9 9 0 0 1-15 6.7L3 16"/></svg>
            </button>
          </div>
          <div className="flex items-center gap-2">
            <select
              value={filter}
              onChange={e => setFilter(e.target.value)}
              className="text-xs bg-gray-800 border border-gray-700 text-gray-300 rounded px-2 py-1"
            >
              {EVENT_TYPE_OPTIONS.map(t => <option key={t} value={t}>{t}</option>)}
            </select>
            <button
              onClick={handleClear}
              disabled={clearing}
              className="text-xs px-2 py-1 bg-red-950 hover:bg-red-900 text-red-400 border border-red-900 rounded transition-colors disabled:opacity-50"
            >
              초기화
            </button>
            <button onClick={onClose} className="text-gray-500 hover:text-gray-300 text-lg leading-none ml-1">✕</button>
          </div>
        </div>
        {/* 로그 목록 */}
        <div className="overflow-y-auto flex-1 p-4 flex flex-col gap-2">
          {filtered.length === 0 ? (
            <p className="text-xs text-gray-500 text-center py-8">기록된 로그가 없습니다.</p>
          ) : (
            filtered.map(log => {
              const date = new Date(log.occurred_at * 1000).toLocaleString('ko-KR');
              const isError = log.event_type.includes('error') || log.event_type.includes('fail');
              let payloadStr = '';
              if (log.payload) {
                try { payloadStr = JSON.stringify(JSON.parse(log.payload), null, 2); }
                catch { payloadStr = log.payload; }
              }
              return (
                <div key={log.id} className={`rounded-lg border p-3 text-xs font-mono ${isError ? 'border-red-900 bg-red-950/30' : 'border-gray-800 bg-gray-900'}`}>
                  <div className="flex items-center gap-2 mb-1">
                    <span className={`font-semibold ${isError ? 'text-red-400' : 'text-emerald-400'}`}>{log.event_type}</span>
                    {log.project_id != null && <span className="text-gray-600">project#{log.project_id}</span>}
                    <span className="text-gray-600 ml-auto">{date}</span>
                  </div>
                  {payloadStr && <pre className="text-gray-400 whitespace-pre-wrap break-all">{payloadStr}</pre>}
                </div>
              );
            })
          )}
          <div ref={bottomRef} />
        </div>
      </div>
    </div>
  );
}

function DevToolsSection() {
  const [devResult, setDevResult] = useState<string | null>(null);
  const [pluginLogs, setPluginLogs] = useState<PluginLogEntry[] | null>(null);
  const [loadingBtn, setLoadingBtn] = useState<string | null>(null);

  const run = async (key: string, fn: () => Promise<void>) => {
    setLoadingBtn(key);
    setDevResult(null);
    try { await fn(); } finally { setLoadingBtn(null); }
  };

  const btnClass = (key: string) =>
    `px-3 py-1.5 text-xs rounded-lg border transition-colors flex items-center gap-1.5 ${
      loadingBtn === key
        ? 'bg-gray-700 border-gray-600 text-gray-400 cursor-not-allowed'
        : 'bg-gray-800 hover:bg-gray-700 text-gray-300 border-gray-700'
    }`;

  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider">개발 도구</h2>
      <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-3">
        <p className="text-sm text-gray-400">로컬 개발 환경에서 사용 가능한 도구</p>
        <div className="flex flex-wrap gap-2">
          <button
            className={btnClass('reindex-inc')}
            disabled={loadingBtn !== null}
            onClick={() => run('reindex-inc', async () => {
              const res = await invoke<{ indexed: number; message: string }>('trigger_reindex', { full: false });
              setDevResult(res.message);
            })}
          >
            {loadingBtn === 'reindex-inc' && <span className="animate-spin">⟳</span>}
            DB 인덱싱 (증분)
          </button>
          <button
            className={`${btnClass('reindex-full')} border-amber-900/50 hover:border-amber-700/50 text-amber-200/80 hover:bg-amber-950/20`}
            disabled={loadingBtn !== null}
            onClick={() => {
              if (confirm('모든 프로젝트의 모든 문서를 강제로 다시 인덱싱하시겠습니까?\n이 작업은 데이터 양에 따라 시간이 오래 걸릴 수 있습니다.')) {
                run('reindex-full', async () => {
                  const res = await invoke<{ indexed: number; message: string }>('trigger_reindex', { full: true });
                  setDevResult(res.message);
                });
              }
            }}
          >
            {loadingBtn === 'reindex-full' && <span className="animate-spin">⟳</span>}
            전체 강제 재인덱싱
          </button>
          <button
            className={btnClass('status')}
            disabled={loadingBtn !== null}
            onClick={() => run('status', async () => {
              const res = await invoke<{ total_documents: number; total_projects: number }>('search_engine_status');
              setDevResult(`문서 ${res.total_documents}개, 프로젝트 ${res.total_projects}개`);
            })}
          >
            {loadingBtn === 'status' && <span className="animate-spin">⟳</span>}
            검색 엔진 상태
          </button>
          <button
            className={btnClass('logs')}
            disabled={loadingBtn !== null}
            onClick={() => run('logs', async () => {
              const res = await invoke<{ logs: PluginLogEntry[] }>('get_plugin_logs');
              setPluginLogs(res.logs);
            })}
          >
            {loadingBtn === 'logs' && <span className="animate-spin">⟳</span>}
            플러그인 로그
          </button>
          <button
            className={btnClass('embedding')}
            disabled={loadingBtn !== null}
            onClick={() => run('embedding', async () => {
              const res = await invoke<{ model: string; model_loaded: boolean; dimension: number; embedded_chunks: number; total_documents: number; status: string }>('get_embedding_status');
              const statusLabel = res.status === 'active' ? '✓ 활성' : res.status === 'ready' ? '⚡ 준비됨 (재인덱싱 필요)' : '✗ 미활성';
              setDevResult(`임베딩: ${res.model} [${statusLabel}] | 문서 ${res.total_documents}개 중 ${res.embedded_chunks}청크 벡터화`);
            })}
          >
            {loadingBtn === 'embedding' && <span className="animate-spin">⟳</span>}
            임베딩 상태
          </button>
          <button
            className={btnClass('sync')}
            disabled={loadingBtn !== null}
            onClick={() => run('sync', async () => {
              const res = await invoke<{ message: string }>('trigger_sync');
              setDevResult(res.message);
            })}
          >
            {loadingBtn === 'sync' && <span className="animate-spin">⟳</span>}
            동기화 강제 실행
          </button>
          <button
            className={`${btnClass('repair')} border-red-900/50 hover:border-red-700/50 text-red-200/80 hover:bg-red-950/20`}
            disabled={loadingBtn !== null}
            onClick={() => {
              if (confirm('벡터 검색 인덱스 테이블을 재생성하시겠습니까?\n기존의 모든 벡터 데이터가 삭제되며, 전체 재인덱싱을 진행해야 검색이 가능해집니다.')) {
                run('repair', async () => {
                  await invoke('search_engine_repair_index');
                  setDevResult('✓ 벡터 테이블 재생성 완료 (전체 재인덱싱을 실행해 주세요)');
                });
              }
            }}
          >
            {loadingBtn === 'repair' && <span className="animate-spin">⟳</span>}
            벡터 인덱스 복구
          </button>
        </div>
        {devResult && <p className="text-xs text-emerald-400 mt-1">{devResult}</p>}
        <p className="text-xs text-gray-600">
          * 대부분의 개발 도구는 Phase 6 (관측성/디버깅)에서 구현됩니다
        </p>
      </div>
      {pluginLogs != null && (
        <PluginLogModal initialLogs={pluginLogs} onClose={() => setPluginLogs(null)} />
      )}
    </section>
  );
}
