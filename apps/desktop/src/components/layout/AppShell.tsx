import { ReactNode } from "react";
import { NavLink } from "react-router-dom";
import { ChatDrawer } from "./ChatDrawer";
import { useChatStore } from "../../stores/useChatStore";

const NAV_ITEMS = [
  { to: "/", label: "대시보드" },
  { to: "/search", label: "검색" },
  { to: "/projects", label: "프로젝트" },
  { to: "/workspace", label: "워크스페이스" },
  { to: "/market", label: "마켓" },
  { to: "/settings", label: "설정" },
];

export function AppShell({ children }: { children: ReactNode }) {
  const { isOpen, toggle } = useChatStore();

  return (
    <div className="flex h-screen bg-gray-950 text-gray-100">
      {/* 사이드바 */}
      <nav className="w-48 flex-shrink-0 border-r border-gray-800 flex flex-col p-4 gap-1">
        <span className="text-lg font-bold mb-4 text-indigo-400">doxus</span>
        {NAV_ITEMS.map(({ to, label }) => (
          <NavLink
            key={to}
            to={to}
            end={to === "/"}
            className={({ isActive }) =>
              `px-3 py-2 rounded-lg text-sm transition-colors ${
                isActive
                  ? "bg-indigo-600 text-white"
                  : "text-gray-400 hover:text-white hover:bg-gray-800"
              }`
            }
          >
            {label}
          </NavLink>
        ))}
        <div className="mt-auto">
          <button
            onClick={toggle}
            className="w-full px-3 py-2 rounded-lg text-sm text-gray-400 hover:text-white hover:bg-gray-800 transition-colors"
          >
            {isOpen ? "채팅 닫기" : "AI 채팅"}
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
