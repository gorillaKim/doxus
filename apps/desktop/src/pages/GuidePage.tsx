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

function ToolItem({ name, description }: { name: string; description: string }) {
  return (
    <div className="flex flex-col gap-1.5 p-4 rounded-xl bg-white/5 border border-white/5 hover:bg-white/10 transition-colors group">
      <code className="text-indigo-400 font-mono text-sm group-hover:text-indigo-300">{name}</code>
      <p className="text-gray-400 text-xs leading-relaxed">{description}</p>
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
    { id: 'mcp', label: 'MCP 도구', icon: '🤖' },
    { id: 'cli', label: 'CLI 명령어', icon: '📟' },
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
          <div className="flex flex-col gap-10 animate-in fade-in slide-in-from-right-4 duration-500">
            <div className="flex flex-col gap-4">
              <SectionTitle>에이전트 컨텍스트 (MCP Core Tools)</SectionTitle>
              <p className="text-gray-400 text-sm -mt-2">AI 에이전트(Claude 등)가 여러분의 로컬 지식을 탐색하기 위해 사용하는 전문 도구들입니다.</p>
              
              <div className="mt-4 flex flex-col gap-8">
                <div className="flex flex-col gap-4">
                  <h3 className="text-white font-semibold text-sm flex items-center gap-2">
                    <span className="w-1 h-3 bg-indigo-500 rounded-full" />
                    지식 검색 및 조회
                  </h3>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
                    <ToolItem name="doxus_search" description="하이브리드 검색을 통해 관련 문서 및 코드 조각을 찾습니다." />
                    <ToolItem name="doxus_get_document" description="특정 문서의 전체 내용과 상세 메타데이터를 불러옵니다." />
                    <ToolItem name="doxus_get_section" description="문서 내 특정 섹션(헤딩)만 추출하여 토큰을 절약합니다." />
                    <ToolItem name="doxus_get_toc" description="문서의 전체 목차 구조를 트리 형태로 파악합니다." />
                    <ToolItem name="doxus_list_documents" description="프로젝트 내 인덱싱된 모든 문서의 목록과 ID를 확인합니다." />
                    <ToolItem name="doxus_get_ranking" description="가장 많이 조회되거나 참조된 인기 문서 순위를 확인합니다." />
                    <ToolItem name="doxus_resolve_alias" description="별칭(Alias)이나 위키링크 이름을 통해 원본 문서를 찾습니다." />
                    <ToolItem name="doxus_inspect_document" description="문서의 최신성, 청크 수 등 상세 상태 정보를 조회합니다." />
                  </div>
                </div>

                <div className="flex flex-col gap-4">
                  <h3 className="text-white font-semibold text-sm flex items-center gap-2">
                    <span className="w-1 h-3 bg-emerald-500 rounded-full" />
                    지식 그래프 및 분석
                  </h3>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
                    <ToolItem name="doxus_get_backlinks" description="해당 문서를 인용하거나 참조하고 있는 역방향 링크 목록을 확인합니다." />
                    <ToolItem name="doxus_get_links" description="현재 문서에서 다른 문서로 연결되는 정방향 링크 목록을 확인합니다." />
                    <ToolItem name="doxus_find_path" description="두 문서 사이의 최단 연결 경로(최대 6홉)를 탐색합니다." />
                    <ToolItem name="doxus_get_cluster" description="특정 문서 중심의 지식 클러스터를 통해 연관 맥락을 파악합니다." />
                    <ToolItem name="doxus_find_related" description="RRF 알고리즘을 기반으로 현재 문서와 가장 유사한 자료를 추천받습니다." />
                    <ToolItem name="doxus_explain_search" description="특정 검색 결과가 왜 관련성이 높게 평가되었는지 근거를 분석합니다." />
                  </div>
                </div>

                <div className="flex flex-col gap-4">
                  <h3 className="text-white font-semibold text-sm flex items-center gap-2">
                    <span className="w-1 h-3 bg-amber-500 rounded-full" />
                    프로젝트 및 시스템 관리
                  </h3>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
                    <ToolItem name="doxus_status" description="서버 운영 상태 및 전체 프로젝트/문서 인덱싱 통계를 확인합니다." />
                    <ToolItem name="doxus_agent_summary" description="현재 지식 베이스의 특징, 주요 키워드 등에 대한 에이전트용 브리핑을 받습니다." />
                    <ToolItem name="doxus_list_projects" description="등록된 모든 지식 소스 프로젝트와 해당 소스의 상태를 조회합니다." />
                    <ToolItem name="doxus_index_project" description="지정된 프로젝트의 전체 데이터를 분석하여 인덱스를 갱신합니다." />
                    <ToolItem name="doxus_sync_project" description="기존 프로젝트의 바뀐 부분만 빠르게 동기화하여 최신 상태를 유지합니다." />
                    <ToolItem name="doxus_setup_project_agent" description="에이전트가 정보를 더 잘 찾을 수 있도록 프로젝트 지침 파일을 생성합니다." />
                  </div>
                </div>

                <div className="flex flex-col gap-4">
                  <h3 className="text-white font-semibold text-sm flex items-center gap-2">
                    <span className="w-1 h-3 bg-rose-500 rounded-full" />
                    플러그인 및 익스텐션
                  </h3>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
                    <ToolItem name="doxus_search_plugins" description="마켓플레이스에서 새로운 데이터 소스 플러그인을 검색합니다." />
                    <ToolItem name="doxus_install_plugin" description="원격 또는 로컬의 WASM 플러그인을 시스템에 설치합니다." />
                    <ToolItem name="doxus_status_plugin" description="특정 플러그인의 활성화 여부 및 연결된 인스턴스 정보를 확인합니다." />
                    <ToolItem name="doxus_logs_plugin" description="플러그인 실행 중 발생한 로그를 확인하여 문제를 디버깅합니다." />
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}

        {activeTab === 'cli' && (
          <div className="flex flex-col gap-10 animate-in fade-in slide-in-from-right-4 duration-500">
            <div className="flex flex-col gap-6">
              <SectionTitle>명령줄 인터페이스 (Full CLI Reference)</SectionTitle>
              <p className="text-gray-400 text-sm -mt-4">터미널에서 Doxus의 모든 기능을 활용하는 강력한 방법들입니다.</p>
              
              <div className="grid grid-cols-1 gap-8 mt-4">
                <div className="glass-card rounded-2xl p-6 border border-white/5 flex flex-col gap-6">
                  <div className="flex flex-col gap-4">
                    <h3 className="text-white font-bold flex items-center gap-2">
                      <span className="w-1.5 h-1.5 rounded-full bg-indigo-500" />
                      프로젝트 및 데이터 관리 (project, index)
                    </h3>
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                      <div className="flex flex-col gap-2">
                        <code className="text-xs bg-gray-900 px-2 py-1 rounded text-indigo-400 w-fit">project add &lt;name&gt; &lt;path&gt;</code>
                        <p className="text-xs text-gray-400">새로운 로컬 폴더나 저장소를 프로젝트로 등록합니다.</p>
                      </div>
                      <div className="flex flex-col gap-2">
                        <code className="text-xs bg-gray-900 px-2 py-1 rounded text-indigo-400 w-fit">project remove &lt;name&gt;</code>
                        <p className="text-xs text-gray-400">프로젝트의 인덱스 데이터를 삭제합니다. (원본 파일은 유지)</p>
                      </div>
                      <div className="flex flex-col gap-2">
                        <code className="text-xs bg-gray-900 px-2 py-1 rounded text-indigo-400 w-fit">index</code>
                        <p className="text-xs text-gray-400">등록된 모든 프로젝트를 스캔하고 최신 지식으로 동기화합니다.</p>
                      </div>
                      <div className="flex flex-col gap-2">
                        <code className="text-xs bg-gray-900 px-2 py-1 rounded text-indigo-400 w-fit">project list</code>
                        <p className="text-xs text-gray-400">현재 관리 중인 프로젝트의 목록과 동기화 상태를 확인합니다.</p>
                      </div>
                    </div>
                    <CodeBlock code={`# 새 프로젝트 추가 후 인덱싱 실행 예시
doxus project add brain ~/Documents/Obsidian
doxus index`} />
                  </div>
                </div>

                <div className="glass-card rounded-2xl p-6 border border-white/5 flex flex-col gap-6">
                  <div className="flex flex-col gap-4">
                    <h3 className="text-white font-bold flex items-center gap-2">
                      <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                      검색 및 지식 탐색 (search, status)
                    </h3>
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                      <div className="flex flex-col gap-2">
                        <code className="text-xs bg-gray-900 px-2 py-1 rounded text-emerald-400 w-fit">search "&lt;query&gt;"</code>
                        <p className="text-xs text-gray-400">전체 지식 베이스에서 하이브리드 검색을 수행합니다.</p>
                      </div>
                      <div className="flex flex-col gap-2">
                        <code className="text-xs bg-gray-900 px-2 py-1 rounded text-emerald-400 w-fit">search "&lt;q&gt;" --project &lt;p&gt;</code>
                        <p className="text-xs text-gray-400">특정 프로젝트 내로 검색 범위를 한정합니다.</p>
                      </div>
                      <div className="flex flex-col gap-2">
                        <code className="text-xs bg-gray-900 px-2 py-1 rounded text-emerald-400 w-fit">status</code>
                        <p className="text-xs text-gray-400">인덱싱된 문서 총수와 각 타입별 청크 비율 등 통계를 확인합니다.</p>
                      </div>
                    </div>
                    <CodeBlock code={`# 특정 주제 검색 후 5개만 보기
doxus search "Rust ownership" --limit 5`} />
                  </div>
                </div>

                <div className="glass-card rounded-2xl p-6 border border-white/5 flex flex-col gap-6">
                  <div className="flex flex-col gap-4">
                    <h3 className="text-white font-bold flex items-center gap-2">
                      <span className="w-1.5 h-1.5 rounded-full bg-amber-500" />
                      고급 그래프 및 플러그인 (graph, plugin)
                    </h3>
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                      <div className="flex flex-col gap-2">
                        <code className="text-xs bg-gray-900 px-2 py-1 rounded text-amber-400 w-fit">graph links &lt;proj&gt; &lt;id&gt;</code>
                        <p className="text-xs text-gray-400">특정 문서와 연결된 모든 링크 정보를 터미널에 출력합니다.</p>
                      </div>
                      <div className="flex flex-col gap-2">
                        <code className="text-xs bg-gray-900 px-2 py-1 rounded text-amber-400 w-fit">graph path &lt;p&gt; &lt;from&gt; &lt;to&gt;</code>
                        <p className="text-xs text-gray-400">두 지식 사이의 연결 고리를 최단 경로로 추적합니다.</p>
                      </div>
                      <div className="flex flex-col gap-2">
                        <code className="text-xs bg-gray-900 px-2 py-1 rounded text-amber-400 w-fit">plugin install &lt;id&gt;</code>
                        <p className="text-xs text-gray-400">ID를 통해 새로운 외부 데이터 소스 플러그인을 설치합니다.</p>
                      </div>
                    </div>
                  </div>
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
