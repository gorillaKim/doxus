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

type StatusLevel = 'ok' | 'warn' | 'error' | 'unknown';

function statusLevel(status: string): StatusLevel {
  if (status === 'ok' || status === 'running' || status === 'connected' || status === 'installed') return 'ok';
  if (status === 'warn' || status === 'not started') return 'warn';
  if (status === 'error' || status === 'not found' || status === 'not installed') return 'error';
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
        <div className="flex items-center gap-3 pt-1">
          <button
            onClick={onTest}
            disabled={testLoading}
            className="px-3 py-1.5 text-xs bg-gray-800 hover:bg-gray-700 text-gray-300 rounded-lg border border-gray-700 transition-colors disabled:opacity-50"
          >
            {testLoading ? '테스트 중...' : (testLabel ?? '연결 테스트')}
          </button>
          {testResult && (
            <span className="text-xs text-gray-400">{testResult}</span>
          )}
        </div>
      )}
    </div>
  );
}

export default function SettingsPage() {
  const [sysStatus, setSysStatus] = useState<SystemStatus | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  // App settings state
  const [appSettings, setAppSettings] = useState<AppSettings>({
    embedding_model: 'onnx',
    language: 'ko',
    theme: 'system',
  });
  const [settingsSaving, setSettingsSaving] = useState(false);
  const [settingsResult, setSettingsResult] = useState<string | null>(null);
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

  useEffect(() => {
    invoke<AppSettings>('load_settings')
      .then(setAppSettings)
      .catch(() => { /* use defaults */ });

    invoke<SystemStatus>('get_system_status')
      .then(setSysStatus)
      .catch(() => {
        setSysStatus({
          app: { version: '0.1.0', status: 'running' },
          database: { path: '~/.doxus/db/doxus.db', exists: false, status: 'unknown' },
          mcp: { status: 'unknown', note: 'MCP 서버는 별도 프로세스로 실행됩니다' },
          cli: { status: 'unknown', path: '' },
          agent: { status: 'not started', note: 'Agent sidecar는 Phase 3에서 구현됩니다' },
        });
      })
      .finally(() => setIsLoading(false));

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
    setIsLoading(true);
    try {
      const s = await invoke<SystemStatus>('get_system_status');
      setSysStatus(s);
    } catch {
      // ignore
    } finally {
      setIsLoading(false);
    }
  };

  const handleDbTest = async () => {
    setDbTestLoading(true);
    setDbTestResult(null);
    try {
      await invoke('get_workspaces');
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
    <div className="flex flex-col gap-8 max-w-2xl">
      {/* 헤더 */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-white tracking-tight">설정</h1>
          <p className="text-sm text-gray-400 mt-0.5">앱 상태 확인 및 구성 요소 테스트</p>
        </div>
        <button
          onClick={handleRefresh}
          disabled={isLoading}
          className="px-3 py-1.5 text-sm border border-gray-700 text-gray-400 rounded-lg hover:bg-gray-800 hover:text-gray-200 disabled:opacity-50 transition-colors"
        >
          {isLoading ? '새로고침 중...' : '새로고침'}
        </button>
      </div>

      {/* 시스템 상태 */}
      <section className="flex flex-col gap-3">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider">시스템 상태</h2>
        {isLoading ? (
          <div className="flex items-center justify-center h-32">
            <p className="text-gray-500 text-sm">상태 확인 중...</p>
          </div>
        ) : sysStatus ? (
          <div className="flex flex-col gap-3">
            <StatusCard
              title="앱 (doxus desktop)"
              status={sysStatus.app.status}
              detail={`버전 ${sysStatus.app.version}`}
            />
            <StatusCard
              title="데이터베이스 (SQLite)"
              status={sysStatus.database.status}
              detail={sysStatus.database.path}
              onTest={handleDbTest}
              testLabel="DB 연결 테스트"
              testLoading={dbTestLoading}
              testResult={dbTestResult}
            />
            <StatusCard
              title="MCP 서버 (doxus-mcp)"
              status={sysStatus.mcp.status}
              note={sysStatus.mcp.note}
              onTest={handleMcpTest}
              testLabel="MCP 연결 테스트"
              testLoading={mcpTestLoading}
              testResult={mcpTestResult}
            />
            <StatusCard
              title="CLI (doxus-cli)"
              status={sysStatus.cli.status}
              detail={sysStatus.cli.path || undefined}
              note={sysStatus.cli.status === 'not installed'
                ? 'cargo install doxus-cli 로 설치하세요'
                : undefined}
            />
            <StatusCard
              title="에이전트 사이드카"
              status={sysStatus.agent.status}
              note={sysStatus.agent.note}
            />
            <StatusCard
              title="Claude (AI 에이전트)"
              status={claudeStatus}
              note="Claude Code CLI 또는 ANTHROPIC_API_KEY 필요"
              onTest={handleClaudeTest}
              testLabel="연결 테스트"
              testLoading={claudeTestLoading}
              testResult={claudeTestResult}
            />
            <StatusCard
              title="Gemini (AI 에이전트)"
              status={geminiStatus}
              note="Gemini CLI 또는 GEMINI_API_KEY 필요"
              onTest={handleGeminiTest}
              testLabel="연결 테스트"
              testLoading={geminiTestLoading}
              testResult={geminiTestResult}
            />
          </div>
        ) : null}
      </section>

      {/* 앱 설정 */}
      <section className="flex flex-col gap-3">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider">앱 설정</h2>
        <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-4">
          <div className="flex items-center justify-between gap-4">
            <label className="text-sm text-gray-400 shrink-0">임베딩 모델</label>
            <select
              value={appSettings.embedding_model}
              onChange={(e) => setAppSettings({ ...appSettings, embedding_model: e.target.value as AppSettings['embedding_model'] })}
              className="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded-lg px-3 py-1.5 focus:outline-none focus:border-gray-500"
            >
              <option value="onnx">ONNX (내장, 기본값)</option>
              <option value="ollama">Ollama (외부 서버)</option>
            </select>
          </div>
          <div className="flex items-center justify-between gap-4">
            <label className="text-sm text-gray-400 shrink-0">언어</label>
            <select
              value={appSettings.language}
              onChange={(e) => setAppSettings({ ...appSettings, language: e.target.value as AppSettings['language'] })}
              className="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded-lg px-3 py-1.5 focus:outline-none focus:border-gray-500"
            >
              <option value="ko">한국어</option>
              <option value="en">English</option>
            </select>
          </div>
          <div className="flex items-center justify-between gap-4">
            <label className="text-sm text-gray-400 shrink-0">테마</label>
            <select
              value={appSettings.theme}
              onChange={(e) => setAppSettings({ ...appSettings, theme: e.target.value as AppSettings['theme'] })}
              className="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded-lg px-3 py-1.5 focus:outline-none focus:border-gray-500"
            >
              <option value="system">시스템</option>
              <option value="light">라이트</option>
              <option value="dark">다크</option>
            </select>
          </div>
          <div className="flex items-center gap-3 pt-1">
            <button
              onClick={handleSaveSettings}
              disabled={settingsSaving}
              className="px-4 py-1.5 text-sm bg-blue-600 hover:bg-blue-500 text-white rounded-lg transition-colors disabled:opacity-50"
            >
              {settingsSaving ? '저장 중...' : '설정 저장'}
            </button>
            {settingsResult && (
              <span className="text-xs text-gray-400">{settingsResult}</span>
            )}
          </div>
        </div>
      </section>

      {/* 앱 정보 */}
      <section className="flex flex-col gap-3">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider">앱 정보</h2>
        <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 flex flex-col gap-2">
          <InfoRow label="앱 이름" value="doxus" />
          <InfoRow label="버전" value={sysStatus?.app.version ?? '—'} />
          <InfoRow label="플랫폼" value="macOS (Tauri v2)" />
          <InfoRow label="DB 경로" value={sysStatus?.database.path ?? '—'} mono />
        </div>
      </section>

      {/* 개발 도구 */}
      <DevToolsSection />
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

const EVENT_TYPE_OPTIONS = ['전체', 'index_start', 'index_complete', 'sync_start', 'sync_complete', 'plugin_error'];

function PluginLogModal({ initialLogs, onClose }: { initialLogs: PluginLogEntry[]; onClose: () => void }) {
  const [logs, setLogs] = useState<PluginLogEntry[]>(initialLogs);
  const [filter, setFilter] = useState('전체');
  const [clearing, setClearing] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

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
          <span className="text-sm font-semibold text-gray-200">플러그인 로그 ({filtered.length}건)</span>
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
            className={btnClass('reindex')}
            disabled={loadingBtn !== null}
            onClick={() => run('reindex', async () => {
              const res = await invoke<{ indexed: number; message: string }>('trigger_reindex');
              setDevResult(res.message);
            })}
          >
            {loadingBtn === 'reindex' && <span className="animate-spin">⟳</span>}
            DB 재인덱싱
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
