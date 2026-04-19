import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

interface McpStatus {
  connected: boolean;
  path: string;
  config: any;
}

interface ClaudeMcpStatus {
  desktop: McpStatus;
  cli: McpStatus;
}

export default function AgentPage() {
  const [claudeStatus, setClaudeStatus] = useState<ClaudeMcpStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  const [showPreview, setShowPreview] = useState(false);
  const [previewContent, setPreviewContent] = useState('');

  const fetchClaudeStatus = async () => {
    try {
      const res = await invoke<ClaudeMcpStatus>('get_claude_mcp_config');
      setClaudeStatus(res);
    } catch (e) {
      console.error('Failed to fetch Claude status', e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchClaudeStatus();
  }, []);

  const handleToggleClaude = async (target: 'desktop' | 'cli', isConnected: boolean) => {
    const actionKey = `claude-${target}`;
    setActionLoading(actionKey);
    try {
      if (isConnected) {
        if (!confirm(`Claude ${target === 'desktop' ? 'Desktop' : 'Code (CLI)'}에서 Doxus MCP 연동을 해제하시겠습니까?`)) return;
        await invoke('remove_claude_mcp_config', { target });
      } else {
        await invoke('upsert_claude_mcp_config', { target });
      }
      await fetchClaudeStatus();
    } catch (e) {
      alert(`연동 설정 실패: ${String(e)}`);
    } finally {
      setActionLoading(null);
    }
  };

  const handleOpenPreview = async () => {
    try {
      const content = await invoke<string>('get_claude_md_template');
      setPreviewContent(content);
      setShowPreview(true);
    } catch (e) {
      alert(`미리보기 로드 실패: ${String(e)}`);
    }
  };

  return (
    <div className="flex flex-col gap-8 max-w-5xl mx-auto pb-12">
      {showPreview && (
        <PreviewModal 
          content={previewContent} 
          onClose={() => setShowPreview(false)} 
        />
      )}

      {/* Header */}
      <div className="flex flex-col gap-2">
        <h1 className="text-3xl font-bold text-white tracking-tight flex items-center gap-3">
          <span className="text-4xl">🤖</span> AI Agent Integrations
        </h1>
        <p className="text-gray-400">
          Doxus의 지식 데이터를 AI 에이전트와 연결하여 강력한 AI 비서를 만드세요.
        </p>
      </div>

      {/* Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
        {/* Claude Desktop Card */}
        <div className="bg-gray-900/50 backdrop-blur-xl border border-white/5 rounded-2xl p-6 flex flex-col gap-4 hover:border-indigo-500/30 transition-all group">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-12 h-12 bg-[#D97757]/10 rounded-xl flex items-center justify-center text-2xl">
                🎨
              </div>
              <div>
                <h2 className="text-xl font-semibold text-white">Claude Desktop</h2>
                <p className="text-xs text-gray-500">GUI Application</p>
              </div>
            </div>
            {claudeStatus?.desktop.connected ? (
              <span className="px-2.5 py-1 bg-emerald-500/10 text-emerald-400 text-xs font-bold rounded-full border border-emerald-500/20">
                CONNECTED
              </span>
            ) : (
              <span className="px-2.5 py-1 bg-gray-800 text-gray-500 text-xs font-bold rounded-full border border-gray-700">
                DISCONNECTED
              </span>
            )}
          </div>

          <p className="text-sm text-gray-400 leading-relaxed">
            Claude Desktop 앱에 Doxus MCP 서버를 등록합니다. 
          </p>

          <div className="mt-auto pt-4 flex flex-col gap-4">
            <div className="flex items-center justify-between p-4 bg-black/20 rounded-2xl border border-white/5 group-hover:border-indigo-500/20 transition-all">
              <div className="flex flex-col">
                <span className="text-sm font-semibold text-white">데스크톱 앱 연동</span>
                <span className="text-[10px] text-gray-500 font-mono">
                  {claudeStatus?.desktop.connected ? 'Desktop Config에 연결됨' : '연결 해제됨'}
                </span>
              </div>
              <button
                onClick={() => handleToggleClaude('desktop', !!claudeStatus?.desktop.connected)}
                disabled={actionLoading === 'claude-desktop'}
                className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors focus:outline-none ${
                    claudeStatus?.desktop.connected ? 'bg-indigo-600' : 'bg-gray-700'
                } ${actionLoading === 'claude-desktop' ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}`}
              >
                <span
                  className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                      claudeStatus?.desktop.connected ? 'translate-x-6' : 'translate-x-1'
                  }`}
                />
              </button>
            </div>
          </div>
        </div>

        {/* Claude CLI Card */}
        <div className="bg-gray-900/50 backdrop-blur-xl border border-white/5 rounded-2xl p-6 flex flex-col gap-4 hover:border-indigo-500/30 transition-all group">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-12 h-12 bg-indigo-500/10 rounded-xl flex items-center justify-center text-2xl">
                🐚
              </div>
              <div>
                <h2 className="text-xl font-semibold text-white">Claude Code</h2>
                <p className="text-xs text-gray-500">CLI / Terminal Agent</p>
              </div>
            </div>
            {claudeStatus?.cli.connected ? (
              <span className="px-2.5 py-1 bg-emerald-500/10 text-emerald-400 text-xs font-bold rounded-full border border-emerald-500/20">
                CONNECTED
              </span>
            ) : (
              <span className="px-2.5 py-1 bg-gray-800 text-gray-500 text-xs font-bold rounded-full border border-gray-700">
                DISCONNECTED
              </span>
            )}
          </div>

          <p className="text-sm text-gray-400 leading-relaxed">
            터미널용 Claude Code(CLI) 및 OMC 플러그인을 위해 지식 베이스를 연결합니다.
          </p>

          <div className="mt-auto pt-4 flex flex-col gap-4">
            <div className="flex items-center justify-between p-4 bg-black/20 rounded-2xl border border-white/5 group-hover:border-indigo-500/20 transition-all">
              <div className="flex flex-col">
                <span className="text-sm font-semibold text-white">CLI/터미널 연동</span>
                <span className="text-[10px] text-gray-500 font-mono">
                  {claudeStatus?.cli.connected ? 'settings.json에 등록됨' : '연결 해제됨'}
                </span>
              </div>
              <button
                onClick={() => handleToggleClaude('cli', !!claudeStatus?.cli.connected)}
                disabled={actionLoading === 'claude-cli'}
                className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors focus:outline-none ${
                    claudeStatus?.cli.connected ? 'bg-indigo-600' : 'bg-gray-700'
                } ${actionLoading === 'claude-cli' ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}`}
              >
                <span
                  className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                      claudeStatus?.cli.connected ? 'translate-x-6' : 'translate-x-1'
                  }`}
                />
              </button>
            </div>
          </div>
        </div>

        {/* Gemini Placeholder */}
        <div className="bg-gray-900/30 border border-dashed border-white/10 rounded-2xl p-6 flex flex-col gap-4 opacity-70 grayscale hover:grayscale-0 transition-all">
          <div className="flex items-center gap-3">
            <div className="w-12 h-12 bg-blue-500/10 rounded-xl flex items-center justify-center text-2xl">
              ✨
            </div>
            <div>
              <h2 className="text-xl font-semibold text-gray-300">Google Gemini</h2>
              <p className="text-xs text-gray-500">Google AI</p>
            </div>
          </div>
          <p className="text-sm text-gray-400 italic leading-relaxed">
            지원 예정: Gemini CLI 또는 Web 에이전트와 연동하여 다중 모달 검색을 지원할 예정입니다.
          </p>
          <div className="mt-auto">
             <span className="text-xs text-gray-600 font-semibold uppercase tracking-widest">COMING SOON</span>
          </div>
        </div>
      </div>

      {/* Note Section */}
      <div className="bg-indigo-500/5 border border-indigo-500/10 rounded-2xl p-6 flex items-start gap-4">
        <div className="text-2xl mt-1">💡</div>
        <div className="flex flex-col gap-2">
          <h3 className="text-indigo-300 font-semibold">동작 원리 및 활용 팁</h3>
          <p className="text-sm text-gray-400 leading-relaxed">
            Doxus의 에이전트 브리지는 로컬 프로젝트 폴더의 <code className="text-indigo-400 font-mono text-xs">CLAUDE.md</code> 파일을 통해 에이전트에게 지식을 습득하는 법을 가르칩니다.
            에이전트는 사용자가 작성한 문서를 읽고, 그 안에 포함된 링크와 연결고리를 스스로 추적하며 답변의 정확도를 높입니다.
          </p>
          <button 
            onClick={handleOpenPreview}
            className="text-xs text-indigo-400 hover:text-indigo-300 font-bold flex items-center gap-1 transition-colors w-fit pt-2"
          >
            🔍 삽입되는 지침 내용 미리보기
          </button>
        </div>
      </div>

      {/* Project Agent Onboarding Section */}
      <ProjectAgentSection onOpenPreview={handleOpenPreview} />
    </div>
  );
}

function PreviewModal({ content, onClose }: { content: string; onClose: () => void }) {
  return (
    <div className="fixed inset-0 bg-black/80 backdrop-blur-xl z-[100] flex items-center justify-center p-8 animate-in fade-in duration-300">
      <div className="bg-gray-900 border border-white/10 rounded-3xl w-full max-w-4xl max-h-[85vh] flex flex-col shadow-2xl animate-in zoom-in-95 duration-300">
        <div className="flex items-center justify-between p-6 border-b border-white/5">
          <div className="flex items-center gap-3">
            <span className="text-2xl">📝</span>
            <h2 className="text-xl font-bold text-white">CLAUDE.md 삽입 지침 미리보기</h2>
          </div>
          <button 
            onClick={onClose}
            className="w-10 h-10 flex items-center justify-center rounded-full hover:bg-white/5 text-gray-500 hover:text-white transition-colors"
          >
            ✕
          </button>
        </div>
        
        <div className="flex-1 overflow-y-auto p-8 prose prose-invert prose-indigo max-w-none">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>
            {content}
          </ReactMarkdown>
        </div>

        <div className="p-6 border-t border-white/5 flex justify-end">
          <button 
            onClick={onClose}
            className="px-6 py-2.5 bg-indigo-600 hover:bg-indigo-500 text-white font-bold rounded-xl transition-all active:scale-95"
          >
            확인 및 닫기
          </button>
        </div>
      </div>
    </div>
  );
}

function ProjectAgentSection({ onOpenPreview }: { onOpenPreview: () => void }) {
  const [projects, setProjects] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [actionId, setActionId] = useState<string | null>(null);

  const fetchProjects = async () => {
    try {
      const { projects: res } = await invoke<{ projects: any[] }>('list_projects');
      setProjects(res);
    } catch (e) {
      console.error('Failed to fetch projects', e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchProjects();
  }, []);

  const handleGenerateGuide = async (name: string, path: string) => {
    setActionId(name);
    try {
      await invoke('generate_project_claude_md', { path });
      alert(`'${name}' 프로젝트에 CLAUDE.md 가이드가 생성되었습니다.`);
    } catch (e) {
      alert(`가이드 생성 실패: ${String(e)}`);
    } finally {
      setActionId(null);
    }
  };

  const handleGenerateGlobalGuide = async () => {
    setActionId('global');
    try {
      await invoke('generate_global_claude_md');
      alert(`글로벌 가이드(~/.claude/CLAUDE.md)가 성공적으로 업데이트되었습니다.`);
    } catch (e) {
      alert(`글로벌 가이드 생성 실패: ${String(e)}`);
    } finally {
      setActionId(null);
    }
  };

  if (loading) return null;

  return (
    <section className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h2 className="text-xl font-bold text-white tracking-tight">Project Agent Onboarding</h2>
          <p className="text-sm text-gray-500">
            로컬 프로젝트 폴더에 에이전트 지침서(<code className="text-indigo-400 font-mono text-xs">CLAUDE.md</code>)를 생성합니다.
          </p>
        </div>
        <div className="flex items-center gap-3">
          <button 
            onClick={onOpenPreview}
            className="px-4 py-2 bg-white/5 hover:bg-white/10 text-gray-400 hover:text-white rounded-xl text-xs font-bold transition-all border border-white/5"
          >
            가이드 미리보기
          </button>
          <button
            onClick={handleGenerateGlobalGuide}
            disabled={actionId === 'global'}
            className="flex-shrink-0 px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-xl text-sm font-bold transition-all shadow-lg shadow-indigo-500/20 disabled:opacity-50 flex items-center gap-2 group"
          >
            <span className="group-hover:scale-110 transition-transform">🌐</span>
            {actionId === 'global' ? '글로벌 설정 중...' : '글로벌 가이드 적용'}
          </button>
        </div>
      </div>

      <div className="bg-gray-900/50 border border-white/5 rounded-2xl overflow-hidden">
        <table className="w-full text-left text-sm">
          <thead>
            <tr className="border-b border-white/5 bg-white/5">
              <th className="px-6 py-3 font-semibold text-gray-300">프로젝트</th>
              <th className="px-6 py-3 font-semibold text-gray-300">경로</th>
              <th className="px-6 py-3 font-semibold text-gray-300 text-right">관리</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-white/5">
            {projects.filter(p => p.source_type === 'obsidian' || p.source_type === 'local').map((p) => (
              <tr key={p.name} className="hover:bg-white/[0.02] transition-colors">
                <td className="px-6 py-4">
                  <div className="font-medium text-white">{p.display_name}</div>
                  <div className="text-[10px] text-gray-500 uppercase">{p.source_type}</div>
                </td>
                <td className="px-6 py-4 text-xs text-gray-500 font-mono truncate max-w-xs" title={p.path}>
                  {p.path}
                </td>
                <td className="px-6 py-4 text-right">
                  <button
                    onClick={() => handleGenerateGuide(p.name, p.path)}
                    disabled={actionId === p.name}
                    className="px-3 py-1.5 bg-indigo-500/10 hover:bg-indigo-500/20 text-indigo-300 border border-indigo-500/20 rounded-lg text-xs font-semibold transition-all disabled:opacity-50"
                  >
                    {actionId === p.name ? '생성 중...' : 'CLAUDE.md 가이드 생성'}
                  </button>
                </td>
              </tr>
            ))}
            {projects.length === 0 && (
              <tr>
                <td colSpan={3} className="px-6 py-8 text-center text-gray-500 italic">
                  연동 가이드 생성 가능 프로젝트가 없습니다.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
