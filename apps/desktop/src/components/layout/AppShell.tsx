import { ReactNode, useState } from "react";
import { NavLink } from "react-router-dom";
import { ChatDrawer } from "./ChatDrawer";
import { useChatStore } from "../../stores/useChatStore";
import logo from "../../assets/doxus-logo-minimal.png";

// ── SVG 아이콘 ──────────────────────────────────────────────────────────────
function IconDashboard() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0">
      <rect x="3" y="3" width="7" height="7" rx="1" />
      <rect x="14" y="3" width="7" height="7" rx="1" />
      <rect x="3" y="14" width="7" height="7" rx="1" />
      <rect x="14" y="14" width="7" height="7" rx="1" />
    </svg>
  );
}
function IconSearch() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0">
      <circle cx="11" cy="11" r="7" />
      <line x1="16.5" y1="16.5" x2="22" y2="22" />
    </svg>
  );
}
function IconProjects() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0">
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" />
    </svg>
  );
}
// IconWorkspace removed as feature is deprecated
function IconMarket() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0">
      <path d="M6 2L3 6v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6l-3-4z" />
      <line x1="3" y1="6" x2="21" y2="6" />
      <path d="M16 10a4 4 0 0 1-8 0" />
    </svg>
  );
}
function IconSettings() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}
function IconAgent() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0 text-amber-500">
      <path d="M12 8V4H8" />
      <rect width="16" height="12" x="4" y="8" rx="2" />
      <path d="M2 14h2" />
      <path d="M20 14h2" />
      <path d="M15 13v2" />
      <path d="M9 13v2" />
    </svg>
  );
}
function IconChat() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0">
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
    </svg>
  );
}
function IconGraph() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0">
      <circle cx="18" cy="5" r="3" />
      <circle cx="6" cy="12" r="3" />
      <circle cx="18" cy="19" r="3" />
      <line x1="8.59" y1="13.51" x2="15.42" y2="17.49" />
      <line x1="15.41" y1="6.51" x2="8.59" y2="10.49" />
    </svg>
  );
}
function IconGuide() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0">
      <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
      <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
    </svg>
  );
}
function IconFreshness() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0">
      <path d="M12 2v20M17 5H9.5a3.5 3.5 0 0 0 0 7h5a3.5 3.5 0 0 1 0 7H6" />
    </svg>
  );
}
function IconScheduler() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0">
      <circle cx="12" cy="12" r="10" />
      <polyline points="12 6 12 12 16 14" />
    </svg>
  );
}
function IconSidebarCollapse({ collapsed }: { collapsed: boolean }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 shrink-0">
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <line x1="9" y1="3" x2="9" y2="21" />
      {collapsed
        ? <polyline points="13 9 17 12 13 15" />
        : <polyline points="15 9 11 12 15 15" />}
    </svg>
  );
}

// ── 네비게이션 아이템 정의 ──────────────────────────────────────────────────
const NAV_ITEMS = [
  { to: "/", label: "대시보드", icon: <IconDashboard /> },
  { to: "/graph", label: "그래프", icon: <IconGraph /> },
  { to: "/search", label: "검색", icon: <IconSearch /> },
  { to: "/projects", label: "프로젝트", icon: <IconProjects /> },
  { to: "/freshness", label: "신선도", icon: <IconFreshness /> },
  { to: "/scheduler", label: "스케줄", icon: <IconScheduler /> },
  { to: "/market", label: "마켓", icon: <IconMarket /> },
  { to: "/agent", label: "에이전트", icon: <IconAgent /> },
  { to: "/guide", label: "가이드", icon: <IconGuide /> },
  { to: "/settings", label: "설정", icon: <IconSettings /> },
];

function getInitialSidebarOpen(): boolean {
  const stored = localStorage.getItem("doxus-sidebar-open");
  return stored === null ? true : stored === "true";
}

export function AppShell({ children }: { children: ReactNode }) {
  const { isOpen, toggle } = useChatStore();
  const [sidebarOpen, setSidebarOpen] = useState<boolean>(getInitialSidebarOpen);

  function toggleSidebar() {
    const next = !sidebarOpen;
    setSidebarOpen(next);
    localStorage.setItem("doxus-sidebar-open", String(next));
  }

  return (
    <div className="flex h-screen bg-mesh text-gray-100 overflow-hidden font-sans">
      {/* 사이드바 */}
      <nav
        data-testid="sidebar"
        className={`${sidebarOpen ? "w-56" : "w-16"} flex-shrink-0 border-r border-white/5 bg-gray-950/20 backdrop-blur-3xl flex flex-col transition-all duration-300 ease-in-out overflow-hidden`}
      >
        {/* 헤더: 로고 + 토글 버튼 */}
        <div className={`flex items-center px-3 py-4 ${sidebarOpen ? "justify-between" : "justify-center"}`}>
          {sidebarOpen && (
            <div className="flex items-center gap-2 overflow-hidden">
              <img src={logo} alt="doxus logo" className="w-6 h-6 shrink-0" />
              <span className="text-base font-bold text-indigo-400 truncate">doxus</span>
            </div>
          )}
          <button
            data-testid="sidebar-toggle"
            onClick={toggleSidebar}
            title={sidebarOpen ? "사이드바 접기" : "사이드바 펼치기"}
            className="p-1.5 rounded-lg text-gray-500 hover:text-gray-200 hover:bg-gray-800 transition-colors"
          >
            <IconSidebarCollapse collapsed={!sidebarOpen} />
          </button>
        </div>

        {/* 구분선 */}
        <div className="border-t border-white/5 mx-4 mb-3 opacity-50" />

        {/* 네비게이션 */}
        <div className="flex flex-col gap-0.5 px-2 flex-1">
          {NAV_ITEMS.map(({ to, label, icon }) => (
            <NavLink
              key={to}
              to={to}
              end={to === "/"}
              title={!sidebarOpen ? label : undefined}
              className={({ isActive }) =>
                `flex items-center gap-3 rounded-xl text-sm font-medium transition-all duration-200 ${
                  sidebarOpen ? "px-3 py-2.5" : "px-0 py-2.5 justify-center mx-2"
                } ${
                  isActive
                    ? "bg-indigo-500/20 text-indigo-300 shadow-inner ring-1 ring-indigo-500/20"
                    : "text-gray-400 hover:text-white hover:bg-white/5"
                }`
              }
            >
              {icon}
              {sidebarOpen && <span>{label}</span>}
            </NavLink>
          ))}
        </div>

        {/* 하단: AI 채팅 버튼 */}
        <div className="px-3 py-4 border-t border-white/5 bg-gray-950/10">
          <button
            onClick={toggle}
            title={!sidebarOpen ? "AI 채팅" : undefined}
            className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-xl text-sm font-medium transition-all duration-200 ${
              !sidebarOpen ? "justify-center" : ""
            } ${isOpen 
                ? "bg-indigo-500 text-white shadow-lg shadow-indigo-500/20" 
                : "text-gray-400 hover:text-white hover:bg-white/5 border border-transparent hover:border-white/5"
            }`}
          >
            <IconChat />
            {sidebarOpen && <span>{isOpen ? "채팅 닫기" : "AI Librarian"}</span>}
          </button>
        </div>
      </nav>

      {/* 메인 콘텐츠 */}
      <main className="flex-1 overflow-auto p-6">{children}</main>

      {/* 채팅 드로어 */}
      {isOpen && <ChatDrawer />}
    </div>
  );
}
