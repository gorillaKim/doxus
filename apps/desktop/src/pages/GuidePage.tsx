import { useState } from 'react';

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="text-[10px] font-bold text-gray-500 uppercase tracking-[0.2em] mb-4 flex items-center gap-2">
      <span className="w-1.5 h-1.5 rounded-full bg-indigo-500/50" />
      {children}
    </h2>
  );
}

function FeatureCard({ title, description, icon }: { title: string; description: string; icon: React.ReactNode }) {
  return (
    <div className="glass-card rounded-2xl p-6 border border-white/5 hover:border-indigo-500/30 transition-all duration-300 group">
      <div className="flex items-start gap-4">
        <div className="p-3 rounded-xl bg-gray-900/50 text-indigo-400 group-hover:scale-110 transition-transform duration-300">
          {icon}
        </div>
        <div className="flex flex-col gap-1">
          <h3 className="text-white font-bold text-lg">{title}</h3>
          <p className="text-gray-400 text-sm leading-relaxed">{description}</p>
        </div>
      </div>
    </div>
  );
}

function CodeBlock({ code }: { code: string }) {
  return (
    <div className="relative group">
      <pre className="bg-gray-950/80 border border-white/5 rounded-xl p-4 font-mono text-sm text-indigo-300 overflow-x-auto">
        <code>{code}</code>
      </pre>
      <div className="absolute top-3 right-3 text-[10px] uppercase font-bold text-gray-600 tracking-wider">Shell</div>
    </div>
  );
}

export default function GuidePage() {
  const [activeTab, setActiveTab] = useState<'app' | 'mcp' | 'cli'>('app');

  const tabs = [
    { id: 'app', label: '데스크톱 앱', icon: '💻' },
    { id: 'mcp', label: 'MCP', icon: '🤖' },
    { id: 'cli', label: 'CLI', icon: '📟' },
  ] as const;

  return (
    <div className="max-w-5xl mx-auto py-6 flex flex-col gap-8 animate-in fade-in slide-in-from-bottom-4 duration-700">
      {/* 헤더 */}
      <div className="flex flex-col gap-2">
        <h1 className="text-4xl font-extrabold text-white tracking-tight bg-clip-text text-transparent bg-gradient-to-br from-white to-gray-500">
          사용 가이드
        </h1>
        <p className="text-gray-500 text-sm font-medium">Doxus를 100% 활용하는 법을 알아보세요</p>
      </div>

      {/* 탭 헤더 */}
      <div className="flex p-1.5 bg-gray-950/40 backdrop-blur-xl border border-white/5 rounded-2xl w-fit">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`flex items-center gap-2 px-6 py-2.5 rounded-xl text-sm font-semibold transition-all duration-300 ${
              activeTab === tab.id
                ? 'bg-indigo-600 text-white shadow-lg shadow-indigo-600/20'
                : 'text-gray-500 hover:text-gray-300'
            }`}
          >
            <span className="text-lg">{tab.icon}</span>
            {tab.label}
          </button>
        ))}
      </div>

      {/* 탭 콘텐츠 */}
      <div className="min-h-[400px]">
        {activeTab === 'app' && (
          <div className="flex flex-col gap-8 animate-in fade-in slide-in-from-right-4 duration-500">
            <SectionTitle>주요 기능 안내</SectionTitle>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
              <FeatureCard
                title="통합 하이브리드 검색"
                description="Obsidian, Confluence, GitHub 등 흩어진 지식을 벡터 검색과 키워드 검색을 조합한 RRF 알고리즘으로 정확하게 찾아냅니다."
                icon={<svg className="w-6 h-6" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" /></svg>}
              />
              <FeatureCard
                title="지식 그래프 시각화"
                description="문서 간의 위키링크와 백링크를 자동으로 분석하여 지식의 연결망을 시각화하고 숨겨진 맥락을 발견하게 도와줍니다."
                icon={<svg className="w-6 h-6" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0h12a2 2 0 002-2v-4a2 2 0 00-2-2h-2.343M11 7.343l1.172-1.172a4 4 0 115.656 5.656l-1.172 1.172" /></svg>}
              />
              <FeatureCard
                title="WASM 플러그인 확장"
                description="필요한 데이터 소스가 있다면 WASM 기반 플러그인을 설치하여 검색 허브에 손쉽게 추가할 수 있습니다."
                icon={<svg className="w-6 h-6" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" /></svg>}
              />
              <FeatureCard
                title="로컬 퍼스트 보안"
                description="모든 데이터와 임베딩 모델은 사용자의 로컬 환경에 저장되며, 외부 서버로 지식이 유출되지 않습니다."
                icon={<svg className="w-6 h-6" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" /></svg>}
              />
            </div>
          </div>
        )}

        {activeTab === 'mcp' && (
          <div className="flex flex-col gap-8 animate-in fade-in slide-in-from-right-4 duration-500">
            <div className="flex flex-col gap-4">
              <SectionTitle>에이전트 컨텍스트 연동 (MCP)</SectionTitle>
              <div className="glass-card rounded-2xl p-8 border border-white/5 flex flex-col gap-6">
                <p className="text-gray-300 leading-relaxed">
                  Doxus는 <strong className="text-indigo-400">Model Context Protocol (MCP)</strong>를 지원합니다. 
                  Claude Desktop이나 다른 라이브러리 에이전트가 여러분의 로컬 지식 베이스를 직접 검색하고 읽을 수 있도록 설정하세요.
                </p>

                <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                  <div className="flex flex-col gap-2 p-4 rounded-xl bg-white/5 border border-white/5">
                    <span className="text-indigo-400 font-bold text-sm">STEP 1</span>
                    <p className="text-[13px] text-gray-400">Claude Desktop 설정에서 doxus-mcp 경로를 추가합니다.</p>
                  </div>
                  <div className="flex flex-col gap-2 p-4 rounded-xl bg-white/5 border border-white/5">
                    <span className="text-indigo-400 font-bold text-sm">STEP 2</span>
                    <p className="text-[13px] text-gray-400">에이전트에게 "doxus에서 최근 작업 내역 찾아줘"라고 요청합니다.</p>
                  </div>
                  <div className="flex flex-col gap-2 p-4 rounded-xl bg-white/5 border border-white/5">
                    <span className="text-indigo-400 font-bold text-sm">STEP 3</span>
                    <p className="text-[13px] text-gray-400">에이전트가 로컬 데이터를 바탕으로 답변을 생성합니다.</p>
                  </div>
                </div>

                <div className="flex flex-col gap-3">
                  <span className="text-xs font-bold text-gray-500 tracking-widest uppercase">설정 예시 (Claude Desktop)</span>
                  <CodeBlock code={JSON.stringify({
  mcpServers: {
    doxus: {
      command: "doxus-mcp",
      args: []
    }
  }
}, null, 2)} />
                </div>
              </div>
            </div>
          </div>
        )}

        {activeTab === 'cli' && (
          <div className="flex flex-col gap-8 animate-in fade-in slide-in-from-right-4 duration-500">
            <div className="flex flex-col gap-4">
              <SectionTitle>명령줄 인터페이스 (CLI)</SectionTitle>
              <div className="grid grid-cols-1 gap-6">
                <div className="glass-card rounded-2xl p-6 border border-white/5 flex flex-col gap-4">
                  <h3 className="text-white font-bold flex items-center gap-2">
                    <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                    프로젝트 관리 및 인덱싱
                  </h3>
                  <p className="text-sm text-gray-400">새로운 지식 소스를 추가하고 검색이 가능하도록 인덱싱을 수행합니다.</p>
                  <CodeBlock code="# 프로젝트 추가\ndoxus project add brain /path/to/your/vault\n\n# 모든 프로젝트 인덱싱 수행\ndoxus index" />
                </div>

                <div className="glass-card rounded-2xl p-6 border border-white/5 flex flex-col gap-4">
                  <h3 className="text-white font-bold flex items-center gap-2">
                    <span className="w-1.5 h-1.5 rounded-full bg-indigo-500" />
                    터미널 검색
                  </h3>
                  <p className="text-sm text-gray-400">앱을 켜지 않고도 터미널에서 즉시 검색 결과를 확인할 수 있습니다.</p>
                  <CodeBlock code="# 하이브리드 검색 실행\ndoxus search \"검색하고 싶은 내용\"\n\n# 특정 프로젝트 내에서만 검색\ndoxus search \"내용\" --project brain" />
                </div>

                <div className="glass-card rounded-2xl p-6 border border-white/5 flex flex-col gap-4">
                  <h3 className="text-white font-bold flex items-center gap-2">
                    <span className="w-1.5 h-1.5 rounded-full bg-amber-500" />
                    상태 및 그래프 확인
                  </h3>
                  <p className="text-sm text-gray-400">시스템 연동 상태와 문서 간의 관계를 조회합니다.</p>
                  <CodeBlock code="# 시스템 상태 조회\ndoxus status\n\n# 백링크 확인\ndoxus graph backlinks brain <doc_id>" />
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* 푸터 팁 */}
      <div className="p-6 rounded-2xl bg-indigo-500/5 border border-indigo-500/10 flex items-center gap-4">
        <span className="text-2xl">💡</span>
        <p className="text-sm text-indigo-300/80 font-medium whitespace-pre-line">
          Doxus는 사용자의 피드백을 통해 계속 진화합니다. 
          추가 기능 제안이나 버그 리포트는 GitHub 저장소를 방문해 주세요.
        </p>
      </div>
    </div>
  );
}
